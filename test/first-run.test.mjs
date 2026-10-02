import test from "node:test";
import assert from "node:assert/strict";
import { PassThrough } from "node:stream";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { shouldOffer, wasOffered, markOffered, askYesNo } from "../src/first-run.mjs";

test("the setup check is offered only on a plain interactive first launch", () => {
  assert.equal(shouldOffer({ args: [], interactive: true, offered: false }), true);
  assert.equal(shouldOffer({ args: [], interactive: true, offered: true }), false, "once per user");
  assert.equal(shouldOffer({ args: [], interactive: false, offered: false }), false, "nobody to ask");
  for (const args of [["-p", "fix it"], ["--resume"], ["explain this repo"]]) {
    assert.equal(shouldOffer({ args, interactive: true, offered: false }), false, args.join(" "));
  }
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

test("an empty answer or yes accepts, and no declines", async () => {
  assert.equal(await answer("\n"), true);
  assert.equal(await answer("y\n"), true);
  assert.equal(await answer("Yes\n"), true);
  assert.equal(await answer("n\n"), false);
  assert.equal(await answer("NO\n"), false);
});
