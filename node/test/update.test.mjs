import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import {
  applyUpdate,
  checkForUpdate,
  compareVersions,
  isCheckDue,
  needsInstall,
  readState,
  updateNotice,
  writeState,
  CHECK_EVERY_MS,
} from "../src/update.mjs";

// These tests drive real git: a bare repository stands in for GitHub, clones are made the way the
// installer makes them (including the shallow `--depth 1` one), and the "upstream" moves on by
// real commits. What is checked is what the user's checkout looks like afterwards.

const git = (cwd, ...args) =>
  execFileSync("git", ["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false", ...args], {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();

const pkg = (version) => `${JSON.stringify({ name: "jev-router", version }, null, 2)}\n`;

/** An origin at 0.1.0, a working copy that pushes to it, and a helper to release from it. */
function fixture(t) {
  const base = mkdtempSync(join(tmpdir(), "jev-update-"));
  t.after(() => rmSync(base, { recursive: true, force: true }));
  const origin = join(base, "origin.git");
  const upstream = join(base, "upstream");
  git(base, "init", "--bare", "-b", "master", origin);
  git(base, "clone", origin, upstream);
  git(upstream, "checkout", "-b", "master");
  writeFileSync(join(upstream, "package.json"), pkg("0.1.0"));
  writeFileSync(join(upstream, "pnpm-lock.yaml"), "lock: 1\n");
  git(upstream, "add", ".");
  git(upstream, "commit", "-m", "first");
  git(upstream, "push", "-u", "origin", "master");

  const release = (version, files = {}) => {
    writeFileSync(join(upstream, "package.json"), pkg(version));
    for (const [name, text] of Object.entries(files)) writeFileSync(join(upstream, name), text);
    git(upstream, "add", ".");
    git(upstream, "commit", "-m", `release ${version}`);
    git(upstream, "push", "origin", "master");
  };
  const cloneTo = (name, { shallow = false } = {}) => {
    const dir = join(base, name);
    if (shallow) git(base, "clone", "--depth", "1", pathToFileURL(origin).href, dir);
    else git(base, "clone", origin, dir);
    return dir;
  };
  return { base, origin, upstream, release, cloneTo };
}

const version = (dir) => JSON.parse(readFileSync(join(dir, "package.json"), "utf8")).version;

for (const shallow of [false, true]) {
  const kind = shallow ? "a shallow clone, as the installer makes" : "a full clone";

  test(`an install that is level with origin has no update (${kind})`, async (t) => {
    const { cloneTo } = fixture(t);
    const install = cloneTo("install", { shallow });
    const found = await checkForUpdate(install);
    assert.equal(found.available, false);
    assert.equal(updateNotice(found, version(install)), null);
  });

  test(`a release upstream is found, announced, and applied by fast-forward (${kind})`, async (t) => {
    const { release, cloneTo } = fixture(t);
    const install = cloneTo("install", { shallow });
    release("0.2.0", { "new-file.txt": "hello\n" });

    const found = await checkForUpdate(install);
    assert.equal(found.available, true);
    assert.equal(found.latest, "0.2.0");
    assert.equal(
      updateNotice(found, version(install)),
      "[jev] Update available: 0.1.0 -> 0.2.0. Run `jev-claude --update`.",
    );
    assert.equal(version(install), "0.1.0", "looking must not change the install");

    const applied = await applyUpdate(install);
    assert.deepEqual(applied, { status: "updated", from: "0.1.0", to: "0.2.0" });
    assert.equal(version(install), "0.2.0");
    assert.equal(readFileSync(join(install, "new-file.txt"), "utf8"), "hello\n");
    assert.equal(git(install, "status", "--porcelain"), "", "a clean checkout afterwards");

    assert.equal((await checkForUpdate(install)).available, false, "nothing further to find");
    assert.deepEqual(await applyUpdate(install), { status: "current", version: "0.2.0" });
  });
}

test("changed dependencies ask for an install; unchanged ones do not", async (t) => {
  const { release, cloneTo } = fixture(t);
  const install = cloneTo("install");
  release("0.1.1", { "notes.txt": "docs only\n" });
  const calls = [];
  const recorded = async (root) => {
    calls.push(root);
    return 0;
  };

  assert.equal((await applyUpdate(install, { install: recorded })).status, "updated");
  assert.deepEqual(calls, [], "a change that leaves package.json's dependencies alone installs nothing");

  release("0.2.0", { "pnpm-lock.yaml": "lock: 2\n" });
  assert.equal((await applyUpdate(install, { install: recorded })).status, "updated");
  assert.deepEqual(calls, [install], "a new lockfile installs once, in the install folder");

  release("0.3.0", { "pnpm-lock.yaml": "lock: 3\n" });
  const failed = await applyUpdate(install, { install: async () => 1 });
  assert.equal(failed.status, "failed", "a failed install is reported, not hidden");
  assert.match(failed.reason, /exited with 1/);

  assert.equal(needsInstall(["README.md", "src/proxy.mjs"]), false);
  assert.equal(needsInstall(["package.json"]), false, "a version bump alone is not a dependency change");
  assert.equal(needsInstall(["README.md", "pnpm-lock.yaml"]), true);
});

test("a copy with local changes is left exactly as it is", async (t) => {
  const { release, cloneTo } = fixture(t);
  const install = cloneTo("install");
  writeFileSync(join(install, "pnpm-lock.yaml"), "lock: 1\nmy edit\n");
  release("0.2.0");

  const found = await checkForUpdate(install);
  assert.equal(found.available, false, "no notice for a copy that cannot be updated");
  const applied = await applyUpdate(install);
  assert.equal(applied.status, "refused");
  assert.match(applied.reason, /local changes/);
  assert.match(readFileSync(join(install, "pnpm-lock.yaml"), "utf8"), /my edit/, "the edit survives");
  assert.equal(version(install), "0.1.0");
});

test("a development clone ahead of origin is not touched", async (t) => {
  const { release, cloneTo } = fixture(t);
  const install = cloneTo("install");
  writeFileSync(join(install, "mine.txt"), "work in progress\n");
  git(install, "add", ".");
  git(install, "commit", "-m", "my own commit");
  const head = git(install, "rev-parse", "HEAD");

  const level = await applyUpdate(install);
  assert.equal(level.status, "current", "ahead of origin with nothing new upstream: nothing to do");
  assert.equal(git(install, "rev-parse", "HEAD"), head);

  release("0.2.0");
  const found = await checkForUpdate(install);
  assert.equal(found.available, false, "diverged: neither announced nor applied");
  const applied = await applyUpdate(install);
  assert.equal(applied.status, "refused");
  assert.match(applied.reason, /commits that origin\/master does not/);
  assert.equal(git(install, "rev-parse", "HEAD"), head, "no merge, no rewrite");
});

test("a detached checkout is refused", async (t) => {
  const { release, cloneTo } = fixture(t);
  const install = cloneTo("install");
  git(install, "checkout", "--detach");
  release("0.2.0");
  const applied = await applyUpdate(install);
  assert.equal(applied.status, "refused");
  assert.match(applied.reason, /not on a branch/);
});

test("an unreachable origin is reported, never thrown, and the install is unchanged", async (t) => {
  const { base, origin, cloneTo } = fixture(t);
  const install = cloneTo("install");
  rmSync(origin, { recursive: true, force: true });

  assert.equal((await checkForUpdate(install)).available, false);
  const applied = await applyUpdate(install);
  assert.equal(applied.status, "refused");
  assert.match(applied.reason, /could not reach origin/);
  assert.equal(version(install), "0.1.0");
  assert.ok(base);
});

test("folders that are not a clone of their own are refused", async (t) => {
  const { base, cloneTo } = fixture(t);
  const plain = join(base, "plain");
  mkdirSync(plain);
  const refusedPlain = await applyUpdate(plain);
  assert.equal(refusedPlain.status, "refused");
  assert.match(refusedPlain.reason, /not a git clone/);

  // An install copied into some other repository must not be updated through that repository.
  const outer = cloneTo("outer");
  const inner = join(outer, "vendored", "jev-claude");
  mkdirSync(inner, { recursive: true });
  writeFileSync(join(inner, "package.json"), pkg("0.0.1"));
  const refusedInner = await applyUpdate(inner);
  assert.equal(refusedInner.status, "refused");
  assert.match(refusedInner.reason, /not a git clone of its own/);
});

test("the check is due when missing, stale, unreadable, or from the future", () => {
  const now = Date.parse("2026-10-04T12:00:00Z");
  const at = (ms) => ({ checkedAt: new Date(now - ms).toISOString() });
  assert.equal(isCheckDue(null, now), true);
  assert.equal(isCheckDue({}, now), true);
  assert.equal(isCheckDue({ checkedAt: "yesterday-ish" }, now), true);
  assert.equal(isCheckDue(at(CHECK_EVERY_MS - 60_000), now), false, "just inside the window");
  assert.equal(isCheckDue(at(CHECK_EVERY_MS), now), true, "exactly at the window");
  assert.equal(isCheckDue(at(-60_000), now), true, "a clock that went backwards must not silence checks");
});

test("versions compare as numbers, not text", () => {
  assert.ok(compareVersions("0.10.0", "0.9.9") > 0, "0.10 is newer than 0.9");
  assert.ok(compareVersions("0.6.5", "0.6.6") < 0);
  assert.equal(compareVersions("1.0", "1.0.0"), 0);
  assert.ok(compareVersions("0.7.0-beta.1", "0.6.9") > 0, "a pre-release tag is ignored");
});

test("a notice appears only for a genuinely newer version", () => {
  const found = { available: true, latest: "0.7.0" };
  assert.match(updateNotice(found, "0.6.5"), /0\.6\.5 -> 0\.7\.0/);
  assert.equal(updateNotice(found, "0.7.0"), null, "already installed some other way");
  assert.equal(updateNotice(found, "0.8.0"), null, "a development copy ahead of the release");
  assert.equal(updateNotice({ available: false, latest: "0.7.0" }, "0.6.5"), null);
  assert.equal(updateNotice(null, "0.6.5"), null);
  assert.equal(updateNotice(found, null), null, "an unreadable local version says nothing");
});

test("the state file round-trips and a damaged one reads as no state", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "jev-update-state-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const file = join(dir, "nested", "update.json");
  assert.equal(readState(file), null, "missing");
  const state = { checkedAt: "2026-10-04T12:00:00.000Z", available: true, latest: "0.7.0" };
  writeState(state, file);
  assert.deepEqual(readState(file), state);
  writeFileSync(file, "{ not json");
  assert.equal(readState(file), null, "damaged");
  writeFileSync(file, "42");
  assert.equal(readState(file), null, "valid JSON that is not a state object");
});
