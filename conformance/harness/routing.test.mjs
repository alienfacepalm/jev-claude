// Routed, passthrough, auxiliary, continuation, and failure turns through the proxy under test,
// with the Jev request, the forwarded request, the status file, and the status line checked for
// each (SPEC 7.2-7.7, 8.2, 12, 16.3).
import test from "node:test";
import assert from "node:assert/strict";
import { join } from "node:path";
import {
  CLIENT_HEADERS,
  DIM,
  RESET,
  SEP,
  STATIC_MENU,
  assertForwardedHeaders,
  assertJevCall,
  assertRelayedHeaders,
  assertStatusFile,
  contextTokensOf,
  continuation,
  fixtureBody,
  icon,
  jevAnswer,
  jevRequest,
  node,
  readJSON,
  rewritten,
  routedStatus,
  runStatusLine,
  send,
  setup,
  UPSTREAM_REPLY_HEADERS,
  FIXTURE,
} from "./helpers.mjs";

const HAIKU = "claude-haiku-4-5-20251001";
const SONNET = "claude-sonnet-5-5";
const GREEN = "\x1b[32m";
const CYAN = "\x1b[36m";
const PATH = FIXTURE.url;

/** The status line's stdin for a session, in a directory that does not exist (so no git branch). */
const statusInput = (proxy, session, extra = {}) =>
  JSON.stringify({ session_id: session, workspace: { current_dir: join(proxy.dirs.base, "no-such-dir", "proj") }, context_window: { used_percentage: 12.5 }, ...extra });
const tail = `${SEP}${icon("❐")} proj${SEP}${icon("≡")} 13%\n`;

const fileOf = (proxy, session) => readJSON(join(proxy.dirs.status, `${session}.json`));

test("a routed turn: the Jev request, the rewritten request, the status file, and the status line", async (t) => {
  const session = "harness-routed";
  const answer = jevAnswer(HAIKU, 0.92, [2, 3, 1]);
  const { jev, upstream, proxy } = await setup(t, { answer: () => answer });
  const body = fixtureBody(session);
  const from = Date.now();

  const res = await send(proxy.port, { path: PATH, headers: CLIENT_HEADERS, body });
  const to = Date.now();

  assert.equal(res.status, 200);
  assert.equal(res.body.toString(), '{"id":"msg_1","type":"message","model":"claude-haiku-4-5-20251001","content":[]}');
  assertRelayedHeaders(UPSTREAM_REPLY_HEADERS, res.headers);

  const prompt = node.proxy.newTurnPrompt(body);
  assert.equal(prompt, "rename the variable x to count in utils.js", "reminders stripped and the trailing system message skipped");
  const request = jevRequest({ prompt, current: SONNET, contextTokens: contextTokensOf(body), models: STATIC_MENU });
  assert.equal(jev.posts().length, 1, "Jev is asked once");
  assertJevCall(jev.posts()[0], request);

  assert.equal(upstream.requests.length, 1);
  const forwarded = upstream.requests[0];
  assert.equal(forwarded.method, "POST");
  assert.equal(forwarded.url, PATH);
  const expectedBody = rewritten(body, "haiku", HAIKU);
  assert.equal(forwarded.body.toString(), expectedBody.toString(), "the body is the captured request routed to Haiku");
  const sent = JSON.parse(forwarded.body);
  assert.equal(sent.model, HAIKU);
  assert.equal(sent.thinking, undefined, "Haiku takes no adaptive thinking");
  assert.equal(sent.context_management, undefined, "nor the thinking-clearing edit");
  assert.equal(sent.output_config, undefined, "nor an effort");
  assertForwardedHeaders(res.sent, forwarded, { host: upstream.url.slice("http://".length), body: expectedBody });

  const key = node.proxy.conversationKey(body);
  const label = node.proxy.agentLabel(body);
  const decision = routedStatus({ body, tier: "haiku", model: HAIKU, jev: answer, reason: "jev", request, effort: null });
  const agent = { label, main: true, tier: "haiku", model: HAIKU, confidence: 0.92, effort: null, reason: "jev", at: 0 };
  assertStatusFile(
    fileOf(proxy, session),
    { ...decision, agents: { [key]: agent }, history: [{ ...decision, agent: { key, label, main: true } }] },
    { from, to },
  );

  const line = runStatusLine(proxy.env, proxy.dirs.cwd, statusInput(proxy, session));
  assert.equal(line, `${icon("✧✦")} ${GREEN}Haiku 4.5${RESET} ${DIM}(92%)${RESET}${tail}`);

  // The fire-and-forget prewarm (SPEC 5.4) reaches Jev's origin.
  const deadline = Date.now() + 3000;
  while (!jev.requests.some((r) => r.method === "HEAD") && Date.now() < deadline) await new Promise((r) => setTimeout(r, 50));
  assert.ok(jev.requests.some((r) => r.method === "HEAD" && r.url === "/"), "a HEAD request warmed the Jev connection");
});

test("an auxiliary call is not routed, a tool continuation keeps the tier, and a sub-agent gets its own", async (t) => {
  const session = "harness-continuation";
  const subTask = "Search the codebase for callers of handler 7 and report them";
  const answers = {
    "rename the variable x to count in utils.js": jevAnswer(HAIKU, 0.92, [2, 3, 1]),
    [subTask]: jevAnswer(SONNET, 0.81, [4, 4, 5]),
  };
  const { jev, upstream, proxy } = await setup(t, { answer: (req) => answers[req.state.request] });
  const body = fixtureBody(session);
  const from = Date.now();

  // Claude Code's title call: the sentinel, no tools. It runs on the default tier, asks nobody,
  // records nothing, and does not become the session's main thread.
  const aux = { model: "jev-router", metadata: body.metadata, max_tokens: 32, messages: [{ role: "user", content: "Summarize this conversation in five words" }] };
  await send(proxy.port, { path: PATH, headers: CLIENT_HEADERS, body: aux });
  assert.equal(jev.posts().length, 0, "no Jev call for a call without tools");
  assert.equal(upstream.requests[0].body.toString(), rewritten(aux, "sonnet", SONNET).toString(), "the default tier, with its effort");
  assert.equal(fileOf(proxy, session), null, "nothing recorded");

  await send(proxy.port, { path: PATH, headers: CLIENT_HEADERS, body });
  const afterTurn = fileOf(proxy, session);
  assert.equal(jev.posts().length, 1);

  const next = continuation(body);
  await send(proxy.port, { path: PATH, headers: CLIENT_HEADERS, body: next });
  assert.equal(jev.posts().length, 1, "Jev is asked once per turn, not per tool call");
  assert.equal(upstream.requests[2].body.toString(), rewritten(next, "haiku", HAIKU).toString(), "the continuation stays on Haiku");
  assert.deepEqual(fileOf(proxy, session), afterTurn, "a continuation records nothing new");

  const sub = fixtureBody(session);
  sub.messages[0].content.at(-1).text = subTask;
  await send(proxy.port, { path: PATH, headers: CLIENT_HEADERS, body: sub });
  const to = Date.now();
  assert.equal(jev.posts().length, 2);
  const subRequest = jevRequest({ prompt: subTask, current: SONNET, contextTokens: contextTokensOf(sub), models: STATIC_MENU });
  assertJevCall(jev.posts()[1], subRequest);
  assert.equal(upstream.requests[3].body.toString(), rewritten(sub, "sonnet", SONNET).toString());

  const mainKey = node.proxy.conversationKey(body);
  const subKey = node.proxy.conversationKey(sub);
  const mainLabel = node.proxy.agentLabel(body);
  const subLabel = node.proxy.agentLabel(sub);
  const mainRequest = jevRequest({ prompt: node.proxy.newTurnPrompt(body), current: SONNET, contextTokens: contextTokensOf(body), models: STATIC_MENU });
  const first = routedStatus({ body, tier: "haiku", model: HAIKU, jev: answers["rename the variable x to count in utils.js"], reason: "jev", request: mainRequest, effort: null });
  const second = routedStatus({ body: sub, tier: "sonnet", model: SONNET, jev: answers[subTask], reason: "jev/no-change", request: subRequest, effort: "high" });
  assertStatusFile(
    fileOf(proxy, session),
    {
      ...second,
      agents: {
        [mainKey]: { label: mainLabel, main: true, tier: "haiku", model: HAIKU, confidence: 0.92, effort: null, reason: "jev", at: 0 },
        [subKey]: { label: subLabel, main: false, tier: "sonnet", model: SONNET, confidence: 0.81, effort: "high", reason: "jev/no-change", at: 0 },
      },
      history: [
        { ...first, agent: { key: mainKey, label: mainLabel, main: true } },
        { ...second, agent: { key: subKey, label: subLabel, main: false } },
      ],
    },
    { from, to },
  );

  const line = runStatusLine(proxy.env, proxy.dirs.cwd, statusInput(proxy, session));
  assert.equal(line, `${icon("✧✦")} ${GREEN}Haiku 4.5${RESET} ${DIM}(92%)${RESET}${SEP}${icon("✦")} ${CYAN}Sonnet 5.5${RESET}${tail}`);
});

test("a model the user picked passes through untouched and pauses routing", async (t) => {
  const session = "harness-manual";
  const { jev, upstream, proxy } = await setup(t);
  const body = { ...fixtureBody(session), model: "claude-opus-5-5" };
  const raw = Buffer.from(JSON.stringify(body));
  const from = Date.now();

  const res = await send(proxy.port, { path: PATH, headers: CLIENT_HEADERS, body: raw });
  const to = Date.now();
  assert.equal(res.status, 200);
  assert.equal(jev.posts().length, 0, "the user's choice is not second-guessed");
  assert.equal(upstream.requests[0].body.toString(), rewritten(body, "nonsense").toString());
  assert.equal(upstream.requests[0].body.toString(), raw.toString(), "a compact body comes out byte for byte");
  assertForwardedHeaders(res.sent, upstream.requests[0], { host: upstream.url.slice("http://".length), body: raw });

  const key = node.proxy.conversationKey(body);
  const label = node.proxy.agentLabel(body);
  const manual = { agents: { [key]: { label, main: true, model: "claude-opus-5-5", manual: true, at: 0 } }, manual: true, at: 0 };
  assertStatusFile(fileOf(proxy, session), manual, { from, to });

  // A continuation of that turn, and Claude Code's own Haiku title call, change nothing.
  const before = fileOf(proxy, session);
  await send(proxy.port, { path: PATH, headers: CLIENT_HEADERS, body: continuation(body) });
  const title = { model: HAIKU, metadata: body.metadata, max_tokens: 32, messages: [{ role: "user", content: "title this" }] };
  await send(proxy.port, { path: PATH, headers: CLIENT_HEADERS, body: title });
  assert.equal(upstream.requests[2].body.toString(), JSON.stringify(title));
  assert.deepEqual(fileOf(proxy, session), before);
  assert.equal(jev.posts().length, 0);

  const line = runStatusLine(proxy.env, proxy.dirs.cwd, statusInput(proxy, session, { model: { display_name: "Opus 5.5" } }));
  assert.equal(line, `${DIM}☞ manual${RESET} Opus 5.5${tail}`);
});

test("a Jev answer naming a model that is not on the menu counts as no answer", async (t) => {
  const session = "harness-off-menu";
  const answer = jevAnswer("claude-opus-4-1", 0.9);
  const { jev, upstream, proxy } = await setup(t, { answer: () => answer });
  const body = fixtureBody(session);
  const from = Date.now();
  await send(proxy.port, { path: PATH, headers: CLIENT_HEADERS, body });
  const to = Date.now();

  const request = jevRequest({ prompt: node.proxy.newTurnPrompt(body), current: SONNET, contextTokens: contextTokensOf(body), models: STATIC_MENU });
  assertJevCall(jev.posts()[0], request);
  assert.equal(upstream.requests[0].body.toString(), rewritten(body, "sonnet", SONNET).toString(), "the current tier, never the sentinel");

  const key = node.proxy.conversationKey(body);
  const label = node.proxy.agentLabel(body);
  const decision = routedStatus({ body, tier: "sonnet", model: SONNET, jev: answer, reason: "jev-unavailable/no-change", request, effort: "high" });
  assertStatusFile(
    fileOf(proxy, session),
    {
      ...decision,
      agents: { [key]: { label, main: true, tier: "sonnet", model: SONNET, confidence: 0.9, effort: "high", reason: "jev-unavailable/no-change", at: 0 } },
      history: [{ ...decision, agent: { key, label, main: true } }],
    },
    { from, to },
  );
  const line = runStatusLine(proxy.env, proxy.dirs.cwd, statusInput(proxy, session));
  assert.equal(line, `${icon("✧✦")} ${CYAN}Sonnet 5.5${RESET} ${DIM}(90%)${RESET}${SEP}${icon("◔")} high ${DIM}(router offline)${RESET}${tail}`);
});

test("when Jev fails after its one retry, the turn runs on the default tier", async (t) => {
  const session = "harness-jev-down";
  const { jev, upstream, proxy } = await setup(t, { answer: () => ({ status: 500, body: '{"error":"down"}' }) });
  const body = fixtureBody(session);
  const from = Date.now();
  const res = await send(proxy.port, { path: PATH, headers: CLIENT_HEADERS, body });
  const to = Date.now();

  assert.equal(res.status, 200, "a Jev failure never reaches the user's request");
  const request = jevRequest({ prompt: node.proxy.newTurnPrompt(body), current: SONNET, contextTokens: contextTokensOf(body), models: STATIC_MENU });
  assert.equal(jev.posts().length, 2, "one attempt and one retry on a 500");
  assertJevCall(jev.posts()[0], request);
  assertJevCall(jev.posts()[1], request, { retry: 1 });
  assert.equal(upstream.requests[0].body.toString(), rewritten(body, "sonnet", SONNET).toString());

  const key = node.proxy.conversationKey(body);
  const label = node.proxy.agentLabel(body);
  const decision = routedStatus({ body, tier: "sonnet", model: SONNET, jev: null, reason: "jev-unavailable/no-change", request, effort: "high" });
  assertStatusFile(
    fileOf(proxy, session),
    {
      ...decision,
      agents: { [key]: { label, main: true, tier: "sonnet", model: SONNET, confidence: null, effort: "high", reason: "jev-unavailable/no-change", at: 0 } },
      history: [{ ...decision, agent: { key, label, main: true } }],
    },
    { from, to },
  );
  const line = runStatusLine(proxy.env, proxy.dirs.cwd, statusInput(proxy, session));
  assert.equal(line, `${icon("✧✦")} ${CYAN}Sonnet 5.5${RESET}${SEP}${icon("◔")} high ${DIM}(router offline)${RESET}${tail}`);
});

test("a token count request is processed like a turn", async (t) => {
  const { jev, upstream, proxy } = await setup(t);
  const body = fixtureBody("harness-count-tokens");
  await send(proxy.port, { path: "/v1/messages/count_tokens?beta=true", headers: CLIENT_HEADERS, body });
  assert.equal(jev.posts().length, 1);
  assert.equal(upstream.requests[0].url, "/v1/messages/count_tokens?beta=true");
  assert.equal(upstream.requests[0].body.toString(), rewritten(body, "haiku", HAIKU).toString());
  assert.ok(fileOf(proxy, "harness-count-tokens"), "and recorded like one");
});

test("malformed and non-object bodies are forwarded, never the sentinel", async (t) => {
  const { jev, upstream, proxy } = await setup(t);
  const cases = [
    ["not JSON is forwarded as sent", "/v1/messages", "{not json", "{not json"],
    ["a JSON null is forwarded as sent", "/v1/messages", "null ", "null "],
    ["a number is re-serialised", "/v1/messages", " 42 ", "42"],
    ["an array is re-serialised", "/v1/messages", "[1, 2,\n3]", "[1,2,3]"],
    ["a string is re-serialised", "/v1/messages", ' "x" ', '"x"'],
    ["other paths are not processed", "/v1/other", '{ "model" : "jev-router" }', '{ "model" : "jev-router" }'],
  ];
  for (const [name, path, sentBody, want] of cases) {
    const before = upstream.requests.length;
    const res = await send(proxy.port, { path, headers: CLIENT_HEADERS, body: sentBody });
    assert.equal(res.status, 200, name);
    const forwarded = upstream.requests[before];
    assert.equal(forwarded.body.toString(), want, name);
    assertForwardedHeaders(res.sent, forwarded, { host: upstream.url.slice("http://".length), body: Buffer.from(want) });
  }

  // A routed body whose processing throws (a null content block) still leaves on a real model:
  // the conversation's tier, here the default, with the effort that tier is given.
  const broken = { model: "jev-router", metadata: fixtureBody("harness-broken").metadata, tools: [{ name: "Bash" }], messages: [{ role: "user", content: [null] }] };
  await send(proxy.port, { path: "/v1/messages", headers: CLIENT_HEADERS, body: broken });
  assert.equal(upstream.requests.at(-1).body.toString(), rewritten(broken, "sonnet", SONNET).toString());
  assert.equal(jev.posts().length, 0, "nothing was asked");
  assert.equal(readJSON(join(proxy.dirs.status, "harness-broken.json")), null, "and nothing recorded");
});
