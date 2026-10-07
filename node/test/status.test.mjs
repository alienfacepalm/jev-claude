import "./isolate-status.mjs";
import test from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { readdirSync } from "node:fs";
import { join } from "node:path";
import { readStatus, STATUS_DIR, writeDecision } from "../src/status.mjs";

const MAIN = { key: "main", label: "main", main: true };
const decision = (prompt) => ({ tier: "sonnet", prompt, model: "claude-sonnet-5-5", reason: "jev", at: Date.now() });
const leftovers = () => readdirSync(STATUS_DIR).filter((name) => name.endsWith(".tmp"));

/**
 * Opens `file` from another process the way antivirus, the indexer and a plain reader do on
 * Windows (read sharing, no delete sharing, so nothing can be renamed over it), and resolves once
 * the handle is open. The handle is closed after `ms`; the returned promise `released` settles then.
 */
async function holdOpen(file, ms) {
  const script =
    `$f = [IO.File]::Open('${file.replaceAll("'", "''")}', 'Open', 'Read', 'Read'); ` +
    `[Console]::Out.WriteLine('held'); [Console]::Out.Flush(); ` +
    `[Threading.Thread]::Sleep(${ms}); $f.Close()`;
  const child = spawn("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], {
    stdio: ["ignore", "pipe", "inherit"],
    windowsHide: true,
  });
  const released = new Promise((resolve) => child.once("exit", resolve));
  await new Promise((resolve, reject) => {
    child.stdout.on("data", (c) => c.toString().includes("held") && resolve());
    child.once("exit", (code) => reject(new Error(`powershell exited ${code} before holding the file`)));
  });
  return { released, child };
}

const windowsOnly = { skip: process.platform !== "win32" && "only Windows refuses to rename over an open file" };

test("a status write lands while another process briefly holds the file open", windowsOnly, async () => {
  const id = `held-briefly-${process.pid}`;
  writeDecision(id, decision("first"), MAIN);
  const { released } = await holdOpen(join(STATUS_DIR, `${id}.json`), 50);
  // Synchronous, so it retries while the other process still holds the file.
  writeDecision(id, decision("second"), MAIN);
  assert.equal(await released, 0);
  const status = readStatus(id);
  assert.equal(status.prompt, "second", "the write was retried until the file was free");
  assert.deepEqual(
    status.history.map((d) => d.prompt),
    ["first", "second"],
  );
  assert.deepEqual(leftovers(), []);
});

test("a status write that never gets the file is dropped and leaves no temporary file", windowsOnly, async () => {
  const id = `held-long-${process.pid}`;
  writeDecision(id, decision("first"), MAIN);
  const { released } = await holdOpen(join(STATUS_DIR, `${id}.json`), 2000);
  writeDecision(id, decision("second"), MAIN);
  // The temp file held the prompt text; it must be gone even though the rename never happened.
  assert.deepEqual(leftovers(), []);
  await released;
  assert.equal(readStatus(id).prompt, "first", "the held file kept its previous content");
});
