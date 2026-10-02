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
  { name: "sonnet", id: "claude-sonnet-5-5", family: "sonnet", thinking: true, effort: true, floor: "high", price: 2 },
  { name: "opus", id: "claude-opus-5-5", family: "opus", thinking: true, effort: true, floor: "high", price: 4 },
  { name: "fable", id: "claude-fable-5-1", family: "fable", thinking: true, effort: true, floor: "high", price: 10 },
];

/**
 * The effort a tier is given when the request does not name one.
 *
 * The API's own default differs by model - Sonnet 5.5 thinks at `high`, Opus 5.5 at `medium` -
 * so a silent switch between them changes reasoning depth as well as model, in the opposite
 * direction to the one intended: routing "up" to Opus and landing on `medium` is a weaker think
 * than the Sonnet it came from, at twice the price. Naming the floor keeps a tier change a
 * change of model alone. A request that carries its own effort is left as it is: that is the
 * user's choice, made in Claude Code, and it outranks this.
 */
export const effortFloor = (name) => TIERS.find((t) => t.name === name)?.floor ?? null;

/** Input price per million tokens, for telling Jev what "cheapest" costs. A cached snapshot. */
export const priceOf = (name) => TIERS.find((t) => t.name === name)?.price ?? null;

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
 * Fable bills extra usage credits, so it is opt-in. Everything else is covered by a normal
 * subscription.
 */
export const availableTiers = () =>
  TIER_NAMES.filter((n) => n !== "fable" || process.env.JEV_ALLOW_FABLE === "1");

export const THRESHOLDS = {
  /** Below this Jev confidence we refuse to downgrade and cap upgrades at `uncertainCeiling`. */
  minConfidence: 0.3,
  /**
   * Where an unsure answer lands.
   *
   * Picking between four tiers at this confidence is close to a guess, and the two ways to be
   * wrong do not cost the same: a weak model on hard work burns the whole turn and is retried
   * on a stronger one anyway, while a strong model on easy work costs the difference once. So
   * an unsure turn settles here rather than on the guess - and never below the tier already in
   * use, which only a confident answer may give up.
   */
  uncertainDefault: "opus",
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
 * Haiku. So it takes an instruction verb plus a model name (the Codex names too), and the
 * generic words only when they say "model" or "tier". A hyphen after the name, as in
 * "haiku-style", is a description rather than a model.
 */
const OVERRIDE_NAMES = {
  haiku: { names: "haiku|luna", generic: "fast" },
  sonnet: { names: "sonnet|terra", generic: "balanced" },
  opus: { names: "opus|sol", generic: "strong" },
  fable: { names: "fable|astra", generic: "long" },
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
  sonnet: {
    what: "Everyday coding and agent work, including most of it: a capable model, not a fallback.",
    signals: ["Implement a specified function, test existing behaviour, or fix an understood local bug"],
    not_for: "Open-ended architecture, subtle concurrency, or unknown-cause debugging.",
  },
  opus: {
    what: "Hard reasoning, ambiguity, or high blast radius - and twice the price of sonnet.",
    signals: ["Unknown-cause debugging, cross-module design, security, auth, concurrency, or migrations"],
    not_for: "Routine work with a clear implementation, which sonnet does as well for half the cost.",
  },
  fable: {
    what: "Very large or long-running work beyond a normal focused session.",
    signals: ["Whole-repo migration, unusually large context, or multi-hour autonomous execution"],
    not_for: "Anything a strong model can finish in one focused session.",
  },
};

/** Build a Jev choice from the exact models available to this account and CLI. */
export const questionForModels = (models) =>
  choice(
    [
      "Pick the cheapest exact model that can fully complete this coding request in one pass, without retrying on a stronger model.",
      "Each tier is offered as its newest version only. Judge required reasoning, not requested reply length.",
      "Every model here runs at high effort, so a stronger tier buys a stronger model and not more thinking.",
      "Changing tier mid-conversation discards the prompt cache and re-reads the whole history, so prefer the current model where the work has not changed shape.",
    ],
    Object.fromEntries(
      models.map(({ id, tier, description }) => [
        id,
        {
          model: description ?? id,
          // What the choice costs, so "cheapest sufficient" is a judgement with a number in it
          // rather than an ordering Jev has to assume.
          cost: priceOf(tier) ? `$${priceOf(tier)} per million input tokens` : undefined,
          ...GUIDANCE[tier],
        },
      ]),
    ),
  );

/** Whether policy accepted Jev's exact model, including a version change within one tier. */
export const shouldUseExactModel = (reason, chosenTier, finalTier) =>
  (reason === "jev" || reason === "jev/no-change") && chosenTier === finalTier;
