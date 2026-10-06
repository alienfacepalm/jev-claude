import { readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { AUTO_MODEL } from "./config.mjs";
import { STATUS_DIR, ensureDir } from "./status.mjs";

export const USER_SETTINGS = join(homedir(), ".claude", "settings.json");

/**
 * Where the model from before the last session is kept, so a session that was killed before it
 * could clean up does not cost the user their saved default on the next run.
 */
export const SAVED_MODEL_MEMO = join(STATUS_DIR, "saved-model.json");

const memoOf = (memo) => {
  try {
    return JSON.parse(readFileSync(memo, "utf8")).model;
  } catch {
    return undefined;
  }
};

/**
 * The model saved as the user's default. A sentinel left behind by a session that did not exit
 * cleanly is not a preference, so it resolves to the model remembered from before that session.
 * A real model is remembered for the same reason.
 */
export function readSavedModel(file = USER_SETTINGS, memo = SAVED_MODEL_MEMO) {
  let model;
  try {
    model = JSON.parse(readFileSync(file, "utf8")).model;
  } catch {
    return undefined;
  }
  if (model === AUTO_MODEL) return memoOf(memo);
  try {
    if (memo === SAVED_MODEL_MEMO) ensureDir();
    writeFileSync(memo, JSON.stringify({ model: model ?? null }), { mode: 0o600 });
  } catch {
    // Remembering is best effort; restoring still works for a clean exit.
  }
  return model;
}

/**
 * Puts `previous` back if the settings file now holds the sentinel. Selecting a row with
 * Enter makes Claude Code save it as the default for new sessions, and a saved "jev-router"
 * would break plain `claude`, which has no proxy to resolve it. Anything other than an exact
 * sentinel match is left alone, so a real model chosen during the session survives.
 */
export function restoreSavedModel(previous, file = USER_SETTINGS) {
  try {
    const settings = JSON.parse(readFileSync(file, "utf8"));
    if (settings.model !== AUTO_MODEL) return false;
    if (previous == null) delete settings.model;
    else settings.model = previous;
    writeFileSync(file, `${JSON.stringify(settings, null, 2)}\n`);
    return true;
  } catch {
    return false;
  }
}
