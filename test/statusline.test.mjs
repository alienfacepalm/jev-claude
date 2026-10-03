import "./isolate-status.mjs";
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { writeDecision } from "../src/status.mjs";

const SCRIPT = new URL("../bin/jev-statusline.mjs", import.meta.url);
const MAIN = { key: "main", label: "main", main: true };

/** Runs the real status line the way Claude Code does, and returns its text without colours. */
function render(sessionId) {
  const input = JSON.stringify({ session_id: sessionId, workspace: { current_dir: "/work/proj" }, context_window: { used_percentage: 8 } });
  const out = spawnSync(process.execPath, [SCRIPT.pathname.replace(/^\/([A-Za-z]:)/, "$1")], { input, encoding: "utf8" });
  assert.equal(out.status, 0, out.stderr);
  // eslint-disable-next-line no-control-regex
  return out.stdout.replace(/\x1b\[[0-9;]*m/g, "").trim();
}

test("shows the effort the turn ran at next to the model and confidence", () => {
  const id = `statusline-effort-${process.pid}`;
  writeDecision(id, { tier: "sonnet", model: "claude-sonnet-5-5", confidence: 0.94, effort: "high", reason: "jev", at: Date.now() }, MAIN);
  assert.equal(render(id), "Sonnet 5.5 (94%) · effort high · proj · 8% context");
});

test("shows a higher effort when Claude Code asked for one", () => {
  const id = `statusline-xhigh-${process.pid}`;
  writeDecision(id, { tier: "opus", model: "claude-opus-5-5", confidence: 0.91, effort: "xhigh", reason: "jev", at: Date.now() }, MAIN);
  assert.match(render(id), /^Opus 5.5 \(91%\) · effort xhigh · /);
});

test("says nothing about effort for Haiku, which takes none", () => {
  const id = `statusline-haiku-${process.pid}`;
  writeDecision(id, { tier: "haiku", model: "claude-haiku-4-5-20251001", confidence: 0.97, effort: null, reason: "jev", at: Date.now() }, MAIN);
  const line = render(id);
  assert.match(line, /^Haiku 4.5 \(97%\) · proj/);
  assert.doesNotMatch(line, /effort/);
});

test("a session recorded before effort was tracked still renders", () => {
  const id = `statusline-old-${process.pid}`;
  writeDecision(id, { tier: "sonnet", model: "claude-sonnet-5-5", confidence: 0.8, reason: "jev", at: Date.now() }, MAIN);
  const line = render(id);
  assert.match(line, /^Sonnet 5.5 \(80%\)/);
  assert.doesNotMatch(line, /effort/);
});
