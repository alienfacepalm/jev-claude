"""Port of node/test/hardening.test.mjs, plus the Python port's own stream and server hardening."""

import http.client
import http.server
import json
import os
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from collections.abc import Callable, Sequence
from typing import Any

from jev_router.config import id_of
from jev_router.env import child_env, load_env
from jev_router.jsstr import JsObject
from jev_router.launch import LaunchSpec, launch_spec, quote_for_cmd, resolve_command, shim_script, spawn_spec
from jev_router.proxy import Route, RouteRequest, start_proxy
from jev_router.status import STATUS_DIR, dump_body

from . import REPO_ROOT, SRC, present
from .support import QuietServer, Upstream, post, request

HAIKU = id_of("haiku")
OPUS = id_of("opus")
SONNET = id_of("sonnet")
PID = os.getpid()

TOOL_RESULT = {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]}
TOOL_USE = {"role": "assistant", "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {}}]}

# Values whose quoting the old shell path broke: embedded quotes, a space, and cmd metacharacters.
AWKWARD = ['name="Jev Router"', "fix a&b|c", "50% done", 'say "hi"', "plain"]

type Send = Callable[[object], tuple[int, dict[str, str], bytes]]


def sure(choice: str | None) -> Route:
    def route(args: RouteRequest) -> JsObject:
        return {"choice": choice, "confidence": 0.97, "ms": 1}

    return route


def metadata(session: str) -> dict[str, str]:
    return {"user_id": json.dumps({"session_id": session})}


def tempdir(test: unittest.TestCase, prefix: str) -> str:
    path = tempfile.mkdtemp(prefix=prefix)
    test.addCleanup(shutil.rmtree, path, True)
    return path


def routed_request() -> bytes:
    """A raw HTTP/1.1 POST of a request the proxy routes."""
    body = json.dumps(
        {"model": "jev-router", "tools": [{"name": "Bash"}], "messages": [{"role": "user", "content": "go"}]}
    ).encode()
    return (
        b"POST /v1/messages HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: "
        + str(len(body)).encode()
        + b"\r\n\r\n"
        + body
    )


def read_until(sock: socket.socket, marker: bytes) -> bytes:
    """Reads from a socket until `marker` arrives or the peer closes."""
    received = b""
    while marker not in received:
        chunk = sock.recv(4096)
        if not chunk:
            break
        received += chunk
    return received


class _StreamServer:
    """A raw upstream that answers every request with `behaviour(handler)`."""

    def __init__(self, behaviour: Callable[[http.server.BaseHTTPRequestHandler], None]) -> None:
        class Handler(http.server.BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"
            disable_nagle_algorithm = True

            def log_message(self, format: str, *args: object) -> None:  # noqa: A002 - the base class's name
                pass

            def do_POST(self) -> None:
                self.rfile.read(int(self.headers.get("content-length") or 0))
                behaviour(self)

        # The drop test ends connections on purpose; that is not an error worth printing.
        self.server = QuietServer(("127.0.0.1", 0), Handler)
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, kwargs={"poll_interval": 0.1}, daemon=True).start()

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()


class _SilentUpstream:
    """A raw-socket upstream that sends `head` and one SSE event, then stays silent.

    It records whether the proxy hung up on it (EOF) and when, or that it waited the full 5 s.
    """

    def __init__(self, head: bytes) -> None:
        self.listener = socket.create_server(("127.0.0.1", 0))
        self.url = f"http://127.0.0.1:{self.listener.getsockname()[1]}"
        self.hung_up_at: float | None = None
        self.timed_out = False
        self.done = threading.Event()
        self._head = head
        threading.Thread(target=self._serve, daemon=True).start()

    def _serve(self) -> None:
        connection, _ = self.listener.accept()
        with connection:
            request = read_until(connection, b"\r\n\r\n")
            head, _, body = request.partition(b"\r\n\r\n")
            length = next(
                (
                    int(line.split(b":", 1)[1])
                    for line in head.split(b"\r\n")
                    if line.lower().startswith(b"content-length:")
                ),
                0,
            )
            while len(body) < length:
                body += connection.recv(4096)
            connection.sendall(self._head + b'data: {"n":0}\n\n')
            connection.settimeout(5)
            try:
                while connection.recv(4096):
                    pass
                self.hung_up_at = time.monotonic()
            except TimeoutError:
                self.timed_out = True
            except OSError:
                self.hung_up_at = time.monotonic()  # a reset is a hang-up too
        self.done.set()

    def close(self) -> None:
        self.listener.close()


class Routing(unittest.TestCase):
    def proxy_for(self, route: Route) -> tuple[Upstream, Send]:
        upstream = Upstream()
        self.addCleanup(upstream.close)
        running = start_proxy(upstream_url=upstream.url, route=route)
        self.addCleanup(running.close)

        def send(body: object) -> tuple[int, dict[str, str], bytes]:
            return post(running.port, body)

        return upstream, send

    def test_the_main_thread_keeps_its_tier_after_a_session_has_run_50_sub_agents(self) -> None:
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

    def test_a_conversation_the_proxy_has_not_routed_yet_may_still_downgrade(self) -> None:
        """a conversation the proxy has not routed yet may still downgrade"""
        upstream, send = self.proxy_for(sure(HAIKU))
        history = "x" * 120_000
        send(
            {
                "model": "jev-router",
                "tools": [{"name": "Bash"}],
                "messages": [
                    {"role": "user", "content": history},
                    TOOL_USE,
                    TOOL_RESULT,
                    {"role": "user", "content": "fix the typo"},
                ],
            }
        )
        self.assertEqual(upstream.bodies()[0]["model"], HAIKU)

    def test_a_routing_failure_never_forwards_the_sentinel(self) -> None:
        """a routing failure never forwards the sentinel"""

        def blow_up(args: RouteRequest) -> object:
            raise RuntimeError("router blew up")

        upstream, send = self.proxy_for(blow_up)
        send({"model": "jev-router", "tools": [{"name": "Bash"}], "messages": [{"role": "user", "content": "hello"}]})
        self.assertEqual(
            upstream.bodies()[0]["model"], SONNET, "a failure lands on the default tier, never the sentinel"
        )

    def test_print_mode_keeps_the_conversations_tier_when_the_session_id_appears_later(self) -> None:
        """print mode keeps the conversation's tier when the session id appears later"""
        upstream, send = self.proxy_for(sure(HAIKU))
        opening = {"role": "user", "content": f"print-mode {PID}"}
        base = {"model": "jev-router", "tools": [{"name": "Bash"}]}
        send({**base, "messages": [opening]})
        send({**base, "metadata": metadata(f"late-{PID}"), "messages": [opening, TOOL_USE, TOOL_RESULT]})
        self.assertEqual(upstream.bodies()[0]["model"], HAIKU)
        self.assertEqual(upstream.bodies()[1]["model"], HAIKU)


class Streams(unittest.TestCase):
    def test_a_client_that_leaves_stops_the_upstream_response(self) -> None:
        """a client that leaves stops the upstream response"""
        gave_up = threading.Event()
        outcome: dict[str, bool] = {}

        def stream(handler: http.server.BaseHTTPRequestHandler) -> None:
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
        client.sendall(routed_request())
        read_until(client, b"data:")
        client.close()
        self.assertTrue(gave_up.wait(10), "the upstream stream ended")
        self.assertTrue(outcome["cut_short"], "the upstream stream was cut short rather than run to the end")

    def test_a_client_that_leaves_a_silent_connection_close_stream_hangs_up_on_the_upstream(self) -> None:
        """SPEC 7.2 step 7 for a reply framed by connection close, which http.client hands to the response.

        The upstream stays silent after its first event, so only the client watcher can end it.
        """
        heads = {
            "connection: close": b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
            "no framing headers": b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n",
        }
        for name, head in heads.items():
            with self.subTest(framing=name):
                upstream = _SilentUpstream(head)
                self.addCleanup(upstream.close)
                running = start_proxy(upstream_url=upstream.url, route=sure(HAIKU))
                self.addCleanup(running.close)

                client = socket.create_connection(("127.0.0.1", running.port))
                client.sendall(routed_request())
                self.assertIn(b"data:", read_until(client, b"data:"))
                client.close()
                left_at = time.monotonic()

                self.assertTrue(upstream.done.wait(10))
                self.assertFalse(upstream.timed_out, "the proxy never hung up on the upstream")
                self.assertLess(present(upstream.hung_up_at) - left_at, 1.5, "hung up soon after the client left")

    def test_an_upstream_that_drops_mid_stream_fails_the_client_instead_of_hanging_it(self) -> None:
        """an upstream that drops mid-stream fails the client instead of hanging it"""

        def drop(handler: http.server.BaseHTTPRequestHandler) -> None:
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
        body = json.dumps(
            {"model": "jev-router", "tools": [{"name": "Bash"}], "messages": [{"role": "user", "content": "go"}]}
        )
        connection.request("POST", "/v1/messages", body=body, headers={"content-type": "application/json"})
        response = connection.getresponse()
        self.assertEqual(response.status, 200)
        complete = True
        try:
            response.read()
        except TimeoutError:
            self.fail("client still waiting after 3s")
        except (http.client.IncompleteRead, OSError):
            complete = False
        self.assertFalse(complete, "the client sees an incomplete response, not a clean end")


class Server(unittest.TestCase):
    def test_a_client_that_resets_its_connection_prints_nothing_on_the_terminal(self) -> None:
        """The launcher's stderr is Claude Code's terminal: a client reset must not print a traceback."""
        upstream = Upstream()
        self.addCleanup(upstream.close)
        scratch = tempdir(self, "jev-reset-")
        env = {k: v for k, v in os.environ.items() if not k.startswith(("JEV_", "TYPESAFE_", "ANTHROPIC_", "CLAUDE_"))}
        env.update(
            {
                "PYTHONPATH": SRC,
                "ANTHROPIC_BASE_URL": upstream.url,
                "TYPESAFE_BASE_URL": upstream.url,
                "JEV_STATUS_DIR": os.path.join(scratch, "status"),
                "HOME": scratch,
                "USERPROFILE": scratch,
            }
        )
        child = subprocess.Popen(
            [sys.executable, "-m", "jev_router.cli.proxy_host"],
            cwd=scratch,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        try:
            line = present(child.stdout).readline().decode()
            self.assertRegex(line, r"^PORT=\d+")
            port = int(line.strip().split("=", 1)[1])

            client = socket.create_connection(("127.0.0.1", port))
            client.sendall(b"GET /x HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n")
            reply = read_until(client, b'"message"}')
            self.assertIn(b"200 OK", reply)
            time.sleep(0.2)  # the handler is back waiting for the next request on this connection
            client.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0))
            client.close()  # with a zero linger this is a reset, not a FIN

            status, _, _ = request(port, "GET", "/y")
            self.assertEqual(status, 200, "the proxy still serves after the reset")
        finally:
            child.terminate()
            _, stderr = child.communicate(timeout=30)
        self.assertEqual(stderr, b"", "nothing reached the terminal")


class Env(unittest.TestCase):
    def test_a_projects_env_may_only_set_jevs_own_keys(self) -> None:
        """a project's .env may only set jev's own keys"""
        cwd = tempdir(self, "jev-env-cwd-")
        home = tempdir(self, "jev-env-home-")
        with open(os.path.join(cwd, ".env"), "w", encoding="utf-8") as handle:
            handle.write(
                "\n".join(
                    [
                        "JEV_API_KEY=from-project",
                        "JEV_OPUS_EFFORT=low",
                        "JEV_FORCE_EFFORT=max",
                        "JEV_SONNET_FORCE_EFFORT=low",
                        "ANTHROPIC_BASE_URL=https://attacker.example",
                        "TYPESAFE_BASE_URL=https://attacker.example",
                        "NODE_OPTIONS=--require /tmp/evil.js",
                        "JEV_DUMP=/tmp/loot",
                        "JEV_DEBUG=project",
                    ]
                )
            )
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

    def test_a_claude_api_key_in_the_users_own_file_reaches_claude_code_but_never_from_a_projects_env(self) -> None:
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
        self.assertEqual(
            child["ANTHROPIC_API_KEY"], "sk-ant-mine", "Claude Code needs it; only the Jev key is withheld"
        )
        self.assertNotIn("JEV_API_KEY", child)

        from_project_only = load_env(cwd=cwd, home=tempdir(self, "jev-env-home-"), env={})
        self.assertNotIn("ANTHROPIC_API_KEY", from_project_only)

    def test_a_blank_key_in_a_copied_env_example_does_not_hide_the_real_one(self) -> None:
        """a blank key in a copied .env.example does not hide the real one"""
        cwd = tempdir(self, "jev-env-cwd-")
        home = tempdir(self, "jev-env-home-")
        shutil.copyfile(os.path.join(REPO_ROOT, ".env.example"), os.path.join(cwd, ".env"))
        with open(os.path.join(home, ".jev-router.env"), "w", encoding="utf-8") as handle:
            handle.write("JEV_API_KEY=from-home\n")
        env = load_env(cwd=cwd, home=home, env={})
        self.assertEqual(env["JEV_API_KEY"], "from-home")
        self.assertNotIn("JEV_ALLOW_FABLE", env, "commented-out settings stay unset")


def shim_dir(test: unittest.TestCase, name: str, with_script: bool = True) -> tuple[str, str]:
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


def run(spec: LaunchSpec, args: Sequence[str]) -> Any:
    child = spawn_spec(spec, args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE)
    out, _ = child.communicate(timeout=60)
    return json.loads(out.decode("utf-8"))


class Launch(unittest.TestCase):
    def test_prefers_claudes_cmd_shim_over_its_ps1_and_runs_the_script_behind_it_directly(self) -> None:
        """prefers Claude's .cmd shim over its .ps1 and runs the script behind it directly"""
        node = resolve_command("node")
        if not node:
            self.skipTest("node is not on PATH")
        directory, script = shim_dir(self, "claude")
        file = present(resolve_command("claude", exts=[".exe", ".cmd", ".bat", ".ps1"], path=directory, win=True))
        self.assertEqual(file, os.path.join(directory, "claude.cmd"))
        self.assertEqual(shim_script(file), script)
        spec = launch_spec(file)
        self.assertEqual(spec, {"command": node, "prefix": [script]})
        self.assertEqual(run(spec, AWKWARD), AWKWARD)

    def test_a_powershell_shim_is_run_past_the_default_execution_policy(self) -> None:
        """a PowerShell shim is run past the default execution policy"""
        self.assertEqual(
            launch_spec("C:\\bin\\claude.ps1")["prefix"][:4], ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]
        )

    def test_case_folding_in_file_extensions_is_ascii_only(self) -> None:
        """SPEC 3.1: `/\\.ps1$/i` does not match the long s (U+017F), which Unicode folds to "s"."""
        self.assertEqual(launch_spec("C:\\bin\\claude.p\u017f1"), {"command": "C:\\bin\\claude.p\u017f1", "prefix": []})
        self.assertEqual(launch_spec("C:\\bin\\claude.PS1")["command"], "powershell.exe")

    @unittest.skipUnless(sys.platform == "win32", "cmd.exe exists only on Windows")
    def test_a_shim_with_no_script_to_run_directly_is_quoted_for_cmd_exe(self) -> None:
        """a shim with no script to run directly is quoted for cmd.exe"""
        directory, _ = shim_dir(self, "opaque", with_script=False)
        spec = launch_spec(os.path.join(directory, "opaque.cmd"))
        self.assertTrue(spec.get("shim"), "falls back to cmd.exe")
        self.assertEqual(run(spec, AWKWARD), AWKWARD)

    def test_cmd_quoting_escapes_metacharacters(self) -> None:
        """cmd quoting escapes metacharacters"""
        self.assertNotRegex(quote_for_cmd("a&b|c"), r"[^^][&|]", "every & and | is caret-escaped")
        self.assertNotRegex(quote_for_cmd("50%"), r"[^^]%")


class Dump(unittest.TestCase):
    def test_jev_dump_1_writes_owner_only_dumps_into_the_status_directory_never_over_each_other(self) -> None:
        """JEV_DUMP=1 writes owner-only dumps into the status directory, never over each other"""
        first = present(dump_body({"a": 1.0}, "1"))
        second = present(dump_body({"a": 2.0}, "1"))
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
