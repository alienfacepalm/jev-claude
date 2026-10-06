import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { bumpLevel, nextVersion, messagesBetween } from "../scripts/bump-version.mjs";

test("a release counts every commit since the last tag, not just the last push", (t) => {
  const cwd = mkdtempSync(join(tmpdir(), "jev-bump-"));
  t.after(() => rmSync(cwd, { recursive: true, force: true }));
  const git = (...args) => execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
  git("init", "-q");
  git("config", "user.email", "test@example.com");
  git("config", "user.name", "test");
  const commit = (message) => {
    git("commit", "-q", "--allow-empty", "-m", message);
    return git("rev-parse", "HEAD");
  };
  const first = commit("chore(release): v0.4.0");
  git("tag", "v0.4.0");
  commit("feat: pushed first, but its run failed");
  const before = commit("docs: also in the failed push");
  const after = commit("fix: the push whose run succeeds");

  const messages = messagesBetween(before, after, { cwd });
  assert.equal(messages.length, 3, "everything since v0.4.0");
  assert.equal(bumpLevel(messages), "minor", "the earlier feat still counts");

  rmSync(join(cwd, ".git", "refs", "tags", "v0.4.0"));
  assert.equal(messagesBetween(before, after, { cwd }).length, 1, "with no tag, only the pushed range");
  assert.equal(messagesBetween("0".repeat(40), first, { cwd }).length, 1, "a first push counts its last commit");
});

test("a feature is a minor bump, anything else a patch", () => {
  assert.equal(bumpLevel(["fix: keep the cache", "feat(statusline): show versions"]), "minor");
  assert.equal(bumpLevel(["fix: keep the cache", "docs: explain .env.example"]), "patch");
  assert.equal(bumpLevel(["Show sub-agent models with their version"]), "patch", "no prefix is a patch");
});

test("a breaking change is a major bump, in the subject or the body", () => {
  assert.equal(bumpLevel(["feat!: drop Node 18"]), "major");
  assert.equal(bumpLevel(["refactor(proxy)!: new status format"]), "major");
  assert.equal(bumpLevel(["feat: new config\n\nBREAKING CHANGE: JEV_TIER renamed"]), "major");
});

test("the workflow's own release commits do not count", () => {
  assert.equal(bumpLevel(["chore(release): v0.5.0"]), "patch");
  assert.equal(bumpLevel(["chore(release): v0.5.0", "feat: something"]), "minor");
});

test("a bump resets the lower parts of the version", () => {
  assert.equal(nextVersion("0.4.0", "patch"), "0.4.1");
  assert.equal(nextVersion("0.4.7", "minor"), "0.5.0");
  assert.equal(nextVersion("0.4.7", "major"), "1.0.0");
});
