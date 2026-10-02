import test from "node:test";
import assert from "node:assert/strict";
import { PassThrough } from "node:stream";
import { mkdtempSync, mkdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { shouldOffer, shadowsSkill, wasOffered, markOffered, askYesNo } from "../src/first-run.mjs";

test("the setup check is offered only on a plain interactive first launch", () => {
  assert.equal(shouldOffer({ args: [], interactive: true, offered: false }), true);
  assert.equal(shouldOffer({ args: [], interactive: true, offered: true }), false, "once per user");
  assert.equal(shouldOffer({ args: [], interactive: false, offered: false }), false, "nobody to ask");
  for (const args of [["-p", "fix it"], ["--resume"], ["explain this repo"]]) {
    assert.equal(shouldOffer({ args, interactive: true, offered: false }), false, args.join(" "));
  }
});

test("the offer is not made where a repository defines its own jev-calibrate skill", (t) => {
  const repo = mkdtempSync(join(tmpdir(), "jev-shadow-"));
  t.after(() => rmSync(repo, { recursive: true, force: true }));
  const router = join(repo, "router");
  mkdirSync(join(router, ".claude", "skills", "jev-calibrate"), { recursive: true });
  const other = join(repo, "other");
  mkdirSync(join(other, ".claude", "skills", "jev-calibrate"), { recursive: true });

  assert.equal(shadowsSkill(other, router), true, "someone else's skill of the same name");
  assert.equal(shadowsSkill(router, router), false, "the router's own skill, in its own repository");
  assert.equal(shadowsSkill(repo, router), false, "no such skill here");
  assert.equal(shouldOffer({ args: [], interactive: true, offered: false, shadowed: true }), false);
});

test("an offer is remembered whatever the answer", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "jev-first-run-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const file = join(dir, "nested", "first-run.json");
  assert.equal(wasOffered(file), false);
  markOffered(false, file);
  assert.equal(wasOffered(file), true, "a no is remembered too, so it is never asked again");
});

const answer = async (text) => {
  const input = new PassThrough();
  const pending = askYesNo("? ", { input, output: new PassThrough() });
  input.end(text);
  return pending;
};

test("a closed or failing input settles with no answer instead of hanging", async () => {
  const closed = new PassThrough();
  const pending = askYesNo("? ", { input: closed, output: new PassThrough() });
  closed.end();
  assert.equal(await pending, null);

  const broken = new PassThrough();
  const failing = askYesNo("? ", { input: broken, output: new PassThrough() });
  broken.destroy(new Error("EIO"));
  assert.equal(await failing, null);
});

test("an empty answer or yes accepts, and no declines", async () => {
  assert.equal(await answer("\n"), true);
  assert.equal(await answer("y\n"), true);
  assert.equal(await answer("Yes\n"), true);
  assert.equal(await answer("n\n"), false);
  assert.equal(await answer("NO\n"), false);
});
