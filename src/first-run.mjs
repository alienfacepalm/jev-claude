import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { createInterface } from "node:readline/promises";

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
export const shouldOffer = ({ args, interactive, offered }) => interactive && !offered && args.length === 0;

/** Asks a yes/no question; an empty answer is yes, and a closed or failing input is no. */
export async function askYesNo(question, { input = process.stdin, output = process.stderr } = {}) {
  const rl = createInterface({ input, output });
  try {
    return !/^(n|no)$/i.test((await rl.question(question)).trim());
  } catch {
    return false;
  } finally {
    rl.close();
  }
}
