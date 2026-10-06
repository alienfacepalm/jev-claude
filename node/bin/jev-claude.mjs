#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { startProxy } from "../src/proxy.mjs";
import { AUTO_MODEL } from "../src/config.mjs";
import { readSavedModel, restoreSavedModel } from "../src/settings.mjs";
import { LOG_FILE } from "../src/log.mjs";
import { loadEnv, childEnv } from "../src/env.mjs";
import { resolveCommand, launchSpec, spawnSpec } from "../src/launch.mjs";
import { SETTINGS_FILE, writePrivate } from "../src/status.mjs";
import { shouldOffer, shadowsSkill, wasOffered, markOffered, askYesNo } from "../src/first-run.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
// The repository root (the parent of node/), which holds the .claude/skills Claude Code is given.
const ROOT = dirname(dirname(HERE));

/**
 * Registers "Jev Router" as an extra row in Claude Code's /model picker and starts the session
 * on it. Claude Code sends the id verbatim because it does not validate model names behind a
 * custom base URL, which is what lets the proxy tell "route this" from "the user picked a
 * model". Capabilities are declared so Claude Code still composes thinking and effort for
 * the tiers that support them; the proxy strips what the routed model cannot accept.
 */
function autoModelEnv() {
  const env = {
    ANTHROPIC_CUSTOM_MODEL_OPTION: AUTO_MODEL,
    ANTHROPIC_CUSTOM_MODEL_OPTION_NAME: "Jev Router",
    ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION: "Route each turn to the cheapest model that can do it",
    ANTHROPIC_CUSTOM_MODEL_OPTION_SUPPORTED_CAPABILITIES:
      "thinking,adaptive_thinking,interleaved_thinking,effort,max_effort",
    // Some Claude Code versions validate the model client-side before it reaches the proxy;
    // this defers to the API so "jev-router" can pass through for rewriting.
    CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT: "1",
  };
  // ANTHROPIC_MODEL applies to this session only and is never written to settings, so the
  // default costs the user nothing permanent. A model they set themselves still wins.
  if (!process.env.ANTHROPIC_MODEL) env.ANTHROPIC_MODEL = AUTO_MODEL;
  return env;
}

/**
 * Claude Code saves a picker row chosen with Enter as the default for new sessions, so the
 * value from before this session is captured now and put back on the way out.
 */
const savedModelBefore = readSavedModel();

/**
 * Claude Code's UI shows the model it asked for, never the one the proxy routed to, so a
 * status line is the only way to surface the decision. `--settings` merges rather than
 * replaces, but a status line the user configured themselves still takes priority: theirs
 * is a deliberate choice and silently overwriting it would be worse than showing nothing.
 */
function statusLineArgs() {
  if (process.env.JEV_NO_STATUSLINE) return [];
  for (const dir of [join(process.cwd(), ".claude"), join(homedir(), ".claude")]) {
    try {
      if (JSON.parse(readFileSync(join(dir, "settings.json"), "utf8")).statusLine) return [];
    } catch {
      // No settings file, or unreadable; nothing to preserve.
    }
  }
  // Passed as a file rather than inline JSON, which does not survive a Windows shell. The file
  // tells Claude Code what command to run, so it lives in the owner-only status directory:
  // in a shared /tmp, anyone able to rewrite it could run commands as this user.
  const command = `"${process.execPath}" "${join(HERE, "jev-statusline.mjs")}"`;
  try {
    writePrivate(SETTINGS_FILE, JSON.stringify({ statusLine: { type: "command", command } }));
  } catch {
    return [];
  }
  return ["--settings", SETTINGS_FILE];
}

loadEnv();

const args = process.argv.slice(2);
args.push("--add-dir", ROOT);
// The Jev key is jev's alone; Claude Code and every command it runs go without it.
const env = childEnv();

const claude = resolveCommand("claude");
if (!claude) {
  process.stderr.write(
    "[jev] Claude Code is not installed, or `claude` is not on your PATH.\n" +
      "[jev] jev-claude runs the real Claude Code CLI; install it first:\n" +
      "[jev]   https://code.claude.com/docs/en/setup\n",
  );
  process.exit(1);
}

// Offered once, on the first plain interactive launch: a read-only report on the setup, so a new
// user learns straight away whether routing is on and which models it will use. The prompt goes
// first because `--add-dir` takes every argument after it as another directory.
if (
  shouldOffer({
    args: process.argv.slice(2),
    interactive: Boolean(process.stdin.isTTY && process.stdout.isTTY),
    offered: wasOffered(),
    // A repository with its own skill of that name would be what runs on the user's "yes", so
    // the offer waits for a launch from somewhere else.
    shadowed: shadowsSkill(process.cwd(), ROOT),
  })
) {
  const answer = await askYesNo(
    "[jev] First run: check your Jev Router setup now with /jev-calibrate?\n" +
      "[jev] It only reads your setup and changes nothing, using a little of your Claude usage. [Y/n] ",
  );
  // Ctrl+C at the prompt stops the launch, as it would anywhere else in a terminal. No answer at
  // all (the input closed) is not a decision, so nothing is recorded and the offer comes back.
  if (answer === "interrupt") process.exit(130);
  if (answer !== null) markOffered(answer);
  // `check` keeps it to the report even in the router's own repository, where /jev-calibrate
  // would otherwise go on to re-tune: the offer promised to change nothing.
  if (answer === true) args.unshift("/jev-calibrate check");
}

if (process.env.JEV_API_KEY || process.env.TYPESAFE_API_KEY) {
  // A host that already proxies Claude Code stays in the chain. Lattis points
  // ANTHROPIC_BASE_URL at its own daemon, which holds its separate Claude sign-in and its
  // accounting; going straight to the API from here would spend that sign-in's credentials
  // somewhere it cannot see, and most likely fail to authenticate at all. So jev picks the
  // model and hands the request to whoever was upstream before it.
  const inherited = process.env.ANTHROPIC_BASE_URL;
  const { port, close } = await startProxy(inherited ? { upstreamURL: inherited } : {});
  if (inherited && process.env.JEV_DEBUG) process.stderr.write(`[jev] upstream ${inherited}\n`);
  env.ANTHROPIC_BASE_URL = `http://127.0.0.1:${port}`;
  env.CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY = "1";
  Object.assign(env, autoModelEnv());
  process.on("exit", () => {
    close();
    restoreSavedModel(savedModelBefore);
  });
  args.push(...statusLineArgs());
  if (process.env.JEV_DEBUG && process.stdout.isTTY) {
    process.stderr.write(`[jev] routing decisions -> ${LOG_FILE}\n`);
  }
} else {
  process.stderr.write(
    `[jev] no JEV_API_KEY found - starting Claude Code without routing\n` +
      `[jev] set it in ${join(homedir(), ".jev-router.env")} to enable routing\n`,
  );
}

const child = spawnSpec(launchSpec(claude), args, { stdio: "inherit", env });

child.on("error", (err) => {
  process.stderr.write(`[jev] could not start Claude Code: ${err.message}\n`);
  process.exit(1);
});
child.on("exit", (code, signal) => process.exit(signal ? 1 : (code ?? 0)));

// Closing the terminal (SIGHUP) or a `kill` (SIGTERM) would otherwise end this process without
// running the "exit" handler, leaving "jev-router" saved as the default model and the proxy gone
// from under Claude Code. Pass the signal on and leave through the child's exit instead.
for (const signal of ["SIGHUP", "SIGTERM"]) {
  process.on(signal, () => {
    try {
      child.kill(signal);
    } catch {
      // Already gone; its exit handler is on its way.
    }
    setTimeout(() => process.exit(1), 5000).unref();
  });
}
// The terminal delivers Ctrl+C to Claude Code as well, and it decides whether that ends the
// session. Dying here on it would pull the proxy out from under a session that carries on.
process.on("SIGINT", () => {});
