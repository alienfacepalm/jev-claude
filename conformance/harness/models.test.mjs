// GET /v1/models through the proxy under test: the reply is relayed whatever its status, the
// account's catalog feeds the Jev menu, and calibration.json records it (SPEC 7.2 step 5, 8.3).
import assert from "node:assert/strict";
import { join } from "node:path";
import {
  CLIENT_HEADERS,
  assertForwardedHeaders,
  assertJevCall,
  assertRelayedHeaders,
  assertStatusFile,
  contextTokensOf,
  fixtureBody,
  jevAnswer,
  jevRequest,
  node,
  readJSON,
  rewritten,
  send,
  setup,
  FIXTURE,
  test,
} from "./helpers.mjs";

const CATALOG = {
  data: [
    { type: "model", id: "claude-opus-4-8", display_name: "Claude Opus 4.8", created_at: "2026-01-05T00:00:00Z" },
    { type: "model", id: "claude-opus-5-5", display_name: "Claude Opus 5.5", created_at: "2026-09-02T00:00:00Z", max_input_tokens: 1000000 },
    { type: "model", id: "claude-opus-6", display_name: "Claude Opus 6", created_at: "2026-11-01T00:00:00Z", max_input_tokens: 1000000 },
    { type: "model", id: "claude-sonnet-5-5", display_name: "Claude Sonnet 5.5", created_at: "2026-08-11T00:00:00Z" },
    { type: "model", id: "claude-haiku-4-5-20251001", display_name: "Claude Haiku 4.5", created_at: "2025-10-01T00:00:00Z", max_input_tokens: 200000 },
    { type: "model", id: "claude-3-7-sonnet-20250219", display_name: "Claude Sonnet 3.7", created_at: "2025-02-19T00:00:00Z" },
  ],
  has_more: false,
  first_id: "claude-opus-4-8",
};
const MODELS_HEADERS = { "content-type": "application/json", "request-id": "req_models", "anthropic-organization-id": "org_harness", "x-cache": "miss" };

/** An upstream whose /v1/models answers with `reply` and whose messages endpoint answers simply. */
const upstreamWith = (reply) => (record, res) => {
  if (record.url.startsWith("/v1/models")) {
    const r = reply();
    res.writeHead(r.status, r.headers ?? MODELS_HEADERS);
    return res.end(r.body);
  }
  res.writeHead(200, { "content-type": "application/json" });
  res.end('{"id":"msg_1","type":"message"}');
};

const calibrationOf = (proxy) => readJSON(join(proxy.dirs.status, "calibration.json"));
const modelsHeaders = { "anthropic-version": "2023-06-01", "x-api-key": "sk-ant-api03-harness", "accept-encoding": "gzip, br", "user-agent": "claude-cli/2.1.287" };

test("a 200 catalog is relayed, recorded for calibration, and becomes the Jev menu", async (t) => {
  const catalogText = JSON.stringify(CATALOG);
  const { jev, upstream, proxy } = await setup(t, {
    answer: () => jevAnswer("claude-opus-6", 0.88, [6, 7, 5]),
    upstream: upstreamWith(() => ({ status: 200, body: catalogText })),
  });
  const from = Date.now();
  const res = await send(proxy.port, { method: "GET", path: "/v1/models?limit=1000", headers: modelsHeaders });
  const afterModels = Date.now();

  assert.equal(res.status, 200);
  assert.equal(res.body.toString(), catalogText, "relayed byte for byte");
  assertRelayedHeaders(MODELS_HEADERS, res.headers);
  const forwarded = upstream.requests[0];
  assert.equal(forwarded.method, "GET");
  assert.equal(forwarded.url, "/v1/models?limit=1000");
  assert.equal(forwarded.body.length, 0);
  assertForwardedHeaders(res.sent, forwarded, { host: upstream.url.slice("http://".length), body: Buffer.alloc(0), acceptEncodingRemoved: true });

  const menu = node.proxy.newestPerTier(node.proxy.claudeModels(CATALOG.data));
  assert.deepEqual(
    menu.map((m) => m.id),
    ["claude-opus-6", "claude-sonnet-5-5", "claude-haiku-4-5-20251001"],
  );
  assertStatusFile(calibrationOf(proxy), { newer: ["claude-opus-6"], models: menu.map((m) => m.id), at: 0 }, { from, to: afterModels });

  const body = fixtureBody("harness-catalog");
  await send(proxy.port, { path: FIXTURE.url, headers: { ...CLIENT_HEADERS }, body });
  const request = jevRequest({ prompt: node.proxy.newTurnPrompt(body), current: "claude-sonnet-5-5", contextTokens: contextTokensOf(body), models: menu });
  assertJevCall(jev.posts()[0], request);
  assert.equal(upstream.requests[1].body.toString(), rewritten(body, "opus", "claude-opus-6").toString(), "Jev's exact model is used");
  const status = readJSON(join(proxy.dirs.status, "harness-catalog.json"));
  assert.equal(status.model, "claude-opus-6");
  assert.equal(status.tier, "opus");
});

test("a 401 is relayed as sent and calibration falls back to the configured models", async (t) => {
  const error = '{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}';
  const { upstream, proxy } = await setup(t, { upstream: upstreamWith(() => ({ status: 401, body: error })) });
  const from = Date.now();
  const res = await send(proxy.port, { method: "GET", path: "/v1/models", headers: modelsHeaders });
  const to = Date.now();

  assert.equal(res.status, 401);
  assert.equal(res.body.toString(), error);
  assertRelayedHeaders(MODELS_HEADERS, res.headers);
  assertForwardedHeaders(res.sent, upstream.requests[0], { host: upstream.url.slice("http://".length), body: Buffer.alloc(0), acceptEncodingRemoved: true });
  assertStatusFile(calibrationOf(proxy), { newer: [], models: ["claude-haiku-4-5-20251001", "claude-sonnet-5-5", "claude-opus-5-5", "claude-fable-5-1"], at: 0 }, { from, to });
});

test("a catalog whose data cannot be iterated, or that is not JSON, writes no calibration file", async (t) => {
  const replies = [
    { status: 200, body: '{"data":{"id":"claude-opus-6"}}' },
    { status: 200, body: '{"data":5}' },
    { status: 502, body: "<html>bad gateway</html>", headers: { "content-type": "text/html" } },
    // A string iterates its characters, none of which is a model, so the static ids are written.
    { status: 200, body: '{"data":"claude-opus-6"}' },
  ];
  let next = 0;
  const { proxy } = await setup(t, { upstream: upstreamWith(() => replies[next++]) });

  for (const reply of replies.slice(0, 3)) {
    const res = await send(proxy.port, { method: "GET", path: "/v1/models", headers: modelsHeaders });
    assert.equal(res.status, reply.status);
    assert.equal(res.body.toString(), reply.body, "relayed whatever happened to the catalog");
    assert.equal(calibrationOf(proxy), null, `no calibration file after ${reply.body}`);
  }
  const from = Date.now();
  await send(proxy.port, { method: "GET", path: "/v1/models", headers: modelsHeaders });
  assertStatusFile(calibrationOf(proxy), { newer: [], models: ["claude-haiku-4-5-20251001", "claude-sonnet-5-5", "claude-opus-5-5", "claude-fable-5-1"], at: 0 }, { from, to: Date.now() });
});
