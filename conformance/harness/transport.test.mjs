// How the proxy under test carries bytes: live streaming, aborts in both directions, the 502 path,
// HEAD probes, redirects, compressed bodies, and an upstream base path (SPEC 7.1, 7.2, 7.7).
import test from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import { gzipSync } from "node:zlib";
import {
  CLIENT_HEADERS,
  assertForwardedHeaders,
  assertRelayedHeaders,
  fakeJev,
  fakeUpstream,
  fixtureBody,
  send,
  setup,
  startProxy,
} from "./helpers.mjs";

// A model the user picked, so these tests exercise transport without asking Jev anything.
const passthrough = () => ({ ...fixtureBody("harness-transport"), model: "claude-opus-5-5", stream: true });
const SSE_HEADERS = { "content-type": "text/event-stream; charset=utf-8", "request-id": "req_stream", "x-harness": "sse" };
const pause = (ms) => new Promise((r) => setTimeout(r, ms));

test("a streamed response reaches the client chunk by chunk, before the upstream finishes", async (t) => {
  const events = ['event: message_start\ndata: {"type":"message_start"}\n\n', 'event: content_block_delta\ndata: {"type":"content_block_delta","delta":{"text":"hi"}}\n\n', 'event: message_stop\ndata: {"type":"message_stop"}\n\n'];
  const writes = [];
  const { proxy } = await setup(t, {
    upstream: async (record, res) => {
      res.writeHead(200, SSE_HEADERS);
      for (const [i, event] of events.entries()) {
        if (i) await pause(400);
        res.write(event);
        writes.push(Date.now());
      }
      res.end();
    },
  });
  const res = await send(proxy.port, { headers: CLIENT_HEADERS, body: passthrough() });

  assert.equal(res.status, 200);
  assert.equal(res.body.toString(), events.join(""), "every byte arrives, in order");
  assertRelayedHeaders(SSE_HEADERS, res.headers);
  assert.ok(res.chunks[0].at < writes[1], "the first event arrived before the upstream sent the second");
  assert.ok(res.chunks.length >= 3, "events were not held back and delivered together");
});

test("a client that disconnects stops the upstream response", async (t) => {
  let resolveClosed;
  const closed = new Promise((r) => (resolveClosed = r));
  const { proxy } = await setup(t, {
    upstream: (record, res) => {
      res.writeHead(200, SSE_HEADERS);
      let sent = 0;
      const timer = setInterval(() => {
        res.write(`data: {"n":${sent}}\n\n`);
        if (++sent === 200) {
          clearInterval(timer);
          res.end();
        }
      }, 20);
      res.on("close", () => {
        clearInterval(timer);
        resolveClosed(sent);
      });
    },
  });
  const payload = Buffer.from(JSON.stringify(passthrough()));
  const client = http.request({ host: "127.0.0.1", port: proxy.port, method: "POST", path: "/v1/messages", headers: { ...CLIENT_HEADERS, "content-length": payload.length }, agent: false });
  client.on("response", (res) => res.once("data", () => client.destroy()));
  client.on("error", () => {});
  client.end(payload);

  const sent = await Promise.race([closed, pause(5000).then(() => "still streaming")]);
  assert.notEqual(sent, "still streaming", "the upstream connection was closed after the client left");
  assert.ok(sent < 200, `the upstream stopped early (after ${sent} of 200 events)`);
});

test("an upstream that drops mid-stream ends the client's response instead of hanging it", async (t) => {
  const { proxy } = await setup(t, {
    upstream: (record, res, req) => {
      res.writeHead(200, SSE_HEADERS);
      res.write('data: {"n":0}\n\n');
      setTimeout(() => req.socket.destroy(), 100);
    },
  });
  const payload = Buffer.from(JSON.stringify(passthrough()));
  const outcome = await new Promise((done) => {
    const timer = setTimeout(() => done("still waiting after 5s"), 5000);
    const client = http.request({ host: "127.0.0.1", port: proxy.port, method: "POST", path: "/v1/messages", headers: { ...CLIENT_HEADERS, "content-length": payload.length }, agent: false });
    client.on("response", (res) => {
      res.resume();
      res.on("close", () => {
        clearTimeout(timer);
        done(res.complete ? "complete" : "incomplete");
      });
    });
    client.on("error", () => {
      clearTimeout(timer);
      done("incomplete");
    });
    client.end(payload);
  });
  assert.equal(outcome, "incomplete");
});

test("an unreachable upstream answers 502 with a JSON error", async (t) => {
  const jev = await fakeJev(() => ({}));
  t.after(() => jev.close());
  // A port that was just free and is now closed: connections are refused.
  const gone = await fakeUpstream();
  await gone.close();
  const proxy = await startProxy(t, { jevURL: jev.url, upstreamURL: gone.url });

  const res = await send(proxy.port, { headers: CLIENT_HEADERS, body: passthrough() });
  assert.equal(res.status, 502);
  assert.equal(res.headers["content-type"], "application/json");
  const error = JSON.parse(res.body);
  assert.deepEqual(Object.keys(error), ["type", "error"]);
  assert.equal(error.type, "error");
  assert.deepEqual(Object.keys(error.error), ["message"]);
  assert.equal(typeof error.error.message, "string");
});

test("HEAD is answered by the proxy itself", async (t) => {
  const { upstream, proxy } = await setup(t);
  const res = await send(proxy.port, { method: "HEAD", path: "/" });
  assert.equal(res.status, 200);
  assert.equal(res.body.length, 0);
  const deep = await send(proxy.port, { method: "HEAD", path: "/v1/messages?beta=true" });
  assert.equal(deep.status, 200);
  assert.equal(upstream.requests.length, 0, "nothing reached the upstream");
});

test("redirects are relayed, not followed, and compressed bodies pass through undecoded", async (t) => {
  const gzipped = gzipSync(Buffer.from('{"id":"msg_gz","type":"message"}'));
  const replies = {
    "/v1/messages": (res) => {
      res.writeHead(307, { location: "https://elsewhere.example/v1/messages", "content-type": "text/plain", "x-redirect": "yes" });
      res.end("moved");
    },
    "/v1/messages/count_tokens": (res) => {
      res.writeHead(200, { "content-type": "application/json", "content-encoding": "gzip", "request-id": "req_gz" });
      res.end(gzipped);
    },
  };
  const { upstream, proxy } = await setup(t, { upstream: (record, res) => replies[record.url](res) });

  const moved = await send(proxy.port, { headers: CLIENT_HEADERS, body: passthrough() });
  assert.equal(moved.status, 307);
  assert.equal(moved.headers.location, "https://elsewhere.example/v1/messages");
  assert.equal(moved.body.toString(), "moved");
  assertRelayedHeaders({ "content-type": "text/plain", "x-redirect": "yes" }, moved.headers);
  assert.equal(upstream.requests.length, 1, "the redirect was not followed");

  const zipped = await send(proxy.port, { path: "/v1/messages/count_tokens", headers: CLIENT_HEADERS, body: passthrough() });
  assert.equal(zipped.status, 200);
  assert.ok(zipped.body.equals(gzipped), "the gzip bytes arrive exactly as the upstream sent them");
  assertRelayedHeaders({ "content-type": "application/json", "content-encoding": "gzip", "request-id": "req_gz" }, zipped.headers);
  assert.equal(upstream.requests[1].headers["accept-encoding"], CLIENT_HEADERS["accept-encoding"], "a message request keeps the client's accept-encoding");
});

test("an upstream base path is kept, minus one trailing slash, and the host carries its port", async (t) => {
  const jev = await fakeJev(() => ({}));
  const upstream = await fakeUpstream();
  t.after(() => jev.close());
  t.after(() => upstream.close());
  const proxy = await startProxy(t, { jevURL: jev.url, upstreamURL: `${upstream.url}/gateway/anthropic/` });

  const body = Buffer.from(JSON.stringify(passthrough()));
  const res = await send(proxy.port, { path: "/v1/messages?beta=true", headers: CLIENT_HEADERS, body });
  assert.equal(res.status, 200);
  assert.equal(upstream.requests[0].url, "/gateway/anthropic/v1/messages?beta=true");
  assert.equal(upstream.requests[0].body.toString(), body.toString());
  assertForwardedHeaders(res.sent, upstream.requests[0], { host: `127.0.0.1:${upstream.port}`, body });
});
