#!/usr/bin/env node
// Packages the Rust programs into the archive that installs without Node.js:
//
//   node node/scripts/package-rust-release.mjs --bin-dir rust/target/release --out dist [--target windows-x64]
//
// The archive unpacks into one folder laid out like a clone as far as the Rust programs care
// (SPEC.md 2.4 finds the root by walking up from bin/ to the nearest .claude/skills/jev-calibrate):
//
//   bin/                       the seven programs
//   .claude/skills/            /jev-calibrate, /jev-explain, /jev-legend, calling those programs
//   package.json               just the name and the release version (SPEC.md 2.3)
//   .env.example  LICENSE
//
// The skills are not copies kept in step by hand: they are made here from the repository's own
// skills by swapping the `node .../node/bin/<program>.mjs` call for the native program, so a
// change to a skill reaches both editions. --stage-only leaves the folder and skips the tar.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, copyFileSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const PROGRAMS = [
  "jev-claude",
  "jev-statusline",
  "jev-explain",
  "jev-legend",
  "jev-check",
  "jev-update-check",
  "jev-proxy-host",
];

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

// The same call in every skill: the Node entry point reached from the skill's own folder.
const NODE_CALL = /node "\$\{CLAUDE_SKILL_DIR\}\/\.\.\/\.\.\/\.\.\/node\/bin\/(jev-[a-z]+)\.mjs"/;

/** The platform name used in archive names: `windows-x64`, `linux-arm64`, `macos-x64`... */
export function hostTarget() {
  const os = { win32: "windows", darwin: "macos", linux: "linux" }[process.platform];
  const arch = { x64: "x64", arm64: "arm64" }[process.arch];
  if (!os || !arch) throw new Error(`no release target for ${process.platform}/${process.arch}`);
  return `${os}-${arch}`;
}

/**
 * One repository skill turned into the native edition's: the program is called by name (it is on
 * PATH after install, and the same line runs under Bash or PowerShell), and `allowed-tools` lets
 * exactly that program run in either shell. Throws when the skill no longer looks as expected, so
 * a changed skill fails the build instead of shipping a skill that still needs Node.
 */
export function nativeSkill(text, skill) {
  const call = NODE_CALL.exec(text);
  if (!call) throw new Error(`${skill}: no call to a node/bin program found`);
  const program = call[1];
  const allowed = /^allowed-tools: Bash\(node \*\)$/m;
  if (!allowed.test(text)) throw new Error(`${skill}: allowed-tools is not "Bash(node *)"`);
  let out = text
    .replace(NODE_CALL, program)
    .replace(allowed, `allowed-tools: Bash(${program} *) PowerShell(${program} *)`);
  // Re-tuning edits the repository's Node scripts; an installed copy only ever reports.
  const retune = out.indexOf("\n## Repository: re-calibrate");
  if (retune >= 0) {
    out = `${out.slice(0, retune).trimEnd()}\n`;
    out = out.replace(" In the router's own repository, `retune` re-tunes it for new Claude models.", "");
  }
  if (/\bnode\b[^\n]*\.mjs|node\/(bin|src|scripts)/.test(out)) {
    throw new Error(`${skill}: still refers to the Node implementation`);
  }
  return out;
}

/** The release version, from the root package.json (SPEC.md 2.3). */
export function releaseVersion(root = REPO_ROOT) {
  return JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;
}

/**
 * Builds the folder that the archive unpacks into. `binDir` holds the built programs; the target
 * says whether they carry `.exe`.
 */
export function stageBundle({ binDir, stageDir, target = hostTarget(), root = REPO_ROOT }) {
  const exe = target.startsWith("windows") ? ".exe" : "";
  rmSync(stageDir, { recursive: true, force: true });
  mkdirSync(join(stageDir, "bin"), { recursive: true });
  for (const program of PROGRAMS) {
    const from = join(binDir, program + exe);
    try {
      copyFileSync(from, join(stageDir, "bin", program + exe));
    } catch {
      throw new Error(`${from} is missing: build with "cargo build --release --locked" in rust/ first`);
    }
    if (!exe) chmodSync(join(stageDir, "bin", program), 0o755);
  }

  const skillsFrom = join(root, ".claude", "skills");
  for (const skill of readdirSync(skillsFrom)) {
    const to = join(stageDir, ".claude", "skills", skill);
    mkdirSync(to, { recursive: true });
    const text = readFileSync(join(skillsFrom, skill, "SKILL.md"), "utf8").replace(/\r\n/g, "\n");
    writeFileSync(join(to, "SKILL.md"), nativeSkill(text, skill));
  }

  const version = releaseVersion(root);
  writeFileSync(
    join(stageDir, "package.json"),
    `${JSON.stringify({ name: "jev-router", version, private: true }, null, 2)}\n`,
  );
  copyFileSync(join(root, ".env.example"), join(stageDir, ".env.example"));
  copyFileSync(join(root, "LICENSE"), join(stageDir, "LICENSE"));
  return { version, target };
}

/** Writes `<out>/jev-claude-<target>.tar.gz` from the staged folder; returns its path and SHA-256. */
export function archive({ stageDir, outDir, target }) {
  mkdirSync(outDir, { recursive: true });
  const name = `jev-claude-${target}.tar.gz`;
  // The archive name is relative to outDir: GNU tar reads "C:\..." as a host name.
  const result = spawnSync("tar", ["-czf", name, "-C", stageDir, "."], { cwd: outDir, encoding: "utf8" });
  if (result.status !== 0) throw new Error(`tar failed: ${result.stderr || result.error}`);
  const file = join(outDir, name);
  return { file, sha256: createHash("sha256").update(readFileSync(file)).digest("hex") };
}

function main(argv) {
  const options = { "bin-dir": join(REPO_ROOT, "rust", "target", "release"), out: join(REPO_ROOT, "dist") };
  let stageOnly = false;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--stage-only") stageOnly = true;
    else if (argv[i].startsWith("--")) options[argv[i].slice(2)] = argv[++i];
    else throw new Error(`unexpected argument ${argv[i]}`);
  }
  const target = options.target ?? hostTarget();
  const out = resolve(options.out);
  const stageDir = join(out, `jev-claude-${target}`);
  const { version } = stageBundle({ binDir: resolve(options["bin-dir"]), stageDir, target });
  console.log(`staged jev-claude ${version} for ${target} in ${stageDir}`);
  if (stageOnly) return;
  const { file, sha256 } = archive({ stageDir, outDir: out, target });
  rmSync(stageDir, { recursive: true, force: true });
  console.log(`${sha256}  ${file}`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
