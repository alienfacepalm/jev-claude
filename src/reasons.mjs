/**
 * How a routing decision is said to a person.
 *
 * Reason codes are internal: `decide` returns them, `shouldUseExactModel` branches on them, and
 * tests pin them. None of that is a reason to show someone `downgrade-not-worth-cache-rebuild`.
 * Anything a person reads is translated here, and nowhere else, so the two surfaces that have
 * the room for it - the status line and the explanation panel - never drift apart or invent
 * their own wording.
 *
 * `short` is for the status line, which has one line to share with the model, the agents, the
 * directory and the context gauge: a few words, lower case, no punctuation. `long` is for the
 * panel, which can afford a sentence and should answer "why am I on this model" outright.
 */
const REASONS = [
  {
    match: "override",
    short: "you asked for it",
    long: "you named this model in the prompt",
  },
  {
    match: "jev-unavailable",
    short: "router offline",
    long: "the router could not be reached, so the model was left alone",
  },
  {
    match: "low-confidence-default",
    short: "router unsure",
    long: "the router was unsure, so this ran one tier below its pick, and no lower than the default model",
  },
  {
    match: "downgrade-not-worth-cache-rebuild",
    short: "keeping the cache",
    long: "a cheaper model would have to re-read the whole conversation, which costs more than it saves",
  },
  {
    // Checked after the others: it arrives as a suffix on another reason (`+unavailable`), and
    // what the account could not run matters more than what was asked for.
    match: "unavailable",
    short: "nearest available",
    long: "the chosen tier is not available on this account, so the nearest one was used",
  },
];

const find = (reason) => (reason ? REASONS.find((r) => reason.includes(r.match)) : undefined);

/**
 * A few words for the status line, or null when the decision speaks for itself.
 *
 * A plain Jev recommendation says nothing: it is the common case, the model is already on the
 * line, and a parenthetical repeating "Jev recommendation" after every turn is noise. Everything
 * else is a decision that went against the obvious one, which is exactly when a person wants to
 * know why.
 */
export const shortReason = (reason) => find(reason)?.short ?? null;

/** A sentence for the explanation panel; every decision has one, including the ordinary one. */
export const longReason = (reason) => find(reason)?.long ?? "the router's recommendation";

/** Whether a decision left the model where it already was, which the panel words differently. */
export const isNoChange = (reason) => Boolean(reason?.includes("no-change"));
