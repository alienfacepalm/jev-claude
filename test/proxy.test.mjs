import "./isolate-status.mjs";
import test from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import {
  sanitizeSchema,
  newTurnPrompt,
  applyTier,
  claudeModels,
  newestPerTier,
  newerThanCalibrated,
  conversationKey,
  sessionOf,
  agentOf,
  agentLabel,
  startProxy,
} from "../src/proxy.mjs";

test("only the sentinel model is routed", () => {
  assert.equal(isAuto("jev-router"), true);
  assert.equal(isAuto("claude-opus-4-6"), false, "a model the user picked is theirs");
  assert.equal(isAuto("claude-haiku-4-5-20251001"), false, "internal Haiku calls pass through");
  assert.equal(isAuto(undefined), false);
});

test("the sentinel is not mistaken for a real tier", () => {
  assert.equal(tierOf("jev-router"), null);
});
import { tierOf, isAuto, effortFloor } from "../src/config.mjs";
import {
  writeDecision,
  writeStatus,
  readStatus,
  markManual,
  agentView,
  pruneStale,
  STATUS_DIR,
  writeCalibration,
  readCalibration,
} from "../src/status.mjs";
import { mkdirSync, statSync, utimesSync, writeFileSync, existsSync, rmSync } from "node:fs";
import { join } from "node:path";

test("reads the session id out of Claude Code's metadata", () => {
  const sid = "11111111-2222-4333-8444-555555555555";
  assert.equal(sessionOf({ metadata: { user_id: JSON.stringify({ session_id: sid }) } }), sid);
  assert.equal(sessionOf({ metadata: { user_id: "not-json" } }), "");
  assert.equal(sessionOf({}), "");
});

test("status round-trips per session and misses cleanly", () => {
  const sid = `test-${process.pid}`;
  writeStatus(sid, { tier: "opus", confidence: 0.87, reason: "jev" });
  assert.deepEqual(readStatus(sid), { tier: "opus", confidence: 0.87, reason: "jev" });
  assert.equal(readStatus("no-such-session"), null);
  assert.doesNotThrow(() => writeStatus("", { tier: "opus" }));
});

test("status files are private to their owner", { skip: process.platform === "win32" }, () => {
  const sid = `perm-${process.pid}`;
  writeStatus(sid, { tier: "opus" });
  assert.equal(statSync(STATUS_DIR).mode & 0o777, 0o700);
  assert.equal(statSync(join(STATUS_DIR, `${sid}.json`)).mode & 0o777, 0o600);
});

test("stale status files are pruned and fresh ones kept", () => {
  mkdirSync(STATUS_DIR, { recursive: true });
  const stale = join(STATUS_DIR, `stale-${process.pid}.json`);
  const fresh = join(STATUS_DIR, `fresh-${process.pid}.json`);
  writeFileSync(stale, "{}");
  writeFileSync(fresh, "{}");
  const old = new Date(Date.now() - 8 * 24 * 60 * 60 * 1000);
  utimesSync(stale, old, old);
  assert.ok(pruneStale() >= 1);
  assert.equal(existsSync(stale), false);
  assert.equal(existsSync(fresh), true);
});

test("routing status retains the exact recent Jev exchanges", () => {
  const sid = `history-${process.pid}`;
  writeDecision(sid, { prompt: "first", jev: { request: { id: 1 }, response: { confidence: 0.6 } } });
  writeDecision(sid, { prompt: "second", jev: { request: { id: 2 }, response: { confidence: 0.8 } } });
  const status = readStatus(sid);
  assert.equal(status.prompt, "second");
  assert.deepEqual(status.history.map(({ prompt }) => prompt), ["first", "second"]);
  assert.equal(status.history[0].jev.response.confidence, 0.6);
});

test("recognises older model versions within a tier", () => {
  assert.equal(tierOf("claude-sonnet-4-6"), "sonnet");
  assert.equal(tierOf("claude-sonnet-5"), "sonnet");
  assert.equal(tierOf("claude-haiku-4-5-20251001"), "haiku");
  assert.equal(tierOf("claude-opus-4-1"), "opus");
  assert.equal(tierOf("claude-fable-5-1[1m]"), "fable");
  assert.equal(tierOf("mystery-9"), null);
  assert.equal(tierOf(undefined), null);
});

test("keeps available Claude model versions as separate Jev choices", () => {
  assert.deepEqual(
    claudeModels([
      { id: "claude-opus-5-5", display_name: "Claude Opus 5" },
      { id: "claude-opus-4-8", display_name: "Claude Opus 4.8" },
    ]).map(({ id, tier }) => ({ id, tier })),
    [
      { id: "claude-opus-5-5", tier: "opus" },
      { id: "claude-opus-4-8", tier: "opus" },
    ],
  );
});

test("a Claude API key reaches Anthropic untouched, on routed and manual requests alike", async (t) => {
  const seen = [];
  const upstream = http.createServer((req, res) => {
    req.on("data", () => {});
    req.on("end", () => {
      seen.push({ url: req.url, key: req.headers["x-api-key"], auth: req.headers.authorization });
      res.setHeader("content-type", "application/json");
      res.end(req.url.startsWith("/v1/models") ? '{"data":[]}' : '{"id":"msg_1","type":"message"}');
    });
  });
  await new Promise((resolve) => upstream.listen(0, "127.0.0.1", resolve));
  t.after(() => upstream.close());
  const calibrationFile = join(STATUS_DIR, `calibration-key-test-${process.pid}.json`);
  t.after(() => rmSync(calibrationFile, { force: true }));
  const { port, close } = await startProxy({
    upstreamURL: `http://127.0.0.1:${upstream.address().port}`,
    route: async () => ({ choice: "claude-sonnet-5-5", confidence: 0.9, ms: 1 }),
    calibrationFile,
  });
  t.after(close);

  const headers = { "content-type": "application/json", "x-api-key": "sk-ant-api03-test" };
  await fetch(`http://127.0.0.1:${port}/v1/models`, { headers });
  for (const model of ["jev-router", "claude-opus-5-5"]) {
    await fetch(`http://127.0.0.1:${port}/v1/messages`, {
      method: "POST",
      headers,
      body: JSON.stringify({ model, tools: [{ name: "Bash" }], messages: [{ role: "user", content: "hi" }] }),
    });
  }
  // An auth token, as ANTHROPIC_AUTH_TOKEN or a gateway sends it, passes through the same way.
  await fetch(`http://127.0.0.1:${port}/v1/messages`, {
    method: "POST",
    headers: { "content-type": "application/json", authorization: "Bearer gateway-token" },
    body: JSON.stringify({ model: "claude-opus-5-5", messages: [{ role: "user", content: "hi" }] }),
  });

  assert.deepEqual(seen.map((s) => s.key), ["sk-ant-api03-test", "sk-ant-api03-test", "sk-ant-api03-test", undefined]);
  assert.equal(seen[3].auth, "Bearer gateway-token");
});

test("Claude proxy sends exact account models to Jev and routes the chosen version", async (t) => {
  const seen = [];
  const upstream = http.createServer((req, res) => {
    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", () => {
      if (req.url.startsWith("/v1/models")) {
        res.setHeader("content-type", "application/json");
        return res.end(JSON.stringify({
          data: [
            { id: "claude-opus-4-8", display_name: "Claude Opus 4.8", created_at: "2026-01-05" },
            { id: "claude-opus-5-5", display_name: "Claude Opus 5.5", created_at: "2026-09-02" },
            { id: "claude-sonnet-5-5", display_name: "Claude Sonnet 5.5", created_at: "2026-08-11" },
          ],
        }));
      }
      seen.push(JSON.parse(Buffer.concat(chunks)));
      res.setHeader("content-type", "application/json");
      res.end('{"id":"msg_1","type":"message","model":"claude-opus-5-5"}');
    });
  });
  await new Promise((resolve) => upstream.listen(0, "127.0.0.1", resolve));
  t.after(() => upstream.close());

  // A scratch file, so the test never writes the real status line notice.
  const calibrationFile = join(STATUS_DIR, `calibration-proxy-test-${process.pid}.json`);
  t.after(() => rmSync(calibrationFile, { force: true }));
  const { port, close } = await startProxy({
    upstreamURL: `http://127.0.0.1:${upstream.address().port}`,
    // Only the newest of each tier is on the menu, newest first: the older Opus the account
    // can still reach is not a choice Jev gets to make.
    route: async ({ models }) => {
      assert.deepEqual(models.map((model) => model.id), ["claude-opus-5-5", "claude-sonnet-5-5"]);
      return { choice: "claude-opus-5-5", confidence: 0.91, ms: 1 };
    },
    calibrationFile,
  });
  t.after(close);

  await fetch(`http://127.0.0.1:${port}/v1/models`).then((response) => response.json());
  const recorded = readCalibration(calibrationFile);
  assert.deepEqual(recorded.models, ["claude-opus-5-5", "claude-sonnet-5-5"], "the account's newest per tier");
  assert.deepEqual(recorded.newer, [], "nothing newer than the router was tuned for");
  await fetch(`http://127.0.0.1:${port}/v1/messages`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      model: "jev-router",
      tools: [{ name: "Bash" }],
      messages: [{ role: "user", content: "debug this race" }],
    }),
  });

  assert.equal(seen[0].model, "claude-opus-5-5");
});

test("a routed request without metadata is recorded under the conversation key", async (t) => {
  const upstream = http.createServer((req, res) => {
    req.on("data", () => {});
    req.on("end", () => {
      res.setHeader("content-type", "application/json");
      res.end('{"id":"msg_1","type":"message","model":"claude-sonnet-5-5"}');
    });
  });
  await new Promise((resolve) => upstream.listen(0, "127.0.0.1", resolve));
  t.after(() => upstream.close());

  const { port, close } = await startProxy({
    upstreamURL: `http://127.0.0.1:${upstream.address().port}`,
    route: async () => ({ choice: "claude-sonnet-5-5", confidence: 0.77, ms: 1 }),
  });
  t.after(close);

  // Exactly what `claude -p` sends first: no metadata, so no session id.
  const body = {
    model: "jev-router",
    tools: [{ name: "Bash" }],
    messages: [{ role: "user", content: `rename this variable ${process.pid}` }],
  };
  await fetch(`http://127.0.0.1:${port}/v1/messages`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });

  assert.equal(sessionOf(body), "", "the request carries no session id");
  const status = readStatus(conversationKey(body));
  assert.ok(status, "the decision is filed under the conversation key instead of being dropped");
  assert.equal(status.tier, "sonnet");
  assert.equal(status.confidence, 0.77);
  assert.equal(status.effort, "high", "records the effort that went out, here Sonnet's own since the request named none");
  assert.equal(agentView(status).main.effort, "high", "and carries it to the per-agent entry the status line reads");
});

const withTools = (messages) => ({ tools: [{ name: "Bash" }], messages });

test("converts a draft-04 boolean exclusiveMinimum into a draft 2020-12 number", () => {
  const schema = { type: "object", properties: { topN: { minimum: 0, exclusiveMinimum: true } } };
  sanitizeSchema(schema);
  assert.deepEqual(schema.properties.topN, { exclusiveMinimum: 0 });
});

test("drops a false exclusiveMaximum and keeps the bound", () => {
  const schema = { properties: { n: { maximum: 10, exclusiveMaximum: false } } };
  sanitizeSchema(schema);
  assert.deepEqual(schema.properties.n, { maximum: 10 });
});

test("leaves an already-valid numeric bound alone", () => {
  const schema = { properties: { n: { exclusiveMinimum: 5 } } };
  sanitizeSchema(schema);
  assert.equal(schema.properties.n.exclusiveMinimum, 5);
});

test("reaches schemas nested in arrays and sub-objects", () => {
  const schema = { anyOf: [{ items: { minimum: 1, exclusiveMinimum: true } }] };
  sanitizeSchema(schema);
  assert.deepEqual(schema.anyOf[0].items, { exclusiveMinimum: 1 });
});

test("survives null and primitive nodes", () => {
  assert.doesNotThrow(() => sanitizeSchema(null));
  assert.doesNotThrow(() => sanitizeSchema({ a: null, b: 3, c: "x" }));
});

test("reads a plain string prompt as a new turn", () => {
  assert.equal(newTurnPrompt(withTools([{ role: "user", content: "fix the bug" }])), "fix the bug");
});

test("reads a text block prompt as a new turn", () => {
  const body = withTools([{ role: "user", content: [{ type: "text", text: "fix the bug" }] }]);
  assert.equal(newTurnPrompt(body), "fix the bug");
});

test("hook context after the prompt does not hide the turn", () => {
  const body = withTools([
    { role: "user", content: "refactor the parser" },
    { role: "system", content: [{ type: "text", text: "SessionStart hook additional context: ..." }] },
  ]);
  assert.equal(newTurnPrompt(body), "refactor the parser");
});

test("ignores a tool_result continuation mid-turn", () => {
  const body = withTools([
    { role: "user", content: "fix the bug" },
    { role: "assistant", content: [{ type: "tool_use", id: "t1", name: "Bash", input: {} }] },
    { role: "user", content: [{ type: "tool_result", tool_use_id: "t1", content: "done" }] },
  ]);
  assert.equal(newTurnPrompt(body), null);
});

test("ignores auxiliary calls that carry no tools", () => {
  const body = { messages: [{ role: "user", content: "summarise this" }] };
  assert.equal(newTurnPrompt(body), null);
});

test("ignores a request whose last message is from the assistant", () => {
  const body = withTools([{ role: "assistant", content: "thinking" }]);
  assert.equal(newTurnPrompt(body), null);
});

test("ignores an empty prompt", () => {
  assert.equal(newTurnPrompt(withTools([{ role: "user", content: "   " }])), null);
});

test("survives a malformed body", () => {
  assert.equal(newTurnPrompt(undefined), null);
  assert.equal(newTurnPrompt({}), null);
  assert.equal(newTurnPrompt({ tools: [], messages: [] }), null);
});

test("strips system reminders Claude Code injects into the prompt", () => {
  const body = withTools([
    {
      role: "user",
      content: "fix the bug\n<system-reminder>be careful\nabout things</system-reminder>",
    },
  ]);
  assert.equal(newTurnPrompt(body), "fix the bug");
});

test("a prompt that is only a system reminder is not a turn", () => {
  const body = withTools([{ role: "user", content: "<system-reminder>noise</system-reminder>" }]);
  assert.equal(newTurnPrompt(body), null);
});

test("routing to haiku strips fields haiku cannot accept", () => {
  const body = {
    model: "claude-sonnet-4-6",
    thinking: { type: "adaptive" },
    output_config: { effort: "medium" },
    context_management: { edits: [{ type: "clear_thinking_20251015", keep: "all" }] },
  };
  applyTier(body, "haiku");
  assert.equal(body.model, "claude-haiku-4-5-20251001");
  assert.equal(body.thinking, undefined);
  assert.equal(body.output_config, undefined);
  assert.equal(body.context_management, undefined);
});

test("routing to haiku keeps context-management strategies unrelated to thinking", () => {
  const body = {
    model: "claude-sonnet-4-6",
    context_management: { edits: [{ type: "clear_tool_uses_20250919" }, { type: "clear_thinking_20251015" }] },
  };
  applyTier(body, "haiku");
  assert.deepEqual(body.context_management, { edits: [{ type: "clear_tool_uses_20250919" }] });
});

test("routing to opus leaves thinking and effort intact", () => {
  const body = {
    model: "claude-sonnet-4-6",
    thinking: { type: "adaptive" },
    output_config: { effort: "medium" },
  };
  applyTier(body, "opus");
  assert.equal(body.model, "claude-opus-5-5");
  assert.deepEqual(body.thinking, { type: "adaptive" });
  assert.deepEqual(body.output_config, { effort: "medium" });
});

test("an unknown tier leaves the request untouched", () => {
  const body = { model: "claude-sonnet-4-6", thinking: { type: "adaptive" } };
  applyTier(body, "nonsense");
  assert.equal(body.model, "claude-sonnet-4-6");
});

test("names each tier's own effort when the request does not", () => {
  const opus = { model: "jev-router", thinking: { type: "adaptive" } };
  applyTier(opus, "opus");
  assert.deepEqual(opus.output_config, { effort: "medium" });
  const sonnet = { model: "jev-router", thinking: { type: "adaptive" } };
  applyTier(sonnet, "sonnet");
  assert.deepEqual(sonnet.output_config, { effort: "high" });
});

test("JEV_<TIER>_EFFORT overrides a tier's effort, and a bad value is ignored", () => {
  assert.equal(effortFloor("opus", { JEV_OPUS_EFFORT: "High" }), "high");
  assert.equal(effortFloor("sonnet", { JEV_SONNET_EFFORT: "low" }), "low");
  assert.equal(effortFloor("opus", { JEV_OPUS_EFFORT: "turbo" }), "medium", "unknown value falls back to the default");
  assert.equal(effortFloor("opus", {}), "medium");
  assert.equal(effortFloor("haiku", { JEV_HAIKU_EFFORT: "high" }), null, "haiku takes no effort");
});

test("a new major version is picked as the newest of its tier, dated or not", () => {
  const ids = (catalog) => newestPerTier(claudeModels(catalog)).map((m) => m.id);
  assert.deepEqual(ids([{ id: "claude-opus-5-5" }, { id: "claude-opus-6" }, { id: "claude-opus-4-8" }]), ["claude-opus-6"]);
  assert.deepEqual(ids([{ id: "claude-sonnet-5-5" }, { id: "claude-sonnet-5-10" }]), ["claude-sonnet-5-10"]);
  assert.deepEqual(
    ids([{ id: "claude-haiku-4-5-20251001" }, { id: "claude-haiku-4-6" }]),
    ["claude-haiku-4-6"],
    "a date suffix is not a minor version",
  );
});

test("keeps an effort the request already carries", () => {
  const body = { model: "jev-router", thinking: { type: "adaptive" }, output_config: { effort: "low" } };
  applyTier(body, "opus");
  assert.deepEqual(body.output_config, { effort: "low" }, "the user's own choice outranks the floor");
});

test("never names an effort for a tier that cannot take one", () => {
  const body = { model: "jev-router", output_config: { effort: "high" } };
  applyTier(body, "haiku");
  assert.equal(body.output_config, undefined);
});

test("a conversation keeps one key as it grows, and differs from a sub-agent", () => {
  const main = { messages: [{ role: "user", content: "main task" }] };
  const grown = {
    messages: [{ role: "user", content: "main task" }, { role: "assistant", content: "ok" }],
  };
  const sub = { messages: [{ role: "user", content: "sub-agent task" }] };
  assert.equal(conversationKey(main), conversationKey(grown));
  assert.notEqual(conversationKey(main), conversationKey(sub));
});

test("the key ignores the cache_control breakpoint Claude Code moves between requests", () => {
  const first = {
    messages: [
      {
        role: "user",
        content: [
          { type: "text", text: "<system-reminder>x</system-reminder>" },
          { type: "text", text: "do the thing", cache_control: { type: "ephemeral", ttl: "1h" } },
        ],
      },
    ],
  };
  const later = {
    messages: [
      {
        role: "user",
        content: [
          { type: "text", text: "<system-reminder>x</system-reminder>" },
          { type: "text", text: "do the thing" },
        ],
      },
      { role: "assistant", content: "working" },
    ],
  };
  assert.equal(conversationKey(first), conversationKey(later));
});

test("the same opening text in two sessions gets two keys", () => {
  const mk = (id) => ({
    metadata: { user_id: JSON.stringify({ session_id: id }) },
    messages: [{ role: "user", content: "same opening" }],
  });
  assert.notEqual(conversationKey(mk("a")), conversationKey(mk("b")));
});

test("the key survives metadata that is not JSON", () => {
  const body = { metadata: { user_id: "not-json" }, messages: [{ role: "user", content: "hi" }] };
  assert.doesNotThrow(() => conversationKey(body));
});

test("the first tool-bearing conversation in a session is the main thread", () => {
  const mains = new Map();
  const sid = JSON.stringify({ session_id: "s-main" });
  const mk = (text) => ({
    metadata: { user_id: sid },
    tools: [{ name: "Read" }],
    messages: [{ role: "user", content: text }],
  });

  const main = agentOf(mk("the user's opening prompt"), mains);
  const sub = agentOf(mk("search the repo for conversationKey"), mains);

  assert.equal(main.main, true);
  assert.equal(sub.main, false, "a later conversation in the same session is a sub-agent");
  assert.notEqual(main.key, sub.key);
  assert.equal(sub.label, "search the repo for conversationKey");
  assert.equal(agentOf(mk("the user's opening prompt"), mains).main, true, "the main key is stable");
});

test("auxiliary calls without tools never claim the main slot", () => {
  const mains = new Map();
  const sid = JSON.stringify({ session_id: "s-aux" });
  const aux = agentOf(
    { metadata: { user_id: sid }, messages: [{ role: "user", content: "summarise this" }] },
    mains,
  );
  const real = agentOf(
    {
      metadata: { user_id: sid },
      tools: [{ name: "Read" }],
      messages: [{ role: "user", content: "the real prompt" }],
    },
    mains,
  );
  assert.equal(aux.main, false, "a toolless call never registers itself as the main thread");
  assert.equal(real.main, true, "the first real agent turn is the main thread");
  assert.notEqual(mains.get("s-aux"), aux.key, "the aux call did not take the main slot");
  assert.equal(mains.get("s-aux"), real.key);
});

test("agent labels are trimmed of reminders and length", () => {
  assert.equal(
    agentLabel({ messages: [{ role: "user", content: "<system-reminder>noise</system-reminder> real task" }] }),
    "real task",
  );
  assert.equal(agentLabel({ messages: [{ role: "user", content: "x".repeat(80) }] }).length, 48);
  assert.equal(agentLabel({}), "");
});

test("each agent's model is recorded separately within one session", () => {
  const sid = `agents-${process.pid}`;
  const main = { key: "k-main", label: "fix the race", main: true };
  const sub = { key: "k-sub", label: "grep for callers", main: false };

  writeDecision(sid, { tier: "opus", model: "claude-opus-5-5", confidence: 0.94, at: 1000 }, main);
  writeDecision(sid, { tier: "haiku", model: "claude-haiku-4-5", confidence: 0.81, at: 2000 }, sub);

  const view = agentView(readStatus(sid), { now: 2000 });
  assert.equal(view.main.tier, "opus", "a sub-agent's choice does not overwrite the main thread");
  assert.equal(view.main.label, "fix the race");
  assert.equal(view.subagents.length, 1);
  assert.equal(view.subagents[0].tier, "haiku");
  assert.equal(readStatus(sid).history.length, 2, "history still records every decision");
});

test("stale sub-agents drop out of the live view but the main thread stays", () => {
  const sid = `stale-agents-${process.pid}`;
  writeDecision(sid, { tier: "opus", at: 0 }, { key: "m", label: "main", main: true });
  writeDecision(sid, { tier: "haiku", at: 0 }, { key: "s", label: "old sub", main: false });

  const view = agentView(readStatus(sid), { now: 10 * 60_000 });
  assert.equal(view.main.tier, "opus");
  assert.deepEqual(view.subagents, [], "a sub-agent that has not been routed recently is not live");
});

test("a sub-agent pinned to its own model does not pause the session", () => {
  const sid = `manual-agents-${process.pid}`;
  writeDecision(sid, { tier: "opus", model: "claude-opus-5-5", at: 1000 }, { key: "m", label: "main", main: true });
  markManual(sid, "claude-haiku-4-5", { key: "s", label: "pinned sub", main: false });

  const status = readStatus(sid);
  assert.equal(status.manual, false, "only the main thread's choice pauses routing");
  assert.equal(agentView(status).main.tier, "opus", "the main decision survives a sub-agent's write");
  assert.equal(agentView(status).subagents[0].manual, true);

  markManual(sid, "claude-sonnet-5", { key: "m", label: "main", main: true });
  assert.equal(readStatus(sid).manual, true, "the main thread picking a model does pause it");
});

test("an id in the old version-first naming never outranks a current model", () => {
  const catalog = [
    { id: "claude-3-7-sonnet-20250219" },
    { id: "claude-sonnet-5-5" },
    { id: "claude-3-5-haiku-20241022" },
    { id: "claude-haiku-4-5-20251001" },
  ];
  assert.deepEqual(
    newestPerTier(claudeModels(catalog)).map((m) => m.id),
    ["claude-sonnet-5-5", "claude-haiku-4-5-20251001"],
  );
  assert.deepEqual(newerThanCalibrated(catalog), [], "a retired model is not news");
});

test("a provider prefix does not hide the version", () => {
  assert.deepEqual(
    newestPerTier(claudeModels([{ id: "anthropic.claude-opus-5-5" }, { id: "anthropic.claude-opus-6" }])).map((m) => m.id),
    ["anthropic.claude-opus-6"],
  );
});

test("flags a model newer than the router was calibrated for, and nothing else", () => {
  assert.deepEqual(newerThanCalibrated([]), [], "no catalog yet: nothing to report");
  assert.deepEqual(
    newerThanCalibrated([{ id: "claude-opus-5-5" }, { id: "claude-sonnet-5-5" }, { id: "claude-opus-4-8" }]),
    [],
    "the calibrated versions and older ones are not news",
  );
  assert.deepEqual(
    newerThanCalibrated([{ id: "claude-opus-6" }, { id: "claude-opus-5-5" }, { id: "claude-sonnet-5-5" }]),
    ["claude-opus-6"],
  );
});

test("the calibration notice round-trips and reads empty when absent", () => {
  const file = join(STATUS_DIR, `calibration-test-${process.pid}.json`);
  assert.deepEqual(readCalibration(file), { newer: [], models: [], at: null });
  writeCalibration({ newer: ["claude-opus-6"], models: ["claude-opus-6", "claude-sonnet-5-5"] }, file);
  const read = readCalibration(file);
  assert.deepEqual(read.newer, ["claude-opus-6"]);
  assert.deepEqual(read.models, ["claude-opus-6", "claude-sonnet-5-5"]);
  assert.equal(typeof read.at, "number");
  writeCalibration({}, file);
  assert.deepEqual(readCalibration(file).newer, []);
  rmSync(file, { force: true });
});
