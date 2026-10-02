import test from "node:test";
import assert from "node:assert/strict";
import { shortName } from "../src/model-names.mjs";

test("a model id reads as its family and version", () => {
  assert.equal(shortName("claude-opus-5-5"), "Opus 5.5");
  assert.equal(shortName("claude-sonnet-5-5"), "Sonnet 5.5");
  assert.equal(shortName("claude-fable-5-1"), "Fable 5.1");
  assert.equal(shortName("claude-opus-6"), "Opus 6", "a whole-number release");
  assert.equal(shortName("claude-sonnet-5-10"), "Sonnet 5.10");
});

test("a date suffix or a context tag is not part of the version", () => {
  assert.equal(shortName("claude-haiku-4-5-20251001"), "Haiku 4.5");
  assert.equal(shortName("claude-opus-4-6[1m]"), "Opus 4.6");
});

test("anything that is not a Claude model id has no short name", () => {
  assert.equal(shortName("gpt-5.6-sol"), null);
  assert.equal(shortName("jev-router"), null);
  assert.equal(shortName(undefined), null);
});
