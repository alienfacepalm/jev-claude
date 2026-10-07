"""The Jev client's wire behaviour (SPEC 5.1, 5.3) against a real loopback fake Jev.

Node has no unit test file for router.mjs (its SDK is tested upstream); these cover what the port
wrote in its place: headers, one retry, the per-attempt timeout and the overall deadline.
"""

import http.server
import json
import os
import re
import socket
import threading
import time
import unittest
from collections.abc import Sequence
from typing import Any
from unittest import mock

from jev_router import router
from jev_router.jsstr import JsObject

from . import as_dict, present
from .support import Recorded, Upstream

MODELS: list[JsObject] = [
    {"id": "claude-sonnet-5-5", "tier": "sonnet", "releasedAt": "", "description": "claude-sonnet-5-5"}
]
GOOD = {
    "answers": {
        "model": {"choice": "claude-sonnet-5-5", "confidence": 0.8},
        "task_complexity": {"score": 3},
        "reasoning_required": {"score": 4},
        "tool_complexity": {"score": 2},
    }
}

type Reply = tuple[int, bytes, float]


def _attempt_threads() -> list[threading.Thread]:
    return [t for t in threading.enumerate() if t.name == "jev-attempt" and t.is_alive()]


class JevClient(unittest.TestCase):
    def setUp(self) -> None:
        self.logged: list[str] = []
        silence = mock.patch.object(router, "log", self.logged.append)
        silence.start()
        self.addCleanup(silence.stop)
        router.reset_settings()
        self.addCleanup(router.reset_settings)

    def jev(self, replies: Sequence[Reply]) -> Upstream:
        """A fake Jev answering each POST with the next `(status, body, delay)` in `replies`."""
        pending = list(replies)

        def handle(record: Recorded, handler: http.server.BaseHTTPRequestHandler) -> None:
            status, body, delay = pending.pop(0) if pending else (500, b"{}", 0)
            if delay:
                time.sleep(delay)
            try:
                Upstream.reply(handler, status, body)
            except OSError:
                pass

        server = Upstream(handle)
        self.addCleanup(server.close)
        return server

    def ask(self, server: Upstream, **env: str) -> JsObject | None:
        return router.ask_jev(
            prompt="fix it",
            current="claude-sonnet-5-5",
            context_tokens=20000,
            models=MODELS,
            env={"JEV_API_KEY": "secret", "TYPESAFE_BASE_URL": server.url + "/", **env},
        )

    def test_sends_the_spec_headers_and_body_and_reads_the_answer(self) -> None:
        server = self.jev([(200, json.dumps(GOOD).encode(), 0)])
        result = present(self.ask(server))
        self.assertEqual(result["choice"], "claude-sonnet-5-5")
        self.assertEqual(
            result["metrics"],
            {"taskComplexity": 3 / 9, "reasoningRequired": 4 / 9, "toolComplexity": 2 / 9, "contextSize": 0.1},
        )
        self.assertIsInstance(result["ms"], int)
        [sent] = server.requests
        self.assertEqual(sent["url"], "/v1/systemone", "trailing slashes on the base are removed")
        self.assertEqual(sent["headers"]["authorization"], "Bearer secret")
        self.assertEqual(sent["headers"]["accept"], "application/json")
        self.assertEqual(sent["headers"]["content-type"], "application/json")
        self.assertRegex(sent["headers"]["user-agent"], r"^jev-router-python/\d+\.\d+\.\d+")
        self.assertNotIn("x-typesafe-retry-count", sent["headers"])
        self.assertNotIn("x-typesafe-sdk", sent["headers"])
        body = json.loads(sent["body"])
        self.assertEqual(list(body), ["state", "questions", "model"])
        self.assertEqual(body["model"], "jev-latest")

    def test_one_retry_on_a_5xx_carries_the_retry_count(self) -> None:
        server = self.jev([(503, b"{}", 0), (200, json.dumps(GOOD).encode(), 0)])
        self.assertEqual(present(self.ask(server))["choice"], "claude-sonnet-5-5")
        self.assertEqual(len(server.requests), 2)
        self.assertEqual(server.requests[1]["headers"]["x-typesafe-retry-count"], "1")

    def test_a_failure_after_the_retry_returns_none_and_logs_without_the_key(self) -> None:
        server = self.jev([(500, b"{}", 0), (500, b"{}", 0)])
        self.assertIsNone(self.ask(server))
        self.assertEqual(len(server.requests), 2)
        self.assertEqual(len(self.logged), 1)
        self.assertRegex(self.logged[0], r"^routing failed, keeping claude-sonnet-5-5: ")
        self.assertNotIn("secret", self.logged[0])

    def test_a_non_retryable_status_is_not_retried(self) -> None:
        server = self.jev([(401, b"{}", 0)])
        self.assertIsNone(self.ask(server))
        self.assertEqual(len(server.requests), 1)

    def test_a_slow_attempt_times_out_and_the_deadline_ends_the_call(self) -> None:
        server = self.jev([(200, json.dumps(GOOD).encode(), 5), (200, json.dumps(GOOD).encode(), 5)])
        started = time.monotonic()
        self.assertIsNone(self.ask(server))
        elapsed = time.monotonic() - started
        self.assertGreaterEqual(elapsed, 2.9, "a timed-out attempt is retried until the 3000 ms deadline")
        self.assertLess(elapsed, 3.6, "the deadline cuts the call off")
        self.assertEqual(len(server.requests), 2)

    def test_an_attempt_abandoned_while_connecting_never_sends_its_post(self) -> None:
        """SPEC 5.3: the timeout covers connect, and the deadline aborts whatever is in flight."""
        server = self.jev([(200, json.dumps(GOOD).encode(), 0)])
        real_connect = socket.create_connection

        # A connect that takes 2 s, as slow DNS or a lost SYN would; the socket and server are real.
        def slow_connect(*args: Any, **kwargs: Any) -> socket.socket:
            time.sleep(2)
            return real_connect(*args, **kwargs)

        with mock.patch.object(socket, "create_connection", slow_connect):
            started = time.monotonic()
            self.assertIsNone(self.ask(server))
            elapsed = time.monotonic() - started
            self.assertGreaterEqual(elapsed, 2.9, "both attempts were stuck connecting until the deadline")
            self.assertLess(elapsed, 3.6, "the deadline cuts the call off")
            # The second attempt's connect finishes about 0.8 s after the deadline; give it time.
            deadline = time.monotonic() + 4
            while _attempt_threads() and time.monotonic() < deadline:
                time.sleep(0.05)
        self.assertEqual(_attempt_threads(), [], "every abandoned attempt has finished")
        time.sleep(0.3)
        self.assertEqual(server.requests, [], "no POST was sent after the caller gave up")

    def test_settings_from_the_environment_are_read_once_per_process(self) -> None:
        """SPEC 20.3: the key, base URL and default model are fixed once a key has been seen."""
        first = self.jev([(200, json.dumps(GOOD).encode(), 0), (200, json.dumps(GOOD).encode(), 0)])
        second = self.jev([(200, json.dumps(GOOD).encode(), 0)])
        with mock.patch.dict(os.environ):
            os.environ.pop("TYPESAFE_API_KEY", None)
            os.environ.pop("JEV_API_KEY", None)
            self.assertIsNone(router.ask_jev(prompt="p", current="c", models=MODELS), "no key: no routing")
            self.assertEqual(first.requests, [])

            os.environ.update(
                {"JEV_API_KEY": "secret", "TYPESAFE_BASE_URL": first.url, "TYPESAFE_DEFAULT_MODEL": "model-a"}
            )
            self.assertIsNotNone(router.ask_jev(prompt="p", current="c", models=MODELS), "a key that appears is used")

            os.environ.update(
                {"JEV_API_KEY": "other", "TYPESAFE_BASE_URL": second.url, "TYPESAFE_DEFAULT_MODEL": "model-b"}
            )
            self.assertIsNotNone(router.ask_jev(prompt="p", current="c", models=MODELS))
        self.assertEqual(second.requests, [], "a later change to the environment is not picked up")
        self.assertEqual(len(first.requests), 2)
        later = first.requests[1]
        self.assertEqual(later["headers"]["authorization"], "Bearer secret")
        self.assertEqual(json.loads(later["body"])["model"], "model-a")

    def test_answers_missing_a_score_is_a_failure_but_a_missing_model_answer_is_not(self) -> None:
        missing_score = {
            "answers": {
                "model": {"choice": "x"},
                "task_complexity": {"score": 1},
                "reasoning_required": None,
                "tool_complexity": {"score": 1},
            }
        }
        self.assertIsNone(self.ask(self.jev([(200, json.dumps(missing_score).encode(), 0)])))
        no_model = {
            "answers": {"task_complexity": {"score": 1}, "reasoning_required": {"score": 1}, "tool_complexity": {}}
        }
        result = present(self.ask(self.jev([(200, json.dumps(no_model).encode(), 0)])))
        self.assertNotIn("choice", result)
        metrics = as_dict(result["metrics"])
        self.assertNotEqual(metrics["toolComplexity"], metrics["toolComplexity"], "NaN without a score")

    def test_no_key_at_all_is_a_failure_and_an_empty_key_is_still_sent(self) -> None:
        server = self.jev([(200, json.dumps(GOOD).encode(), 0)])
        self.assertIsNone(
            router.ask_jev(
                prompt="p", current="c", context_tokens=0, models=MODELS, env={"TYPESAFE_BASE_URL": server.url}
            )
        )
        self.assertEqual(server.requests, [])
        router.ask_jev(
            prompt="p",
            current="c",
            context_tokens=0,
            models=MODELS,
            env={"JEV_API_KEY": "", "TYPESAFE_API_KEY": "real", "TYPESAFE_BASE_URL": server.url},
        )
        self.assertTrue(re.fullmatch(r"Bearer ?", server.requests[0]["headers"]["authorization"]))

    def test_an_empty_menu_sends_nothing(self) -> None:
        self.assertIsNone(
            router.ask_jev(prompt="p", current="c", context_tokens=0, models=[], env={"JEV_API_KEY": "k"})
        )
        self.assertEqual(self.logged, [])

    def test_retry_after_headers(self) -> None:
        self.assertEqual(router.retry_after_ms({"retry-after-ms": "250"}), 250)
        self.assertEqual(router.retry_after_ms({"retry-after": "2"}), 2000)
        self.assertIsNone(router.retry_after_ms({"retry-after": "-1"}))
        self.assertEqual(router.retry_after_ms({"retry-after": "Thu, 01 Jan 1970 00:00:10 GMT"}, now_ms=4000), 6000)
        self.assertEqual(router.retry_after_ms({"retry-after-ms": "x", "retry-after": "1"}), 1000)
        self.assertEqual(
            router.retry_delay_ms(0, {"retry-after": "120"}) <= 150, True, "over 60 s falls back to backoff"
        )
        for _ in range(50):
            delay = router.retry_delay_ms(1)
            self.assertTrue(225 <= delay <= 300, delay)


if __name__ == "__main__":
    unittest.main()
