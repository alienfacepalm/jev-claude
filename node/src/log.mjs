import { appendFileSync, chmodSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

export const LOG_FILE = join(homedir(), ".jev-claude.log");

// Claude Code owns the terminal in interactive mode and redraws over anything we print, so
// writing to stderr there corrupts its UI. Log to a file instead and leave stderr alone.
// In print mode (`-p`) there is no TUI to damage, so stderr stays convenient for piping.
const interactive = process.stdout.isTTY;

// Debug lines quote the start of each prompt, so the log is the owner's alone.
const FILE_MODE = 0o600;
let tightened = false;

export function log(line) {
  const text = `[jev] ${line}\n`;
  if (!interactive) return void process.stderr.write(text);
  try {
    appendFileSync(LOG_FILE, `${new Date().toISOString()} ${text}`, { mode: FILE_MODE });
    if (!tightened) {
      tightened = true;
      // `mode` only applies on creation; tighten a log left by an earlier version too.
      chmodSync(LOG_FILE, FILE_MODE);
    }
  } catch {
    // A broken log file must never take down the session.
  }
}

export const debug = (line) => process.env.JEV_DEBUG && log(line);
