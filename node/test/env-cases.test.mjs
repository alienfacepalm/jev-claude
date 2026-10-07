// The .env golden cases (SPEC 9.1, conformance/cases/parse-env.json) run through env.mjs's real
// file path: each text is written to a throwaway ~/.jev-router.env and loaded with loadEnv. The
// cases were generated on the pinned Node runtime; a Node whose util.parseEnv disagrees (22.0 to
// 22.15 and every 23.x) fails here instead of quietly loading different keys than the ports do.
import test from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { loadEnv } from "../src/env.mjs";

const CASES = JSON.parse(readFileSync(new URL("../../conformance/cases/parse-env.json", import.meta.url), "utf8"));

test("every parse-env golden case loads the same keys from the user's .jev-router.env", (t) => {
  const base = mkdtempSync(join(tmpdir(), "jev-env-cases-"));
  t.after(() => rmSync(base, { recursive: true, force: true }));
  assert.ok(CASES.length > 0);
  for (const [i, { name, input, expected }] of CASES.entries()) {
    const home = join(base, `home-${i}`);
    const cwd = join(base, `cwd-${i}`);
    mkdirSync(home);
    mkdirSync(cwd);
    writeFileSync(join(home, ".jev-router.env"), input.text);
    // A null-prototype object, so a key such as `__proto__` is stored like any other.
    const env = loadEnv({ cwd, home, env: Object.create(null) });
    // loadEnv skips blank values: a blank line in a copied .env.example must not hide a real key.
    const want = Object.fromEntries(Object.entries(expected).filter(([, value]) => value !== ""));
    assert.deepEqual({ ...env }, want, `case "${name}"`);
  }
});
