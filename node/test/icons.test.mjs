import test from "node:test";
import assert from "node:assert/strict";
import { icons } from "../src/icons.mjs";

const isText = (set) => set.dir === "dir";

test("symbols everywhere but the legacy Windows console", () => {
  assert.equal(isText(icons({}, "darwin")), false);
  assert.equal(isText(icons({}, "linux")), false);
  assert.equal(isText(icons({}, "win32")), true, "conhost fonts lack the glyphs");
});

test("a Windows terminal that announces itself gets symbols", () => {
  for (const env of [{ WT_SESSION: "1" }, { TERM_PROGRAM: "vscode" }, { TERM_PROGRAM: "mintty" }, { ConEmuPID: "42" }]) {
    assert.equal(isText(icons(env, "win32")), false, JSON.stringify(env));
  }
});

test("JEV_ICONS overrides the guess in both directions", () => {
  assert.equal(isText(icons({ JEV_ICONS: "text" }, "darwin")), true);
  assert.equal(isText(icons({ JEV_ICONS: "ASCII" }, "darwin")), true);
  assert.equal(isText(icons({ JEV_ICONS: "symbols" }, "win32")), false);
});

test("every item has a symbol and a word", () => {
  const keys = Object.keys(icons({ JEV_ICONS: "text" }, "darwin"));
  assert.deepEqual(Object.keys(icons({ JEV_ICONS: "symbols" }, "darwin")), keys);
  for (const set of ["text", "symbols"]) {
    for (const [name, label] of Object.entries(icons({ JEV_ICONS: set }, "darwin"))) assert.ok(label, `${set}.${name}`);
  }
});
