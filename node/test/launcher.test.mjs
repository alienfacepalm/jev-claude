import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { isClaudeSubcommand } from "../src/launch.mjs";

const LAUNCHER = fileURLToPath(new URL("../bin/jev-claude.mjs", import.meta.url));
const ROOT = dirname(dirname(dirname(LAUNCHER)));

// What the stand-in prints: the arguments and the variables that say whether a session was set up.
const FAKE_CLAUDE = `process.stdout.write(JSON.stringify({
  args: process.argv.slice(2),
  env: Object.fromEntries(
    ["ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL", "JEV_API_KEY"].map((k) => [k, process.env[k] ?? null]),
  ),
}));
`;

/**
 * A `claude` the launcher finds on PATH: an npm-style `.cmd` shim on Windows (the form the launcher
 * runs directly through node), an executable script elsewhere.
 */
function fakeClaude(dir) {
  const script = join(dir, "claude-fake.mjs");
  writeFileSync(script, FAKE_CLAUDE);
  if (process.platform === "win32") {
    writeFileSync(join(dir, "claude.cmd"), `@ECHO off\r\n"node"  "%~dp0\\claude-fake.mjs" %*\r\n`);
  } else {
    const file = join(dir, "claude");
    writeFileSync(file, `#!/usr/bin/env node\n${FAKE_CLAUDE}`);
    chmodSync(file, 0o755);
  }
}

/** Runs the real launcher in an empty home and working directory, with a Jev key set. */
function launch(args) {
  const base = realpathSync(mkdtempSync(join(tmpdir(), "jev-launcher-")));
  try {
    const bin = join(base, "bin");
    const home = join(base, "home");
    const cwd = join(base, "cwd");
    for (const dir of [bin, home, cwd]) mkdirSync(dir);
    fakeClaude(bin);
    const env = { ...process.env, PATH: `${bin}${delimiter}${process.env.PATH}`, HOME: home, USERPROFILE: home };
    env.JEV_API_KEY = "test-key";
    env.JEV_STATUS_DIR = join(base, "status");
    for (const name of ["ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL", "TYPESAFE_API_KEY", "JEV_NO_STATUSLINE"])
      delete env[name];
    const out = spawnSync(process.execPath, [LAUNCHER, ...args], { cwd, env, encoding: "utf8", timeout: 30_000 });
    assert.equal(out.status, 0, `${out.stdout}\n${out.stderr}`);
    return { ...JSON.parse(out.stdout), stderr: out.stderr };
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
}

test("only the first argument, spelled exactly, makes a claude subcommand", () => {
  for (const name of ["mcp", "plugin", "plugins", "doctor", "update", "upgrade", "auth", "agents", "kill", "stop"]) {
    assert.equal(isClaudeSubcommand([name, "x"]), true, name);
  }
  assert.equal(isClaudeSubcommand([]), false);
  assert.equal(isClaudeSubcommand(["update the docs"]), false, "a prompt that starts with the word");
  assert.equal(isClaudeSubcommand(["-p", "mcp"]), false, "the word later on");
  assert.equal(isClaudeSubcommand(["MCP"]), false);
  assert.equal(isClaudeSubcommand(["--model", "opus"]), false);
});

test("a session launch gets --add-dir, the settings file and the proxy", () => {
  const seen = launch([]);
  assert.equal(seen.args.at(-4), "--add-dir");
  assert.equal(seen.args.at(-3), ROOT);
  assert.equal(seen.args.at(-2), "--settings");
  assert.match(seen.env.ANTHROPIC_BASE_URL, /^http:\/\/127\.0\.0\.1:\d+$/);
  assert.equal(seen.env.ANTHROPIC_MODEL, "jev-router");
  assert.equal(seen.env.JEV_API_KEY, null, "the key is the launcher's alone");
});

test("a prompt that starts with a subcommand's name is still a session", () => {
  const seen = launch(["update the docs"]);
  assert.equal(seen.args[0], "update the docs");
  assert.ok(seen.args.includes("--add-dir"));
  assert.match(seen.env.ANTHROPIC_BASE_URL, /^http:/);
});

test("claude subcommands run untouched: no --add-dir, no proxy, no routing model, no key", () => {
  for (const args of [["mcp", "list"], ["plugin", "install", "x", "--scope", "user"], ["doctor"], ["update"]]) {
    const seen = launch(args);
    assert.deepEqual(seen.args, args);
    assert.equal(seen.env.ANTHROPIC_BASE_URL, null, args.join(" "));
    assert.equal(seen.env.ANTHROPIC_MODEL, null, args.join(" "));
    assert.equal(seen.env.JEV_API_KEY, null, "the key is stripped from the subcommand's environment too");
  }
});

test("a subcommand without a key does not announce that routing is off", () => {
  // No key: a subcommand is not a session, so there is nothing to say about routing.
  const base = realpathSync(mkdtempSync(join(tmpdir(), "jev-launcher-nokey-")));
  try {
    const bin = join(base, "bin");
    mkdirSync(bin);
    fakeClaude(bin);
    const env = { ...process.env, PATH: `${bin}${delimiter}${process.env.PATH}`, HOME: base, USERPROFILE: base };
    for (const name of ["JEV_API_KEY", "TYPESAFE_API_KEY", "ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL"]) delete env[name];
    const out = spawnSync(process.execPath, [LAUNCHER, "mcp", "list"], {
      cwd: base,
      env,
      encoding: "utf8",
      timeout: 30_000,
    });
    assert.equal(out.status, 0, out.stderr);
    assert.doesNotMatch(out.stderr, /no JEV_API_KEY/);
    assert.deepEqual(JSON.parse(out.stdout).args, ["mcp", "list"]);
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
});
