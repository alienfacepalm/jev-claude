import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { parseEnv } from "node:util";

/**
 * Keys a project's own `.env` may set. The launch directory is often a repository someone else
 * wrote, and loading its `.env` wholesale would let it point ANTHROPIC_BASE_URL (and with it
 * Claude Code's credentials), TYPESAFE_BASE_URL (and with it the Jev key and every prompt) or
 * NODE_OPTIONS anywhere it likes. JEV_DUMP is left out for the same reason: it names a path.
 */
const PROJECT_KEYS = new Set([
  "JEV_API_KEY",
  "TYPESAFE_API_KEY",
  "JEV_DEBUG",
  "JEV_ALLOW_FABLE",
  "JEV_NO_STATUSLINE",
]);
const isProjectKey = (key) =>
  PROJECT_KEYS.has(key) || /^JEV_CODEX_[A-Z]+_MODEL$/.test(key) || /^JEV_[A-Z]+_EFFORT$/.test(key);

/** Keys only jev itself reads, removed from the environment handed to Claude Code or Codex. */
export const PRIVATE_KEYS = ["JEV_API_KEY", "TYPESAFE_API_KEY"];

function read(file) {
  try {
    return parseEnv(readFileSync(file, "utf8"));
  } catch {
    // Missing or unreadable; the key may still come from the real environment.
    return {};
  }
}

/**
 * Loads jev's settings. Existing environment variables win, followed by the project-local
 * `.env` (allow-listed keys only), the shared user-level file, then the legacy Claude-specific
 * one. The two files in the home directory belong to the user and are loaded in full.
 */
export function loadEnv({ cwd = process.cwd(), home = homedir(), env = process.env } = {}) {
  const project = Object.entries(read(join(cwd, ".env"))).filter(([key]) => isProjectKey(key));
  for (const [key, value] of [
    ...project,
    ...Object.entries(read(join(home, ".jev-router.env"))),
    ...Object.entries(read(join(home, ".jev-claude.env"))),
  ]) {
    env[key] ??= value;
  }
  return env;
}

/** A copy of `env` without the Jev key, which nothing in the child process needs. */
export function childEnv(env = process.env) {
  const out = { ...env };
  for (const key of PRIVATE_KEYS) delete out[key];
  return out;
}
