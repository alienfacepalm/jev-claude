// Shared machinery for the black-box harness (SPEC.md 16.3): fake Jev and Anthropic servers on
// loopback, the implementation's proxy and status line as child processes, and the header rules
// of SPEC 7.7. Nothing here is a test; `node --test conformance/harness` only runs *.test.mjs.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import http from "node:http";
import { tmpdir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = resolve(fileURLToPath(new URL("../..", import.meta.url)));
export const FIXTURE = JSON.parse(readFileSync(join(ROOT, "conformance", "fixtures", "claude-code-print-request.json"), "utf8"));
export const JEV_KEY = "jev-harness-key";

// The reference implementation, used as the oracle for what a request should become. The child
// processes under test are separate programs; these imports never touch their state.
export const node = {
  config: await import(new URL("../../node/src/config.mjs", import.meta.url)),
  proxy: await import(new URL("../../node/src/proxy.mjs", import.meta.url)),
};

export const DIM = "\x1b[2m";
export const BOLD = "\x1b[1m";
export const RESET = "\x1b[0m";
export const icon = (mark) => `${BOLD}${mark}${RESET}`;
export const SEP = ` ${DIM}·${RESET} `;

/**
 * Splits a command line the way a POSIX shell would for the simple cases a harness needs: spaces
 * separate words, double or single quotes group them, and a backslash escapes only a following
 * `"` or `\` (so Windows paths such as `C:\go\bin\x.exe` survive unquoted).
 */
export function splitCommandLine(line) {
  const words = [];
  let word = "";
  let inWord = false;
  let quote = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote === "'") {
      if (c === "'") quote = null;
      else word += c;
    } else if (c === "\\" && (line[i + 1] === '"' || line[i + 1] === "\\")) {
      word += line[++i];
      inWord = true;
    } else if (quote === '"') {
      if (c === '"') quote = null;
      else word += c;
    } else if (c === '"' || c === "'") {
      quote = c;
      inWord = true;
    } else if (/\s/.test(c)) {
      if (inWord) words.push(word);
      word = "";
      inWord = false;
    } else {
      word += c;
      inWord = true;
    }
  }
  if (quote) throw new Error(`unterminated quote in command line: ${line}`);
  if (inWord) words.push(word);
  return words;
}

/**
 * The command for one program under test. Children run in a fresh temporary directory, so a word
 * that names a path relative to the repository root is made absolute.
 */
export function commandFor(variable, fallback) {
  const line = process.env[variable];
  const words = line ? splitCommandLine(line) : fallback;
  return words.map((w) => (!w.startsWith("-") && !isAbsolute(w) && /[\\/]/.test(w) && existsSync(join(ROOT, w)) ? join(ROOT, w) : w));
}

const proxyCommand = () => commandFor("JEV_IMPL_CMD_PROXY", [process.execPath, join(ROOT, "node", "scripts", "proxy-host.mjs")]);
const statusLineCommand = () => commandFor("JEV_IMPL_CMD_STATUSLINE", [process.execPath, join(ROOT, "node", "bin", "jev-statusline.mjs")]);

/** The parent environment minus everything that would change a child's behaviour. */
export function cleanEnv(extra = {}) {
  const env = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (!/^(JEV_|TYPESAFE_|ANTHROPIC_|CLAUDE_)/i.test(key)) env[key] = value;
  }
  return { ...env, ...extra };
}

const children = new Set();
function kill(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  // taskkill /T also ends anything the command started (a launcher script, say), which a plain
  // kill on Windows would leave running and holding the port.
  if (process.platform === "win32") spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], { stdio: "ignore" });
  else child.kill("SIGKILL");
}
process.on("exit", () => {
  for (const child of children) kill(child);
});

/** A loopback HTTP server that records every request in full before handing it to `handle`. */
export async function recordingServer(handle) {
  const requests = [];
  const sockets = new Set();
  const server = http.createServer((req, res) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      const record = { method: req.method, url: req.url, headers: { ...req.headers }, rawHeaders: [...req.rawHeaders], body: Buffer.concat(chunks), at: Date.now() };
      requests.push(record);
      handle(record, res, req);
    });
  });
  server.on("connection", (s) => {
    sockets.add(s);
    s.on("close", () => sockets.delete(s));
  });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const { port } = server.address();
  return {
    port,
    url: `http://127.0.0.1:${port}`,
    requests,
    close: () =>
      new Promise((done) => {
        for (const s of sockets) s.destroy();
        server.close(() => done());
      }),
  };
}

/** A Jev response as SPEC 5.2 describes it. */
export function jevAnswer(choice, confidence, scores = [3, 4, 2]) {
  const score = (s) => ({ type: "score", score: s, confidence: 0.8, legend: { 0: "None" }, probabilities: { [s]: 0.8 } });
  return {
    model: "jev-1",
    usage: { input_tokens: 812, output_tokens: 9 },
    answers: {
      model: { type: "choice", choice, confidence, probabilities: { [choice]: confidence } },
      task_complexity: score(scores[0]),
      reasoning_required: score(scores[1]),
      tool_complexity: score(scores[2]),
    },
  };
}

/**
 * A fake Jev. `answer(requestBody)` returns `{status, body}` or a response object to send as JSON
 * with status 200. The prewarm HEAD is recorded with everything else; `posts()` is the real calls.
 */
export async function fakeJev(answer) {
  const jev = await recordingServer((record, res) => {
    if (record.method !== "POST") return res.writeHead(200).end();
    let reply;
    try {
      reply = answer(JSON.parse(record.body.toString()));
    } catch (err) {
      reply = { status: 500, body: JSON.stringify({ error: String(err) }) };
    }
    const status = reply?.status ?? 200;
    const body = reply?.status ? reply.body : JSON.stringify(reply);
    res.writeHead(status, { "content-type": "application/json" });
    res.end(body);
  });
  jev.posts = () => jev.requests.filter((r) => r.method === "POST");
  return jev;
}

export const UPSTREAM_REPLY_HEADERS = {
  "content-type": "application/json",
  "request-id": "req_harness_1",
  "anthropic-ratelimit-tokens-remaining": "123456",
  "x-should-retry": "false",
  "x-harness-trace": "abc",
  server: "fake-anthropic",
};

/** A fake Anthropic API. `handle(record, res)` answers; the default is a small JSON message. */
export async function fakeUpstream(handle) {
  return recordingServer(
    handle ??
      ((record, res) => {
        res.writeHead(200, UPSTREAM_REPLY_HEADERS);
        res.end('{"id":"msg_1","type":"message","model":"claude-haiku-4-5-20251001","content":[]}');
      }),
  );
}

/**
 * Starts the proxy under test with the environment SPEC 16.3 lists and resolves once it has
 * printed its port. Everything it starts is torn down by `t.after`.
 */
export async function startProxy(t, { jevURL, upstreamURL, env: extra = {} }) {
  const base = mkdtempSync(join(tmpdir(), "jev-harness-"));
  const dirs = { base, status: join(base, "status"), home: join(base, "home"), temp: join(base, "temp"), cwd: join(base, "cwd") };
  for (const dir of [dirs.home, dirs.temp, dirs.cwd]) mkdirSync(dir);
  const env = cleanEnv({
    JEV_STATUS_DIR: dirs.status,
    HOME: dirs.home,
    USERPROFILE: dirs.home,
    TEMP: dirs.temp,
    TMP: dirs.temp,
    TMPDIR: dirs.temp,
    JEV_ICONS: "symbols",
    TYPESAFE_BASE_URL: jevURL,
    JEV_API_KEY: JEV_KEY,
    ANTHROPIC_BASE_URL: upstreamURL,
    ...extra,
  });
  const [command, ...args] = proxyCommand();
  const child = spawn(command, args, { cwd: dirs.cwd, env, stdio: ["ignore", "pipe", "pipe"], windowsHide: true });
  children.add(child);
  let stderr = "";
  child.stderr.on("data", (c) => (stderr += c));
  t.after(async () => {
    kill(child);
    if (child.exitCode === null && child.signalCode === null) await new Promise((done) => child.once("exit", done));
    children.delete(child);
    rmSync(base, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
  });
  const port = await new Promise((done, fail) => {
    let out = "";
    const timer = setTimeout(() => fail(new Error(`proxy printed no PORT= line within 20s; stderr:\n${stderr}`)), 20_000);
    child.stdout.on("data", (c) => {
      out += c;
      const m = /^PORT=(\d+)\r?$/m.exec(out);
      if (m) {
        clearTimeout(timer);
        done(Number(m[1]));
      }
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      fail(new Error(`proxy exited with ${code} before printing its port; stderr:\n${stderr}`));
    });
  });
  return { port, dirs, env, stderr: () => stderr };
}

/** One fake Jev, one fake upstream, and the proxy between them, all closed after the test. */
export async function setup(t, { answer = () => jevAnswer("claude-haiku-4-5-20251001", 0.92), upstream: handle, env } = {}) {
  const jev = await fakeJev(answer);
  const upstream = await fakeUpstream(handle);
  t.after(() => jev.close());
  t.after(() => upstream.close());
  const proxy = await startProxy(t, { jevURL: jev.url, upstreamURL: upstream.url, env });
  return { jev, upstream, proxy };
}

/**
 * One HTTP request with exactly the headers given (plus the `host` and `connection` node:http
 * always sends), recording each body chunk with its arrival time.
 */
export function send(port, { method = "POST", path = "/v1/messages", headers = {}, body } = {}) {
  const payload = body === undefined ? null : Buffer.isBuffer(body) ? body : Buffer.from(typeof body === "string" ? body : JSON.stringify(body));
  const all = { ...headers, ...(payload ? { "content-length": String(payload.length) } : {}) };
  return new Promise((done, fail) => {
    const req = http.request({ host: "127.0.0.1", port, method, path, headers: all, agent: false }, (res) => {
      const chunks = [];
      res.on("data", (data) => chunks.push({ at: Date.now(), data }));
      res.on("end", () => done({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks.map((c) => c.data)), chunks, sent: all }));
      res.on("error", fail);
    });
    req.on("error", fail);
    req.end(payload ?? undefined);
  });
}

/** Headers a real Claude Code request carries, as far as the proxy is concerned. */
export const CLIENT_HEADERS = {
  "content-type": "application/json",
  accept: "application/json",
  "anthropic-version": "2023-06-01",
  "anthropic-beta": "claude-code-20250219,interleaved-thinking-2025-05-14",
  "x-api-key": "sk-ant-api03-harness",
  "user-agent": "claude-cli/2.1.287 (external, cli)",
  "x-app": "cli",
  "x-stainless-retry-count": "0",
  "accept-encoding": "gzip, deflate, br",
};

const HOP = new Set(["connection", "keep-alive", "transfer-encoding"]);

/**
 * SPEC 7.7, upstream side: `host` names the upstream, `content-length` is the forwarded body's
 * length (absent for an empty body), every other client header arrives unchanged except the
 * hop-by-hop ones and `accept-encoding`, and the proxy's HTTP client adds nothing else.
 */
export function assertForwardedHeaders(sent, received, { host, body, acceptEncodingRemoved = false }) {
  const got = received.headers;
  assert.equal(got.host, host, "host names the upstream");
  if (body.length) assert.equal(got["content-length"], String(body.length), "content-length is the forwarded body's length");
  else assert.equal(got["content-length"], undefined, "no content-length for an empty body");
  for (const [name, value] of Object.entries(sent)) {
    const key = name.toLowerCase();
    if (HOP.has(key) || key === "host" || key === "content-length" || key === "accept-encoding") continue;
    assert.equal(got[key], value, `client header ${key} reaches upstream unchanged`);
  }
  if (acceptEncodingRemoved) assert.equal(got["accept-encoding"], undefined, "accept-encoding removed");
  const allowed = new Set([...Object.keys(sent).map((k) => k.toLowerCase()), "host", "content-length", ...HOP]);
  for (const key of Object.keys(got)) assert.ok(allowed.has(key), `the proxy added a header the client did not send: ${key}`);
}

const comparedOnClient = (name) => ["content-type", "content-encoding", "request-id"].includes(name) || name.startsWith("anthropic-") || name.startsWith("x-");

/** SPEC 7.7, client side: the compared headers are exactly the upstream's. */
export function assertRelayedHeaders(upstreamHeaders, clientHeaders) {
  const want = Object.fromEntries(Object.entries(upstreamHeaders).map(([k, v]) => [k.toLowerCase(), String(v)]).filter(([k]) => comparedOnClient(k)));
  const got = Object.fromEntries(Object.entries(clientHeaders).filter(([k]) => comparedOnClient(k)));
  assert.deepEqual(got, want, "content-type, content-encoding, request-id, anthropic-* and x-* are relayed as sent");
}

/** The request body with its session id replaced, so each scenario writes its own status file. */
export function fixtureBody(session) {
  const body = structuredClone(FIXTURE.body);
  const meta = JSON.parse(body.metadata.user_id);
  meta.session_id = session;
  body.metadata.user_id = JSON.stringify(meta);
  return body;
}

/** The same conversation one step later: a tool ran and its result is being sent back. */
export function continuation(body) {
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

/** What the proxy should forward for a routed or defaulted body, by the reference functions. */
export function rewritten(body, tier, model = node.config.idOf(tier)) {
  const copy = structuredClone(body);
  copy.tools?.forEach((t) => node.proxy.sanitizeSchema(t.input_schema));
  node.proxy.applyTier(copy, tier, model, {});
  return Buffer.from(JSON.stringify(copy));
}

/** The exact body the proxy's Jev client should send (SPEC 5.1, 6.1). */
export function jevRequest({ prompt, current, contextTokens, models }) {
  return {
    state: {
      request: prompt,
      session: { current_model: current, context_tokens: contextTokens },
      environment: { available_models: models.map((m) => m.id) },
    },
    questions: { ...node.config.QUESTIONS, model: node.config.questionForModels(models) },
  };
}

export const contextTokensOf = (body) => Math.round(JSON.stringify(body.messages).length / 4);
export const STATIC_MENU = node.proxy.newestPerTier(node.proxy.claudeModels([]));

/** Checks the Jev call: path, the headers SPEC 5.1 fixes, and the body byte for byte. */
export function assertJevCall(call, request, { retry } = {}) {
  assert.equal(call.method, "POST");
  assert.equal(call.url, "/v1/systemone");
  assert.equal(call.headers.authorization, `Bearer ${JEV_KEY}`);
  assert.equal(call.headers.accept, "application/json");
  assert.equal(call.headers["content-type"], "application/json");
  // Node's SDK sends its own User-Agent (and X-TypeSafe-SDK/-Runtime, which ports omit); ports send
  // jev-router-<lang>/<version>.
  assert.match(call.headers["user-agent"] ?? "", /^(typesafe-sdk|jev-router-(go|rust|python))\/\S+$/);
  assert.equal(call.headers["x-typesafe-retry-count"], retry === undefined ? undefined : String(retry));
  assert.equal(call.body.toString(), JSON.stringify({ ...request, model: "jev-latest" }), "the Jev request body, byte for byte");
}

/** Reads a status-directory file, or null when it does not exist. */
export function readJSON(file) {
  return existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : null;
}

/**
 * Compares a status file with what it should hold, key order included. Every `at` must be a clock
 * reading taken during the test; they are then zeroed on both sides.
 */
export function assertStatusFile(actual, expected, { from, to }) {
  const zero = (value, check) => {
    if (Array.isArray(value)) return value.map((v) => zero(v, check));
    if (value && typeof value === "object") {
      return Object.fromEntries(Object.entries(value).map(([k, v]) => {
        if (k !== "at") return [k, zero(v, check)];
        if (check) assert.ok(typeof v === "number" && v >= from && v <= to, `at ${v} is a clock reading within the test (${from}..${to})`);
        return [k, 0];
      }));
    }
    return value;
  };
  assert.ok(actual, "the status file exists");
  assert.equal(JSON.stringify(zero(actual, true)), JSON.stringify(zero(expected, false)));
}

/** The decision a routed turn records (SPEC 7.3 step 5, 8.2), with `at` to be zeroed. */
export function routedStatus({ body, tier, model, jev, reason, request, effort }) {
  const prompt = node.proxy.newTurnPrompt(body);
  const ctx = contextTokensOf(body);
  const answers = jev?.answers;
  const decision = {
    tier,
    prompt,
    model,
    confidence: answers?.model?.confidence ?? null,
    metrics: jev
      ? {
          taskComplexity: answers.task_complexity.score / 9,
          reasoningRequired: answers.reasoning_required.score / 9,
          toolComplexity: answers.tool_complexity.score / 9,
          contextSize: Math.min(ctx / 200000, 1),
        }
      : null,
    reason,
    jev: jev ? { request, response: jev } : null,
    effort,
    at: 0,
  };
  return decision;
}

/** Runs the status line under test with `env`, writing `stdin`; resolves its stdout. */
export function runStatusLine(env, cwd, stdin) {
  const [command, ...args] = statusLineCommand();
  const out = spawnSync(command, args, { input: stdin, cwd, env, encoding: "utf8", windowsHide: true, timeout: 20_000 });
  assert.equal(out.status, 0, `status line exited ${out.status}: ${out.stderr}`);
  return out.stdout;
}
