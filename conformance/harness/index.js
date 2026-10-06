// Entry point for `node --test conformance/harness` (SPEC.md 16.4). Node 24 treats that argument as
// a file pattern, matches the directory itself, and loads it as a module, which resolves here; so
// this file loads every test file in the directory, and the command runs the whole harness.
// Running `node --test "conformance/harness/*.test.mjs"` runs the same tests, one process per file.
import { readdirSync } from "node:fs";

for (const name of readdirSync(new URL(".", import.meta.url)).filter((n) => n.endsWith(".test.mjs")).sort()) {
  await import(new URL(name, import.meta.url));
}
