"""Every golden case in conformance/cases (SPEC 16.2), checked against this implementation.

The case files were generated from the Node functions themselves; the tagged encoding is
described in conformance/cases/README.md and decoded here before calling or comparing.
"""

import http.server
import math
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import unittest

from jev_router import config, env as envmod, explain, icons, jsjson, jsstr, launch, legend, model_names, policy
from jev_router import proxy, reasons, router, status, update, worktree
from jev_router.jsstr import UNDEFINED

from . import REPO_ROOT, SRC

CASES = os.path.join(REPO_ROOT, "conformance", "cases")


class _Throws:
    def __repr__(self):
        return "THROWS"


class _Clock:
    def __repr__(self):
        return "CLOCK"


THROWS = _Throws()
CLOCK = _Clock()


def decode(value):
    """Decode the tagged encoding at any depth."""
    if isinstance(value, list):
        return [decode(v) for v in value]
    if isinstance(value, dict):
        if len(value) == 1:
            (tag, inner), = value.items()
            if tag == "$undefined":
                return UNDEFINED
            if tag == "$number":
                return {"NaN": math.nan, "Infinity": math.inf, "-Infinity": -math.inf, "-0": -0.0}[inner]
            if tag == "$utf16":
                units = b"".join(int(u).to_bytes(2, "little") for u in inner)
                return units.decode("utf-16-le", "surrogatepass")
            if tag == "$hex":
                return bytes.fromhex(inner)
            if tag == "$throws":
                return THROWS
            if tag == "$clock":
                return CLOCK
        return {k: decode(v) for k, v in value.items()}
    return value


def load(name):
    with open(os.path.join(CASES, name), "rb") as handle:
        return [decode(case) for case in jsjson.parse(handle.read())]


def _is_num(v):
    return isinstance(v, (int, float)) and not isinstance(v, bool)


def mismatch(actual, expected, ordered=True, path="$"):
    """None when equal, else a description of the first difference."""
    if expected is CLOCK:
        return None if _is_num(actual) else f"{path}: expected a clock number, got {actual!r}"
    if expected is UNDEFINED or expected is None or isinstance(expected, (bool, str)):
        if actual is expected or (type(actual) is type(expected) and actual == expected):
            return None
        return f"{path}: expected {expected!r}, got {actual!r}"
    if _is_num(expected):
        if not _is_num(actual):
            return f"{path}: expected number {expected!r}, got {actual!r}"
        x, y = float(actual), float(expected)
        if x != x and y != y:
            return None
        if x == y and math.copysign(1, x) == math.copysign(1, y):
            return None
        return f"{path}: expected {expected!r}, got {actual!r}"
    if isinstance(expected, bytes):
        return None if actual == expected else f"{path}: expected {expected!r}, got {actual!r}"
    if isinstance(expected, list):
        if not isinstance(actual, list) or len(actual) != len(expected):
            return f"{path}: expected list {expected!r}, got {actual!r}"
        for i, (a, e) in enumerate(zip(actual, expected)):
            problem = mismatch(a, e, ordered, f"{path}[{i}]")
            if problem:
                return problem
        return None
    if isinstance(expected, dict):
        if not isinstance(actual, dict):
            return f"{path}: expected object {expected!r}, got {actual!r}"
        ek = [k for k in jsjson.js_keys(expected) if expected[k] is not UNDEFINED]
        ak = [k for k in jsjson.js_keys(actual) if actual[k] is not UNDEFINED]
        if (ek != ak) if ordered else (sorted(ek) != sorted(ak)):
            return f"{path}: expected keys {ek}, got {ak}"
        for k in ek:
            problem = mismatch(actual[k], expected[k], ordered, f"{path}.{k}")
            if problem:
                return problem
        return None
    return f"{path}: cannot compare {expected!r}"


def opt(inp, key, default=UNDEFINED):
    return inp[key] if key in inp else default


class Golden(unittest.TestCase):
    def check_file(self, name, call, ordered=True):
        cases = load(name)
        self.assertTrue(cases, name)
        failures = []
        for case in cases:
            expected = case["expected"]
            try:
                actual = call(case["input"])
            except Exception as error:  # noqa: BLE001
                if expected is THROWS:
                    continue
                failures.append(f"{case['name']}: raised {type(error).__name__}: {error}")
                continue
            if expected is THROWS:
                failures.append(f"{case['name']}: expected a throw, got {actual!r}")
                continue
            problem = mismatch(actual, expected, ordered)
            if problem:
                failures.append(f"{case['name']}: {problem}")
        if failures:
            self.fail(f"{name}: {len(failures)} of {len(cases)} failed\n  " + "\n  ".join(failures[:20]))

    def test_detect_override(self):
        self.check_file("detect-override.json", lambda i: policy.detect_override(opt(i, "prompt")))

    def test_decide(self):
        def call(i):
            kwargs = dict(prompt=opt(i, "prompt"), jev=opt(i, "jev", UNDEFINED), current=opt(i, "current"),
                          available=opt(i, "available", []))
            if "contextTokens" in i:
                kwargs["context_tokens"] = i["contextTokens"]
            return policy.decide(**kwargs)

        self.check_file("decide.json", call)

    def test_effort_floor(self):
        self.check_file("effort-floor.json", lambda i: config.effort_floor(i["name"], i["env"]))

    def test_forced_effort(self):
        self.check_file("forced-effort.json", lambda i: config.forced_effort(i["name"], i["env"]))

    def test_fable_allowed(self):
        def call(i):
            allowed = config.fable_allowed(i["env"])
            self.assertEqual("fable" in config.available_tiers(i["env"]), allowed)
            return allowed

        self.check_file("fable-allowed.json", call)

    def test_new_turn_prompt(self):
        self.check_file("new-turn-prompt.json", lambda i: proxy.new_turn_prompt(opt(i, "body")))

    def test_agent_label(self):
        self.check_file(
            "agent-label.json",
            lambda i: proxy.agent_label(opt(i, "body"), i["max"]) if "max" in i else proxy.agent_label(opt(i, "body")),
        )

    def test_apply_tier(self):
        def call(i):
            body = i["body"]
            proxy.apply_tier(body, i["tier"], opt(i, "model"), i.get("env", {}))
            return body

        self.check_file("apply-tier.json", call)

    def test_sanitize_schema(self):
        def call(i):
            node = opt(i, "node")
            proxy.sanitize_schema(node)
            return node

        self.check_file("sanitize-schema.json", call)

    def test_version_of(self):
        self.check_file("version-of.json", lambda i: proxy.version_of(i))

    def test_short_name(self):
        self.check_file("short-name.json", lambda i: model_names.short_name(opt(i, "model")))

    def test_claude_models(self):
        self.check_file("claude-models.json", lambda i: proxy.claude_models(i["catalog"]))

    def test_newest_per_tier(self):
        self.check_file("newest-per-tier.json", lambda i: proxy.newest_per_tier(i["models"]))

    def test_newer_than_calibrated(self):
        self.check_file("newer-than-calibrated.json", lambda i: proxy.newer_than_calibrated(i["catalog"]))

    def test_session_of(self):
        self.check_file("session-of.json", lambda i: proxy.session_of(opt(i, "body")))

    def test_conversation_key(self):
        self.check_file("conversation-key.json", lambda i: proxy.conversation_key(opt(i, "body")))

    def test_agent_of(self):
        def call(i):
            mains = proxy.JsMap()
            results = [proxy.agent_of(body, mains) for body in i["steps"]]
            return {"results": results, "mains": mains.entries()}

        self.check_file("agent-of.json", call)

    def test_write_decision(self):
        # Per conformance/cases/README.md: an empty status directory, with the once-per-process
        # prune (SPEC 8.1) already done before the first step, so it never runs during a sequence.
        # The clock is pinned as the generator pinned it; `$clock` values match any number.
        directory = tempfile.mkdtemp(prefix="jev-golden-wd-")
        self.addCleanup(shutil.rmtree, directory, True)
        saved = (status.DIR, status._pruned, status.now_ms)
        self.addCleanup(lambda: setattr(status, "DIR", saved[0]))
        self.addCleanup(lambda: setattr(status, "_pruned", saved[1]))
        self.addCleanup(lambda: setattr(status, "now_ms", saved[2]))
        status.DIR = directory
        status.prune_stale()
        status._pruned = True
        status.now_ms = lambda: 1_800_000_000_000

        def call(i):
            files = []
            for step in i["steps"]:
                agent = step.get("agent", None)
                if step["op"] == "writeDecision":
                    status.write_decision(step["id"], step["decision"], agent)
                else:
                    status.mark_manual(step["id"], step["model"], agent)
                files.append({"file": status.read_status(step["id"])})
            return files

        self.check_file("write-decision.json", call)

    def test_agent_view(self):
        self.check_file(
            "agent-view.json",
            lambda i: status.agent_view(opt(i, "status"), fresh_ms=i.get("freshMs", 90000), now=i["now"]),
        )

    def test_main_decision(self):
        self.check_file("main-decision.json", lambda i: status.main_decision(opt(i, "status")))

    def test_icons(self):
        self.check_file("icons.json", lambda i: icons.icons(i["env"], i["platform"]))

    def test_reasons(self):
        def call(i):
            reason = opt(i, "reason")
            return {"short": reasons.short_reason(reason), "long": reasons.long_reason(reason),
                    "noChange": reasons.is_no_change(reason)}

        self.check_file("reasons.json", call)

    def test_format_explanation(self):
        self.check_file("format-explanation.json", lambda i: explain.format_explanation(opt(i, "status")))

    def test_format_agents(self):
        self.check_file("format-agents.json", lambda i: explain.format_agents(opt(i, "status"), i["now"]))

    def test_format_legend(self):
        self.check_file("format-legend.json", lambda i: legend.format_legend(i["set"]))

    def test_location_info(self):
        def call(i):
            asked = []

            def branch_of(directory):
                asked.append(directory)
                return i.get("branch", UNDEFINED)

            return {"result": worktree.location_info(opt(i, "input"), branch_of), "asked": asked}

        self.check_file("location-info.json", call)

    def test_compare_versions(self):
        self.check_file("compare-versions.json", lambda i: update.compare_versions(opt(i, "a"), opt(i, "b")))

    def test_update_notice(self):
        self.check_file("update-notice.json", lambda i: update.update_notice(opt(i, "state"), opt(i, "currentVersion")))

    def test_is_check_due(self):
        self.check_file(
            "is-check-due.json",
            lambda i: update.is_check_due(opt(i, "state"), i["now"], i.get("everyMs", update.CHECK_EVERY_MS)),
        )

    def test_needs_install(self):
        self.check_file("needs-install.json", lambda i: update.needs_install(i["changedFiles"]))

    def test_quote_for_cmd(self):
        self.check_file("quote-for-cmd.json", lambda i: launch.quote_for_cmd(opt(i, "arg")))

    def test_parse_env(self):
        self.check_file("parse-env.json", lambda i: envmod.parse_env(jsstr.utf8(i["text"])), ordered=False)

    def test_stringify(self):
        def call(i):
            text = jsjson.stringify(opt(i, "value"), i.get("indent"))
            return {"text": text, "utf16Length": UNDEFINED if text is UNDEFINED else jsstr.u16_len(text)}

        self.check_file("stringify.json", call)

    def test_parse(self):
        def call(i):
            value = jsjson.parse(i["bytes"])
            return {"value": value, "text": jsjson.stringify(value)}

        self.check_file("parse.json", call)

    def test_math(self):
        ops = {
            "MathRound": lambda v: jsstr.math_round(jsstr.to_number(v)),
            "toFixed2": jsstr.to_fixed2,
            "ToNumber": jsstr.to_number,
            "roundPercent": lambda v: jsstr.math_round(jsstr.to_number(v) * 100),
            "toFixed2OfToNumber": lambda v: jsstr.to_fixed2(jsstr.to_number(v)),
        }
        self.check_file("math.json", lambda i: ops[i["op"]](opt(i, "value")))

    def test_jev_request(self):
        """The Jev request goes to /v1/systemone on a loopback fake Jev, and the result matches."""
        seen = []
        state = {"response": b""}

        class FakeJev(http.server.BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *args):
                pass

            def do_POST(self):
                body = self.rfile.read(int(self.headers.get("content-length") or 0))
                seen.append({"method": self.command, "path": self.path, "body": body, "headers": dict(self.headers.items())})
                payload = state["response"]
                self.send_response(200)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), FakeJev)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        fake_env = {"JEV_API_KEY": "k", "TYPESAFE_BASE_URL": f"http://127.0.0.1:{server.server_address[1]}"}
        self.assertEqual(router.base_url({}) + "/v1/systemone", "https://api.typesafe.ai/v1/systemone")

        quiet = router.log
        router.log = lambda line: None
        self.addCleanup(setattr, router, "log", quiet)

        def call(i):
            seen.clear()
            response = i["response"]
            state["response"] = response if isinstance(response, bytes) else jsstr.utf8(response)
            result = router.ask_jev(prompt=opt(i, "prompt"), current=opt(i, "current"),
                                    context_tokens=opt(i, "contextTokens"), models=i["models"], env=fake_env)
            if result is not None:
                self.assertIsInstance(result.pop("ms"), int)
            if not seen:
                return {"method": None, "body": None, "result": result}
            self.assertEqual(seen[0]["path"], "/v1/systemone")
            return {"method": seen[0]["method"], "body": seen[0]["body"].decode("utf-8"), "result": result}

        cases = load("jev-request.json")
        failures = []
        for case in cases:
            expected = dict(case["expected"])
            if expected.get("url") is not None:
                self.assertTrue(expected["url"].endswith("/v1/systemone"))
            expected.pop("url", None)
            try:
                actual = call(case["input"])
            except Exception as error:  # noqa: BLE001
                failures.append(f"{case['name']}: raised {error!r}")
                continue
            problem = mismatch(actual, expected)
            if problem:
                failures.append(f"{case['name']}: {problem}")
        if failures:
            self.fail(f"jev-request.json: {len(failures)} of {len(cases)} failed\n  " + "\n  ".join(failures[:20]))

    def test_status_line(self):
        """The real jev-statusline program, run as Claude Code runs it, byte for byte."""
        cases = load("status-line.json")
        failures = []
        base_env = {
            k: v for k, v in os.environ.items()
            if not k.startswith(("JEV_", "TYPESAFE_", "ANTHROPIC_", "CLAUDE_"))
        }
        for case in cases:
            i = case["input"]
            scratch = tempfile.mkdtemp(prefix="jev-golden-sl-")
            try:
                status_dir = os.path.join(scratch, "status")
                work = os.path.join(scratch, "work")
                home = os.path.join(scratch, "home")
                for d in (status_dir, work, home):
                    os.makedirs(d)
                if "status" in i:
                    with open(os.path.join(status_dir, i["statusFile"]), "wb") as handle:
                        handle.write(jsjson.dumps_bytes(i["status"]))
                if "statusText" in i:
                    with open(os.path.join(status_dir, i["statusFile"]), "wb") as handle:
                        handle.write(jsstr.utf8(i["statusText"]))
                if "calibration" in i:
                    with open(os.path.join(status_dir, "calibration.json"), "wb") as handle:
                        handle.write(jsjson.dumps_bytes(i["calibration"]))
                if "calibrationText" in i:
                    with open(os.path.join(status_dir, "calibration.json"), "wb") as handle:
                        handle.write(jsstr.utf8(i["calibrationText"]))
                env = dict(base_env)
                env.update({"JEV_STATUS_DIR": status_dir, "HOME": home, "USERPROFILE": home, "TEMP": home,
                            "TMP": home, "TMPDIR": home, "PYTHONPATH": SRC})
                if "icons" in i and i["icons"] is not UNDEFINED:
                    env["JEV_ICONS"] = i["icons"]
                out = subprocess.run([sys.executable, "-m", "jev_router.cli.statusline"], input=jsstr.utf8(i["stdin"]),
                                     capture_output=True, cwd=work, env=env, timeout=60)
                if out.returncode != 0 or out.stderr:
                    failures.append(f"{case['name']}: exit {out.returncode}, stderr {out.stderr[-300:]!r}")
                    continue
                expected = jsstr.utf8(case["expected"]["stdout"])
                if out.stdout != expected:
                    failures.append(f"{case['name']}:\n    expected {expected!r}\n    got      {out.stdout!r}")
            finally:
                shutil.rmtree(scratch, True)
        if failures:
            self.fail(f"status-line.json: {len(failures)} of {len(cases)} failed\n  " + "\n  ".join(failures[:20]))


if __name__ == "__main__":
    unittest.main()
