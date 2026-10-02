import test from "node:test";
import assert from "node:assert/strict";
import { bumpLevel, nextVersion } from "../scripts/bump-version.mjs";

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
