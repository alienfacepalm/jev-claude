import "./isolate-status.mjs";
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync, execFileSync } from "node:child_process";
import { mkdtempSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { writeDecision } from "../src/status.mjs";

const SCRIPT = fileURLToPath(new URL("../bin/jev-statusline.mjs", import.meta.url));
const MAIN = { key: "main", label: "main", main: true };

/** Runs the real status line the way Claude Code does, and returns its text without colours. */
function render(sessionId, workspace = {}, extra = {}, icons = "text") {
  const input = JSON.stringify({
    session_id: sessionId,
    workspace: { current_dir: "/work/proj", ...workspace },
    context_window: { used_percentage: 8 },
    ...extra,
  });
  const out = spawnSync(process.execPath, [SCRIPT], {
    input,
    encoding: "utf8",
    env: { ...process.env, JEV_ICONS: icons },
  });
  assert.equal(out.status, 0, out.stderr);
  // biome-ignore lint/suspicious/noControlCharactersInRegex: strips the ANSI colour codes the status line emits
  return out.stdout.replace(/\x1b\[[0-9;]*m/g, "").trim();
}

test("symbols replace the words, and the branch uses the Powerline glyph", () => {
  const id = `statusline-symbols-${process.pid}`;
  writeDecision(
    id,
    { tier: "sonnet", model: "claude-sonnet-5-5", confidence: 0.94, effort: "high", reason: "jev", at: Date.now() },
    MAIN,
  );
  const line = render(id, {}, { worktree: { name: "login-fix", branch: "fix/login" } }, "symbols");
  assert.equal(line, "✧✦ Sonnet 5.5 (94%) · ◔ high · ❐ proj · \ue0a0 fix/login · ⌂ login-fix · ≡ 8% · /work/proj");
});

test("shows the effort the turn ran at next to the model and confidence", () => {
  const id = `statusline-effort-${process.pid}`;
  writeDecision(
    id,
    { tier: "sonnet", model: "claude-sonnet-5-5", confidence: 0.94, effort: "high", reason: "jev", at: Date.now() },
    MAIN,
  );
  assert.equal(render(id), "model Sonnet 5.5 (94%) · effort high · dir proj · ctx 8% · /work/proj");
});

/** The raw status line text for a session, colours included. */
function renderRaw(sessionId, input = {}) {
  const out = spawnSync(process.execPath, [SCRIPT], {
    input: JSON.stringify({ session_id: sessionId, context_window: { used_percentage: 8 }, ...input }),
    encoding: "utf8",
    env: { ...process.env, JEV_ICONS: "text" },
  });
  assert.equal(out.status, 0, out.stderr);
  return out.stdout;
}

test("the whole working directory comes last, dimmed, as Claude Code sent it", () => {
  const id = `statusline-fullpath-${process.pid}`;
  const raw = renderRaw(id, { workspace: { current_dir: "/home/me/work/proj" } });
  assert.ok(raw.endsWith("8% \x1b[2m· /home/me/work/proj\x1b[0m\n"), JSON.stringify(raw));
});

test("a Windows path is shown with its backslashes, and its last segment is still the directory name", () => {
  const dirPath = "C:\\Users\\me\\work\\proj";
  const raw = renderRaw(`statusline-winpath-${process.pid}`, { workspace: { current_dir: dirPath } });
  assert.ok(raw.endsWith(`\x1b[2m· ${dirPath}\x1b[0m\n`), JSON.stringify(raw));
  const plain = render(`statusline-winpath-plain-${process.pid}`, { current_dir: dirPath });
  assert.equal(plain.includes(" · dir proj · "), true, "the directory item still carries only the last segment");
});

test("cwd is the fallback for the path when the workspace has no directory", () => {
  const raw = renderRaw(`statusline-cwd-${process.pid}`, { cwd: "/srv/app" });
  assert.ok(raw.endsWith("\x1b[2m· /srv/app\x1b[0m\n"), JSON.stringify(raw));
});

test("no directory in the input means no path part", () => {
  const raw = renderRaw(`statusline-nodir-${process.pid}`);
  assert.ok(raw.endsWith("8%\n"), JSON.stringify(raw));
});

test("shows a higher effort when Claude Code asked for one", () => {
  const id = `statusline-xhigh-${process.pid}`;
  writeDecision(
    id,
    { tier: "opus", model: "claude-opus-5-5", confidence: 0.91, effort: "xhigh", reason: "jev", at: Date.now() },
    MAIN,
  );
  assert.match(render(id), /^model Opus 5.5 \(91%\) · effort xhigh · /);
});

test("says nothing about effort for Haiku, which takes none", () => {
  const id = `statusline-haiku-${process.pid}`;
  writeDecision(
    id,
    {
      tier: "haiku",
      model: "claude-haiku-4-5-20251001",
      confidence: 0.97,
      effort: null,
      reason: "jev",
      at: Date.now(),
    },
    MAIN,
  );
  const line = render(id);
  assert.match(line, /^model Haiku 4.5 \(97%\) · dir proj/);
  assert.doesNotMatch(line, /effort/);
});

test("a session recorded before effort was tracked still renders", () => {
  const id = `statusline-old-${process.pid}`;
  writeDecision(
    id,
    { tier: "sonnet", model: "claude-sonnet-5-5", confidence: 0.8, reason: "jev", at: Date.now() },
    MAIN,
  );
  const line = render(id);
  assert.match(line, /^model Sonnet 5.5 \(80%\)/);
  assert.doesNotMatch(line, /effort/);
});

test("inside a worktree, the branch and the worktree are each named", () => {
  const id = `statusline-worktree-${process.pid}`;
  const line = render(id, {}, { worktree: { name: "login-fix", branch: "fix/login" } });
  assert.match(line, / · dir proj · branch fix\/login · worktree login-fix · ctx 8% · \/work\/proj$/);
});

test("a worktree named like the directory is not said twice", () => {
  const line = render(`statusline-samename-${process.pid}`, { current_dir: "/work/COR-1", git_worktree: "COR-1" });
  assert.match(line, / · worktree COR-1 · ctx 8% · \/work\/COR-1$/);
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
    assert.ok(line.endsWith(` · branch main-line · ctx 8% · ${dir}`), line);
    assert.doesNotMatch(line, /worktree/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a directory that is not a git checkout shows neither a branch nor a worktree", () => {
  assert.doesNotMatch(render(`statusline-nogit-${process.pid}`), /branch|worktree/);
});

test("a space separates the sub-agents symbol from the first model name", () => {
  const id = `statusline-agents-${process.pid}`;
  writeDecision(
    id,
    { tier: "sonnet", model: "claude-sonnet-5-5", confidence: 0.94, effort: "high", reason: "jev", at: Date.now() },
    MAIN,
  );
  writeDecision(
    id,
    { tier: "haiku", model: "claude-haiku-4-5-20251001", confidence: 0.9, reason: "jev", at: Date.now() },
    { key: "a1", label: "a1" },
  );
  assert.match(render(id, {}, {}, "symbols"), /✦ Haiku 4\.5/);
});
