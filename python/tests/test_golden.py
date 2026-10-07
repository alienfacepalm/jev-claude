"""Every golden case in conformance/cases (SPEC 16.2), checked against this implementation.

The case files were generated from the Node functions themselves; the tagged encoding is
described in conformance/cases/README.md and decoded here before calling or comparing. Each case
runs as its own subTest, so every failing case is reported, by name.
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
from collections.abc import Callable
from typing import Any
from unittest import mock

from jev_router import (
    config,
    explain,
    icons,
    jsjson,
    jsstr,
    launch,
    legend,
    model_names,
    policy,
    proxy,
    reasons,
    router,
    status,
    update,
    worktree,
)
from jev_router import env as envmod
from jev_router.jsstr import UNDEFINED

from . import REPO_ROOT, SRC, as_dict

CASES = os.path.join(REPO_ROOT, "conformance", "cases")

# A golden input or expectation: decoded JSON of any shape, as `json.loads` would type it.
type Case = dict[str, Any]


class _Throws:
    def __repr__(self) -> str:
        return "THROWS"


class _Clock:
    def __repr__(self) -> str:
        return "CLOCK"


THROWS = _Throws()
CLOCK = _Clock()


def decode(value: Any) -> Any:
    """Decode the tagged encoding at any depth."""
    if isinstance(value, list):
        return [decode(v) for v in value]
    if isinstance(value, dict):
        if len(value) == 1:
            ((tag, inner),) = value.items()
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


def load(name: str) -> list[Case]:
    with open(os.path.join(CASES, name), "rb") as handle:
        cases = jsjson.parse(handle.read())
    if not isinstance(cases, list):
        raise AssertionError(f"{name} is not a list of cases")
    return [as_dict(decode(case)) for case in cases]


def _is_num(v: object) -> bool:
    return isinstance(v, (int, float)) and not isinstance(v, bool)


def mismatch(actual: Any, expected: Any, ordered: bool = True, path: str = "$") -> str | None:
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
        if math.isnan(x) and math.isnan(y):
            return None
        if x == y and math.copysign(1, x) == math.copysign(1, y):
            return None
        return f"{path}: expected {expected!r}, got {actual!r}"
    if isinstance(expected, bytes):
        return None if actual == expected else f"{path}: expected {expected!r}, got {actual!r}"
    if isinstance(expected, list):
        if not isinstance(actual, list) or len(actual) != len(expected):
            return f"{path}: expected list {expected!r}, got {actual!r}"
        for i, (a, e) in enumerate(zip(actual, expected, strict=True)):
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


def opt(inp: Case, key: str, default: object = UNDEFINED) -> Any:
    """`inp[key]`, or `default` when the case leaves the argument out."""
    return inp.get(key, default)


class Golden(unittest.TestCase):
    def check_file(self, name: str, call: Callable[[Case], object], ordered: bool = True) -> None:
        cases = load(name)
        self.assertTrue(cases, name)
        for case in cases:
            with self.subTest(case=case["name"]):
                expected = case["expected"]
                if expected is THROWS:
                    # JavaScript throws a TypeError, a SyntaxError, ...: any exception is that throw.
                    with self.assertRaises(Exception):  # noqa: B017
                        call(case["input"])
                else:
                    self.assertIsNone(mismatch(call(case["input"]), expected, ordered))

    def test_detect_override(self) -> None:
        self.check_file("detect-override.json", lambda i: policy.detect_override(opt(i, "prompt")))

    def test_decide(self) -> None:
        def call(i: Case) -> object:
            return policy.decide(
                prompt=opt(i, "prompt"),
                jev=opt(i, "jev"),
                current=opt(i, "current"),
                available=opt(i, "available", []),
                context_tokens=opt(i, "contextTokens", 0),
            )

        self.check_file("decide.json", call)

    def test_effort_floor(self) -> None:
        self.check_file("effort-floor.json", lambda i: config.effort_floor(i["name"], i["env"]))

    def test_forced_effort(self) -> None:
        self.check_file("forced-effort.json", lambda i: config.forced_effort(i["name"], i["env"]))

    def test_fable_allowed(self) -> None:
        def call(i: Case) -> object:
            allowed = config.fable_allowed(i["env"])
            self.assertEqual("fable" in config.available_tiers(i["env"]), allowed)
            return allowed

        self.check_file("fable-allowed.json", call)

    def test_new_turn_prompt(self) -> None:
        self.check_file("new-turn-prompt.json", lambda i: proxy.new_turn_prompt(opt(i, "body")))

    def test_agent_label(self) -> None:
        self.check_file(
            "agent-label.json",
            lambda i: proxy.agent_label(opt(i, "body"), i["max"]) if "max" in i else proxy.agent_label(opt(i, "body")),
        )

    def test_apply_tier(self) -> None:
        def call(i: Case) -> object:
            body = i["body"]
            proxy.apply_tier(body, i["tier"], opt(i, "model"), i.get("env", {}))
            return body

        self.check_file("apply-tier.json", call)

    def test_sanitize_schema(self) -> None:
        def call(i: Case) -> object:
            node = opt(i, "node")
            proxy.sanitize_schema(node)
            return node

        self.check_file("sanitize-schema.json", call)

    def test_version_of(self) -> None:
        self.check_file("version-of.json", proxy.version_of)

    def test_short_name(self) -> None:
        self.check_file("short-name.json", lambda i: model_names.short_name(opt(i, "model")))

    def test_claude_models(self) -> None:
        self.check_file("claude-models.json", lambda i: proxy.claude_models(i["catalog"]))

    def test_newest_per_tier(self) -> None:
        self.check_file("newest-per-tier.json", lambda i: proxy.newest_per_tier(i["models"]))

    def test_newer_than_calibrated(self) -> None:
        self.check_file("newer-than-calibrated.json", lambda i: proxy.newer_than_calibrated(i["catalog"]))

    def test_session_of(self) -> None:
        self.check_file("session-of.json", lambda i: proxy.session_of(opt(i, "body")))

    def test_conversation_key(self) -> None:
        self.check_file("conversation-key.json", lambda i: proxy.conversation_key(opt(i, "body")))

    def test_agent_of(self) -> None:
        def call(i: Case) -> object:
            mains = proxy.JsMap()
            results = [proxy.agent_of(body, mains) for body in i["steps"]]
            return {"results": results, "mains": mains.entries()}

        self.check_file("agent-of.json", call)

    def test_write_decision(self) -> None:
        # Per conformance/cases/README.md: an empty status directory, with the once-per-process
        # prune (SPEC 8.1) already done before the first step, so it never runs during a sequence.
        # The clock is pinned as the generator pinned it; `$clock` values match any number.
        directory = tempfile.mkdtemp(prefix="jev-golden-wd-")
        self.addCleanup(shutil.rmtree, directory, True)
        for name, value in (("DIR", directory), ("now_ms", lambda: 1_800_000_000_000)):
            patcher = mock.patch.object(status, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)
        status.prune_stale()
        pruned = mock.patch.object(status, "_pruned", True)
        pruned.start()
        self.addCleanup(pruned.stop)

        def call(i: Case) -> object:
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

    def test_agent_view(self) -> None:
        self.check_file(
            "agent-view.json",
            lambda i: status.agent_view(opt(i, "status"), fresh_ms=i.get("freshMs", 90000), now=i["now"]),
        )

    def test_main_decision(self) -> None:
        self.check_file("main-decision.json", lambda i: status.main_decision(opt(i, "status")))

    def test_icons(self) -> None:
        self.check_file("icons.json", lambda i: icons.icons(i["env"], i["platform"]))

    def test_reasons(self) -> None:
        def call(i: Case) -> object:
            reason = opt(i, "reason")
            return {
                "short": reasons.short_reason(reason),
                "long": reasons.long_reason(reason),
                "noChange": reasons.is_no_change(reason),
            }

        self.check_file("reasons.json", call)

    def test_format_explanation(self) -> None:
        self.check_file("format-explanation.json", lambda i: explain.format_explanation(opt(i, "status")))

    def test_format_agents(self) -> None:
        self.check_file("format-agents.json", lambda i: explain.format_agents(opt(i, "status"), i["now"]))

    def test_format_legend(self) -> None:
        self.check_file("format-legend.json", lambda i: legend.format_legend(i["set"]))

    def test_location_info(self) -> None:
        def call(i: Case) -> object:
            asked: list[object] = []

            def branch_of(directory: object) -> object:
                asked.append(directory)
                return i.get("branch", UNDEFINED)

            return {"result": worktree.location_info(opt(i, "input"), branch_of), "asked": asked}

        self.check_file("location-info.json", call)

    def test_compare_versions(self) -> None:
        self.check_file("compare-versions.json", lambda i: update.compare_versions(opt(i, "a"), opt(i, "b")))

    def test_update_notice(self) -> None:
        self.check_file("update-notice.json", lambda i: update.update_notice(opt(i, "state"), opt(i, "currentVersion")))

    def test_is_check_due(self) -> None:
        self.check_file(
            "is-check-due.json",
            lambda i: update.is_check_due(opt(i, "state"), i["now"], i.get("everyMs", update.CHECK_EVERY_MS)),
        )

    def test_needs_install(self) -> None:
        self.check_file("needs-install.json", lambda i: update.needs_install(i["changedFiles"]))

    def test_quote_for_cmd(self) -> None:
        self.check_file("quote-for-cmd.json", lambda i: launch.quote_for_cmd(opt(i, "arg")))

    def test_parse_env(self) -> None:
        self.check_file("parse-env.json", lambda i: envmod.parse_env(jsstr.utf8(i["text"])), ordered=False)

    def test_stringify(self) -> None:
        def call(i: Case) -> object:
            text = jsjson.stringify(opt(i, "value"), i.get("indent"))
            return {"text": text, "utf16Length": jsstr.u16_len(text) if isinstance(text, str) else UNDEFINED}

        self.check_file("stringify.json", call)

    def test_parse(self) -> None:
        def call(i: Case) -> object:
            value = jsjson.parse(i["bytes"])
            return {"value": value, "text": jsjson.stringify(value)}

        self.check_file("parse.json", call)

    def test_math(self) -> None:
        ops: dict[str, Callable[[Any], object]] = {
            "MathRound": lambda v: jsstr.math_round(jsstr.to_number(v)),
            "toFixed2": jsstr.to_fixed2,
            "ToNumber": jsstr.to_number,
            "roundPercent": lambda v: jsstr.math_round(jsstr.to_number(v) * 100),
            "toFixed2OfToNumber": lambda v: jsstr.to_fixed2(jsstr.to_number(v)),
        }
        self.check_file("math.json", lambda i: ops[i["op"]](opt(i, "value")))

    def test_jev_request(self) -> None:
        """The Jev request goes to /v1/systemone on a loopback fake Jev, and the result matches."""
        seen: list[dict[str, Any]] = []
        state = {"response": b""}

        class FakeJev(http.server.BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, format: str, *args: object) -> None:  # noqa: A002 - the base class's name
                pass

            def do_POST(self) -> None:
                body = self.rfile.read(int(self.headers.get("content-length") or 0))
                seen.append(
                    {"method": self.command, "path": self.path, "body": body, "headers": dict(self.headers.items())}
                )
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

        quiet = mock.patch.object(router, "log", lambda line: None)
        quiet.start()
        self.addCleanup(quiet.stop)

        def call(i: Case) -> object:
            seen.clear()
            response = i["response"]
            state["response"] = response if isinstance(response, bytes) else jsstr.utf8(response)
            result = router.ask_jev(
                prompt=opt(i, "prompt"),
                current=opt(i, "current"),
                context_tokens=opt(i, "contextTokens"),
                models=i["models"],
                env=fake_env,
            )
            if result is not None:
                self.assertIsInstance(result.pop("ms"), int)
            if not seen:
                return {"method": None, "body": None, "result": result}
            self.assertEqual(seen[0]["path"], "/v1/systemone")
            return {"method": seen[0]["method"], "body": seen[0]["body"].decode("utf-8"), "result": result}

        for case in load("jev-request.json"):
            with self.subTest(case=case["name"]):
                expected = dict(case["expected"])
                if expected.get("url") is not None:
                    self.assertTrue(expected["url"].endswith("/v1/systemone"))
                expected.pop("url", None)
                self.assertIsNone(mismatch(call(case["input"]), expected))

    def test_status_line(self) -> None:
        """The real jev-statusline program, run as Claude Code runs it, byte for byte."""
        base_env = {
            k: v for k, v in os.environ.items() if not k.startswith(("JEV_", "TYPESAFE_", "ANTHROPIC_", "CLAUDE_"))
        }
        for case in load("status-line.json"):
            with self.subTest(case=case["name"]):
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
                    env.update(
                        {
                            "JEV_STATUS_DIR": status_dir,
                            "HOME": home,
                            "USERPROFILE": home,
                            "TEMP": home,
                            "TMP": home,
                            "TMPDIR": home,
                            "PYTHONPATH": SRC,
                        }
                    )
                    if "icons" in i and i["icons"] is not UNDEFINED:
                        env["JEV_ICONS"] = i["icons"]
                    out = subprocess.run(
                        [sys.executable, "-m", "jev_router.cli.statusline"],
                        input=jsstr.utf8(i["stdin"]),
                        capture_output=True,
                        cwd=work,
                        env=env,
                        timeout=60,
                        check=False,  # the exit code and stderr are asserted below
                    )
                    self.assertEqual(out.returncode, 0, out.stderr[-300:])
                    self.assertEqual(out.stderr, b"")
                    self.assertEqual(out.stdout, jsstr.utf8(case["expected"]["stdout"]))
                finally:
                    shutil.rmtree(scratch, True)


if __name__ == "__main__":
    unittest.main()
