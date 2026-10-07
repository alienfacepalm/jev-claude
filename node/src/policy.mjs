import { TIER_NAMES, THRESHOLDS, OVERRIDE_PATTERNS, rankOf } from "./config.mjs";

/**
 * The part of a prompt the user wrote themselves. Text carried into it - a sub-agent's report
 * delivered as a message, injected reminders, code, quotations - routinely names a tier without
 * asking for one: a review report quoting `"use strong" -> opus` forced Opus on the turn that
 * delivered it.
 */
const ownWords = (prompt) =>
  String(prompt ?? "")
    .replace(/<agent-message[\s\S]*?<\/agent-message>/g, " ")
    .replace(/<system-reminder>[\s\S]*?<\/system-reminder>/g, " ")
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`[^`\n]*`/g, " ")
    .replace(/"[^"\n]*"/g, " ");

/** The tier the user named explicitly in the prompt, or null. */
export function detectOverride(prompt) {
  const text = ownWords(prompt);
  const hit = OVERRIDE_PATTERNS.find((p) => p.re.test(text));
  return hit ? hit.tier : null;
}

/**
 * Nearest tier the account can actually run. Prefers stepping up rather than down so we
 * never silently hand a hard task to a weaker model, but never steps up into `fable`
 * (which bills extra usage credits) unless that is what was asked for.
 */
function clampToAvailable(tier, available) {
  if (available.includes(tier)) return tier;
  const rank = rankOf(tier);
  const up = TIER_NAMES.filter((t, i) => i > rank && available.includes(t) && (t !== "fable" || tier === "fable"));
  if (up.length) return up[0];
  const down = TIER_NAMES.filter((t, i) => i < rank && available.includes(t));
  return down.length ? down[down.length - 1] : null;
}

/**
 * Turns a Jev answer into the model we will actually run. Pure and total: any missing,
 * malformed, or unavailable input falls back to the model already in use.
 *
 * @param {object} input
 * @param {string} input.prompt        raw user prompt, for explicit-override detection
 * @param {?{choice: string, confidence: number}} input.jev  null when Jev failed
 * @param {string} input.current       tier currently active in the session
 * @param {string[]} input.available   tier names the account can run
 * @param {number} input.contextTokens approximate size of the conversation so far
 * @returns {{tier: string, reason: string, changed: boolean}}
 */
export function decide({ prompt, jev, current, available, contextTokens = 0 }) {
  const settle = (tier, reason) => {
    const final = clampToAvailable(tier, available) ?? current;
    const why = final === tier ? reason : `${reason}+unavailable`;
    return { tier: final, reason: final === current ? `${why}/no-change` : why, changed: final !== current };
  };

  const override = detectOverride(prompt);
  if (override) return settle(override, "override");

  if (!jev || !TIER_NAMES.includes(jev.choice)) return settle(current, "jev-unavailable");

  const target = jev.choice;

  // Written so a missing or non-numeric confidence counts as unsure rather than as certain.
  if (!(jev.confidence >= THRESHOLDS.minConfidence)) {
    // An unsure answer still says which way the work leans, so it runs one tier below the pick:
    // an unsure Fable on Opus, an unsure Opus on Sonnet. Never below the default, which only a
    // confident answer may go under, and never below the tier already in use. One step down also
    // keeps Fable out of reach this way - it bills extra, so only an explicit ask or a confident
    // answer gets there.
    const stepDown = Math.max(rankOf(target) - 1, rankOf(THRESHOLDS.uncertainDefault), rankOf(current));
    return settle(TIER_NAMES[stepDown], "low-confidence-default");
  }

  if (rankOf(target) < rankOf(current) && contextTokens > THRESHOLDS.downgradeMaxContextTokens) {
    return settle(current, "downgrade-not-worth-cache-rebuild");
  }

  return settle(target, "jev");
}
