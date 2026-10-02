// Imported first by every test file that reaches src/status.mjs, so status files land in a
// throwaway directory: the real one holds live sessions' decisions, and `pruneStale` would
// delete any of them older than a week.
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const dir = mkdtempSync(join(tmpdir(), "jev-status-test-"));
process.env.JEV_STATUS_DIR = dir;
process.on("exit", () => rmSync(dir, { recursive: true, force: true }));
