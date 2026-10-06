import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readSavedModel, restoreSavedModel } from "../src/settings.mjs";

const fileWith = (settings) => {
  const file = join(mkdtempSync(join(tmpdir(), "jev-settings-")), "settings.json");
  writeFileSync(file, JSON.stringify(settings, null, 2));
  return file;
};
const modelIn = (file) => JSON.parse(readFileSync(file, "utf8")).model;
const memoFile = () => join(mkdtempSync(join(tmpdir(), "jev-memo-")), "saved-model.json");

test("reads the saved model, ignoring a leftover sentinel", () => {
  assert.equal(readSavedModel(fileWith({ model: "opus" }), memoFile()), "opus");
  assert.equal(readSavedModel(fileWith({ model: "jev-router" }), memoFile()), undefined);
  assert.equal(readSavedModel(fileWith({}), memoFile()), undefined);
  assert.equal(readSavedModel(join(tmpdir(), "does-not-exist.json"), memoFile()), undefined);
});

test("a sentinel left by a killed session resolves to the model from before it", () => {
  const memo = memoFile();
  assert.equal(readSavedModel(fileWith({ model: "claude-opus-4-6" }), memo), "claude-opus-4-6");
  // That session died with the sentinel saved; the next run must not treat it as "no model".
  const file = fileWith({ model: "jev-router" });
  const previous = readSavedModel(file, memo);
  assert.equal(previous, "claude-opus-4-6");
  assert.equal(restoreSavedModel(previous, file), true);
  assert.equal(modelIn(file), "claude-opus-4-6");
});

test("restores the previous model when the sentinel was saved", () => {
  const file = fileWith({ model: "jev-router", permissions: { deny: ["Bash(rm*)"] } });
  assert.equal(restoreSavedModel("opus", file), true);
  assert.equal(modelIn(file), "opus");
  assert.deepEqual(JSON.parse(readFileSync(file, "utf8")).permissions, { deny: ["Bash(rm*)"] });
});

test("removes the sentinel when there was no previous model", () => {
  const file = fileWith({ model: "jev-router" });
  assert.equal(restoreSavedModel(undefined, file), true);
  assert.equal(modelIn(file), undefined);
});

test("leaves a real model the user chose during the session alone", () => {
  const file = fileWith({ model: "claude-opus-4-6" });
  assert.equal(restoreSavedModel("sonnet", file), false);
  assert.equal(modelIn(file), "claude-opus-4-6");
});

test("a missing or unreadable settings file is not an error", () => {
  assert.equal(restoreSavedModel("opus", join(tmpdir(), "nope", "settings.json")), false);
});
