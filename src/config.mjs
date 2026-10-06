// Every routing decision knob lives here, so the whole policy is reviewable in one file.
import { choice, score } from "@typesafe-ai/sdk";

/**
 * Model tiers, cheapest first. `id` is what goes into the API request body; `family` is the
 * substring used to recognise whatever model Claude Code asked for, which may be an older
 * version within the same tier such as `claude-sonnet-4-6`. The capability flags come from
 * the Agent SDK's model catalogue: Haiku supports neither adaptive thinking nor effort, so
 * those fields have to be stripped when routing down to it.
 */
export const TIERS = [
  { name: "haiku", id: "claude-haiku-4-5-20251001", family: "haiku", thinking: false, effort: false },
  { name: "sonnet", id: "claude-sonnet-5-5", family: "sonnet", thinking: true, effort: true, floor: "high" },
  { name: "opus", id: "claude-opus-5-5", family: "opus", thinking: true, effort: true, floor: "medium" },
  { name: "fable", id: "claude-fable-5-1", family: "fable", thinking: true, effort: true, floor: "high" },
];

const EFFORTS = ["low", "medium", "high", "xhigh", "max"];

/**
 * The effort a tier is given when the request does not name one.
 *
 * Each tier has its own default (Sonnet `high`, Opus `medium`, matching what the API itself
 * does for those models) rather than one level for all, so a tier is run at the depth that suits
 * it. `JEV_<TIER>_EFFORT` (for example `JEV_OPUS_EFFORT=high`) overrides it; an unrecognised
 * value is ignored rather than sent to the API. A request that carries its own effort is left
 * as it is: that is the user's choice, made in Claude Code, and it outranks this.
 */
export const effortFloor = (name, env = process.env) => {
  const tier = tierSpec(name);
  if (!tier?.floor) return null;
  const chosen = env[`JEV_${name.toUpperCase()}_EFFORT`]?.trim().toLowerCase();
  return EFFORTS.includes(chosen) ? chosen : tier.floor;
};

/**
 * An effort that replaces whatever the request carries, or null when none is forced.
 *
 * `JEV_<TIER>_FORCE_EFFORT` sets it for one tier and `JEV_FORCE_EFFORT` for every tier that takes
 * an effort, the tier's own setting winning. Unlike `effortFloor` this outranks Claude Code's own
 * effort, which is what makes it a default Claude Code cannot undo: use it when `/effort` or
 * the sent `high` is not the depth you want paid for. An unrecognised value is ignored, and Haiku,
 * which takes no effort, never gets one.
 */
export const forcedEffort = (name, env = process.env) => {
  if (!tierSpec(name)?.effort) return null;
  for (const key of [`JEV_${name.toUpperCase()}_FORCE_EFFORT`, "JEV_FORCE_EFFORT"]) {
    const chosen = env[key]?.trim().toLowerCase();
    if (EFFORTS.includes(chosen)) return chosen;
  }
  return null;
};

export const TIER_NAMES = TIERS.map((t) => t.name);

export const rankOf = (name) => TIER_NAMES.indexOf(name);

export const idOf = (name) => TIERS.find((t) => t.name === name)?.id;

export const tierSpec = (name) => TIERS.find((t) => t.name === name);

/**
 * Sentinel model id offered as an extra row in Claude Code's /model picker. Claude Code
 * sends it verbatim because it does not validate model names behind a custom base URL, so
 * its presence in a request is an exact signal that the user wants this turn routed. Any
 * other model means the user picked one themselves and it must be passed straight through.
 */
export const AUTO_MODEL = "jev-router";

/** Whether a request should be routed, or passed through as the user's own choice. */
export const isAuto = (model) => model === AUTO_MODEL;

/** Tier name for a model string Claude Code sent, or null if we don't recognise it. */
export const tierOf = (model) =>
  TIERS.find((t) => typeof model === "string" && model.includes(t.family))?.name ?? null;

/**
 * Every tier is on offer by default, Fable included. Fable bills extra usage credits rather
 * than being covered by a normal subscription, so `JEV_ALLOW_FABLE=0` (or `false`/`no`) takes it
 * off the menu. Even when allowed, policy never steps up into it as a substitute; only an explicit
 * ask or a confident Jev answer reaches it.
 */
export const fableAllowed = (env = process.env) =>
  !/^(0|false|no|off)$/i.test(env.JEV_ALLOW_FABLE?.trim() ?? "");

export const availableTiers = (env = process.env) =>
  TIER_NAMES.filter((n) => n !== "fable" || fableAllowed(env));

export const THRESHOLDS = {
  /**
   * Below this Jev confidence the pick is not followed exactly: the turn runs one tier below it,
   * and no lower than `uncertainDefault` or the tier already in use.
   *
   * A pick between 0.3 and 0.6 is close to a coin flip, so it is not followed down to Haiku or all
   * the way up to a tier the work may not need, but it still says which way the work leans. In
   * calibration Jev named the right tier for hard work far more often than it was sure of it.
   * Sound picks measured 0.75-0.96.
   */
  minConfidence: 0.6,
  /**
   * The floor for an unsure answer, and the tier a conversation starts on before anything has
   * been routed (which is also where a turn stays if Jev cannot be reached).
   *
   * Sonnet by the user's choice: a capable model, cheaper per task than Opus, with Jev's answers
   * still free to move a turn up. It never lands below the tier already in use, which only a
   * confident answer may give up.
   */
  uncertainDefault: "sonnet",
  /**
   * Switching models invalidates the prompt cache; the next turn re-sends the whole
   * conversation. Measured at ~23.6k cache-creation tokens switching into Opus, so a
   * downgrade only pays off while the conversation is still small.
   */
  downgradeMaxContextTokens: 20000,
  /**
   * Per-attempt Jev HTTP timeout and the hard wall-clock deadline for the whole routing
   * call. Measured: ~300-350ms warm, ~900-1000ms on the first call (TLS handshake), so the
   * deadline leaves room for one retry after a cold-start timeout.
   */
  jevTimeoutMs: 1500,
  jevDeadlineMs: 3000,
  jevMaxRetries: 1,
};

export const CONTEXT_WINDOW_TOKENS = 200000;

const COMPLEXITY_SCALE = [
  "None",
  "Very low",
  "Low",
  "Some",
  "Moderate",
  "Moderate to high",
  "High",
  "Very high",
  "Severe",
  "Extreme",
];

export const COMPLEXITY_MAX_SCORE = COMPLEXITY_SCALE.length - 1;

/**
 * Phrases that mean "the human already decided", checked against the raw prompt.
 *
 * An override beats Jev outright, so a false match silently pins an ordinary prompt: "replace
 * the loop with long polling" used to land on Fable, "help with fast fourier transforms" on
 * Haiku. So it takes an instruction verb plus a model name, and the generic words only when
 * they say "model" or "tier". A hyphen after the name, as in "haiku-style", is a description
 * rather than a model.
 */
const OVERRIDE_NAMES = {
  haiku: { names: "haiku", generic: "fast" },
  sonnet: { names: "sonnet", generic: "balanced" },
  opus: { names: "opus", generic: "strong" },
  fable: { names: "fable", generic: "long" },
};

export const OVERRIDE_PATTERNS = TIERS.map((t) => {
  const { names, generic } = OVERRIDE_NAMES[t.name];
  return {
    tier: t.name,
    re: new RegExp(
      `\\b(?:use|switch to|switch over to|route to)\\s+(?:the\\s+)?(?:claude[-\\s])?` +
        `(?:(?:${names})|${generic}\\s+(?:model|tier))(?![-\\w])`,
      "i",
    ),
  };
});

export const QUESTIONS = {
  task_complexity: score(
    "How complex is the coding task overall, including ambiguity, scope, and blast radius?",
    COMPLEXITY_SCALE,
  ),
  reasoning_required: score(
    "How much reasoning is required to complete the request correctly in one pass?",
    COMPLEXITY_SCALE,
  ),
  tool_complexity: score(
    "How complex is the tool use required, from no tools to many coordinated or stateful operations?",
    COMPLEXITY_SCALE,
  ),
};

const GUIDANCE = {
  haiku: {
    what: "Trivial, mechanical, or purely factual work.",
    signals: ["Rename, reformat, comment, or run one obvious command"],
    not_for: "Design judgement or multi-file reasoning.",
  },
  // Sonnet and Opus are described from measurements at the effort they actually run at: Claude Code
  // sends `high` on every request (checked 2026-10), so both run at high and the tier floors above
  // rarely apply. Anthropic's September 2026 launch charts at high: Opus led on every benchmark, by
  // 5-21 points on agentic coding (Terminal-Bench 64.2 vs 43.0, CursorBench 56.0 vs 47.8,
  // FrontierCode 54.0 vs 49.4) and ~70 Elo on knowledge work (AA-Briefcase 1705 vs 1634), at
  // 1.6-2.6x Sonnet's cost per task.
  sonnet: {
    what:
      "Well-scoped everyday work, and documents and knowledge work, where it does well for well under Opus's cost.",
    signals: [
      "Implement a specified function or change, add tests, or fix a bug whose cause is already known",
      "Write or edit documents, specs, summaries, or analysis",
    ],
    not_for:
      "Open-ended or multi-step coding, changes that must not break existing behaviour, unknown-cause debugging, or judgement calls: Opus scores 5-21 points higher on agentic coding.",
  },
  opus: {
    what:
      "Complex or open-ended coding and work that needs sustained judgement, where it clearly beats Sonnet, for about twice the cost per task.",
    signals: [
      "Unknown-cause or intermittent bugs, multi-step changes across a codebase, cross-module design, API or behaviour-preserving changes, security, auth, concurrency, or migrations",
    ],
    not_for: "Well-scoped changes and routine document or knowledge work, where Sonnet does well for less.",
  },
  // From Anthropic's model guidance: Fable is the step up for the hardest long-running agentic and
  // research work, and for work where Opus at higher effort still falls short; Opus stays the
  // default for most work, complex agentic coding included. Context size is not a reason to pick
  // it: Sonnet and Opus share its 1M window. Adversarial plan review is the user's own addition.
  fable: {
    what:
      "Long-horizon autonomous work, the most demanding reasoning, and adversarial review that hardens a plan, spec, or design by hunting for how it fails.",
    // Kept to a few signals that do not overlap Opus's. A longer list of everything Fable is good
    // at measured worse: Jev split between Opus and Fable, and a red-team prompt that scored 0.80
    // on a short list fell to 0.31, under the confidence bar.
    signals: [
      "Long-horizon autonomous work: a whole-repo migration, a large system built end to end from a spec, or a full deliverable such as financial analysis with spreadsheets and slides",
      "Adversarially review, red-team, stress-test, or poke holes in a plan, spec, or design to harden it",
      "A problem that has already defeated a strong model, such as a bug two attempts have missed",
    ],
    not_for:
      "Writing the plan or spec itself, ordinary code review, or security-focused analysis, where Fable's safety classifiers can decline.",
  },
};

// Per-task figures are from the September 2026 measurements above; Haiku and Fable have no
// published per-task comparison, so only their per-token price is stated.
const COST = {
  haiku: "$1 / $5 per million input / output tokens; the cheapest, but it does no reasoning",
  sonnet: "$2 / $10 per million tokens; about 40-60% less per completed task than Opus",
  opus: "$4 / $20 per million tokens; about 1.6-2.6x Sonnet's cost per completed task",
  fable: "$10 / $50 per million tokens; the most expensive by far",
};

/** Build a Jev choice from the exact models available to this account and CLI. */
export const questionForModels = (models) =>
  choice(
    [
      "Pick the cheapest exact model that can fully complete this coding request in one pass, without retrying on a stronger model.",
      "When you are unsure whether a cheaper model would get it right, pick the stronger one: a failed attempt wastes the whole turn and is rerun anyway, so it costs more than the difference. Speed does not matter.",
      "Each tier is offered as its newest version only. Judge required reasoning, not requested reply length.",
      "Reasoning effort is set per tier and is not something to choose between; judge only which model the work needs.",
      "Changing tier mid-conversation discards the prompt cache and re-reads the whole history, so prefer the current model where the work has not changed shape.",
    ],
    Object.fromEntries(
      models.map(({ id, tier, description }) => [
        id,
        {
          model: description ?? id,
          // What the choice costs per completed task, so "cheapest sufficient" is a judgement with a
          // number in it. Per-token prices alone mislead: Opus is twice Sonnet's per token but uses
          // fewer tokens, so the per-task gap is much smaller.
          cost: COST[tier],
          ...GUIDANCE[tier],
        },
      ]),
    ),
  );

/** Whether policy accepted Jev's exact model, including a version change within one tier. */
export const shouldUseExactModel = (reason, chosenTier, finalTier) =>
  (reason === "jev" || reason === "jev/no-change") && chosenTier === finalTier;
