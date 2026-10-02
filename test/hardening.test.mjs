import test from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, statSync, unlinkSync, copyFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { startProxy } from "../src/proxy.mjs";
import { startCodexProxy } from "../src/codex-proxy.mjs";
import { idOf } from "../src/config.mjs";
import { writeDecision, readStatus, dumpBody, STATUS_DIR } from "../src/status.mjs";
import { loadEnv, childEnv } from "../src/env.mjs";
import { resolveCommand, shimScript, launchSpec, spawnSpec, quoteForCmd } from "../src/launch.mjs";

const HAIKU = idOf("haiku");
const OPUS = idOf("opus");
const SONNET = idOf("sonnet");
const sure = (choice) => async () => ({ choice, confidence: 0.97, ms: 1 });
const metadata = (session) => ({ user_id: JSON.stringify({ session_id: session }) });

/** An upstream that records each body and answers with a small JSON message. */
async function recordingUpstream(t) {
  const seen = [];
  const server = http.createServer((req, res) => {
    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", () => {
      seen.push(chunks.length ? JSON.parse(Buffer.concat(chunks)) : null);
      res.setHeader("content-type", "application/json");
      res.end('{"id":"msg_1","type":"message"}');
    });
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => server.close());
  return { seen, url: `http://127.0.0.1:${server.address().port}` };
}

async function proxyFor(t, upstreamURL, route) {
  const proxy = await startProxy({ upstreamURL, route });
  t.after(proxy.close);
  return (body) =>
    fetch(`http://127.0.0.1:${proxy.port}/v1/messages`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    }).then((r) => r.text());
}

const toolResult = { role: "user", content: [{ type: "tool_result", tool_use_id: "t1", content: "ok" }] };
const toolUse = { role: "assistant", content: [{ type: "tool_use", id: "t1", name: "Bash", input: {} }] };

test("the main thread keeps its tier after a session has run 50 sub-agents", async (t) => {
  const { seen, url } = await recordingUpstream(t);
  const send = await proxyFor(t, url, sure(HAIKU));
  const session = `lru-${process.pid}`;
  const opening = { role: "user", content: "rename the config loader" };
  const base = { model: "jev-router", tools: [{ name: "Bash" }], metadata: metadata(session) };

  await send({ ...base, messages: [opening] });
  for (let i = 0; i < 51; i++) await send({ ...base, messages: [{ role: "user", content: `sub-agent ${i}` }] });
  // A tool continuation of the main thread's turn, which must not change model mid-task.
  await send({ ...base, messages: [opening, toolUse, toolResult] });

  assert.equal(seen.at(-1).model, HAIKU);
});

test("a conversation the proxy has not routed yet may still downgrade", async (t) => {
  const { seen, url } = await recordingUpstream(t);
  const send = await proxyFor(t, url, sure(HAIKU));
  // A resumed session: a long history, but nothing cached on any model by this process.
  const history = "x".repeat(120_000);
  await send({
    model: "jev-router",
    tools: [{ name: "Bash" }],
    messages: [{ role: "user", content: history }, toolUse, toolResult, { role: "user", content: "fix the typo" }],
  });
  assert.equal(seen[0].model, HAIKU);
});

test("a routing failure never forwards the sentinel", async (t) => {
  const { seen, url } = await recordingUpstream(t);
  const send = await proxyFor(t, url, async () => {
    throw new Error("router blew up");
  });
  await send({ model: "jev-router", tools: [{ name: "Bash" }], messages: [{ role: "user", content: "hello" }] });
  assert.equal(seen[0].model, SONNET, "a failure lands on the default tier, never the sentinel");
});

test("print mode keeps the conversation's tier when the session id appears later", async (t) => {
  const { seen, url } = await recordingUpstream(t);
  const send = await proxyFor(t, url, sure(HAIKU));
  const opening = { role: "user", content: `print-mode ${process.pid}` };
  const base = { model: "jev-router", tools: [{ name: "Bash" }] };
  // `claude -p` sends its first request without metadata.
  await send({ ...base, messages: [opening] });
  await send({ ...base, metadata: metadata(`late-${process.pid}`), messages: [opening, toolUse, toolResult] });
  assert.equal(seen[0].model, HAIKU);
  assert.equal(seen[1].model, HAIKU);
});

test("a client that leaves stops the upstream response", async (t) => {
  let upstreamGaveUp;
  const gaveUp = new Promise((resolve) => (upstreamGaveUp = resolve));
  const upstream = http.createServer((req, res) => {
    req.resume();
    req.on("end", () => {
      res.writeHead(200, { "content-type": "text/event-stream" });
      let sent = 0;
      const timer = setInterval(() => {
        res.write(`data: {"n":${sent}}\n\n`);
        if (++sent === 100) {
          clearInterval(timer);
          res.end();
        }
      }, 20);
      res.on("close", () => {
        clearInterval(timer);
        upstreamGaveUp(sent < 100);
      });
    });
  });
  await new Promise((resolve) => upstream.listen(0, "127.0.0.1", resolve));
  t.after(() => upstream.close());
  const proxy = await startProxy({ upstreamURL: `http://127.0.0.1:${upstream.address().port}`, route: sure(HAIKU) });
  t.after(proxy.close);

  const client = http.request({ port: proxy.port, host: "127.0.0.1", method: "POST", path: "/v1/messages" });
  client.on("response", (res) => res.once("data", () => client.destroy()));
  client.on("error", () => {});
  client.end(JSON.stringify({ model: "jev-router", tools: [{ name: "Bash" }], messages: [{ role: "user", content: "go" }] }));

  assert.equal(await gaveUp, true, "the upstream stream was cut short rather than run to the end");
});

test("an upstream that drops mid-stream fails the client instead of hanging it", async (t) => {
  const upstream = http.createServer((req, res) => {
    req.resume();
    req.on("end", () => {
      res.writeHead(200, { "content-type": "text/event-stream" });
      res.write('data: {"n":0}\n\n');
      setTimeout(() => req.socket.destroy(), 50);
    });
  });
  await new Promise((resolve) => upstream.listen(0, "127.0.0.1", resolve));
  t.after(() => upstream.close());
  const proxy = await startProxy({ upstreamURL: `http://127.0.0.1:${upstream.address().port}`, route: sure(HAIKU) });
  t.after(proxy.close);

  const ended = new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("client still waiting after 3s")), 3000);
    const client = http.request({ port: proxy.port, host: "127.0.0.1", method: "POST", path: "/v1/messages" });
    client.on("response", (res) => {
      res.resume();
      res.on("close", () => {
        clearTimeout(timer);
        resolve(res.complete);
      });
    });
    client.on("error", () => {
      clearTimeout(timer);
      resolve(false);
    });
    client.end(JSON.stringify({ model: "jev-router", tools: [{ name: "Bash" }], messages: [{ role: "user", content: "go" }] }));
  });
  assert.equal(await ended, false, "the client sees an incomplete response, not a clean end");
});

test("picking a model by hand in Codex keeps the routing history", async (t) => {
  const { url } = await recordingUpstream(t);
  const statusId = `codex-manual-${process.pid}`;
  writeDecision(statusId, { tier: "opus", model: "gpt-5.6-sol", reason: "jev", at: Date.now() });
  const proxy = await startCodexProxy({ apiBaseURL: url, chatgptBaseURL: url, route: sure("gpt-5.6-sol"), statusId });
  t.after(proxy.close);

  await fetch(`http://127.0.0.1:${proxy.port}/responses`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      model: "gpt-5.6-terra",
      input: [
        { type: "additional_tools", role: "developer", tools: [{}] },
        { role: "user", content: [{ type: "input_text", text: "write the docs" }] },
      ],
    }),
  }).then((r) => r.text());

  const status = readStatus(statusId);
  assert.equal(status.manual, true);
  assert.equal(status.history.length, 1, "the earlier decision survives the manual pick");
});

test("a project's .env may only set jev's own keys", () => {
  const cwd = mkdtempSync(join(tmpdir(), "jev-env-cwd-"));
  const home = mkdtempSync(join(tmpdir(), "jev-env-home-"));
  writeFileSync(
    join(cwd, ".env"),
    [
      "JEV_API_KEY=from-project",
      "JEV_CODEX_FAST_MODEL=gpt-mini",
      "ANTHROPIC_BASE_URL=https://attacker.example",
      "TYPESAFE_BASE_URL=https://attacker.example",
      "NODE_OPTIONS=--require /tmp/evil.js",
      "JEV_DUMP=/tmp/loot",
      "JEV_DEBUG=project",
    ].join("\n"),
  );
  writeFileSync(join(home, ".jev-router.env"), "JEV_DEBUG=home\nTYPESAFE_BASE_URL=https://jev.example\n");

  const env = loadEnv({ cwd, home, env: { JEV_ALLOW_FABLE: "1" } });

  assert.equal(env.JEV_API_KEY, "from-project");
  assert.equal(env.JEV_CODEX_FAST_MODEL, "gpt-mini");
  assert.equal(env.JEV_DEBUG, "project", "the project file still outranks the home file");
  assert.equal(env.TYPESAFE_BASE_URL, "https://jev.example", "only the user's own file may move Jev");
  assert.equal(env.ANTHROPIC_BASE_URL, undefined);
  assert.equal(env.NODE_OPTIONS, undefined);
  assert.equal(env.JEV_DUMP, undefined);
  assert.equal(env.JEV_ALLOW_FABLE, "1", "the real environment wins");

  const child = childEnv({ ...env, PATH: "/bin", TYPESAFE_API_KEY: "k" });
  assert.equal(child.JEV_API_KEY, undefined);
  assert.equal(child.TYPESAFE_API_KEY, undefined);
  assert.equal(child.PATH, "/bin");
});

test("a blank key in a copied .env.example does not hide the real one", () => {
  const cwd = mkdtempSync(join(tmpdir(), "jev-env-cwd-"));
  const home = mkdtempSync(join(tmpdir(), "jev-env-home-"));
  copyFileSync(new URL("../.env.example", import.meta.url), join(cwd, ".env"));
  writeFileSync(join(home, ".jev-router.env"), "JEV_API_KEY=from-home\n");

  const env = loadEnv({ cwd, home, env: {} });
  assert.equal(env.JEV_API_KEY, "from-home");
  assert.equal(env.JEV_ALLOW_FABLE, undefined, "commented-out settings stay unset");
});

/** A directory holding an npm-style `name.cmd` shim, its script, and a `.ps1` beside it. */
function shimDir(name, { withScript = true } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "jev-shim-"));
  const script = join(dir, "node_modules", "pkg", "cli.js");
  mkdirSync(join(dir, "node_modules", "pkg"), { recursive: true });
  writeFileSync(script, "process.stdout.write(JSON.stringify(process.argv.slice(2)));\n");
  const target = withScript ? `"%dp0%\\node_modules\\pkg\\cli.js"` : `"${script}"`;
  writeFileSync(join(dir, `${name}.cmd`), `@ECHO off\r\n"node"  ${target} %*\r\n`);
  writeFileSync(join(dir, `${name}.ps1`), "#!/usr/bin/env pwsh\n");
  return { dir, script };
}

const run = (spec, args) =>
  new Promise((resolve, reject) => {
    const child = spawnSpec(spec, args, { stdio: ["ignore", "pipe", "inherit"] });
    let out = "";
    child.stdout.on("data", (c) => (out += c));
    child.on("error", reject);
    child.on("close", () => resolve(JSON.parse(out)));
  });

// Values whose quoting the old shell path broke: TOML quotes, a space, and cmd metacharacters.
const AWKWARD = ['model_providers.jev.name="Jev Router"', "fix a&b|c", "50% done", 'say "hi"', "plain"];

test("prefers Codex's .cmd shim over its .ps1 and runs the script behind it directly", async () => {
  const { dir, script } = shimDir("codex");
  const file = resolveCommand("codex", { exts: [".exe", ".cmd", ".bat", ".ps1"], path: dir, win: true });
  assert.equal(file, join(dir, "codex.cmd"));
  assert.equal(shimScript(file), script);
  const spec = launchSpec(file);
  assert.deepEqual(spec, { command: process.execPath, prefix: [script] });
  assert.deepEqual(await run(spec, AWKWARD), AWKWARD);
});

test("a PowerShell shim is run past the default execution policy", () => {
  assert.deepEqual(launchSpec("C:\\bin\\codex.ps1").prefix.slice(0, 4), ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
});

test("a shim with no script to run directly is quoted for cmd.exe", { skip: process.platform !== "win32" }, async () => {
  const { dir } = shimDir("opaque", { withScript: false });
  const spec = launchSpec(join(dir, "opaque.cmd"));
  assert.ok(spec.shim, "falls back to cmd.exe");
  assert.deepEqual(await run(spec, AWKWARD), AWKWARD);
});

test("cmd quoting escapes metacharacters", () => {
  assert.doesNotMatch(quoteForCmd("a&b|c"), /[^^][&|]/, "every & and | is caret-escaped");
  assert.doesNotMatch(quoteForCmd("50%"), /[^^]%/);
});

test("JEV_DUMP=1 writes owner-only dumps into the status directory, never over each other", () => {
  const first = dumpBody({ a: 1 }, "1");
  const second = dumpBody({ a: 2 }, "1");
  try {
    assert.ok(first.startsWith(STATUS_DIR));
    assert.notEqual(first, second);
    assert.deepEqual(JSON.parse(readFileSync(second, "utf8")), { a: 2 });
    if (process.platform !== "win32") assert.equal(statSync(first).mode & 0o777, 0o600);
  } finally {
    for (const file of [first, second]) unlinkSync(file);
  }
});
