"""The Jev client's wire behaviour (SPEC 5.1, 5.3) against a real loopback fake Jev.

Node has no unit test file for router.mjs (its SDK is tested upstream); these cover what the port
wrote in its place: headers, one retry, the per-attempt timeout and the overall deadline.
"""

import json
import re
import threading
import time
import unittest

from jev_router import router

from .support import Upstream

MODELS = [{"id": "claude-sonnet-5-5", "tier": "sonnet", "releasedAt": "", "description": "claude-sonnet-5-5"}]
GOOD = {
    "answers": {
        "model": {"choice": "claude-sonnet-5-5", "confidence": 0.8},
        "task_complexity": {"score": 3},
        "reasoning_required": {"score": 4},
        "tool_complexity": {"score": 2},
    }
}


class JevClient(unittest.TestCase):
    def setUp(self):
        quiet = router.log
        self.logged = []
        router.log = self.logged.append
        self.addCleanup(setattr, router, "log", quiet)

    def jev(self, replies):
        """A fake Jev answering each POST with the next `(status, body, delay)` in `replies`."""
        pending = list(replies)

        def handle(record, handler):
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

    def ask(self, server, **env):
        return router.ask_jev(prompt="fix it", current="claude-sonnet-5-5", context_tokens=20000, models=MODELS,
                              env={"JEV_API_KEY": "secret", "TYPESAFE_BASE_URL": server.url + "/", **env})

    def test_sends_the_spec_headers_and_body_and_reads_the_answer(self):
        server = self.jev([(200, json.dumps(GOOD).encode(), 0)])
        result = self.ask(server)
        self.assertEqual(result["choice"], "claude-sonnet-5-5")
        self.assertEqual(result["metrics"], {"taskComplexity": 3 / 9, "reasoningRequired": 4 / 9, "toolComplexity": 2 / 9, "contextSize": 0.1})
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

    def test_one_retry_on_a_5xx_carries_the_retry_count(self):
        server = self.jev([(503, b"{}", 0), (200, json.dumps(GOOD).encode(), 0)])
        self.assertEqual(self.ask(server)["choice"], "claude-sonnet-5-5")
        self.assertEqual(len(server.requests), 2)
        self.assertEqual(server.requests[1]["headers"]["x-typesafe-retry-count"], "1")

    def test_a_failure_after_the_retry_returns_none_and_logs_without_the_key(self):
        server = self.jev([(500, b"{}", 0), (500, b"{}", 0)])
        self.assertIsNone(self.ask(server))
        self.assertEqual(len(server.requests), 2)
        self.assertEqual(len(self.logged), 1)
        self.assertRegex(self.logged[0], r"^routing failed, keeping claude-sonnet-5-5: ")
        self.assertNotIn("secret", self.logged[0])

    def test_a_non_retryable_status_is_not_retried(self):
        server = self.jev([(401, b"{}", 0)])
        self.assertIsNone(self.ask(server))
        self.assertEqual(len(server.requests), 1)

    def test_a_slow_attempt_times_out_and_the_deadline_ends_the_call(self):
        server = self.jev([(200, json.dumps(GOOD).encode(), 5), (200, json.dumps(GOOD).encode(), 5)])
        started = time.monotonic()
        self.assertIsNone(self.ask(server))
        elapsed = time.monotonic() - started
        self.assertGreaterEqual(elapsed, 2.9, "a timed-out attempt is retried until the 3000 ms deadline")
        self.assertLess(elapsed, 3.6, "the deadline cuts the call off")
        self.assertEqual(len(server.requests), 2)

    def test_answers_missing_a_score_is_a_failure_but_a_missing_model_answer_is_not(self):
        missing_score = {"answers": {"model": {"choice": "x"}, "task_complexity": {"score": 1}, "reasoning_required": None,
                                     "tool_complexity": {"score": 1}}}
        self.assertIsNone(self.ask(self.jev([(200, json.dumps(missing_score).encode(), 0)])))
        no_model = {"answers": {"task_complexity": {"score": 1}, "reasoning_required": {"score": 1}, "tool_complexity": {}}}
        result = self.ask(self.jev([(200, json.dumps(no_model).encode(), 0)]))
        self.assertNotIn("choice", result)
        self.assertNotEqual(result["metrics"]["toolComplexity"], result["metrics"]["toolComplexity"], "NaN without a score")

    def test_no_key_at_all_is_a_failure_and_an_empty_key_is_still_sent(self):
        server = self.jev([(200, json.dumps(GOOD).encode(), 0)])
        self.assertIsNone(router.ask_jev(prompt="p", current="c", context_tokens=0, models=MODELS,
                                         env={"TYPESAFE_BASE_URL": server.url}))
        self.assertEqual(server.requests, [])
        router.ask_jev(prompt="p", current="c", context_tokens=0, models=MODELS,
                       env={"JEV_API_KEY": "", "TYPESAFE_API_KEY": "real", "TYPESAFE_BASE_URL": server.url})
        self.assertTrue(re.fullmatch(r"Bearer ?", server.requests[0]["headers"]["authorization"]))

    def test_an_empty_menu_sends_nothing(self):
        self.assertIsNone(router.ask_jev(prompt="p", current="c", context_tokens=0, models=[], env={"JEV_API_KEY": "k"}))
        self.assertEqual(self.logged, [])

    def test_retry_after_headers(self):
        self.assertEqual(router.retry_after_ms({"retry-after-ms": "250"}), 250)
        self.assertEqual(router.retry_after_ms({"retry-after": "2"}), 2000)
        self.assertIsNone(router.retry_after_ms({"retry-after": "-1"}))
        self.assertEqual(router.retry_after_ms({"retry-after": "Thu, 01 Jan 1970 00:00:10 GMT"}, now_ms=4000), 6000)
        self.assertEqual(router.retry_after_ms({"retry-after-ms": "x", "retry-after": "1"}), 1000)
        self.assertEqual(router.retry_delay_ms(0, {"retry-after": "120"}) <= 150, True, "over 60 s falls back to backoff")
        for _ in range(50):
            delay = router.retry_delay_ms(1)
            self.assertTrue(225 <= delay <= 300, delay)


if __name__ == "__main__":
    unittest.main()
