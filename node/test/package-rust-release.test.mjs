import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { PROGRAMS, hostTarget, nativeSkill } from "../scripts/package-rust-release.mjs";

const SKILLS = join(dirname(fileURLToPath(import.meta.url)), "..", "..", ".claude", "skills");
const skillText = (name) => readFileSync(join(SKILLS, name, "SKILL.md"), "utf8").replace(/\r\n/g, "\n");

test("every skill in the repository becomes one that runs a native program and nothing else", () => {
  const names = readdirSync(SKILLS);
  assert.deepEqual(names.toSorted(), ["jev-calibrate", "jev-explain", "jev-legend"]);
  for (const name of names) {
    const out = nativeSkill(skillText(name), name);
    const call = out.split("\n").find((line) => line.startsWith("!`"));
    const program = call.slice(2).split(/[ `]/)[0];
    assert.ok(PROGRAMS.includes(program), `${name} calls ${program}, which is not one of the shipped programs`);
    assert.match(
      out,
      new RegExp(`^allowed-tools: Bash\\(${program} \\*\\) PowerShell\\(${program} \\*\\)$`, "m"),
      name,
    );
    assert.doesNotMatch(out, /\bnode\b[^\n]*\.mjs|node\/(bin|src|scripts)/, `${name} still needs Node`);
  }
});

test("jev-explain keeps its session id argument and its instructions", () => {
  const out = nativeSkill(skillText("jev-explain"), "jev-explain");
  assert.ok(out.includes(`!\`jev-explain "\${CLAUDE_SESSION_ID}"\``), out);
  assert.match(out, /Return the report below verbatim in a plain text code block/);
});

test("the installed jev-calibrate only reports: re-tuning needs the repository's Node scripts", () => {
  const out = nativeSkill(skillText("jev-calibrate"), "jev-calibrate");
  assert.ok(out.includes("!`jev-check`"), out);
  assert.doesNotMatch(out, /Repository: re-calibrate|calibrate\.mjs|config\.mjs/);
  assert.match(out, /Report only \(the default\)/);
  assert.match(out, /re-tuning is done in\na clone of the router's repository/);
  assert.match(out, /^description: Check Jev Router's setup and models \(read-only\)\.$/m);
});

test("a skill that no longer calls a node/bin program or lost its allowed-tools fails the build", () => {
  const explain = skillText("jev-explain");
  assert.throws(
    () => nativeSkill(explain.replace(/node "[^"]+"/, "echo"), "jev-explain"),
    /no call to a node\/bin program/,
  );
  assert.throws(() => nativeSkill(explain.replace("Bash(node *)", "Bash(*)"), "jev-explain"), /allowed-tools/);
  const extra = `${explain}\nThen run: node node/scripts/calibrate.mjs\n`;
  assert.throws(() => nativeSkill(extra, "jev-explain"), /still refers to the Node implementation/);
});

test("the archive name is the platform and CPU the installers ask for", () => {
  assert.match(hostTarget(), /^(windows|macos|linux)-(x64|arm64)$/);
});
