"""Port of node/test/proxy-routing.test.mjs.

Driven by a request captured from the real Claude Code CLI (see the fixture's `_source`): print
mode sends no /v1/models probe, posts to `/v1/messages?beta=true`, carries adaptive thinking and a
thinking-clearing context edit, and ends in a `role: "system"` message. Assertions are on what the
upstream received, checked after the response.
"""

import copy
import http.server
import json
import os
import tempfile
import unittest
from collections.abc import Callable
from typing import Any
from unittest import mock

from jev_router import jsjson
from jev_router.jsstr import JsObject
from jev_router.proxy import Route, RouteRequest, start_proxy
from jev_router.status import read_status

from . import FIXTURES, as_dict
from .support import Recorded, Upstream, post

with open(os.path.join(FIXTURES, "claude-code-print-request.json"), "rb") as _handle:
    CAPTURED = as_dict(jsjson.parse(_handle.read()))
SESSION = json.loads(CAPTURED["body"]["metadata"]["user_id"])["session_id"]

type Send = Callable[[object], tuple[int, dict[str, str], bytes]]


def real_request() -> dict[str, Any]:
    return copy.deepcopy(as_dict(CAPTURED["body"]))


def continuation(body: dict[str, Any]) -> dict[str, Any]:
    """The same conversation one step later: Claude ran a tool and is sending back its result."""
    opening, *rest = body["messages"]
    return {
        **body,
        "messages": [
            opening,
            {
                "role": "assistant",
                "content": [{"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "ls"}}],
            },
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "utils.js"}]},
            *rest,
        ],
    }


def answer(choice: str) -> Route:
    def route(args: RouteRequest) -> JsObject:
        return {"choice": choice, "confidence": 0.92, "ms": 1}

    return route


class ProxyRouting(unittest.TestCase):
    def harness(self, route: Route) -> tuple[Upstream, list[str], Send]:
        """A proxy in front of an upstream that records what it got and, like the API, rejects the sentinel."""

        def reject_sentinel(record: Recorded, handler: http.server.BaseHTTPRequestHandler) -> None:
            body = json.loads(record["body"])
            if body.get("model") == "jev-router":
                Upstream.reply(
                    handler,
                    400,
                    b'{"type":"error","error":{"type":"invalid_request_error","message":"model: jev-router"}}',
                )
            else:
                Upstream.reply(handler, 200, b'{"id":"msg_1","type":"message"}')

        upstream = Upstream(reject_sentinel)
        self.addCleanup(upstream.close)
        prompts: list[str] = []

        def recording(args: RouteRequest) -> object:
            prompts.append(args["prompt"])
            return route(args)

        running = start_proxy(upstream_url=upstream.url, route=recording)
        self.addCleanup(running.close)

        def send(body: object) -> tuple[int, dict[str, str], bytes]:
            return post(running.port, body, path=CAPTURED["url"])

        return upstream, prompts, send

    def test_a_real_print_mode_request_is_routed_on_the_users_prompt(self) -> None:
        """a real print-mode request is routed on the user's prompt"""
        upstream, prompts, send = self.harness(answer("claude-haiku-4-5-20251001"))
        status, _, _ = send(real_request())
        self.assertEqual(status, 200)
        self.assertEqual(prompts, ["rename the variable x to count in utils.js"], "reminders stripped, prompt intact")
        sent = upstream.bodies()[0]
        self.assertEqual(upstream.requests[0]["url"], "/v1/messages?beta=true")
        self.assertEqual(sent["model"], "claude-haiku-4-5-20251001")
        self.assertNotIn("thinking", sent)
        self.assertNotIn("context_management", sent)
        self.assertNotIn("effort", sent.get("output_config", {}))
        self.assertEqual(
            as_dict(read_status(SESSION) or {}).get("tier"), "haiku", "the decision reaches the status line"
        )

    def test_an_unwritable_jev_dump_path_does_not_stop_the_turn_being_routed(self) -> None:
        """an unwritable JEV_DUMP path does not stop the turn being routed"""
        unwritable = os.path.join(tempfile.mkdtemp(), "jev-no-such-dir", "nested", "dump")
        dump = mock.patch.dict(os.environ, {"JEV_DUMP": unwritable})
        dump.start()
        self.addCleanup(dump.stop)
        upstream, _, send = self.harness(answer("claude-haiku-4-5-20251001"))
        status, _, _ = send(real_request())
        self.assertEqual(status, 200, "the API would reject the sentinel with a 400")
        self.assertEqual(upstream.bodies()[0]["model"], "claude-haiku-4-5-20251001", "routed as usual, dump or no dump")

    def test_tool_call_continuations_keep_the_tier_the_turn_was_routed_to(self) -> None:
        """tool-call continuations keep the tier the turn was routed to"""
        upstream, prompts, send = self.harness(answer("claude-sonnet-5-5"))
        opening = real_request()
        send(opening)
        send(continuation(opening))
        send(continuation(opening))
        self.assertEqual(len(prompts), 1, "Jev is asked once per turn, not once per tool call")
        self.assertEqual([b["model"] for b in upstream.bodies()], ["claude-sonnet-5-5"] * 3)

    def test_the_main_thread_keeps_its_tier_after_more_than_50_sub_agents_start(self) -> None:
        """the main thread keeps its tier after more than 50 sub-agents start"""

        def route(args: RouteRequest) -> JsObject:
            sub_agent = args["prompt"].startswith("Search the codebase")
            return {
                "choice": "claude-haiku-4-5-20251001" if sub_agent else "claude-sonnet-5-5",
                "confidence": 0.9,
                "ms": 1,
            }

        upstream, _, send = self.harness(route)
        main = real_request()
        send(main)
        for i in range(55):
            sub = real_request()
            sub["messages"][0]["content"][-1]["text"] = (
                f"Search the codebase for callers of handler {i} and report them"
            )
            send(sub)
        send(continuation(main))
        self.assertEqual(upstream.bodies()[-1]["model"], "claude-sonnet-5-5", "not reset to the opus default")


if __name__ == "__main__":
    unittest.main()
