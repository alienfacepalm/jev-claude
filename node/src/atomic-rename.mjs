import { renameSync, unlinkSync } from "node:fs";

// Windows refuses to replace a file another process has open without delete sharing: antivirus,
// the search indexer and any reader opened the plain way all do this for a moment, and the rename
// fails with one of these: about a dozen times in 20,000 back-to-back renames on Windows 10.
const TRANSIENT = new Set(["EPERM", "EACCES", "EBUSY"]);
const ATTEMPTS = 10;
const PAUSE_MS = 20;

// The one blocking sleep in Node that neither spins nor starts a process.
const pause = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);

/**
 * Renames `temp` over `file`, retrying briefly while Windows reports the target as in use. Any
 * other error, or one that outlasts the retries (about 200 ms), removes `temp` and is rethrown:
 * the temporary file can hold prompt text, and nothing else ever cleans it up.
 */
export function renameOver(temp, file) {
  for (let attempt = 1; ; attempt++) {
    try {
      renameSync(temp, file);
      return;
    } catch (err) {
      if (attempt < ATTEMPTS && TRANSIENT.has(err?.code)) {
        pause(PAUSE_MS);
        continue;
      }
      try {
        unlinkSync(temp);
      } catch {
        // Already gone; the original error is the one worth reporting.
      }
      throw err;
    }
  }
}
