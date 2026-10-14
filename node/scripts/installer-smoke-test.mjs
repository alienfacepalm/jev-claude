#!/usr/bin/env node
// Runs the real installers for the Rust edition against a real release archive served from a local
// HTTP server, in a throwaway home directory, and checks what they leave behind:
//
//   node node/scripts/installer-smoke-test.mjs --bin-dir rust/target/release   # packages the build, then installs it
//   node node/scripts/installer-smoke-test.mjs --archive dist/jev-claude-linux-x64.tar.gz
//
// On Windows it runs install.ps1 under Windows PowerShell and PowerShell 7 (when installed) and
// install.sh under Git Bash; elsewhere install.sh under bash. Each installer must succeed, report
// a matching checksum, install every program, and leave a folder whose own jev-claude programs find
// their root without JEV_ROOT. A release with a wrong checksum must install nothing. The PATH is
// never touched (JEV_NO_MODIFY_PATH=1), and no Claude Code installer is run.
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { archive, hostTarget, releaseVersion, stageBundle } from "./package-rust-release.mjs";

const REPO_ROOT = resolve(fileURLToPath(import.meta.url), "..", "..", "..");
const IS_WINDOWS = process.platform === "win32";
const EXE = IS_WINDOWS ? ".exe" : "";

const failures = [];
function check(ok, what) {
  console.log(`${ok ? "  pass" : "  FAIL"}  ${what}`);
  if (!ok) failures.push(what);
}

/** A local stand-in for GitHub's release downloads: serves `root/<path>` and logs nothing. */
function serve(root) {
  const server = createServer((request, response) => {
    const file = join(root, new URL(request.url, "http://x").pathname);
    if (!file.startsWith(root) || !existsSync(file) || !statSync(file).isFile()) {
      response.writeHead(404).end("not found");
      return;
    }
    const body = readFileSync(file);
    response.writeHead(200, { "content-length": body.length });
    response.end(request.method === "HEAD" ? undefined : body);
  });
  return new Promise((done) => server.listen(0, "127.0.0.1", () => done(server)));
}

/**
 * The installers to run on this machine: [label, command, args, extra env, file piped to stdin].
 * The last form of each is the one the README tells people to type, `curl ... | bash` and
 * `irm ... | iex`: the script arrives on stdin or as a string, so no child it starts may read stdin and
 * swallow the rest of it, and there is no script path to find the clone by.
 */
function installers() {
  const list = [];
  if (IS_WINDOWS) {
    const ps1 = join(REPO_ROOT, "install.ps1");
    const run = ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", ps1];
    // Windows PowerShell 5.1 started from PowerShell 7 would inherit its module path and lose its own cmdlets.
    list.push(["install.ps1 (Windows PowerShell)", "powershell.exe", run, { PSModulePath: null }]);
    const piped = ["-NoProfile", "-Command", `Get-Content -Raw '${ps1}' | Invoke-Expression`];
    list.push(["install.ps1 piped to iex (Windows PowerShell)", "powershell.exe", piped, { PSModulePath: null }]);
    if (spawnSync("pwsh", ["-NoProfile", "-Command", "exit 0"]).status === 0) {
      list.push(["install.ps1 (PowerShell 7)", "pwsh", run, {}]);
      list.push(["install.ps1 piped to iex (PowerShell 7)", "pwsh", piped, {}]);
    }
    const gitBash = "C:\\Program Files\\Git\\bin\\bash.exe";
    if (existsSync(gitBash))
      list.push(["install.sh (Git Bash)", gitBash, [join(REPO_ROOT, "install.sh"), "--rust"], {}]);
    if (existsSync(gitBash))
      list.push([
        "install.sh piped to bash (Git Bash)",
        gitBash,
        ["-s", "--", "--rust"],
        {},
        join(REPO_ROOT, "install.sh"),
      ]);
  } else {
    list.push(["install.sh (bash)", "bash", [join(REPO_ROOT, "install.sh"), "--rust"], {}]);
    list.push(["install.sh piped to bash", "bash", ["-s", "--", "--rust"], {}, join(REPO_ROOT, "install.sh")]);
  }
  return list;
}

// Asynchronous: the release server lives in this process and must keep answering while the installer runs.
function install([label, command, args, extra, stdinFile], { releaseUrl, work, fresh = true }) {
  const home = join(work, "home");
  const installDir = join(work, "install");
  if (fresh) rmSync(installDir, { recursive: true, force: true });
  rmSync(home, { recursive: true, force: true });
  mkdirSync(home, { recursive: true });
  // Both keys present, so the installer's key prompts have nothing to ask.
  writeFileSync(join(home, ".jev-router.env"), "JEV_API_KEY=smoke-test\nANTHROPIC_API_KEY=smoke-test\n", {
    mode: 0o600,
  });
  const env = {
    ...process.env,
    ...extra,
    HOME: home,
    USERPROFILE: home,
    JEV_IMPL: "rust",
    JEV_RELEASE_URL: releaseUrl,
    JEV_INSTALL_DIR: installDir,
    JEV_NO_MODIFY_PATH: "1",
    JEV_INSTALL_CLAUDE: "0",
  };
  delete env.JEV_ROOT;
  for (const [name, value] of Object.entries(extra)) if (value === null) delete env[name];
  return new Promise((done) => {
    const child = spawn(command, args, { env, cwd: work, stdio: [stdinFile ? "pipe" : "ignore", "pipe", "pipe"] });
    if (stdinFile) child.stdin.end(readFileSync(stdinFile));
    let output = "";
    child.stdout.on("data", (chunk) => {
      output += chunk;
    });
    child.stderr.on("data", (chunk) => {
      output += chunk;
    });
    const timer = setTimeout(() => child.kill(), 240_000);
    child.on("close", (status) => {
      clearTimeout(timer);
      done({ label, installDir, home, status, output });
    });
  });
}

async function main() {
  const options = {};
  for (let i = 2; i < process.argv.length; i += 2) options[process.argv[i].replace(/^--/, "")] = process.argv[i + 1];
  const work = mkdtempSync(join(tmpdir(), "jev-installer-smoke-"));
  const target = hostTarget();
  const asset = `jev-claude-${target}.tar.gz`;
  const good = join(work, "release-good");
  const downloads = join(good, "latest", "download");
  mkdirSync(downloads, { recursive: true });

  if (options.archive) {
    if (basename(options.archive) !== asset) {
      console.log(`skipped: ${basename(options.archive)} is not this machine's archive (${asset})`);
      return;
    }
    copyFileSync(options.archive, join(downloads, asset));
  } else {
    const stageDir = join(work, "stage");
    stageBundle({
      binDir: resolve(options["bin-dir"] ?? join(REPO_ROOT, "rust", "target", "release")),
      stageDir,
      target,
    });
    archive({ stageDir, outDir: downloads, target });
  }
  const sha256 = createHash("sha256")
    .update(readFileSync(join(downloads, asset)))
    .digest("hex");
  writeFileSync(join(downloads, "SHA256SUMS"), `${sha256}  ${asset}\n`);

  const bad = join(work, "release-bad", "latest", "download");
  mkdirSync(bad, { recursive: true });
  copyFileSync(join(downloads, asset), join(bad, asset));
  writeFileSync(join(bad, "SHA256SUMS"), `${"0".repeat(64)}  ${asset}\n`);

  const goodServer = await serve(good);
  const badServer = await serve(join(work, "release-bad"));
  const url = (server) => `http://127.0.0.1:${server.address().port}`;
  const version = releaseVersion();

  try {
    for (const entry of installers()) {
      console.log(`\n${entry[0]}`);
      const done = await install(entry, { releaseUrl: url(goodServer), work });
      check(done.status === 0, `exits 0 (got ${done.status})`);
      if (done.status !== 0) console.log(done.output);
      check(done.output.includes("checksum matches"), "reports the checksum matched");
      check(done.output.includes("All required checks passed"), "reports its dependency checks");
      check(/Mode\s+installed/.test(done.output), "runs the installed jev-check, which reports an installed copy");
      for (const program of [
        "jev-claude",
        "jev-statusline",
        "jev-explain",
        "jev-legend",
        "jev-check",
        "jev-update-check",
        "jev-proxy-host",
      ]) {
        check(existsSync(join(done.installDir, "bin", program + EXE)), `installs ${program}`);
      }
      check(existsSync(join(done.installDir, ".claude", "skills", "jev-calibrate", "SKILL.md")), "installs the skills");
      const installedVersion = JSON.parse(readFileSync(join(done.installDir, "package.json"), "utf8")).version;
      check(installedVersion === version, `installs release ${version} (got ${installedVersion})`);

      // The installed programs, started on their own: no clone, no JEV_ROOT.
      const env = { ...process.env, HOME: done.home, USERPROFILE: done.home };
      delete env.JEV_ROOT;
      const own = spawnSync(join(done.installDir, "bin", `jev-check${EXE}`), { env, encoding: "utf8" });
      check(
        own.status === 0 && /Mode\s+installed/.test(own.stdout),
        "jev-check run by itself finds its root without JEV_ROOT",
      );

      // Again over the top: an update replaces the folder and says so.
      const again = await install(entry, { releaseUrl: url(goodServer), work, fresh: false });
      check(again.status === 0, "a second run succeeds");
      check(/reinstalled version/.test(again.output), "and says it reinstalled the same version");

      // A download that does not match its published checksum must install nothing.
      const tampered = await install(entry, { releaseUrl: url(badServer), work });
      check(tampered.status !== 0, "a wrong checksum fails the install");
      check(/checksums differ/.test(tampered.output), "and says why");
      check(!existsSync(tampered.installDir), "and leaves nothing installed");
    }
  } finally {
    goodServer.close();
    badServer.close();
    rmSync(work, { recursive: true, force: true });
  }

  console.log(failures.length ? `\n${failures.length} check(s) failed` : "\nall installer checks passed");
  process.exit(failures.length ? 1 : 0);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
