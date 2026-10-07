"""Port of node/test/proxy.test.mjs."""

import copy
import http.server
import json
import os
import sys
import time
import unittest
from typing import Any

from jev_router import jsjson
from jev_router.config import effort_floor, forced_effort, is_auto, tier_of
from jev_router.jsstr import UNDEFINED, JsObject, JsValue
from jev_router.proxy import (
    JsMap,
    Route,
    RouteRequest,
    RunningProxy,
    agent_label,
    agent_of,
    apply_tier,
    claude_models,
    conversation_key,
    new_turn_prompt,
    newer_than_calibrated,
    newest_per_tier,
    sanitize_schema,
    session_of,
    start_proxy,
)
from jev_router.status import (
    STATUS_DIR,
    agent_view,
    mark_manual,
    prune_stale,
    read_calibration,
    read_status,
    write_calibration,
    write_decision,
    write_status,
)

from . import FIXTURES, as_dict
from .support import Handle, Recorded, Upstream, post, request

PID = os.getpid()

with open(os.path.join(FIXTURES, "claude-code-print-request.json"), "rb") as _handle:
    CAPTURED_BODY = as_dict(as_dict(jsjson.parse(_handle.read()))["body"])


def with_tools(messages: list[JsValue]) -> JsObject:
    return {"tools": [{"name": "Bash"}], "messages": messages}


def remove(path: str) -> None:
    try:
        os.unlink(path)
    except OSError:
        pass


class Sentinel(unittest.TestCase):
    def test_only_the_sentinel_model_is_routed(self) -> None:
        """only the sentinel model is routed"""
        self.assertTrue(is_auto("jev-router"))
        self.assertFalse(is_auto("claude-opus-4-6"), "a model the user picked is theirs")
        self.assertFalse(is_auto("claude-haiku-4-5-20251001"), "internal Haiku calls pass through")
        self.assertFalse(is_auto(UNDEFINED))

    def test_the_sentinel_is_not_mistaken_for_a_real_tier(self) -> None:
        """the sentinel is not mistaken for a real tier"""
        self.assertIsNone(tier_of("jev-router"))


class StatusStore(unittest.TestCase):
    def test_reads_the_session_id_out_of_claude_codes_metadata(self) -> None:
        """reads the session id out of Claude Code's metadata"""
        sid = "11111111-2222-4333-8444-555555555555"
        self.assertEqual(session_of({"metadata": {"user_id": json.dumps({"session_id": sid})}}), sid)
        self.assertEqual(session_of({"metadata": {"user_id": "not-json"}}), "")
        self.assertEqual(session_of({}), "")

    def test_status_round_trips_per_session_and_misses_cleanly(self) -> None:
        """status round-trips per session and misses cleanly"""
        sid = f"test-{PID}"
        write_status(sid, {"tier": "opus", "confidence": 0.87, "reason": "jev"})
        self.assertEqual(read_status(sid), {"tier": "opus", "confidence": 0.87, "reason": "jev"})
        self.assertIsNone(read_status("no-such-session"))
        write_status("", {"tier": "opus"})

    @unittest.skipIf(sys.platform == "win32", "modes are no-ops on Windows")
    def test_status_files_are_private_to_their_owner(self) -> None:
        """status files are private to their owner"""
        sid = f"perm-{PID}"
        write_status(sid, {"tier": "opus"})
        self.assertEqual(os.stat(STATUS_DIR).st_mode & 0o777, 0o700)
        self.assertEqual(os.stat(os.path.join(STATUS_DIR, f"{sid}.json")).st_mode & 0o777, 0o600)

    def test_stale_status_files_are_pruned_and_fresh_ones_kept(self) -> None:
        """stale status files are pruned and fresh ones kept"""
        os.makedirs(STATUS_DIR, exist_ok=True)
        stale = os.path.join(STATUS_DIR, f"stale-{PID}.json")
        fresh = os.path.join(STATUS_DIR, f"fresh-{PID}.json")
        for file in (stale, fresh):
            with open(file, "w") as handle:
                handle.write("{}")
        old = time.time() - 8 * 24 * 60 * 60
        os.utime(stale, (old, old))
        self.assertGreaterEqual(prune_stale(), 1)
        self.assertFalse(os.path.exists(stale))
        self.assertTrue(os.path.exists(fresh))

    def test_routing_status_retains_the_exact_recent_jev_exchanges(self) -> None:
        """routing status retains the exact recent Jev exchanges"""
        sid = f"history-{PID}"
        write_decision(sid, {"prompt": "first", "jev": {"request": {"id": 1}, "response": {"confidence": 0.6}}})
        write_decision(sid, {"prompt": "second", "jev": {"request": {"id": 2}, "response": {"confidence": 0.8}}})
        status = as_dict(read_status(sid))
        self.assertEqual(status["prompt"], "second")
        self.assertEqual([h["prompt"] for h in status["history"]], ["first", "second"])
        self.assertEqual(status["history"][0]["jev"]["response"]["confidence"], 0.6)

    def test_each_agents_model_is_recorded_separately_within_one_session(self) -> None:
        """each agent's model is recorded separately within one session"""
        sid = f"agents-{PID}"
        main = {"key": "k-main", "label": "fix the race", "main": True}
        sub = {"key": "k-sub", "label": "grep for callers", "main": False}
        write_decision(sid, {"tier": "opus", "model": "claude-opus-5-5", "confidence": 0.94, "at": 1000}, main)
        write_decision(sid, {"tier": "haiku", "model": "claude-haiku-4-5", "confidence": 0.81, "at": 2000}, sub)
        view = agent_view(read_status(sid), now=2000)
        self.assertEqual(
            as_dict(view["main"])["tier"], "opus", "a sub-agent's choice does not overwrite the main thread"
        )
        self.assertEqual(as_dict(view["main"])["label"], "fix the race")
        self.assertEqual(len(view["subagents"]), 1)
        self.assertEqual(view["subagents"][0]["tier"], "haiku")
        self.assertEqual(len(as_dict(read_status(sid))["history"]), 2, "history still records every decision")

    def test_stale_sub_agents_drop_out_of_the_live_view_but_the_main_thread_stays(self) -> None:
        """stale sub-agents drop out of the live view but the main thread stays"""
        sid = f"stale-agents-{PID}"
        write_decision(sid, {"tier": "opus", "at": 0}, {"key": "m", "label": "main", "main": True})
        write_decision(sid, {"tier": "haiku", "at": 0}, {"key": "s", "label": "old sub", "main": False})
        view = agent_view(read_status(sid), now=10 * 60_000)
        self.assertEqual(as_dict(view["main"])["tier"], "opus")
        self.assertEqual(view["subagents"], [], "a sub-agent that has not been routed recently is not live")

    def test_a_sub_agent_pinned_to_its_own_model_does_not_pause_the_session(self) -> None:
        """a sub-agent pinned to its own model does not pause the session"""
        sid = f"manual-agents-{PID}"
        write_decision(
            sid, {"tier": "opus", "model": "claude-opus-5-5", "at": 1000}, {"key": "m", "label": "main", "main": True}
        )
        mark_manual(sid, "claude-haiku-4-5", {"key": "s", "label": "pinned sub", "main": False})
        status = as_dict(read_status(sid))
        self.assertFalse(status["manual"], "only the main thread's choice pauses routing")
        self.assertEqual(
            as_dict(agent_view(status)["main"])["tier"], "opus", "the main decision survives a sub-agent's write"
        )
        self.assertTrue(agent_view(status)["subagents"][0]["manual"])
        mark_manual(sid, "claude-sonnet-5", {"key": "m", "label": "main", "main": True})
        self.assertTrue(as_dict(read_status(sid))["manual"], "the main thread picking a model does pause it")

    def test_the_calibration_notice_round_trips_and_reads_empty_when_absent(self) -> None:
        """the calibration notice round-trips and reads empty when absent"""
        file = os.path.join(STATUS_DIR, f"calibration-test-{PID}.json")
        self.addCleanup(remove, file)
        self.assertEqual(read_calibration(file), {"newer": [], "models": [], "at": None})
        write_calibration(newer=["claude-opus-6"], models=["claude-opus-6", "claude-sonnet-5-5"], file=file)
        read = read_calibration(file)
        self.assertEqual(read["newer"], ["claude-opus-6"])
        self.assertEqual(read["models"], ["claude-opus-6", "claude-sonnet-5-5"])
        self.assertIsInstance(read["at"], float)
        write_calibration(file=file)
        self.assertEqual(read_calibration(file)["newer"], [])


class Catalog(unittest.TestCase):
    def test_recognises_older_model_versions_within_a_tier(self) -> None:
        """recognises older model versions within a tier"""
        self.assertEqual(tier_of("claude-sonnet-4-6"), "sonnet")
        self.assertEqual(tier_of("claude-sonnet-5"), "sonnet")
        self.assertEqual(tier_of("claude-haiku-4-5-20251001"), "haiku")
        self.assertEqual(tier_of("claude-opus-4-1"), "opus")
        self.assertEqual(tier_of("claude-fable-5-1[1m]"), "fable")
        self.assertIsNone(tier_of("mystery-9"))
        self.assertIsNone(tier_of(UNDEFINED))

    def test_keeps_available_claude_model_versions_as_separate_jev_choices(self) -> None:
        """keeps available Claude model versions as separate Jev choices"""
        models = claude_models(
            [
                {"id": "claude-opus-5-5", "display_name": "Claude Opus 5"},
                {"id": "claude-opus-4-8", "display_name": "Claude Opus 4.8"},
            ]
        )
        self.assertEqual(
            [{"id": m["id"], "tier": m["tier"]} for m in models],
            [{"id": "claude-opus-5-5", "tier": "opus"}, {"id": "claude-opus-4-8", "tier": "opus"}],
        )

    def test_a_new_major_version_is_picked_as_the_newest_of_its_tier_dated_or_not(self) -> None:
        """a new major version is picked as the newest of its tier, dated or not"""

        def ids(catalog: list[JsObject]) -> list[JsValue]:
            return [m["id"] for m in newest_per_tier(claude_models(catalog))]

        self.assertEqual(
            ids([{"id": "claude-opus-5-5"}, {"id": "claude-opus-6"}, {"id": "claude-opus-4-8"}]), ["claude-opus-6"]
        )
        self.assertEqual(ids([{"id": "claude-sonnet-5-5"}, {"id": "claude-sonnet-5-10"}]), ["claude-sonnet-5-10"])
        self.assertEqual(
            ids([{"id": "claude-haiku-4-5-20251001"}, {"id": "claude-haiku-4-6"}]),
            ["claude-haiku-4-6"],
            "a date suffix is not a minor version",
        )

    def test_an_id_in_the_old_version_first_naming_never_outranks_a_current_model(self) -> None:
        """an id in the old version-first naming never outranks a current model"""
        catalog = [
            {"id": "claude-3-7-sonnet-20250219"},
            {"id": "claude-sonnet-5-5"},
            {"id": "claude-3-5-haiku-20241022"},
            {"id": "claude-haiku-4-5-20251001"},
        ]
        self.assertEqual(
            [m["id"] for m in newest_per_tier(claude_models(catalog))],
            ["claude-sonnet-5-5", "claude-haiku-4-5-20251001"],
        )
        self.assertEqual(newer_than_calibrated(catalog), [], "a retired model is not news")

    def test_a_provider_prefix_does_not_hide_the_version(self) -> None:
        """a provider prefix does not hide the version"""
        models = newest_per_tier(
            claude_models([{"id": "anthropic.claude-opus-5-5"}, {"id": "anthropic.claude-opus-6"}])
        )
        self.assertEqual([m["id"] for m in models], ["anthropic.claude-opus-6"])

    def test_flags_a_model_newer_than_the_router_was_calibrated_for_and_nothing_else(self) -> None:
        """flags a model newer than the router was calibrated for, and nothing else"""
        self.assertEqual(newer_than_calibrated([]), [], "no catalog yet: nothing to report")
        self.assertEqual(
            newer_than_calibrated([{"id": "claude-opus-5-5"}, {"id": "claude-sonnet-5-5"}, {"id": "claude-opus-4-8"}]),
            [],
            "the calibrated versions and older ones are not news",
        )
        self.assertEqual(
            newer_than_calibrated([{"id": "claude-opus-6"}, {"id": "claude-opus-5-5"}, {"id": "claude-sonnet-5-5"}]),
            ["claude-opus-6"],
        )


class ProxyServer(unittest.TestCase):
    def upstream(self, handle: Handle | None = None) -> Upstream:
        server = Upstream(handle)
        self.addCleanup(server.close)
        return server

    def proxy(self, upstream_url: str, route: Route | None = None, calibration_file: str | None = None) -> RunningProxy:
        running = start_proxy(upstream_url=upstream_url, route=route, calibration_file=calibration_file)
        self.addCleanup(running.close)
        return running

    def test_a_claude_api_key_reaches_anthropic_untouched_on_routed_and_manual_requests_alike(self) -> None:
        """a Claude API key reaches Anthropic untouched, on routed and manual requests alike"""

        def answer(record: Recorded, handler: http.server.BaseHTTPRequestHandler) -> None:
            body = b'{"data":[]}' if record["url"].startswith("/v1/models") else b'{"id":"msg_1","type":"message"}'
            Upstream.reply(handler, 200, body)

        upstream = self.upstream(answer)
        calibration = os.path.join(STATUS_DIR, f"calibration-key-test-{PID}.json")
        self.addCleanup(remove, calibration)
        running = self.proxy(
            upstream_url=upstream.url,
            calibration_file=calibration,
            route=lambda args: {"choice": "claude-sonnet-5-5", "confidence": 0.9, "ms": 1},
        )
        headers = {"x-api-key": "sk-ant-api03-test"}
        request(running.port, "GET", "/v1/models", headers=headers)
        for model in ["jev-router", "claude-opus-5-5"]:
            with self.subTest(model=model):
                status, _, _ = post(
                    running.port,
                    {"model": model, "tools": [{"name": "Bash"}], "messages": [{"role": "user", "content": "hi"}]},
                    headers=headers,
                )
                self.assertEqual(status, 200)
        post(
            running.port,
            {"model": "claude-opus-5-5", "messages": [{"role": "user", "content": "hi"}]},
            headers={"authorization": "Bearer gateway-token"},
        )
        self.assertEqual(
            [r["headers"].get("x-api-key") for r in upstream.requests],
            ["sk-ant-api03-test", "sk-ant-api03-test", "sk-ant-api03-test", None],
        )
        self.assertEqual(upstream.requests[3]["headers"]["authorization"], "Bearer gateway-token")

    def test_claude_proxy_sends_exact_account_models_to_jev_and_routes_the_chosen_version(self) -> None:
        """Claude proxy sends exact account models to Jev and routes the chosen version"""

        def answer(record: Recorded, handler: http.server.BaseHTTPRequestHandler) -> None:
            if record["url"].startswith("/v1/models"):
                Upstream.reply(
                    handler,
                    200,
                    json.dumps(
                        {
                            "data": [
                                {
                                    "id": "claude-opus-4-8",
                                    "display_name": "Claude Opus 4.8",
                                    "created_at": "2026-01-05",
                                },
                                {
                                    "id": "claude-opus-5-5",
                                    "display_name": "Claude Opus 5.5",
                                    "created_at": "2026-09-02",
                                },
                                {
                                    "id": "claude-sonnet-5-5",
                                    "display_name": "Claude Sonnet 5.5",
                                    "created_at": "2026-08-11",
                                },
                            ]
                        }
                    ).encode(),
                )
            else:
                Upstream.reply(handler, 200, b'{"id":"msg_1","type":"message","model":"claude-opus-5-5"}')

        upstream = self.upstream(answer)
        calibration = os.path.join(STATUS_DIR, f"calibration-proxy-test-{PID}.json")
        self.addCleanup(remove, calibration)
        menus: list[list[JsValue]] = []

        def route(args: RouteRequest) -> JsObject:
            menus.append([m["id"] for m in args["models"]])
            return {"choice": "claude-opus-5-5", "confidence": 0.91, "ms": 1}

        running = self.proxy(upstream_url=upstream.url, route=route, calibration_file=calibration)
        status, _, body = request(running.port, "GET", "/v1/models")
        self.assertEqual(status, 200)
        self.assertEqual(len(json.loads(body)["data"]), 3)
        recorded = read_calibration(calibration)
        self.assertEqual(recorded["models"], ["claude-opus-5-5", "claude-sonnet-5-5"], "the account's newest per tier")
        self.assertEqual(recorded["newer"], [], "nothing newer than the router was tuned for")
        post(
            running.port,
            {
                "model": "jev-router",
                "tools": [{"name": "Bash"}],
                "messages": [{"role": "user", "content": "debug this race"}],
            },
        )
        self.assertEqual(
            menus, [["claude-opus-5-5", "claude-sonnet-5-5"]], "only the newest of each tier is on the menu"
        )
        self.assertEqual(upstream.bodies()[1]["model"], "claude-opus-5-5")

    def test_a_turn_with_tools_but_no_model_is_recorded_as_manual_and_forwarded_re_serialised(self) -> None:
        """(Python port) a turn with tools but no model is marked manual and forwarded re-serialised, as Node does"""
        upstream = self.upstream()
        running = self.proxy(upstream_url=upstream.url, route=lambda args: None)
        sid = f"no-model-{PID}"
        raw = (
            b'{"tools":[{"name":"Bash"}],"metadata":{"user_id":"{\\"session_id\\":\\"'
            + sid.encode()
            + b'\\"}"},"messages":[{"role":"user","content":"hi"}],  "n": 1.0}'
        )
        post(running.port, raw)
        self.assertEqual(upstream.requests[0]["body"], raw.replace(b',  "n": 1.0', b',"n":1'))
        status = as_dict(read_status(sid))
        [entry] = status["agents"].values()
        self.assertTrue(entry["manual"])
        self.assertNotIn("model", entry, "an undefined model is omitted, as JSON.stringify omits it")
        self.assertTrue(status["manual"])

    def test_a_routed_request_without_metadata_is_recorded_under_the_conversation_key(self) -> None:
        """a routed request without metadata is recorded under the conversation key"""
        upstream = self.upstream(
            lambda r, h: Upstream.reply(h, 200, b'{"id":"msg_1","type":"message","model":"claude-sonnet-5-5"}')
        )
        running = self.proxy(
            upstream_url=upstream.url, route=lambda args: {"choice": "claude-sonnet-5-5", "confidence": 0.77, "ms": 1}
        )
        body: dict[str, Any] = {
            "model": "jev-router",
            "tools": [{"name": "Bash"}],
            "messages": [{"role": "user", "content": f"rename this variable {PID}"}],
        }
        post(running.port, body)
        self.assertEqual(session_of(body), "", "the request carries no session id")
        status = as_dict(read_status(conversation_key(body)))
        self.assertTrue(status, "the decision is filed under the conversation key instead of being dropped")
        self.assertEqual(status["tier"], "sonnet")
        self.assertEqual(status["confidence"], 0.77)
        self.assertEqual(status["effort"], "high", "records the effort that went out, here Sonnet's own")
        self.assertEqual(as_dict(agent_view(status)["main"])["effort"], "high", "and carries it to the per-agent entry")


class Schemas(unittest.TestCase):
    def test_converts_a_draft_04_boolean_exclusive_minimum_into_a_draft_2020_12_number(self) -> None:
        """converts a draft-04 boolean exclusiveMinimum into a draft 2020-12 number"""
        schema: dict[str, Any] = {"type": "object", "properties": {"topN": {"minimum": 0.0, "exclusiveMinimum": True}}}
        sanitize_schema(schema)
        self.assertEqual(schema["properties"]["topN"], {"exclusiveMinimum": 0.0})

    def test_drops_a_false_exclusive_maximum_and_keeps_the_bound(self) -> None:
        """drops a false exclusiveMaximum and keeps the bound"""
        schema: dict[str, Any] = {"properties": {"n": {"maximum": 10.0, "exclusiveMaximum": False}}}
        sanitize_schema(schema)
        self.assertEqual(schema["properties"]["n"], {"maximum": 10.0})

    def test_leaves_an_already_valid_numeric_bound_alone(self) -> None:
        """leaves an already-valid numeric bound alone"""
        schema: dict[str, Any] = {"properties": {"n": {"exclusiveMinimum": 5.0}}}
        sanitize_schema(schema)
        self.assertEqual(schema["properties"]["n"]["exclusiveMinimum"], 5.0)

    def test_reaches_schemas_nested_in_arrays_and_sub_objects(self) -> None:
        """reaches schemas nested in arrays and sub-objects"""
        schema: dict[str, Any] = {"anyOf": [{"items": {"minimum": 1.0, "exclusiveMinimum": True}}]}
        sanitize_schema(schema)
        self.assertEqual(schema["anyOf"][0]["items"], {"exclusiveMinimum": 1.0})

    def test_survives_null_and_primitive_nodes(self) -> None:
        """survives null and primitive nodes"""
        sanitize_schema(None)
        node = {"a": None, "b": 3.0, "c": "x"}
        sanitize_schema(node)
        self.assertEqual(node, {"a": None, "b": 3.0, "c": "x"})


class NewTurn(unittest.TestCase):
    def test_reads_a_plain_string_prompt_as_a_new_turn(self) -> None:
        """reads a plain string prompt as a new turn"""
        self.assertEqual(new_turn_prompt(with_tools([{"role": "user", "content": "fix the bug"}])), "fix the bug")

    def test_reads_a_text_block_prompt_as_a_new_turn(self) -> None:
        """reads a text block prompt as a new turn"""
        body = with_tools([{"role": "user", "content": [{"type": "text", "text": "fix the bug"}]}])
        self.assertEqual(new_turn_prompt(body), "fix the bug")

    def test_hook_context_after_the_prompt_does_not_hide_the_turn(self) -> None:
        """hook context after the prompt does not hide the turn"""
        body = with_tools(
            [
                {"role": "user", "content": "refactor the parser"},
                {"role": "system", "content": [{"type": "text", "text": "SessionStart hook additional context: ..."}]},
            ]
        )
        self.assertEqual(new_turn_prompt(body), "refactor the parser")

    def test_ignores_a_tool_result_continuation_mid_turn(self) -> None:
        """ignores a tool_result continuation mid-turn"""
        body = with_tools(
            [
                {"role": "user", "content": "fix the bug"},
                {"role": "assistant", "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {}}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "done"}]},
            ]
        )
        self.assertIsNone(new_turn_prompt(body))

    def test_ignores_auxiliary_calls_that_carry_no_tools(self) -> None:
        """ignores auxiliary calls that carry no tools"""
        self.assertIsNone(new_turn_prompt({"messages": [{"role": "user", "content": "summarise this"}]}))

    def test_ignores_a_request_whose_last_message_is_from_the_assistant(self) -> None:
        """ignores a request whose last message is from the assistant"""
        self.assertIsNone(new_turn_prompt(with_tools([{"role": "assistant", "content": "thinking"}])))

    def test_ignores_an_empty_prompt(self) -> None:
        """ignores an empty prompt"""
        self.assertIsNone(new_turn_prompt(with_tools([{"role": "user", "content": "   "}])))

    def test_survives_a_malformed_body(self) -> None:
        """survives a malformed body"""
        self.assertIsNone(new_turn_prompt(UNDEFINED))
        self.assertIsNone(new_turn_prompt({}))
        self.assertIsNone(new_turn_prompt({"tools": [], "messages": []}))

    def test_strips_system_reminders_claude_code_injects_into_the_prompt(self) -> None:
        """strips system reminders Claude Code injects into the prompt"""
        body = with_tools(
            [{"role": "user", "content": "fix the bug\n<system-reminder>be careful\nabout things</system-reminder>"}]
        )
        self.assertEqual(new_turn_prompt(body), "fix the bug")

    def test_a_prompt_that_is_only_a_system_reminder_is_not_a_turn(self) -> None:
        """a prompt that is only a system reminder is not a turn"""
        self.assertIsNone(
            new_turn_prompt(with_tools([{"role": "user", "content": "<system-reminder>noise</system-reminder>"}]))
        )


class ApplyTier(unittest.TestCase):
    def test_routing_to_haiku_strips_fields_haiku_cannot_accept(self) -> None:
        """routing to haiku strips fields haiku cannot accept"""
        body: dict[str, Any] = {
            "model": "claude-sonnet-4-6",
            "thinking": {"type": "adaptive"},
            "output_config": {"effort": "medium"},
            "context_management": {"edits": [{"type": "clear_thinking_20251015", "keep": "all"}]},
        }
        apply_tier(body, "haiku")
        self.assertEqual(body, {"model": "claude-haiku-4-5-20251001"})

    def test_routing_to_haiku_keeps_context_management_strategies_unrelated_to_thinking(self) -> None:
        """routing to haiku keeps context-management strategies unrelated to thinking"""
        body: dict[str, Any] = {
            "model": "claude-sonnet-4-6",
            "context_management": {
                "edits": [{"type": "clear_tool_uses_20250919"}, {"type": "clear_thinking_20251015"}]
            },
        }
        apply_tier(body, "haiku")
        self.assertEqual(body["context_management"], {"edits": [{"type": "clear_tool_uses_20250919"}]})

    def test_routing_to_opus_leaves_thinking_and_effort_intact(self) -> None:
        """routing to opus leaves thinking and effort intact"""
        body: dict[str, Any] = {
            "model": "claude-sonnet-4-6",
            "thinking": {"type": "adaptive"},
            "output_config": {"effort": "medium"},
        }
        apply_tier(body, "opus", env={})
        self.assertEqual(body["model"], "claude-opus-5-5")
        self.assertEqual(body["thinking"], {"type": "adaptive"})
        self.assertEqual(body["output_config"], {"effort": "medium"})

    def test_an_unknown_tier_leaves_the_request_untouched(self) -> None:
        """an unknown tier leaves the request untouched"""
        body: dict[str, Any] = {"model": "claude-sonnet-4-6", "thinking": {"type": "adaptive"}}
        apply_tier(body, "nonsense")
        self.assertEqual(body, {"model": "claude-sonnet-4-6", "thinking": {"type": "adaptive"}})

    def test_names_each_tiers_own_effort_when_the_request_does_not(self) -> None:
        """names each tier's own effort when the request does not"""
        opus: dict[str, Any] = {"model": "jev-router", "thinking": {"type": "adaptive"}}
        apply_tier(opus, "opus", env={})
        self.assertEqual(opus["output_config"], {"effort": "medium"})
        sonnet: dict[str, Any] = {"model": "jev-router", "thinking": {"type": "adaptive"}}
        apply_tier(sonnet, "sonnet", env={})
        self.assertEqual(sonnet["output_config"], {"effort": "high"})

    def test_jev_tier_effort_overrides_a_tiers_effort_and_a_bad_value_is_ignored(self) -> None:
        """JEV_<TIER>_EFFORT overrides a tier's effort, and a bad value is ignored"""
        self.assertEqual(effort_floor("opus", {"JEV_OPUS_EFFORT": "High"}), "high")
        self.assertEqual(effort_floor("sonnet", {"JEV_SONNET_EFFORT": "low"}), "low")
        self.assertEqual(
            effort_floor("opus", {"JEV_OPUS_EFFORT": "turbo"}), "medium", "unknown value falls back to the default"
        )
        self.assertEqual(effort_floor("opus", {}), "medium")
        self.assertIsNone(effort_floor("haiku", {"JEV_HAIKU_EFFORT": "high"}), "haiku takes no effort")

    def test_keeps_an_effort_the_request_already_carries(self) -> None:
        """keeps an effort the request already carries"""
        body: dict[str, Any] = {
            "model": "jev-router",
            "thinking": {"type": "adaptive"},
            "output_config": {"effort": "low"},
        }
        apply_tier(body, "opus", env={})
        self.assertEqual(body["output_config"], {"effort": "low"}, "the user's own choice outranks the floor")

    def test_never_names_an_effort_for_a_tier_that_cannot_take_one(self) -> None:
        """never names an effort for a tier that cannot take one"""
        body: dict[str, Any] = {"model": "jev-router", "output_config": {"effort": "high"}}
        apply_tier(body, "haiku")
        self.assertNotIn("output_config", body)

    def test_jev_force_effort_replaces_the_effort_claude_code_sent_and_a_per_tier_one_wins_over_it(self) -> None:
        """JEV_FORCE_EFFORT replaces the effort Claude Code sent, and a per-tier one wins over it"""
        self.assertEqual(
            as_dict(CAPTURED_BODY["output_config"])["effort"], "high", "the capture really carries an effort"
        )
        forced = apply_tier(copy.deepcopy(CAPTURED_BODY), "opus", UNDEFINED, {"JEV_FORCE_EFFORT": "low"})
        self.assertEqual(as_dict(forced["output_config"])["effort"], "low", "outranks the effort Claude Code sent")
        per_tier = apply_tier(
            copy.deepcopy(CAPTURED_BODY),
            "opus",
            UNDEFINED,
            {"JEV_FORCE_EFFORT": "low", "JEV_OPUS_FORCE_EFFORT": "xhigh"},
        )
        self.assertEqual(as_dict(per_tier["output_config"])["effort"], "xhigh", "the tier setting beats the global one")
        other = apply_tier(copy.deepcopy(CAPTURED_BODY), "sonnet", UNDEFINED, {"JEV_OPUS_FORCE_EFFORT": "xhigh"})
        self.assertEqual(
            as_dict(other["output_config"])["effort"], "high", "another tier keeps the effort Claude Code sent"
        )

    def test_a_forced_effort_is_ignored_when_unrecognised_and_never_reaches_haiku(self) -> None:
        """a forced effort is ignored when unrecognised and never reaches Haiku"""
        bad = apply_tier(copy.deepcopy(CAPTURED_BODY), "opus", UNDEFINED, {"JEV_FORCE_EFFORT": "turbo"})
        self.assertEqual(as_dict(bad["output_config"])["effort"], "high", "a bad value leaves the request alone")
        haiku = apply_tier(copy.deepcopy(CAPTURED_BODY), "haiku", UNDEFINED, {"JEV_FORCE_EFFORT": "max"})
        self.assertNotIn("output_config", haiku, "Haiku takes no effort, forced or not")
        self.assertIsNone(forced_effort("haiku", {"JEV_HAIKU_FORCE_EFFORT": "max"}))
        self.assertEqual(forced_effort("fable", {"JEV_FORCE_EFFORT": " Max "}), "max", "case and spaces are forgiven")


class Conversations(unittest.TestCase):
    def test_a_conversation_keeps_one_key_as_it_grows_and_differs_from_a_sub_agent(self) -> None:
        """a conversation keeps one key as it grows, and differs from a sub-agent"""
        main = {"messages": [{"role": "user", "content": "main task"}]}
        grown = {"messages": [{"role": "user", "content": "main task"}, {"role": "assistant", "content": "ok"}]}
        sub = {"messages": [{"role": "user", "content": "sub-agent task"}]}
        self.assertEqual(conversation_key(main), conversation_key(grown))
        self.assertNotEqual(conversation_key(main), conversation_key(sub))

    def test_the_key_ignores_the_cache_control_breakpoint_claude_code_moves_between_requests(self) -> None:
        """the key ignores the cache_control breakpoint Claude Code moves between requests"""
        first = {
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "<system-reminder>x</system-reminder>"},
                        {"type": "text", "text": "do the thing", "cache_control": {"type": "ephemeral", "ttl": "1h"}},
                    ],
                }
            ]
        }
        later = {
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "<system-reminder>x</system-reminder>"},
                        {"type": "text", "text": "do the thing"},
                    ],
                },
                {"role": "assistant", "content": "working"},
            ]
        }
        self.assertEqual(conversation_key(first), conversation_key(later))

    def test_the_same_opening_text_in_two_sessions_gets_two_keys(self) -> None:
        """the same opening text in two sessions gets two keys"""

        def mk(sid: str) -> JsObject:
            return {
                "metadata": {"user_id": json.dumps({"session_id": sid})},
                "messages": [{"role": "user", "content": "same opening"}],
            }

        self.assertNotEqual(conversation_key(mk("a")), conversation_key(mk("b")))

    def test_the_key_survives_metadata_that_is_not_json(self) -> None:
        """the key survives metadata that is not JSON"""
        key = conversation_key({"metadata": {"user_id": "not-json"}, "messages": [{"role": "user", "content": "hi"}]})
        self.assertRegex(key, r"^[0-9a-f]{12}$")

    def test_the_first_tool_bearing_conversation_in_a_session_is_the_main_thread(self) -> None:
        """the first tool-bearing conversation in a session is the main thread"""
        mains = JsMap()
        sid = json.dumps({"session_id": "s-main"})

        def mk(text: str) -> JsObject:
            return {
                "metadata": {"user_id": sid},
                "tools": [{"name": "Read"}],
                "messages": [{"role": "user", "content": text}],
            }

        main = agent_of(mk("the user's opening prompt"), mains)
        sub = agent_of(mk("search the repo for conversationKey"), mains)
        self.assertTrue(main["main"])
        self.assertFalse(sub["main"], "a later conversation in the same session is a sub-agent")
        self.assertNotEqual(main["key"], sub["key"])
        self.assertEqual(sub["label"], "search the repo for conversationKey")
        self.assertTrue(agent_of(mk("the user's opening prompt"), mains)["main"], "the main key is stable")

    def test_auxiliary_calls_without_tools_never_claim_the_main_slot(self) -> None:
        """auxiliary calls without tools never claim the main slot"""
        mains = JsMap()
        sid = json.dumps({"session_id": "s-aux"})
        aux = agent_of(
            {"metadata": {"user_id": sid}, "messages": [{"role": "user", "content": "summarise this"}]}, mains
        )
        real = agent_of(
            {
                "metadata": {"user_id": sid},
                "tools": [{"name": "Read"}],
                "messages": [{"role": "user", "content": "the real prompt"}],
            },
            mains,
        )
        self.assertFalse(aux["main"], "a toolless call never registers itself as the main thread")
        self.assertTrue(real["main"], "the first real agent turn is the main thread")
        self.assertNotEqual(mains.get("s-aux"), aux["key"], "the aux call did not take the main slot")
        self.assertEqual(mains.get("s-aux"), real["key"])

    def test_agent_labels_are_trimmed_of_reminders_and_length(self) -> None:
        """agent labels are trimmed of reminders and length"""
        self.assertEqual(
            agent_label(
                {"messages": [{"role": "user", "content": "<system-reminder>noise</system-reminder> real task"}]}
            ),
            "real task",
        )
        self.assertEqual(len(agent_label({"messages": [{"role": "user", "content": "x" * 80}]})), 48)
        self.assertEqual(agent_label({}), "")


if __name__ == "__main__":
    unittest.main()
