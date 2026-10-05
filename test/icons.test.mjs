import test from "node:test";
import assert from "node:assert/strict";
import { icons } from "../src/icons.mjs";

const isText = (set) => set.dir === "";

test("emoji everywhere but the legacy Windows console", () => {
  assert.equal(isText(icons({}, "darwin")), false);
  assert.equal(isText(icons({}, "linux")), false);
  assert.equal(isText(icons({}, "win32")), true, "conhost draws emoji as boxes");
});

test("a Windows terminal that announces itself gets emoji", () => {
  for (const env of [{ WT_SESSION: "1" }, { TERM_PROGRAM: "vscode" }, { TERM_PROGRAM: "mintty" }, { ConEmuPID: "42" }]) {
    assert.equal(isText(icons(env, "win32")), false, JSON.stringify(env));
  }
});

test("JEV_ICONS overrides the guess in both directions", () => {
  assert.equal(isText(icons({ JEV_ICONS: "text" }, "darwin")), true);
  assert.equal(isText(icons({ JEV_ICONS: "ASCII" }, "darwin")), true);
  assert.equal(isText(icons({ JEV_ICONS: "emoji" }, "win32")), false);
});
