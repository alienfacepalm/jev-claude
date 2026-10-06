import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { formatExplanation } from "../src/explain.mjs";
import { shortReason, longReason } from "../src/reasons.mjs";

test("formats the last routing decision", () => {
  const output = formatExplanation({
    prompt: "Explain the router architecture",
    tier: "sonnet",
    confidence: 0.94,
    reason: "jev",
    jev: {
      request: { state: { session: { current_model: "haiku", context_tokens: 6200 } } },
      response: { answers: { model: { choice: "claude-sonnet-5-5", confidence: 0.94 } } },
    },
    metrics: {
      taskComplexity: 0.82,
      reasoningRequired: 0.91,
      toolComplexity: 0.64,
      contextSize: 0.31,
    },
  });

  assert.match(output, /Task complexity     0\.82/);
  assert.match(output, /Prompt: Explain the router/);
  assert.match(output, /Current model: HAIKU/);
  assert.match(output, /Context tokens: 6200/);
  assert.match(output, /Recommended tier: SONNET/);
  assert.match(output, /Selected model: SONNET/);
  assert.match(output, /Confidence: 94%/);
  // The sentence wraps inside the box rather than being cut at its edge.
  assert.match(output, /Decision: the router's\s*│\n│ recommendation/);
});

test("shows Jev's own pick when policy overruled it", () => {
  const output = formatExplanation({
    tier: "opus",
    model: "claude-opus-5-5",
    confidence: 0.97,
    reason: "downgrade-not-worth-cache-rebuild/no-change",
    jev: { response: { answers: { model: { choice: "claude-haiku-4-5-20251001" } } } },
  });
  assert.match(output, /Recommended tier: HAIKU/);
  assert.match(output, /Selected model: CLAUDE-OPUS-5-5/);
});

test("reads the recommendation from sessions recorded before the rename", () => {
  const old = { tier: "opus", jev: { response: { answers: { model_tier: { choice: "sonnet" } } } } };
  assert.match(formatExplanation(old), /Recommended tier: SONNET/);
});

test("Claude skill pre-approves its read-only explanation command", () => {
  const skill = readFileSync(new URL("../../.claude/skills/jev-explain/SKILL.md", import.meta.url), "utf8");
  assert.match(skill, /^allowed-tools: Bash\(node \*\)$/m);
});

test("says a held decision in words a person reads, not the reason code", () => {
  // The codes are internal (`decide` returns them, policy branches on them); nobody should be
  // shown one. Both surfaces translate through src/reasons.mjs.
  assert.equal(shortReason("downgrade-not-worth-cache-rebuild/no-change"), "keeping the cache");
  assert.match(longReason("downgrade-not-worth-cache-rebuild"), /re-read the whole conversation/);
  assert.equal(shortReason("jev-unavailable"), "router offline");
  assert.equal(shortReason("jev+unavailable"), "nearest available");
});

test("the status line stays quiet where the reason is obvious or not actionable", () => {
  // Unsure shows as the confidence percentage already; an override is what the person typed.
  assert.equal(shortReason("low-confidence-default"), null);
  assert.equal(shortReason("override"), null);
  // The panel still explains both.
  assert.match(longReason("low-confidence-default"), /unsure/);
  assert.match(longReason("override"), /named this model/);
});

test("an ordinary recommendation adds nothing to the status line", () => {
  assert.equal(shortReason("jev"), null);
  assert.equal(shortReason("jev/no-change"), null);
  assert.equal(longReason("jev"), "the router's recommendation");
});
