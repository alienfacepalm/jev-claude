import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { gitBranch, worktreeInfo } from "../src/worktree.mjs";

const never = () => assert.fail("the branch should not have been looked up");

test("the main working tree shows nothing, and costs no git call", () => {
  assert.equal(worktreeInfo({ workspace: { current_dir: "/repo" }, cwd: "/repo" }, never), null);
  assert.equal(worktreeInfo({}, never), null);
  assert.equal(worktreeInfo(undefined, never), null);
});

test("a worktree session carries its own name and branch", () => {
  const input = { worktree: { name: "my-feature", branch: "worktree-my-feature", path: "/r/.claude/worktrees/my-feature" } };
  assert.deepEqual(worktreeInfo(input, never), { name: "my-feature", branch: "worktree-my-feature" });
});

test("a linked worktree has only a name, so the branch is read from git in the current directory", () => {
  const asked = [];
  const input = { workspace: { current_dir: "/wt/feature-xyz", git_worktree: "feature-xyz" } };
  const info = worktreeInfo(input, (dir) => (asked.push(dir), "feature/xyz"));
  assert.deepEqual(info, { name: "feature-xyz", branch: "feature/xyz" });
  assert.deepEqual(asked, ["/wt/feature-xyz"]);
});

test("a worktree session without a branch (hook-based) falls back to git", () => {
  const input = { worktree: { name: "scratch", path: "/wt/scratch" } };
  assert.deepEqual(worktreeInfo(input, (dir) => (dir === "/wt/scratch" ? "main-2" : null)), { name: "scratch", branch: "main-2" });
});

test("a detached HEAD still names the worktree", () => {
  const input = { workspace: { current_dir: "/wt/x", git_worktree: "x" } };
  assert.deepEqual(worktreeInfo(input, () => null), { name: "x", branch: null });
});

test("gitBranch reads the branch of a real checkout, and is null elsewhere", () => {
  const dir = realpathSync(mkdtempSync(join(tmpdir(), "jev-worktree-test-")));
  try {
    assert.equal(gitBranch(dir), null, "not a repository");
    execFileSync("git", ["init", "-q", "-b", "topic/a"], { cwd: dir });
    assert.equal(gitBranch(dir), "topic/a");
    assert.equal(gitBranch(undefined), null);
    assert.equal(gitBranch(join(dir, "missing")), null);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
