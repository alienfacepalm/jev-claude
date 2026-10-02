import test from "node:test";
import assert from "node:assert/strict";
import { decide, detectOverride } from "../src/policy.mjs";
import { QUESTIONS, shouldUseExactModel } from "../src/config.mjs";

const ALL = ["haiku", "sonnet", "opus", "fable"];
const sure = (choice) => ({ choice, confidence: 0.95 });
const unsure = (choice) => ({ choice, confidence: 0.2 });
const base = { prompt: "refactor the parser", current: "sonnet", available: ALL, contextTokens: 0 };

test("score rubrics contain only API-valid descriptions", () => {
  for (const question of Object.values(QUESTIONS).filter((q) => q.type === "score")) {
    assert(question.criteria.every((description) => typeof description === "string"));
    assert(question.criteria.length <= 10);
  }
});

test("follows a confident Jev answer", () => {
  assert.deepEqual(decide({ ...base, jev: sure("opus") }), {
    tier: "opus",
    reason: "jev",
    changed: true,
  });
});

test("an explicit user override beats Jev", () => {
  const out = decide({ ...base, prompt: "use haiku to fix this typo", jev: sure("opus") });
  assert.equal(out.tier, "haiku");
  assert.equal(out.reason, "override");
});

test("detectOverride only fires on a real instruction", () => {
  assert.equal(detectOverride("switch to opus"), "opus");
  assert.equal(detectOverride("use luna"), "haiku");
  assert.equal(detectOverride("use the strong model"), "opus");
  assert.equal(detectOverride("Use Claude Haiku for this one"), "haiku");
  assert.equal(detectOverride("the opus of his career"), null);
});

test("detectOverride ignores ordinary prose that mentions a tier word", () => {
  for (const prompt of [
    "help me with fast fourier transform code",
    "replace the polling loop with long polling",
    "the test only fails on fast CI runners",
    "write tests with long input strings",
    "turn on fast refresh in vite",
    "merge with sol branch",
    "refactor this to rely on strong typing",
    "use haiku-style commit messages",
    "use long variable names",
  ]) {
    assert.equal(detectOverride(prompt), null, prompt);
  }
});

test("keeps the current model when Jev is unreachable", () => {
  const out = decide({ ...base, jev: null });
  assert.equal(out.tier, "sonnet");
  assert.equal(out.changed, false);
  assert.match(out.reason, /jev-unavailable/);
});

test("ignores a tier name Jev invented", () => {
  assert.equal(decide({ ...base, jev: sure("gpt-9") }).tier, "sonnet");
});

test("lands a low-confidence answer on the strong default rather than acting on it", () => {
  const out = decide({ ...base, current: "haiku", jev: unsure("sonnet") });
  assert.equal(out.tier, "opus");
  assert.equal(out.reason, "low-confidence-default");
});

test("never downgrades on a low-confidence answer", () => {
  const out = decide({ ...base, jev: unsure("haiku") });
  assert.equal(out.tier, "opus", "an unsure downgrade is not a reason to leave the default");
  assert.match(out.reason, /low-confidence-default/);
});

test("keeps a tier stronger than the default on a low-confidence answer", () => {
  const out = decide({ ...base, current: "fable", jev: unsure("haiku") });
  assert.equal(out.tier, "fable");
  assert.match(out.reason, /no-change/);
});

test("an answer without a confidence is treated as unsure", () => {
  const out = decide({ ...base, current: "haiku", jev: { choice: "haiku" } });
  assert.equal(out.tier, "opus");
  assert.equal(out.reason, "low-confidence-default");
});

test("a low-confidence answer cannot reach fable", () => {
  const out = decide({ ...base, current: "haiku", jev: unsure("fable") });
  assert.equal(out.tier, "opus");
  assert.equal(out.reason, "low-confidence-default");
});

test("still allows a confident upgrade to fable", () => {
  assert.equal(decide({ ...base, jev: sure("fable") }).tier, "fable");
});

test("refuses a downgrade once the cache rebuild costs more than it saves", () => {
  const out = decide({ ...base, current: "opus", jev: sure("haiku"), contextTokens: 80000 });
  assert.equal(out.tier, "opus");
  assert.match(out.reason, /cache-rebuild/);
});

test("allows the same downgrade early in a conversation", () => {
  assert.equal(decide({ ...base, current: "opus", jev: sure("haiku") }).tier, "haiku");
});

test("substitutes upward when the chosen tier is unavailable", () => {
  const out = decide({ ...base, current: "haiku", available: ["haiku", "opus"], jev: sure("sonnet") });
  assert.equal(out.tier, "opus");
  assert.match(out.reason, /unavailable/);
});

test("never substitutes upward into paid fable", () => {
  const out = decide({ ...base, current: "haiku", available: ["haiku", "fable"], jev: sure("opus") });
  assert.equal(out.tier, "haiku");
});

test("accepts exact model changes within the same tier", () => {
  assert.equal(shouldUseExactModel("jev/no-change", "opus", "opus"), true);
  assert.equal(shouldUseExactModel("low-confidence-default/no-change", "opus", "opus"), false);
});
