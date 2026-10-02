import {
  chmodSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  statSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// One file per session rather than a shared map, so concurrent jev-claude sessions can never
// clobber each other's status. Kept in the temp dir so the OS eventually cleans up.
const DIR = join(tmpdir(), "jev-claude");

// Status files hold prompt text and exact Jev exchanges, so only the owner may read them.
// On Linux the temp dir is the shared /tmp; macOS and Windows temp dirs are already per-user,
// where these modes are harmless (Windows ignores them).
const DIR_MODE = 0o700;
const FILE_MODE = 0o600;

// Files not updated for this long belong to finished sessions and are removed.
export const STALE_AFTER_MS = 7 * 24 * 60 * 60 * 1000;
let pruned = false;

// The --settings file jev-claude hands Claude Code. Lives here so it gets the same private
// directory, and is never pruned: a session can outlive the stale cutoff.
export const SETTINGS_FILE = join(DIR, "settings.json");

const fileFor = (sessionId) => join(DIR, `${sessionId.replace(/[^\w-]/g, "")}.json`);

/**
 * Writes `text` to `file` inside the status directory, owner-only.
 *
 * Written to a temporary name and renamed into place, so the status line, which reads these
 * files on every redraw, never sees one half-written and drops back to "waiting".
 */
export function writePrivate(file, text) {
  ensureDir();
  const temp = `${file}.${process.pid}.tmp`;
  writeFileSync(temp, text, { mode: FILE_MODE });
  renameSync(temp, file);
  // `mode` only applies on creation; tighten files written by earlier versions too.
  chmodSync(file, FILE_MODE);
}

/** Publish the latest routing decision so the status line can display it. */
export function writeStatus(sessionId, status) {
  if (!sessionId) return;
  try {
    writePrivate(fileFor(sessionId), JSON.stringify(status));
    if (!pruned) {
      pruned = true;
      pruneStale();
    }
  } catch {
    // Status display is cosmetic and must never interfere with a request.
  }
}

// Agents whose decision is kept in a session file. Claude Code runs sub-agents in parallel
// but never says when one finishes, so the map is bounded by recency instead.
const MAX_AGENTS = 12;

/**
 * Publish a routed prompt and retain recent exact Jev exchanges for diagnosis.
 *
 * `agent` identifies which conversation inside the session was routed: the main thread or
 * one sub-agent. Claude Code runs every agent through one endpoint and one session id, so
 * without this the last agent to be routed would be the only one visible, and a sub-agent
 * dropping to Haiku would read as the whole session dropping to Haiku. Omitted by Codex,
 * which has no sub-agents.
 *
 * @param {string} sessionId
 * @param {object} decision
 * @param {?{key: string, label: string, main: boolean}} agent
 */
export function writeDecision(sessionId, decision, agent = null) {
  const previous = readStatus(sessionId);
  // Tagged with the agent so the explanation can show the main thread's own last decision
  // rather than whichever sub-agent happened to be routed most recently.
  const entry = agent ? { ...decision, agent: { key: agent.key, label: agent.label, main: agent.main } } : decision;
  const history = [...(previous?.history ?? []), entry].slice(-20);
  const agents = agent
    ? mergeAgent(previous?.agents, agent, {
        label: agent.label,
        main: agent.main,
        tier: decision.tier,
        model: decision.model,
        confidence: decision.confidence,
        reason: decision.reason,
        at: decision.at ?? Date.now(),
      })
    : previous?.agents;
  // Flat fields stay the latest decision: Codex and `jev-explain` already read them, and the
  // status line prefers the main agent out of `agents` when it is present.
  writeStatus(sessionId, { ...decision, ...(agents ? { agents } : {}), history });
}

/**
 * Records that an agent is running a model the user chose rather than a routed one.
 *
 * Kept separate from `writeDecision` because a manual choice carries no Jev decision, and
 * separate from `writeStatus` because it must preserve the other agents: Claude Code's own
 * auxiliary calls and any sub-agent pinned to a fixed model arrive here, and a whole-file
 * overwrite is what currently blanks a session's routing state mid-run.
 */
export function markManual(sessionId, model, agent = null) {
  const previous = readStatus(sessionId);
  const agents = agent
    ? mergeAgent(previous?.agents, agent, {
        label: agent.label,
        main: agent.main,
        model,
        manual: true,
        at: Date.now(),
      })
    : previous?.agents;
  // Only the main thread's choice pauses the session as a whole. A sub-agent pinned to its
  // own model says nothing about what the main conversation is doing.
  const manual = agent ? (agent.main ? true : (previous?.manual ?? false)) : true;
  writeStatus(sessionId, { ...previous, ...(agents ? { agents } : {}), manual, at: Date.now() });
}

/**
 * The main thread's most recent full decision, or the latest one when the session has no
 * per-agent record (Codex, or a session that predates agent tracking).
 */
export function mainDecision(status) {
  if (!status) return null;
  const mine = [...(status.history ?? [])].reverse().find((d) => d.agent?.main);
  return mine ?? status;
}

/** Newest-wins merge of one agent into the map, trimmed to the most recent `MAX_AGENTS`. */
function mergeAgent(existing, agent, entry) {
  const agents = { ...(existing ?? {}) };
  agents[agent.key] = { ...agents[agent.key], ...entry };
  const keys = Object.keys(agents);
  if (keys.length > MAX_AGENTS) {
    // Keep the main thread regardless of age; it is the one line always worth showing.
    const ordered = keys
      .filter((k) => !agents[k].main)
      .sort((a, b) => (agents[b].at ?? 0) - (agents[a].at ?? 0));
    for (const stale of ordered.slice(MAX_AGENTS - 1)) delete agents[stale];
  }
  return agents;
}

/**
 * The main thread's entry and the sub-agents considered still live, newest first.
 *
 * Liveness is inferred from recency because Claude Code never reports an agent finishing.
 * `freshMs` is deliberately short: a stale sub-agent line is worse than no line, since it
 * suggests work is still running when it is not.
 */
export function agentView(status, { freshMs = 90_000, now = Date.now() } = {}) {
  const entries = Object.entries(status?.agents ?? {}).map(([key, a]) => ({ key, ...a }));
  return {
    main: entries.find((a) => a.main) ?? null,
    subagents: entries
      .filter((a) => !a.main && now - (a.at ?? 0) <= freshMs)
      .sort((a, b) => (b.at ?? 0) - (a.at ?? 0)),
  };
}

// Shared across sessions rather than per session: which models are newer than the router's
// calibration is a fact about the account, read whenever a session loads the model list.
const CALIBRATION_FILE = join(DIR, "calibration.json");

/** Records the account's models that are newer than the router was calibrated for. */
export function writeCalibration(newer, file = CALIBRATION_FILE) {
  try {
    writePrivate(file, JSON.stringify({ newer, at: Date.now() }));
  } catch {
    // A missed notice is cosmetic and must never interfere with a request.
  }
}

/** Model ids newer than the router's calibration, or an empty list. */
export function readCalibration(file = CALIBRATION_FILE) {
  try {
    const { newer } = JSON.parse(readFileSync(file, "utf8"));
    return Array.isArray(newer) ? newer : [];
  } catch {
    return [];
  }
}

/** Latest routing decision for a session, or null if none has been made yet. */
export function readStatus(sessionId) {
  try {
    return JSON.parse(readFileSync(fileFor(sessionId), "utf8"));
  } catch {
    return null;
  }
}

/**
 * Creates the status directory, owner-only. Directories created by earlier versions were
 * world-readable. chmod throws if another user owns the directory, as they would if they had
 * created it first in a shared /tmp, so every caller's write fails and is skipped rather than
 * landing somewhere that user controls.
 */
export function ensureDir() {
  mkdirSync(DIR, { recursive: true, mode: DIR_MODE });
  chmodSync(DIR, DIR_MODE);
}

let dumped = 0;

/**
 * Saves a request body for diagnosing wire-format changes (`JEV_DUMP`). `1` or `true` files it
 * in the private status directory; any other value is used as a path prefix, as before. Bodies
 * hold the whole conversation, so they are owner-only either way, and numbered so parallel
 * sub-agents in the same millisecond do not overwrite each other.
 */
export function dumpBody(body, setting = process.env.JEV_DUMP) {
  if (!setting) return null;
  const prefix = /^(1|true|yes)$/i.test(setting) ? join(DIR, "dump") : setting;
  const file = `${prefix}.${Date.now()}-${dumped++}.json`;
  try {
    if (prefix.startsWith(DIR)) ensureDir();
    writeFileSync(file, JSON.stringify(body, null, 2), { mode: FILE_MODE });
    return file;
  } catch {
    return null;
  }
}

/** Delete status files untouched for `maxAgeMs`. Runs once per process on the first write. */
export function pruneStale(maxAgeMs = STALE_AFTER_MS, now = Date.now()) {
  let removed = 0;
  try {
    for (const name of readdirSync(DIR)) {
      if (!name.endsWith(".json") || name === "settings.json") continue;
      const file = join(DIR, name);
      try {
        if (now - statSync(file).mtimeMs > maxAgeMs) {
          unlinkSync(file);
          removed++;
        }
      } catch {
        // Another session may have removed or replaced it; ignore.
      }
    }
  } catch {
    // Missing or unreadable directory: nothing to prune.
  }
  return removed;
}

/** Directory holding status files, exposed for tests and diagnostics. */
export const STATUS_DIR = DIR;
