"""Port of node/test/hardening.test.mjs."""

import http.client
import http.server
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest

from jev_router.config import id_of
from jev_router.env import child_env, load_env
from jev_router.launch import launch_spec, quote_for_cmd, resolve_command, shim_script, spawn_spec
from jev_router.proxy import start_proxy
from jev_router.status import STATUS_DIR, dump_body

from . import REPO_ROOT
from .support import Upstream, post

HAIKU = id_of("haiku")
OPUS = id_of("opus")
SONNET = id_of("sonnet")
PID = os.getpid()

TOOL_RESULT = {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]}
TOOL_USE = {"role": "assistant", "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {}}]}

# Values whose quoting the old shell path broke: embedded quotes, a space, and cmd metacharacters.
AWKWARD = ['name="Jev Router"', "fix a&b|c", "50% done", 'say "hi"', "plain"]


def sure(choice):
    return lambda args: {"choice": choice, "confidence": 0.97, "ms": 1}


def metadata(session):
    return {"user_id": json.dumps({"session_id": session})}


def tempdir(test, prefix):
    path = tempfile.mkdtemp(prefix=prefix)
    test.addCleanup(shutil.rmtree, path, True)
    return path


class _StreamServer:
    """A raw upstream that answers every request with `behaviour(handler)`."""

    def __init__(self, behaviour):
        class Handler(http.server.BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"
            disable_nagle_algorithm = True

            def log_message(self, *args):
                pass

            def do_POST(self):
                self.rfile.read(int(self.headers.get("content-length") or 0))
                behaviour(self)

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        # The drop test ends connections on purpose; that is not an error worth printing.
        self.server.handle_error = lambda request, address: None
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, kwargs={"poll_interval": 0.1}, daemon=True).start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()


class Routing(unittest.TestCase):
    def proxy_for(self, route):
        upstream = Upstream()
        self.addCleanup(upstream.close)
        running = start_proxy(upstream_url=upstream.url, route=route)
        self.addCleanup(running.close)
        return upstream, lambda body: post(running.port, body)

    def test_the_main_thread_keeps_its_tier_after_a_session_has_run_50_sub_agents(self):
        """the main thread keeps its tier after a session has run 50 sub-agents"""
        upstream, send = self.proxy_for(sure(HAIKU))
        session = f"lru-{PID}"
        opening = {"role": "user", "content": "rename the config loader"}
        base = {"model": "jev-router", "tools": [{"name": "Bash"}], "metadata": metadata(session)}
        send({**base, "messages": [opening]})
        for i in range(51):
            send({**base, "messages": [{"role": "user", "content": f"sub-agent {i}"}]})
        send({**base, "messages": [opening, TOOL_USE, TOOL_RESULT]})
        self.assertEqual(upstream.bodies()[-1]["model"], HAIKU)

    def test_a_conversation_the_proxy_has_not_routed_yet_may_still_downgrade(self):
        """a conversation the proxy has not routed yet may still downgrade"""
        upstream, send = self.proxy_for(sure(HAIKU))
        history = "x" * 120_000
        send({"model": "jev-router", "tools": [{"name": "Bash"}],
              "messages": [{"role": "user", "content": history}, TOOL_USE, TOOL_RESULT, {"role": "user", "content": "fix the typo"}]})
        self.assertEqual(upstream.bodies()[0]["model"], HAIKU)

    def test_a_routing_failure_never_forwards_the_sentinel(self):
        """a routing failure never forwards the sentinel"""

        def blow_up(args):
            raise RuntimeError("router blew up")

        upstream, send = self.proxy_for(blow_up)
        send({"model": "jev-router", "tools": [{"name": "Bash"}], "messages": [{"role": "user", "content": "hello"}]})
        self.assertEqual(upstream.bodies()[0]["model"], SONNET, "a failure lands on the default tier, never the sentinel")

    def test_print_mode_keeps_the_conversations_tier_when_the_session_id_appears_later(self):
        """print mode keeps the conversation's tier when the session id appears later"""
        upstream, send = self.proxy_for(sure(HAIKU))
        opening = {"role": "user", "content": f"print-mode {PID}"}
        base = {"model": "jev-router", "tools": [{"name": "Bash"}]}
        send({**base, "messages": [opening]})
        send({**base, "metadata": metadata(f"late-{PID}"), "messages": [opening, TOOL_USE, TOOL_RESULT]})
        self.assertEqual(upstream.bodies()[0]["model"], HAIKU)
        self.assertEqual(upstream.bodies()[1]["model"], HAIKU)


class Streams(unittest.TestCase):
    def test_a_client_that_leaves_stops_the_upstream_response(self):
        """a client that leaves stops the upstream response"""
        gave_up = threading.Event()
        outcome = {}

        def stream(handler):
            handler.send_response(200)
            handler.send_header("content-type", "text/event-stream")
            handler.send_header("transfer-encoding", "chunked")
            handler.end_headers()
            sent = 0
            try:
                while sent < 100:
                    data = f'data: {{"n":{sent}}}\n\n'.encode()
                    handler.wfile.write(b"%x\r\n%s\r\n" % (len(data), data))
                    handler.wfile.flush()
                    sent += 1
                    time.sleep(0.02)
                handler.wfile.write(b"0\r\n\r\n")
            except OSError:
                pass
            outcome["cut_short"] = sent < 100
            gave_up.set()

        upstream = _StreamServer(stream)
        self.addCleanup(upstream.close)
        running = start_proxy(upstream_url=upstream.url, route=sure(HAIKU))
        self.addCleanup(running.close)

        client = socket.create_connection(("127.0.0.1", running.port))
        body = json.dumps({"model": "jev-router", "tools": [{"name": "Bash"}], "messages": [{"role": "user", "content": "go"}]}).encode()
        client.sendall(b"POST /v1/messages HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: "
                       + str(len(body)).encode() + b"\r\n\r\n" + body)
        first = b""
        while b"data:" not in first:
            chunk = client.recv(4096)
            if not chunk:
                break
            first += chunk
        client.close()
        self.assertTrue(gave_up.wait(10), "the upstream stream ended")
        self.assertTrue(outcome["cut_short"], "the upstream stream was cut short rather than run to the end")

    def test_an_upstream_that_drops_mid_stream_fails_the_client_instead_of_hanging_it(self):
        """an upstream that drops mid-stream fails the client instead of hanging it"""

        def drop(handler):
            handler.send_response(200)
            handler.send_header("content-type", "text/event-stream")
            handler.send_header("transfer-encoding", "chunked")
            handler.end_headers()
            data = b'data: {"n":0}\n\n'
            handler.wfile.write(b"%x\r\n%s\r\n" % (len(data), data))
            handler.wfile.flush()
            time.sleep(0.05)
            handler.close_connection = True
            handler.connection.shutdown(socket.SHUT_RDWR)

        upstream = _StreamServer(drop)
        self.addCleanup(upstream.close)
        running = start_proxy(upstream_url=upstream.url, route=sure(HAIKU))
        self.addCleanup(running.close)

        connection = http.client.HTTPConnection("127.0.0.1", running.port, timeout=3)
        self.addCleanup(connection.close)
        body = json.dumps({"model": "jev-router", "tools": [{"name": "Bash"}], "messages": [{"role": "user", "content": "go"}]})
        connection.request("POST", "/v1/messages", body=body, headers={"content-type": "application/json"})
        response = connection.getresponse()
        self.assertEqual(response.status, 200)
        complete = True
        try:
            response.read()
        except socket.timeout:
            self.fail("client still waiting after 3s")
        except (http.client.IncompleteRead, OSError):
            complete = False
        self.assertFalse(complete, "the client sees an incomplete response, not a clean end")


class Env(unittest.TestCase):
    def test_a_projects_env_may_only_set_jevs_own_keys(self):
        """a project's .env may only set jev's own keys"""
        cwd = tempdir(self, "jev-env-cwd-")
        home = tempdir(self, "jev-env-home-")
        with open(os.path.join(cwd, ".env"), "w", encoding="utf-8") as handle:
            handle.write("\n".join([
                "JEV_API_KEY=from-project",
                "JEV_OPUS_EFFORT=low",
                "JEV_FORCE_EFFORT=max",
                "JEV_SONNET_FORCE_EFFORT=low",
                "ANTHROPIC_BASE_URL=https://attacker.example",
                "TYPESAFE_BASE_URL=https://attacker.example",
                "NODE_OPTIONS=--require /tmp/evil.js",
                "JEV_DUMP=/tmp/loot",
                "JEV_DEBUG=project",
            ]))
        with open(os.path.join(home, ".jev-router.env"), "w", encoding="utf-8") as handle:
            handle.write("JEV_DEBUG=home\nTYPESAFE_BASE_URL=https://jev.example\n")

        env = load_env(cwd=cwd, home=home, env={"JEV_ALLOW_FABLE": "1"})

        self.assertEqual(env["JEV_API_KEY"], "from-project")
        self.assertEqual(env["JEV_OPUS_EFFORT"], "low")
        self.assertEqual(env["JEV_FORCE_EFFORT"], "max")
        self.assertEqual(env["JEV_SONNET_FORCE_EFFORT"], "low")
        self.assertEqual(env["JEV_DEBUG"], "project", "the project file still outranks the home file")
        self.assertEqual(env["TYPESAFE_BASE_URL"], "https://jev.example", "only the user's own file may move Jev")
        self.assertNotIn("ANTHROPIC_BASE_URL", env)
        self.assertNotIn("NODE_OPTIONS", env)
        self.assertNotIn("JEV_DUMP", env)
        self.assertEqual(env["JEV_ALLOW_FABLE"], "1", "the real environment wins")

        child = child_env({**env, "PATH": "/bin", "TYPESAFE_API_KEY": "k"})
        self.assertNotIn("JEV_API_KEY", child)
        self.assertNotIn("TYPESAFE_API_KEY", child)
        self.assertEqual(child["PATH"], "/bin")

    def test_a_claude_api_key_in_the_users_own_file_reaches_claude_code_but_never_from_a_projects_env(self):
        """a Claude API key in the user's own file reaches Claude Code, but never from a project's .env"""
        cwd = tempdir(self, "jev-env-cwd-")
        home = tempdir(self, "jev-env-home-")
        with open(os.path.join(cwd, ".env"), "w", encoding="utf-8") as handle:
            handle.write("ANTHROPIC_API_KEY=sk-ant-someone-else\n")
        with open(os.path.join(home, ".jev-router.env"), "w", encoding="utf-8") as handle:
            handle.write("JEV_API_KEY=jev\nANTHROPIC_API_KEY=sk-ant-mine\n")

        env = load_env(cwd=cwd, home=home, env={})
        self.assertEqual(env["ANTHROPIC_API_KEY"], "sk-ant-mine")
        child = child_env(env)
        self.assertEqual(child["ANTHROPIC_API_KEY"], "sk-ant-mine", "Claude Code needs it; only the Jev key is withheld")
        self.assertNotIn("JEV_API_KEY", child)

        from_project_only = load_env(cwd=cwd, home=tempdir(self, "jev-env-home-"), env={})
        self.assertNotIn("ANTHROPIC_API_KEY", from_project_only)

    def test_a_blank_key_in_a_copied_env_example_does_not_hide_the_real_one(self):
        """a blank key in a copied .env.example does not hide the real one"""
        cwd = tempdir(self, "jev-env-cwd-")
        home = tempdir(self, "jev-env-home-")
        shutil.copyfile(os.path.join(REPO_ROOT, ".env.example"), os.path.join(cwd, ".env"))
        with open(os.path.join(home, ".jev-router.env"), "w", encoding="utf-8") as handle:
            handle.write("JEV_API_KEY=from-home\n")
        env = load_env(cwd=cwd, home=home, env={})
        self.assertEqual(env["JEV_API_KEY"], "from-home")
        self.assertNotIn("JEV_ALLOW_FABLE", env, "commented-out settings stay unset")


def shim_dir(test, name, with_script=True):
    """A directory holding an npm-style `name.cmd` shim, its script, and a `.ps1` beside it."""
    directory = tempdir(test, "jev-shim-")
    script = os.path.join(directory, "node_modules", "pkg", "cli.js")
    os.makedirs(os.path.dirname(script))
    with open(script, "w", encoding="utf-8") as handle:
        handle.write("process.stdout.write(JSON.stringify(process.argv.slice(2)));\n")
    target = '"%dp0%\\node_modules\\pkg\\cli.js"' if with_script else f'"{script}"'
    with open(os.path.join(directory, f"{name}.cmd"), "w", encoding="utf-8", newline="") as handle:
        handle.write(f'@ECHO off\r\n"node"  {target} %*\r\n')
    with open(os.path.join(directory, f"{name}.ps1"), "w", encoding="utf-8") as handle:
        handle.write("#!/usr/bin/env pwsh\n")
    return directory, script


def run(spec, args):
    child = spawn_spec(spec, args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE)
    out, _ = child.communicate(timeout=60)
    return json.loads(out.decode("utf-8"))


class Launch(unittest.TestCase):
    def test_prefers_claudes_cmd_shim_over_its_ps1_and_runs_the_script_behind_it_directly(self):
        """prefers Claude's .cmd shim over its .ps1 and runs the script behind it directly"""
        node = resolve_command("node")
        if not node:
            self.skipTest("node is not on PATH")
        directory, script = shim_dir(self, "claude")
        file = resolve_command("claude", exts=[".exe", ".cmd", ".bat", ".ps1"], path=directory, win=True)
        self.assertEqual(file, os.path.join(directory, "claude.cmd"))
        self.assertEqual(shim_script(file), script)
        spec = launch_spec(file)
        self.assertEqual(spec, {"command": node, "prefix": [script]})
        self.assertEqual(run(spec, AWKWARD), AWKWARD)

    def test_a_powershell_shim_is_run_past_the_default_execution_policy(self):
        """a PowerShell shim is run past the default execution policy"""
        self.assertEqual(launch_spec("C:\\bin\\claude.ps1")["prefix"][:4], ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])

    @unittest.skipUnless(sys.platform == "win32", "cmd.exe exists only on Windows")
    def test_a_shim_with_no_script_to_run_directly_is_quoted_for_cmd_exe(self):
        """a shim with no script to run directly is quoted for cmd.exe"""
        directory, _ = shim_dir(self, "opaque", with_script=False)
        spec = launch_spec(os.path.join(directory, "opaque.cmd"))
        self.assertTrue(spec.get("shim"), "falls back to cmd.exe")
        self.assertEqual(run(spec, AWKWARD), AWKWARD)

    def test_cmd_quoting_escapes_metacharacters(self):
        """cmd quoting escapes metacharacters"""
        self.assertNotRegex(quote_for_cmd("a&b|c"), r"[^^][&|]", "every & and | is caret-escaped")
        self.assertNotRegex(quote_for_cmd("50%"), r"[^^]%")


class Dump(unittest.TestCase):
    def test_jev_dump_1_writes_owner_only_dumps_into_the_status_directory_never_over_each_other(self):
        """JEV_DUMP=1 writes owner-only dumps into the status directory, never over each other"""
        first = dump_body({"a": 1.0}, "1")
        second = dump_body({"a": 2.0}, "1")
        try:
            self.assertTrue(first.startswith(STATUS_DIR))
            self.assertNotEqual(first, second)
            with open(second, encoding="utf-8") as handle:
                self.assertEqual(json.load(handle), {"a": 2})
            if sys.platform != "win32":
                self.assertEqual(os.stat(first).st_mode & 0o777, 0o600)
        finally:
            for file in (first, second):
                os.unlink(file)


if __name__ == "__main__":
    unittest.main()
