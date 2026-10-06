// Cases for the pure functions in proxy.mjs (SPEC 7.5) and model-names.mjs.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { attempt } from "./tagged.mjs";

/** True when cutting `text` at UTF-16 offset `at` would separate a surrogate pair (SPEC 3.2). */
export const splitsPair = (text, at) =>
  at > 0 && at < text.length && /[\uD800-\uDBFF]/.test(text[at - 1]) && /[\uDC00-\uDFFF]/.test(text[at]);

export default function proxyCases({ root, config, proxy, modelNames }) {
  const fixture = JSON.parse(readFileSync(join(root, "conformance", "fixtures", "claude-code-print-request.json"), "utf8"));
  const captured = () => structuredClone(fixture.body);
  const tools = [{ name: "Bash" }];
  const withTools = (messages) => ({ tools, messages });
  const user = (content) => ({ role: "user", content });
  const toolUse = { role: "assistant", content: [{ type: "tool_use", id: "t1", name: "Bash", input: {} }] };
  const toolResult = user([{ type: "tool_result", tool_use_id: "t1", content: "done" }]);
  const meta = (session_id) => ({ user_id: JSON.stringify({ session_id }) });

  // ---- newTurnPrompt -----------------------------------------------------------------------
  const turnBodies = [
    ["captured print request", captured()],
    ["plain string", withTools([user("fix the bug")])],
    ["text block", withTools([user([{ type: "text", text: "fix the bug" }])])],
    ["hook system message after the turn", withTools([user("refactor the parser"), { role: "system", content: [{ type: "text", text: "SessionStart hook additional context: ..." }] }])],
    ["two system messages after the turn", withTools([user("go"), { role: "system", content: "a" }, { role: "system", content: "b" }])],
    ["tool_result continuation", withTools([user("fix the bug"), toolUse, toolResult])],
    ["no tools", { messages: [user("summarise this")] }],
    ["empty tools", { tools: [], messages: [user("x")] }],
    ["tools not an array", { tools: { name: "Bash" }, messages: [user("x")] }],
    ["last is assistant", withTools([{ role: "assistant", content: "thinking" }])],
    ["blank prompt", withTools([user("   ")])],
    ["undefined body", undefined],
    ["null body", null],
    ["empty body", {}],
    ["empty messages", { tools: [], messages: [] }],
    ["messages missing", { tools }],
    ["reminder stripped", withTools([user("fix the bug\n<system-reminder>be careful\nabout things</system-reminder>")])],
    ["only a reminder", withTools([user("<system-reminder>noise</system-reminder>")])],
    ["two reminders non-greedy", withTools([user("<system-reminder>a</system-reminder>keep<system-reminder>b</system-reminder>")])],
    ["nested-looking reminder", withTools([user("<system-reminder>a<system-reminder>b</system-reminder>c</system-reminder>")])],
    ["unclosed reminder kept", withTools([user("x <system-reminder>never closed")])],
    ["JSWS trimmed", withTools([user("  ﻿　 fix  \t\n\v\f\r ")])],
    ["non-JSWS controls kept", withTools([user("\u001ffix\u0085")])],
    ["mongolian vowel separator kept", withTools([user("᠎fix")])],
    ["blocks joined with newline", withTools([user([{ type: "text", text: "a" }, { type: "image", source: {} }, { type: "text", text: "b" }])])],
    ["null text joins empty", withTools([user([{ type: "text", text: null }, { type: "text", text: "x" }])])],
    ["missing text joins empty", withTools([user([{ type: "text" }, { type: "text", text: "x" }])])],
    ["numeric text", withTools([user([{ type: "text", text: 42 }])])],
    ["boolean text", withTools([user([{ type: "text", text: true }])])],
    ["object text", withTools([user([{ type: "text", text: { a: 1 } }])])],
    ["array text", withTools([user([{ type: "text", text: [1, [2, null], "z"] }])])],
    ["null content block throws", withTools([user([null, { type: "text", text: "x" }])])],
    ["number content", withTools([user(42)])],
    ["null content", withTools([user(null)])],
    ["null message skipped as last", withTools([user("hi"), null])],
    ["role missing", withTools([{ content: "hi" }])],
    ["lone high surrogate survives", withTools([user("fix \ud83d now")])],
    ["lone low surrogate survives", withTools([user([{ type: "text", text: "\ude00 tail" }])])],
    ["emoji kept whole", withTools([user("ship it 🚀")])],
    ["messages object throws", { tools, messages: { 0: user("x") } }],
  ];
  const newTurnPrompt = turnBodies.map(([name, body]) => ({
    name,
    input: { body },
    expected: attempt(() => proxy.newTurnPrompt(structuredClone(body))),
  }));

  // ---- agentLabel ----------------------------------------------------------------------------
  const labelInputs = [
    ["captured print request", { body: captured() }],
    ["reminder stripped", { body: { messages: [user("<system-reminder>noise</system-reminder> real task")] } }],
    ["80 x", { body: { messages: [user("x".repeat(80))] } }],
    ["exactly 48", { body: { messages: [user("y".repeat(48))] } }],
    ["exactly 49", { body: { messages: [user("z".repeat(49))] } }],
    ["empty body", { body: {} }],
    ["null body", { body: null }],
    ["JSWS runs collapse", { body: { messages: [user("  a  b  c﻿　d \t\n e  ")] } }],
    ["non-JSWS kept", { body: { messages: [user("a\u0085b\u001fc᠎d")] } }],
    ["blocks joined with space", { body: { messages: [user([{ type: "text", text: "one" }, { type: "text", text: "two" }])] } }],
    ["non-text blocks skipped", { body: { messages: [user([{ type: "image" }, { type: "text", text: "only" }])] } }],
    ["null text", { body: { messages: [user([{ type: "text", text: null }, { type: "text", text: "x" }])] } }],
    ["numeric text", { body: { messages: [user([{ type: "text", text: 7 }, { type: "text", text: false }])] } }],
    ["first message only", { body: { messages: [user("first"), user("second")] } }],
    ["custom max 10", { body: { messages: [user("abcdefghijklmnop")] }, max: 10 }],
    ["custom max 1", { body: { messages: [user("abc")] }, max: 1 }],
    ["emoji before cut", { body: { messages: [user(`🚀${"a".repeat(60)}`)] } }],
    ["lone surrogate survives", { body: { messages: [user("label \udc00 here")] } }],
    ["lone surrogate before cut", { body: { messages: [user(`\ud800${"b".repeat(60)}`)] } }],
    ["assistant first message", { body: { messages: [{ role: "assistant", content: "hello there" }] } }],
    ["number content", { body: { messages: [user(5)] } }],
  ];
  const agentLabel = labelInputs.map(([name, input]) => {
    const expected = proxy.agentLabel(structuredClone(input.body), input.max);
    const clean = attempt(() => proxy.agentLabel(structuredClone(input.body), Infinity));
    if (typeof clean === "string" && splitsPair(clean, (input.max ?? 48) - 1)) throw new Error(`agent-label ${name} splits a pair`);
    return { name, input, expected };
  });

  // ---- applyTier -----------------------------------------------------------------------------
  const tierBodies = [
    ["haiku strip", "haiku", { model: "claude-sonnet-4-6", thinking: { type: "adaptive" }, output_config: { effort: "medium" }, context_management: { edits: [{ type: "clear_thinking_20251015", keep: "all" }] } }, {}],
    ["haiku keeps unrelated edits", "haiku", { model: "x", context_management: { edits: [{ type: "clear_tool_uses_20250919" }, { type: "clear_thinking_20251015" }] } }, {}],
    ["haiku edits with null and untyped entries", "haiku", { model: "x", context_management: { edits: [null, {}, { type: "THINKING_upper" }, { type: 5 }], other: 1 } }, {}],
    ["haiku edits not an array", "haiku", { model: "x", context_management: { edits: "clear_thinking" } }, {}],
    ["haiku output_config keeps other keys", "haiku", { model: "x", output_config: { effort: "high", format: "json" } }, {}],
    ["haiku empty output_config", "haiku", { model: "x", output_config: {} }, {}],
    ["haiku null output_config", "haiku", { model: "x", output_config: null }, {}],
    ["haiku forced effort ignored", "haiku", { model: "x", output_config: { effort: "high" } }, { JEV_FORCE_EFFORT: "max" }],
    ["opus keeps thinking and effort", "opus", { model: "claude-sonnet-4-6", thinking: { type: "adaptive" }, output_config: { effort: "medium" } }, {}],
    ["unknown tier untouched", "nonsense", { model: "claude-sonnet-4-6", thinking: { type: "adaptive" } }, {}],
    ["opus floor named", "opus", { model: "jev-router", thinking: { type: "adaptive" } }, {}],
    ["sonnet floor named", "sonnet", { model: "jev-router" }, {}],
    ["fable floor named", "fable", { model: "jev-router" }, {}],
    ["request effort kept", "opus", { model: "jev-router", output_config: { effort: "low" } }, {}],
    ["empty-string effort is falsy", "opus", { model: "jev-router", output_config: { effort: "" } }, {}],
    ["zero effort is falsy", "sonnet", { model: "jev-router", output_config: { effort: 0, z: 1 } }, {}],
    ["false effort is falsy", "fable", { model: "jev-router", output_config: { a: 1, effort: false } }, {}],
    ["effort appended after other keys", "opus", { model: "jev-router", output_config: { format: "json" } }, {}],
    ["null output_config gets floor", "opus", { model: "jev-router", output_config: null }, {}],
    ["floor env override", "opus", { model: "jev-router" }, { JEV_OPUS_EFFORT: "High" }],
    ["floor env invalid", "opus", { model: "jev-router" }, { JEV_OPUS_EFFORT: "turbo" }],
    ["floor env ignored when request has effort", "opus", { model: "jev-router", output_config: { effort: "low" } }, { JEV_OPUS_EFFORT: "max" }],
    ["force beats request", "opus", { model: "jev-router", output_config: { effort: "high" } }, { JEV_FORCE_EFFORT: "low" }],
    ["per-tier force beats global", "opus", { model: "jev-router", output_config: { effort: "high" } }, { JEV_FORCE_EFFORT: "low", JEV_OPUS_FORCE_EFFORT: "xhigh" }],
    ["other tier's force ignored", "sonnet", { model: "jev-router", output_config: { effort: "high" } }, { JEV_OPUS_FORCE_EFFORT: "xhigh" }],
    ["invalid force leaves request", "opus", { model: "jev-router", output_config: { effort: "high" } }, { JEV_FORCE_EFFORT: "turbo" }],
    ["force with empty-string effort", "sonnet", { model: "jev-router", output_config: { effort: "" } }, { JEV_FORCE_EFFORT: "max" }],
    ["force and floor env together", "fable", { model: "jev-router" }, { JEV_FABLE_EFFORT: "low", JEV_FORCE_EFFORT: "medium" }],
    ["sonnet keeps thinking edits", "sonnet", { model: "x", thinking: { type: "adaptive" }, context_management: { edits: [{ type: "clear_thinking_20251015" }] } }, {}],
  ];
  const applyTier = tierBodies.map(([name, tier, body, env]) => ({
    name,
    input: { body, tier, env },
    expected: proxy.applyTier(structuredClone(body), tier, undefined, env),
  }));
  for (const tier of ["haiku", "sonnet", "opus", "fable"]) {
    for (const [label, env] of [["no env", {}], ["forced low", { JEV_FORCE_EFFORT: "low" }]]) {
      applyTier.push({
        name: `captured request to ${tier}, ${label}`,
        input: { body: captured(), tier, env },
        expected: proxy.applyTier(captured(), tier, undefined, env),
      });
    }
  }
  applyTier.push({
    name: "explicit model",
    input: { body: captured(), tier: "opus", model: "claude-opus-6", env: {} },
    expected: proxy.applyTier(captured(), "opus", "claude-opus-6", {}),
  });
  applyTier.push({
    name: "explicit model, unknown tier",
    input: { body: { model: "jev-router" }, tier: "mystery", model: "claude-opus-6", env: {} },
    expected: proxy.applyTier({ model: "jev-router" }, "mystery", "claude-opus-6", {}),
  });

  // ---- sanitizeSchema ------------------------------------------------------------------------
  const schemas = [
    ["boolean true with minimum", { type: "object", properties: { topN: { minimum: 0, exclusiveMinimum: true } } }],
    ["false exclusiveMaximum", { properties: { n: { maximum: 10, exclusiveMaximum: false } } }],
    ["numeric bound untouched", { properties: { n: { exclusiveMinimum: 5 } } }],
    ["nested in arrays", { anyOf: [{ items: { minimum: 1, exclusiveMinimum: true } }] }],
    ["arrays of arrays", [[{ maximum: 3, exclusiveMaximum: true }], [{ x: [{ minimum: -1, exclusiveMinimum: true }] }]]],
    ["null", null],
    ["primitives", { a: null, b: 3, c: "x" }],
    ["true without bound", { exclusiveMinimum: true, exclusiveMaximum: true }],
    ["true with string bound", { minimum: "5", exclusiveMinimum: true }],
    ["true with null bound", { maximum: null, exclusiveMaximum: true }],
    ["both bounds", { minimum: 1, exclusiveMinimum: true, maximum: 9, exclusiveMaximum: true, type: "integer" }],
    ["exclusive key position kept", { exclusiveMinimum: true, a: 1, minimum: 2, b: 3 }],
    ["string exclusive untouched", { exclusiveMinimum: "true", minimum: 1 }],
    ["deep object", { a: { b: { c: { d: { minimum: 0, exclusiveMinimum: true } } } } }],
    ["integer-like keys", { b: { minimum: 0, exclusiveMinimum: true }, 2: { maximum: 1, exclusiveMaximum: false }, a: 1, 0: "zero", "01": { exclusiveMinimum: true }, 4294967294: "max index", 4294967295: "not an index" }],
    ["number", 7],
    ["string", "schema"],
  ];
  const sanitizeSchema = schemas.map(([name, node]) => {
    const copy = structuredClone(node);
    proxy.sanitizeSchema(copy);
    return { name, input: { node }, expected: copy };
  });
  for (const [i, tool] of fixture.body.tools.entries()) {
    const copy = structuredClone(tool.input_schema);
    proxy.sanitizeSchema(copy);
    sanitizeSchema.push({ name: `captured tool ${i} ${tool.name}`, input: { node: tool.input_schema }, expected: copy });
  }

  // ---- versionOf / shortName / catalogs ------------------------------------------------------
  const ids = [
    "claude-opus-5-5",
    "claude-sonnet-5-5",
    "claude-haiku-4-5-20251001",
    "claude-fable-5-1",
    "claude-fable-5-1[1m]",
    "claude-opus-6",
    "claude-opus-4-8",
    "claude-opus-4-6[1m]",
    "claude-opus-4-20250514",
    "claude-sonnet-4-20250514",
    "claude-3-7-sonnet-20250219",
    "claude-3-5-haiku-20241022",
    "anthropic.claude-opus-6",
    "anthropic.claude-opus-5-5",
    "us.anthropic.claude-opus-5-5",
    "claude-opus-5-100",
    "claude-sonnet-5-10",
    "claude-opus-5-05",
    "claude-sonnet-5",
    "claude-haiku-4-6",
    "Claude-Opus-5",
    "xclaude-opus-5",
    "claude-opus-",
    "claude-opus-5-",
    "claude-opus-5-5-5",
    "mystery-9",
    "jev-router",
    "",
  ];
  const versionOf = [];
  for (const id of ids) {
    const tier = config.tierOf(id);
    versionOf.push({ name: `${id || "(empty)"} as ${tier}`, input: { id, tier }, expected: proxy.versionOf({ id, tier }) });
  }
  for (const [name, arg] of [
    ["id missing", { tier: "opus" }],
    ["tier missing", { id: "claude-opus-5-5" }],
    ["tier without family", { id: "claude-opus-5-5", tier: "nonsense" }],
    ["mismatched tier", { id: "claude-opus-6", tier: "sonnet" }],
    ["empty object", {}],
  ]) {
    versionOf.push({ name, input: arg, expected: proxy.versionOf(arg) });
  }

  const shortName = [...ids, undefined, null, 42, "claude-opus-5-5 and claude-haiku-4-5", "claude-opus-55"].map((model) => ({
    name: model === undefined ? "undefined" : JSON.stringify(model),
    input: { model },
    expected: modelNames.shortName(model),
  }));

  const catalogs = [
    ["empty", []],
    ["two opus versions", [{ id: "claude-opus-5-5", display_name: "Claude Opus 5" }, { id: "claude-opus-4-8", display_name: "Claude Opus 4.8" }]],
    ["account catalog", [
      { id: "claude-opus-4-8", display_name: "Claude Opus 4.8", created_at: "2026-01-05" },
      { id: "claude-opus-5-5", display_name: "Claude Opus 5.5", created_at: "2026-09-02" },
      { id: "claude-sonnet-5-5", display_name: "Claude Sonnet 5.5", created_at: "2026-08-11" },
    ]],
    ["new major undated", [{ id: "claude-opus-5-5" }, { id: "claude-opus-6" }, { id: "claude-opus-4-8" }]],
    ["minor 10", [{ id: "claude-sonnet-5-5" }, { id: "claude-sonnet-5-10" }]],
    ["date is not a minor", [{ id: "claude-haiku-4-5-20251001" }, { id: "claude-haiku-4-6" }]],
    ["old naming", [{ id: "claude-3-7-sonnet-20250219" }, { id: "claude-sonnet-5-5" }, { id: "claude-3-5-haiku-20241022" }, { id: "claude-haiku-4-5-20251001" }]],
    ["provider prefix", [{ id: "anthropic.claude-opus-5-5" }, { id: "anthropic.claude-opus-6" }]],
    ["dated majors", [{ id: "claude-opus-4-20250514" }, { id: "claude-sonnet-4-20250514" }, { id: "claude-opus-4-1" }]],
    ["calibrated set", [{ id: "claude-opus-5-5" }, { id: "claude-sonnet-5-5" }, { id: "claude-opus-4-8" }]],
    ["newer opus", [{ id: "claude-opus-6" }, { id: "claude-opus-5-5" }, { id: "claude-sonnet-5-5" }]],
    ["version tie broken by date", [
      { id: "claude-opus-5-5", created_at: "2026-09-02T00:00:00Z" },
      { id: "claude-opus-5-5-20261001", created_at: "2026-10-01T00:00:00Z" },
      { id: "claude-opus-5-5-preview", created_at: "2026-08-01T00:00:00Z" },
    ]],
    ["version tie same date keeps order", [
      { id: "claude-sonnet-5-5-a", created_at: "2026-09-02T00:00:00Z" },
      { id: "claude-sonnet-5-5-b", created_at: "2026-09-02T00:00:00Z" },
      { id: "claude-sonnet-5-5-c" },
    ]],
    ["cross-tier version tie", [{ id: "claude-haiku-5-5", created_at: "2026-01-01T00:00:00Z" }, { id: "claude-sonnet-5-5", created_at: "2026-02-01T00:00:00Z" }, { id: "claude-opus-5-5" }]],
    ["full fields", [
      { id: "claude-opus-5-5", display_name: "Claude Opus 5.5", created_at: "2026-09-02T00:00:00Z", max_input_tokens: 1000000, type: "model" },
      { id: "claude-haiku-4-5-20251001", display_name: "Claude Haiku 4.5", created_at: "2025-10-01T00:00:00Z", max_input_tokens: 0 },
      { id: "claude-fable-5-1", created_at: "", max_input_tokens: 200000 },
      { id: "claude-sonnet-5-5", display_name: "" },
    ]],
    ["non-claude and junk entries", [null, {}, { id: 42 }, { id: "gpt-5" }, { id: "jev-router" }, { id: "claude-sonnet-5-5" }]],
    ["every tier newer", [{ id: "claude-haiku-5" }, { id: "claude-sonnet-6" }, { id: "claude-opus-6-1" }, { id: "claude-fable-5-2" }]],
    ["unreadable version only", [{ id: "claude-3-opus-20240229" }]],
  ];
  // SPEC 7.5 says the release-date tiebreak is a code-unit comparison; Node uses localeCompare.
  // They agree on the ISO dates the API sends, and these inputs are checked to stay in that range.
  for (const [name, catalog] of catalogs) {
    const dates = catalog.map((m) => m?.created_at ?? "");
    for (const a of dates) for (const b of dates) {
      if (Math.sign(a.localeCompare(b)) !== Math.sign(a < b ? -1 : a > b ? 1 : 0)) throw new Error(`${name}: localeCompare disagrees`);
    }
  }
  const claudeModels = catalogs.map(([name, catalog]) => ({ name, input: { catalog }, expected: proxy.claudeModels(structuredClone(catalog)) }));
  const newestPerTier = catalogs.map(([name, catalog]) => {
    const models = proxy.claudeModels(structuredClone(catalog));
    return { name, input: { models }, expected: proxy.newestPerTier(models) };
  });
  newestPerTier.push({
    name: "unsorted hand-built list keeps first of each tier",
    input: { models: [{ id: "b", tier: "opus" }, { id: "a", tier: "haiku" }, { id: "c", tier: "opus" }, { id: "d", tier: "haiku" }, { id: "e", tier: "fable" }] },
    expected: proxy.newestPerTier([{ id: "b", tier: "opus" }, { id: "a", tier: "haiku" }, { id: "c", tier: "opus" }, { id: "d", tier: "haiku" }, { id: "e", tier: "fable" }]),
  });
  newestPerTier.push({ name: "empty list", input: { models: [] }, expected: proxy.newestPerTier([]) });
  const newerThanCalibrated = catalogs.map(([name, catalog]) => ({ name, input: { catalog }, expected: proxy.newerThanCalibrated(structuredClone(catalog)) }));

  // ---- sessionOf / conversationKey -----------------------------------------------------------
  const sid = "11111111-2222-4333-8444-555555555555";
  const sessionBodies = [
    ["json session id", { metadata: meta(sid) }],
    ["captured request", captured()],
    ["not json", { metadata: { user_id: "not-json" } }],
    ["empty body", {}],
    ["null body", null],
    ["undefined body", undefined],
    ["metadata null", { metadata: null }],
    ["user_id missing", { metadata: {} }],
    ["user_id null", { metadata: { user_id: null } }],
    ["user_id json null", { metadata: { user_id: "null" } }],
    ["user_id json number", { metadata: { user_id: "42" } }],
    ["user_id raw number", { metadata: { user_id: 42 } }],
    ["session_id null", { metadata: { user_id: '{"session_id":null}' } }],
    ["numeric session_id", { metadata: { user_id: '{"session_id":12345}' } }],
    ["fractional session_id", { metadata: { user_id: '{"session_id":1.5e-7}' } }],
    ["false session_id", { metadata: { user_id: '{"session_id":false}' } }],
    ["object session_id", { metadata: { user_id: '{"session_id":{"a":1}}' } }],
    ["array session_id", { metadata: { user_id: '{"session_id":[1,"two",null]}' } }],
    ["empty session_id", { metadata: { user_id: '{"session_id":""}' } }],
    ["lone surrogate session_id", { metadata: { user_id: '{"session_id":"s-\\ud800-x"}' } }],
    ["duplicate session_id keys", { metadata: { user_id: '{"session_id":"first","session_id":"last"}' } }],
    ["json with whitespace", { metadata: { user_id: ' { "session_id" : "spaced" } ' } }],
  ];
  const sessionOf = sessionBodies.map(([name, body]) => ({ name, input: { body }, expected: proxy.sessionOf(structuredClone(body)) }));

  const keyBodies = [
    ["main task", { messages: [user("main task")] }],
    ["main task grown", { messages: [user("main task"), { role: "assistant", content: "ok" }] }],
    ["sub-agent task", { messages: [user("sub-agent task")] }],
    ["cache_control moved", { messages: [user([{ type: "text", text: "<system-reminder>x</system-reminder>" }, { type: "text", text: "do the thing", cache_control: { type: "ephemeral", ttl: "1h" } }])] }],
    ["cache_control absent", { messages: [user([{ type: "text", text: "<system-reminder>x</system-reminder>" }, { type: "text", text: "do the thing" }]), { role: "assistant", content: "working" }] }],
    ["session a", { metadata: meta("a"), messages: [user("same opening")] }],
    ["session b", { metadata: meta("b"), messages: [user("same opening")] }],
    ["not json metadata", { metadata: { user_id: "not-json" }, messages: [user("hi")] }],
    ["numeric session", { metadata: { user_id: '{"session_id":5}' }, messages: [user("hi")] }],
    ["object session", { metadata: { user_id: '{"session_id":{"a":1}}' }, messages: [user("hi")] }],
    ["array session", { metadata: { user_id: '{"session_id":[1,2]}' }, messages: [user("hi")] }],
    ["lone surrogate text", { messages: [user("abc \ud800 def")] }],
    ["U+FFFD text hashes like a lone surrogate", { messages: [user("abc � def")] }],
    ["lone surrogate session", { metadata: { user_id: '{"session_id":"\\udfff"}' }, messages: [user("x")] }],
    ["blocks joined with nothing", { messages: [user([{ type: "text", text: "a" }, { type: "text", text: "b" }])] }],
    ["null text block", { messages: [user([{ type: "text", text: null }, { type: "text", text: 3 }])] }],
    ["non-ascii text", { messages: [user("naïve café   🚀")] }],
    ["no messages", {}],
    ["null body", null],
    ["captured request", captured()],
    ["captured request without metadata", { ...captured(), metadata: undefined }],
  ];
  const conversationKey = keyBodies.map(([name, body]) => ({ name, input: { body }, expected: proxy.conversationKey(structuredClone(body)) }));

  // ---- agentOf -------------------------------------------------------------------------------
  const real = (session, text) => ({ ...(session === null ? {} : { metadata: meta(session) }), tools: [{ name: "Read" }], messages: [user(text)] });
  const aux = (session, text) => ({ metadata: meta(session), messages: [user(text)] });
  const sequences = [
    ["main then sub", [real("s-main", "the user's opening prompt"), real("s-main", "search the repo for conversationKey"), real("s-main", "the user's opening prompt")]],
    ["aux never claims main", [aux("s-aux", "summarise this"), real("s-aux", "the real prompt"), aux("s-aux", "the real prompt")]],
    ["no session stands alone", [real(null, "print mode"), real(null, "another"), { tools: [], messages: [user("x")] }]],
    ["empty labels fall back", [real("s-empty", "<system-reminder>only</system-reminder>"), real("s-empty", "   ")]],
    ["numeric session ids", [
      { metadata: { user_id: '{"session_id":7}' }, tools, messages: [user("seven")] },
      { metadata: { user_id: '{"session_id":7}' }, tools, messages: [user("seven sub")] },
    ]],
    ["captured request then sub-agent", [captured(), (() => {
      const sub = captured();
      sub.messages[0].content.at(-1).text = "Search the codebase for callers of handler 1 and report them";
      return sub;
    })()]],
  ];
  // SPEC 7.4: `mains` drops its oldest session only once it holds more than 50, before inserting.
  const many = [];
  for (let i = 0; i < 53; i++) many.push(real(`many-${i}`, `main ${i}`));
  many.push(real("many-0", "main 0"), real("many-0", "a new opening after eviction"), real("many-2", "sub of 2"), real("many-52", "sub of 52"));
  sequences.push(["more than 51 sessions", many]);
  const agentOf = sequences.map(([name, steps]) => {
    const mains = new Map();
    const results = steps.map((body) => proxy.agentOf(structuredClone(body), mains));
    return { name, input: { steps }, expected: { results, mains: [...mains.entries()] } };
  });

  return {
    "new-turn-prompt": newTurnPrompt,
    "agent-label": agentLabel,
    "apply-tier": applyTier,
    "sanitize-schema": sanitizeSchema,
    "version-of": versionOf,
    "short-name": shortName,
    "claude-models": claudeModels,
    "newest-per-tier": newestPerTier,
    "newer-than-calibrated": newerThanCalibrated,
    "session-of": sessionOf,
    "conversation-key": conversationKey,
    "agent-of": agentOf,
  };
}
