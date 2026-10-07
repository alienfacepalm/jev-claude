import "./isolate-status.mjs";
import test from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { startProxy } from "../src/proxy.mjs";
import { readStatus } from "../src/status.mjs";

// Driven by a request captured from the real Claude Code CLI (see the fixture's `_source`):
// print mode sends no /v1/models probe, posts to `/v1/messages?beta=true`, carries adaptive
// thinking and a thinking-clearing context edit, and ends in a `role: "system"` message.
// Assertions are on what the upstream received, checked after the response: an assertion
// thrown inside `route` would be swallowed by the proxy's own error handling.

const CAPTURED = JSON.parse(
  readFileSync(new URL("../../conformance/fixtures/claude-code-print-request.json", import.meta.url), "utf8"),
);
const realRequest = () => structuredClone(CAPTURED.body);
const SESSION = JSON.parse(CAPTURED.body.metadata.user_id).session_id;

/** The same conversation one step later: Claude ran a tool and is sending back its result. */
function continuation(body) {
  const [opening, ...rest] = body.messages;
  return {
    ...body,
    messages: [
      opening,
      { role: "assistant", content: [{ type: "tool_use", id: "toolu_1", name: "Bash", input: { command: "ls" } }] },
      { role: "user", content: [{ type: "tool_result", tool_use_id: "toolu_1", content: "utils.js" }] },
      ...rest,
    ],
  };
}

/** A proxy in front of an upstream that records what it got and, like the API, rejects the sentinel. */
async function harness(t, route) {
  const seen = [];
  const upstream = http.createServer((req, res) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      const body = JSON.parse(Buffer.concat(chunks));
      seen.push({ url: req.url, body });
      res.setHeader("content-type", "application/json");
      if (body.model === "jev-router") {
        res.statusCode = 400;
        return res.end('{"type":"error","error":{"type":"invalid_request_error","message":"model: jev-router"}}');
      }
      res.end('{"id":"msg_1","type":"message"}');
    });
  });
  await new Promise((resolve) => upstream.listen(0, "127.0.0.1", resolve));
  t.after(() => upstream.close());

  const prompts = [];
  const { port, close } = await startProxy({
    upstreamURL: `http://127.0.0.1:${upstream.address().port}`,
    route: async (args) => {
      prompts.push(args.prompt);
      return route(args);
    },
  });
  t.after(close);

  const post = (body, url = CAPTURED.url) =>
    fetch(`http://127.0.0.1:${port}${url}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
  return { seen, prompts, post };
}

const answer = (choice) => async () => ({ choice, confidence: 0.92, ms: 1 });

test("a real print-mode request is routed on the user's prompt", async (t) => {
  const { seen, prompts, post } = await harness(t, answer("claude-haiku-4-5-20251001"));

  const res = await post(realRequest());

  assert.equal(res.status, 200);
  assert.deepEqual(prompts, ["rename the variable x to count in utils.js"], "reminders stripped, prompt intact");
  const sent = seen[0].body;
  assert.equal(seen[0].url, "/v1/messages?beta=true");
  assert.equal(sent.model, "claude-haiku-4-5-20251001");
  // Haiku takes neither adaptive thinking nor effort; sending them as captured would be rejected.
  assert.equal(sent.thinking, undefined);
  assert.equal(sent.context_management, undefined);
  assert.equal(sent.output_config?.effort, undefined);
  assert.equal(readStatus(SESSION)?.tier, "haiku", "the decision reaches the status line");
});

test("an unwritable JEV_DUMP path does not stop the turn being routed", async (t) => {
  // A real misconfiguration: the dump directory does not exist, so the dump cannot be written.
  const previous = process.env.JEV_DUMP;
  process.env.JEV_DUMP = join(tmpdir(), "jev-no-such-dir", "nested", "dump");
  t.after(() => {
    if (previous === undefined) delete process.env.JEV_DUMP;
    else process.env.JEV_DUMP = previous;
  });
  const { seen, post } = await harness(t, answer("claude-haiku-4-5-20251001"));

  const res = await post(realRequest());

  assert.equal(res.status, 200, "the API would reject the sentinel with a 400");
  assert.equal(seen[0].body.model, "claude-haiku-4-5-20251001", "routed as usual, dump or no dump");
});

test("tool-call continuations keep the tier the turn was routed to", async (t) => {
  const { seen, prompts, post } = await harness(t, answer("claude-sonnet-5-5"));
  const opening = realRequest();

  await post(opening);
  await post(continuation(opening));
  await post(continuation(opening));

  assert.equal(prompts.length, 1, "Jev is asked once per turn, not once per tool call");
  assert.deepEqual(
    seen.map((s) => s.body.model),
    ["claude-sonnet-5-5", "claude-sonnet-5-5", "claude-sonnet-5-5"],
  );
});

test("the main thread keeps its tier after more than 50 sub-agents start", async (t) => {
  const { seen, post } = await harness(t, async ({ prompt }) => ({
    choice: prompt.startsWith("Search the codebase") ? "claude-haiku-4-5-20251001" : "claude-sonnet-5-5",
    confidence: 0.9,
    ms: 1,
  }));
  const main = realRequest();
  await post(main);

  // Sub-agents share the session id and differ by their opening task. The main thread waits on
  // their results, so it is the oldest, least recently used conversation when it resumes.
  for (let i = 0; i < 55; i++) {
    const sub = realRequest();
    sub.messages[0].content.at(-1).text = `Search the codebase for callers of handler ${i} and report them`;
    await post(sub);
  }
  await post(continuation(main));

  assert.equal(seen.at(-1).body.model, "claude-sonnet-5-5", "not reset to the opus default");
});
