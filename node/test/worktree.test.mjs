import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { gitBranch, locationInfo } from "../src/worktree.mjs";

const never = () => assert.fail("the branch should not have been looked up");

test("outside a git checkout there is nothing to show", () => {
  assert.equal(
    locationInfo({ workspace: { current_dir: "/nowhere" } }, () => null),
    null,
  );
  assert.equal(
    locationInfo({}, () => null),
    null,
  );
  assert.equal(
    locationInfo(undefined, () => null),
    null,
  );
});

test("the main working tree has a branch and no worktree", () => {
  const input = { workspace: { current_dir: "/repo" } };
  assert.deepEqual(
    locationInfo(input, (dir) => (dir === "/repo" ? "master" : null)),
    { branch: "master", worktree: null },
  );
});

test("a worktree session carries its own name and branch, with no git call", () => {
  const input = {
    worktree: { name: "my-feature", branch: "worktree-my-feature", path: "/r/.claude/worktrees/my-feature" },
  };
  assert.deepEqual(locationInfo(input, never), { branch: "worktree-my-feature", worktree: "my-feature" });
});

test("a linked worktree has only a name, so the branch is read from git in the current directory", () => {
  const asked = [];
  const input = { workspace: { current_dir: "/wt/feature-xyz", git_worktree: "feature-xyz" } };
  const info = locationInfo(input, (dir) => {
    asked.push(dir);
    return "feature/xyz";
  });
  assert.deepEqual(info, { branch: "feature/xyz", worktree: "feature-xyz" });
  assert.deepEqual(asked, ["/wt/feature-xyz"]);
});

test("a worktree session without a branch (hook-based) falls back to git", () => {
  const input = { worktree: { name: "scratch", path: "/wt/scratch" } };
  assert.deepEqual(
    locationInfo(input, (dir) => (dir === "/wt/scratch" ? "main-2" : null)),
    { branch: "main-2", worktree: "scratch" },
  );
});

test("a detached HEAD in a worktree still names the worktree", () => {
  const input = { workspace: { current_dir: "/wt/x", git_worktree: "x" } };
  assert.deepEqual(
    locationInfo(input, () => ""),
    { branch: "", worktree: "x" },
  );
});

test("gitBranch: a branch, a detached HEAD, and no checkout", () => {
  const dir = realpathSync(mkdtempSync(join(tmpdir(), "jev-worktree-test-")));
  const git = (...args) =>
    execFileSync("git", ["-c", "user.name=t", "-c", "user.email=t@t", ...args], { cwd: dir, stdio: "ignore" });
  try {
    assert.equal(gitBranch(dir), null, "not a repository");
    git("init", "-q", "-b", "topic/a");
    assert.equal(gitBranch(dir), "topic/a", "works before the first commit");
    git("commit", "-q", "--allow-empty", "-m", "x");
    git("checkout", "-q", "--detach");
    assert.equal(gitBranch(dir), "", "detached");
    assert.equal(gitBranch(undefined), null);
    assert.equal(gitBranch(join(dir, "missing")), null);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
