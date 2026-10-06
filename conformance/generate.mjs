#!/usr/bin/env node
// Writes conformance/cases/<name>.json from the Node implementation itself (SPEC.md 16.2):
// every `expected` is what the real Node function returned for that `input`, never typed by hand.
// Run from anywhere with `node conformance/generate.mjs`; the output is deterministic, so a
// second run must leave `git status conformance/cases` unchanged.
import { mkdirSync, mkdtempSync, readdirSync, rmSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { encode } from "./generator/tagged.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));
const casesDir = join(here, "cases");

// status.mjs and settings.mjs bind the status directory when they load, and the first status write
// prunes week-old files there. Point it at a throwaway directory before anything imports them, so
// generating cases can never touch a live session's files.
const statusDir = mkdtempSync(join(tmpdir(), "jev-conformance-gen-"));
process.env.JEV_STATUS_DIR = statusDir;
process.on("exit", () => rmSync(statusDir, { recursive: true, force: true }));
// The Jev client reads these once, lazily; the jev-request cases stub fetch, so the key is fake.
process.env.JEV_API_KEY = "conformance-test-key";
delete process.env.TYPESAFE_API_KEY;
delete process.env.TYPESAFE_BASE_URL;
delete process.env.TYPESAFE_DEFAULT_MODEL;
delete process.env.JEV_DEBUG;
delete process.env.JEV_DUMP;

const node = (name) => import(new URL(`../node/src/${name}.mjs`, import.meta.url));
const ctx = {
  root: join(here, ".."),
  statusDir,
  config: await node("config"),
  policy: await node("policy"),
  proxy: await node("proxy"),
  status: await node("status"),
  router: await node("router"),
  reasons: await node("reasons"),
  icons: await node("icons"),
  legend: await node("legend"),
  explain: await node("explain"),
  modelNames: await node("model-names"),
  worktree: await node("worktree"),
  update: await node("update"),
  launch: await node("launch"),
  calibration: await import(new URL("../node/scripts/calibration-cases.mjs", import.meta.url)),
};

// One group at a time: the status group pins Date.now and the network group replaces fetch.
const groups = [];
for (const name of ["policy", "proxy", "status", "display", "misc", "json", "network"]) {
  groups.push(await (await import(`./generator/${name}.mjs`)).default(ctx));
}

mkdirSync(casesDir, { recursive: true });
const written = new Set();
for (const group of groups) {
  for (const [file, cases] of Object.entries(group)) {
    if (written.has(file)) throw new Error(`case file ${file} produced twice`);
    const names = new Set();
    const out = cases.map(({ name, input, expected }) => {
      if (names.has(name)) throw new Error(`${file}: duplicate case name ${name}`);
      names.add(name);
      return { name, input: encode(input), expected: encode(expected) };
    });
    writeFileSync(join(casesDir, `${file}.json`), `${JSON.stringify(out, null, 2)}\n`);
    written.add(file);
    console.log(`${file}.json: ${out.length} cases`);
  }
}
// A case file the generator no longer produces must not linger and be trusted by a port.
for (const name of readdirSync(casesDir)) {
  if (name.endsWith(".json") && !written.has(name.slice(0, -5))) unlinkSync(join(casesDir, name));
}
