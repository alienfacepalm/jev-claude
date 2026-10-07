import { agentView } from "./status.mjs";
import { isNoChange, longReason } from "./reasons.mjs";
import { tierOf } from "./config.mjs";

/**
 * What Jev itself recommended, as a tier where the model is recognisable.
 *
 * Jev answers a question named `model` with an exact model id. Shown alongside the selected
 * model, so a turn where policy overruled Jev (an unsure answer, a refused downgrade) shows two
 * different answers rather than the selection twice. `model_tier` is what sessions recorded
 * before the question was renamed.
 */
const recommendationOf = (status) => {
  const answers = status.jev?.response?.answers;
  const choice = answers?.model?.choice ?? answers?.model_tier?.choice;
  if (!choice) return status.tier ?? "unknown";
  return tierOf(choice) ?? choice;
};

const WIDTH = 33;
const row = (text = "") => `│ ${text.slice(0, WIDTH - 2).padEnd(WIDTH - 2)} │`;
const metric = (value) => (Number.isFinite(value) ? value.toFixed(2) : "n/a");
const wrapped = (label, value) => {
  const words = `${label}${value}`.replace(/\s+/g, " ").trim().split(" ");
  const lines = [];
  for (const word of words) {
    if (!lines.length || `${lines.at(-1)} ${word}`.length > WIDTH - 2) lines.push(word);
    else lines[lines.length - 1] += ` ${word}`;
  }
  return lines.map(row);
};

// A decision that kept the model says so, since "why am I still here" is the question a held
// turn raises; the wording itself comes from src/reasons.mjs, which both surfaces share.
const decision = (reason = "") => `${isNoChange(reason) ? "kept this model - " : ""}${longReason(reason)}`;

const AGENT_WIDTH = 52;
const agentRow = (text = "") => `│ ${text.slice(0, AGENT_WIDTH - 2).padEnd(AGENT_WIDTH - 2)} │`;

const age = (at, now) => {
  const s = Math.max(0, Math.round((now - (at ?? now)) / 1000));
  return s < 60 ? `${s}s` : s < 3600 ? `${Math.round(s / 60)}m` : `${Math.round(s / 3600)}h`;
};

/**
 * Every agent routed in this session and the model each one got.
 *
 * Claude Code shows one model for the whole session, and a sub-agent's routing is invisible
 * there even though it is a separate decision. Returns "" when the session has no per-agent
 * record, which is the case for sessions started before agents were tracked.
 */
export function formatAgents(status, now = Date.now()) {
  // Diagnosis wants the whole session, not just what is still running.
  const { main, subagents } = agentView(status, { freshMs: Infinity, now });
  const all = [main, ...subagents].filter(Boolean);
  if (!all.length) return "";

  const lines = [`┌${"─".repeat(AGENT_WIDTH)}┐`, agentRow("Jev Router · agents this session"), agentRow()];
  for (const a of all) {
    const role = (a.main ? "main" : "sub").padEnd(5);
    const model = (a.model ?? a.tier ?? "unknown").toUpperCase().padEnd(22);
    const p = a.manual ? "manual" : a.confidence == null ? "" : `${Math.round(a.confidence * 100)}%`;
    lines.push(agentRow(`${role} ${model} ${p.padEnd(7)} ${age(a.at, now)}`));
    if (a.label && a.label !== "main") lines.push(agentRow(`      ${a.label}`));
  }
  lines.push(`└${"─".repeat(AGENT_WIDTH)}┘`);
  return lines.join("\n");
}

/**
 * The boxed `jev-explain` report for one decision: the prompt and session Jev was sent, its
 * scores, the recommended tier, the model selected, and why. A one-line notice instead when there
 * is no decision yet or the user picked a model manually.
 */
export function formatExplanation(status) {
  if (!status) return "Jev Router: no routing decision has been recorded for this session.";
  if (status.manual) return "Jev Router: routing is paused because you selected a model manually.";

  const m = status.metrics ?? {};
  const request = status.jev?.request?.state;
  const recommendation = recommendationOf(status);
  return [
    `┌${"─".repeat(WIDTH)}┐`,
    row("Jev Router"),
    row(),
    row("Jev request"),
    ...wrapped("Prompt: ", status.prompt ?? "not recorded"),
    // Jev is told the exact model in use, not a tier name.
    row(`Current model: ${(request?.session?.current_model ?? "unknown").toUpperCase()}`),
    row(`Context tokens: ${request?.session?.context_tokens ?? "unknown"}`),
    row(),
    row("Jev response"),
    row(`Task complexity     ${metric(m.taskComplexity)}`),
    row(`Reasoning required  ${metric(m.reasoningRequired)}`),
    row(`Tool complexity     ${metric(m.toolComplexity)}`),
    row(`Context size        ${metric(m.contextSize)}`),
    row(),
    row(`Recommended tier: ${recommendation.toUpperCase()}`),
    row(`Selected model: ${(status.model ?? status.tier ?? "unknown").toUpperCase()}`),
    row(),
    row(`Confidence: ${status.confidence == null ? "n/a" : `${Math.round(status.confidence * 100)}%`}`),
    // Wrapped rather than cut: this line is the panel's answer to "why am I on this model", and
    // a sentence clipped at the box edge answers it worse than no sentence would.
    ...wrapped("Decision: ", decision(status.reason)),
    `└${"─".repeat(WIDTH)}┘`,
  ].join("\n");
}
