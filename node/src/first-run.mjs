import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline";

/**
 * Remembers that the first-run setup check was offered, so it is offered once per user. Kept in
 * the home directory rather than the status directory: the OS clears temp, and asking again after
 * a reboot would be worse than not asking.
 */
export const FIRST_RUN_FILE = join(homedir(), ".jev-router", "first-run.json");

export function wasOffered(file = FIRST_RUN_FILE) {
  try {
    readFileSync(file);
    return true;
  } catch {
    return false;
  }
}

export function markOffered(accepted, file = FIRST_RUN_FILE) {
  try {
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, JSON.stringify({ offeredAt: new Date().toISOString(), accepted }));
  } catch {
    // Unwritable home directory: the offer comes back next time, which is harmless.
  }
}

/**
 * Whether this launch may offer the check. Only a plain interactive start qualifies: once the
 * user passes arguments (a prompt, `--resume`, `-p`), an extra opening prompt would take over
 * what they asked for, so the offer waits for a plain launch.
 */
export const shouldOffer = ({ args, interactive, offered, shadowed = false }) =>
  interactive && !offered && !shadowed && args.length === 0;

/**
 * Whether the launch directory defines its own `jev-calibrate` skill, which Claude Code might run
 * in place of the router's. The router's own repository is the one directory where that skill is
 * the router's.
 */
export function shadowsSkill(cwd, root) {
  return resolve(cwd) !== resolve(root) && existsSync(join(cwd, ".claude", "skills", "jev-calibrate"));
}

/**
 * Asks a yes/no question. Resolves true or false for an answer (an empty answer is yes), null
 * when the input closes or fails without one, and "interrupt" on Ctrl+C. Always settles:
 * `readline/promises` leaves a question pending forever once its input closes, which hung the
 * launcher before Claude Code had started.
 */
export function askYesNo(question, { input = process.stdin, output = process.stderr } = {}) {
  return new Promise((finish) => {
    const rl = createInterface({ input, output });
    let settled = false;
    const done = (value) => {
      if (settled) return;
      settled = true;
      finish(value);
      rl.close();
    };
    // readline re-emits its input's errors on the interface; with no listener there, a failing
    // terminal would crash the launcher.
    rl.on("error", () => done(null));
    rl.on("SIGINT", () => done("interrupt"));
    rl.on("close", () => done(null));
    rl.question(question, (text) => done(!/^(n|no)$/i.test(text.trim())));
  });
}
