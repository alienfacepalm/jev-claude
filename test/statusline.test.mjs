import "./isolate-status.mjs";
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync, execFileSync } from "node:child_process";
import { mkdtempSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { writeDecision } from "../src/status.mjs";

const SCRIPT = new URL("../bin/jev-statusline.mjs", import.meta.url);
const MAIN = { key: "main", label: "main", main: true };

/** Runs the real status line the way Claude Code does, and returns its text without colours. */
function render(sessionId, workspace = {}, extra = {}) {
  const input = JSON.stringify({ session_id: sessionId, workspace: { current_dir: "/work/proj", ...workspace }, context_window: { used_percentage: 8 }, ...extra });
  const out = spawnSync(process.execPath, [SCRIPT.pathname.replace(/^\/([A-Za-z]:)/, "$1")], { input, encoding: "utf8" });
  assert.equal(out.status, 0, out.stderr);
  // eslint-disable-next-line no-control-regex
  return out.stdout.replace(/\x1b\[[0-9;]*m/g, "").trim();
}

test("shows the effort the turn ran at next to the model and confidence", () => {
  const id = `statusline-effort-${process.pid}`;
  writeDecision(id, { tier: "sonnet", model: "claude-sonnet-5-5", confidence: 0.94, effort: "high", reason: "jev", at: Date.now() }, MAIN);
  assert.equal(render(id), "model Sonnet 5.5 (94%) · effort high · dir proj · ctx 8%");
});

test("shows a higher effort when Claude Code asked for one", () => {
  const id = `statusline-xhigh-${process.pid}`;
  writeDecision(id, { tier: "opus", model: "claude-opus-5-5", confidence: 0.91, effort: "xhigh", reason: "jev", at: Date.now() }, MAIN);
  assert.match(render(id), /^model Opus 5.5 \(91%\) · effort xhigh · /);
});

test("says nothing about effort for Haiku, which takes none", () => {
  const id = `statusline-haiku-${process.pid}`;
  writeDecision(id, { tier: "haiku", model: "claude-haiku-4-5-20251001", confidence: 0.97, effort: null, reason: "jev", at: Date.now() }, MAIN);
  const line = render(id);
  assert.match(line, /^model Haiku 4.5 \(97%\) · dir proj/);
  assert.doesNotMatch(line, /effort/);
});

test("a session recorded before effort was tracked still renders", () => {
  const id = `statusline-old-${process.pid}`;
  writeDecision(id, { tier: "sonnet", model: "claude-sonnet-5-5", confidence: 0.8, reason: "jev", at: Date.now() }, MAIN);
  const line = render(id);
  assert.match(line, /^model Sonnet 5.5 \(80%\)/);
  assert.doesNotMatch(line, /effort/);
});

test("inside a worktree, the branch and the worktree are each named", () => {
  const id = `statusline-worktree-${process.pid}`;
  const line = render(id, {}, { worktree: { name: "login-fix", branch: "fix/login" } });
  assert.match(line, / · dir proj · branch fix\/login · worktree login-fix · ctx 8%$/);
});

test("a worktree named like the directory is not said twice", () => {
  const line = render(`statusline-samename-${process.pid}`, { current_dir: "/work/COR-1", git_worktree: "COR-1" });
  assert.match(line, / · worktree COR-1 · ctx 8%$/);
  assert.doesNotMatch(line, /dir/);
});

test("a long branch name is cut with an ellipsis", () => {
  const branch = "COR-1263/multi-edit-inspection-sync";
  const line = render(`statusline-longbranch-${process.pid}`, {}, { worktree: { name: "wt", branch } });
  assert.match(line, / · branch COR-1263\/multi-edit-inspect… · /);
  assert.doesNotMatch(line, /inspection-sync/);
});

test("a linked worktree whose branch cannot be read still names the worktree", () => {
  // /work/proj is not a repository, so there is no branch to look up.
  const line = render(`statusline-linked-${process.pid}`, { git_worktree: "scratch" });
  assert.match(line, / · dir proj · worktree scratch · /);
  assert.doesNotMatch(line, /branch/);
});

test("the main working tree shows its branch and no worktree", () => {
  const dir = realpathSync(mkdtempSync(join(tmpdir(), "jev-statusline-repo-")));
  try {
    execFileSync("git", ["init", "-q", "-b", "main-line"], { cwd: dir });
    const line = render(`statusline-main-${process.pid}`, { current_dir: dir });
    assert.match(line, / · branch main-line · ctx 8%$/);
    assert.doesNotMatch(line, /worktree/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a directory that is not a git checkout shows neither a branch nor a worktree", () => {
  assert.doesNotMatch(render(`statusline-nogit-${process.pid}`), /branch|worktree/);
});
