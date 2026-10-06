#!/usr/bin/env node
// Status line for Claude Code. Claude Code pipes session JSON on stdin and renders whatever
// this prints. See https://code.claude.com/docs/en/statusline
import { readStatus, agentView, readCalibration } from "../src/status.mjs";
import { shortReason } from "../src/reasons.mjs";
import { shortName } from "../src/model-names.mjs";
import { locationInfo } from "../src/worktree.mjs";
import { icons } from "../src/icons.mjs";

// Colour by the model's family, for manual sub-agents that carry a model but no routed tier.
const tierOf = (model) => /claude-([a-z]+)-/.exec(model ?? "")?.[1];

// Longest branch name shown whole; a longer one is cut with an ellipsis so the rest of the line fits.
const MAX_BRANCH = 28;
const clip = (text, max) => (text.length > max ? `${text.slice(0, max - 1)}…` : text);

const DIM = "\x1b[2m";
const BOLD = "\x1b[1m";
const RESET = "\x1b[0m";
// A status line cannot read the terminal's background, so the icons use the default foreground
// (which contrasts with it in any theme) in bold, the one emphasis that stays legible on both
// light and dark backgrounds, instead of a fixed colour that would vanish on one of them.
const I = Object.fromEntries(Object.entries(icons()).map(([name, mark]) => [name, `${BOLD}${mark}${RESET}`]));
const COLOR = { haiku: "\x1b[32m", sonnet: "\x1b[36m", opus: "\x1b[35m", fable: "\x1b[33m" };

// A status line replaces Claude Code's footer hints, so echo the basics it stops showing.
const chunks = [];
for await (const c of process.stdin) chunks.push(c);

let input = {};
try {
  input = JSON.parse(Buffer.concat(chunks).toString() || "{}");
} catch {
  // Malformed input still gets a usable line below.
}

const status = readStatus(input.session_id);
const dir = (input.workspace?.current_dir ?? input.cwd ?? "").split(/[\\/]/).pop();
const pct = Math.round(input.context_window?.used_percentage ?? 0);
const { main, subagents } = agentView(status);

/**
 * The main thread's model, with confidence, the effort it runs at, and the reason when it is
 * not the obvious one. Haiku takes no effort and sessions recorded before effort was tracked
 * have none, so the effort is simply left out for those.
 */
function mainLine(entry) {
  const color = COLOR[entry.tier] ?? "";
  const p = entry.confidence != null ? ` ${DIM}(${Math.round(entry.confidence * 100)}%)${RESET}` : "";
  const level = entry.effort ? ` ${DIM}·${RESET} ${I.effort} ${entry.effort}` : "";
  // Said in words rather than in the reason code, which is an internal name; a plain
  // recommendation has nothing to add, so it says nothing.
  const said = shortReason(entry.reason);
  const why = said ? ` ${DIM}(${said})${RESET}` : "";
  return `${I.model} ${color}${shortName(entry.model) ?? entry.model ?? entry.tier}${RESET}${p}${level}${why}`;
}

let routed = `${DIM}jev: waiting for first prompt${RESET}`;
if (main?.manual || (!main && status?.manual)) {
  // The user picked this model with /model, so show their choice rather than a tier.
  routed = `${DIM}⏸ manual${RESET} ${input.model?.display_name ?? main?.model ?? ""}`.trimEnd();
} else if (main) {
  routed = mainLine(main);
} else if (status) {
  // A session routed before per-agent tracking existed.
  routed = mainLine(status);
}

// Sub-agents run in parallel and each gets its own model, which is the whole point of showing
// them: a sub-agent on Haiku should not look like the main thread dropping to Haiku. Short names
// with the version ("Opus 5.5") rather than full ids, because three full ids do not fit on one
// line; the tier name is the fallback for a model id that does not parse.
let agents = "";
if (subagents.length) {
  const shown = subagents.slice(0, 3);
  const names = shown.map((a) => {
    const color = COLOR[a.tier ?? tierOf(a.model)] ?? "";
    return `${color}${a.manual ? "⏸" : ""}${shortName(a.model) ?? a.tier ?? a.model ?? "?"}${RESET}`;
  });
  const more = subagents.length > shown.length ? `${DIM}+${subagents.length - shown.length}${RESET}` : "";
  agents = ` ${DIM}·${RESET} ${I.agents} ${[...names, more].filter(Boolean).join(`${DIM},${RESET}`)}`;
}

// A model newer than the router's tuning: routing already uses it, but the guidance, costs and
// effort were measured on the one before, so it is worth re-calibrating.
const { newer } = readCalibration();
const notice = newer.length
  ? ` ${DIM}·${RESET} \x1b[33mnew ${newer[0]}${newer.length > 1 ? ` +${newer.length - 1}` : ""}: /jev-calibrate${RESET}`
  : "";

// The branch in any git checkout, and the worktree as well when in one: a directory name does
// not say which branch is checked out there, and a worktree is exactly where one gets lost.
// Every item on the line is a dimmed label (a glyph, or its word) and its value.
const loc = locationInfo(input);
const branchPart = loc?.branch != null ? ` ${DIM}·${RESET} ${I.branch} \x1b[34m${clip(loc.branch || "(detached)", MAX_BRANCH)}${RESET}` : "";
const worktreePart = loc?.worktree ? ` ${DIM}·${RESET} ${I.worktree} \x1b[32m${loc.worktree}${RESET}` : "";
const where = branchPart + worktreePart;
// A worktree is usually a directory of the same name, and saying it twice costs a whole item.
const dirPart = dir && dir !== loc?.worktree ? ` ${DIM}·${RESET} ${I.dir} ${dir}` : "";

process.stdout.write(`${routed}${agents}${dirPart}${where} ${DIM}·${RESET} ${I.context} ${pct}%${notice}\n`);
