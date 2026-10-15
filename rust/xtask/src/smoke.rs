//! Runs the real installers against a real release archive served from a local HTTP server, in a
//! throwaway home directory, and checks what they leave behind:
//!
//! ```text
//! cargo xtask installer-smoke-test --bin-dir target/release   # packages the build, then installs it
//! cargo xtask installer-smoke-test --archive ../dist/jev-claude-linux-x64.tar.gz
//! cargo xtask installer-smoke-test --installers <dir>         # install.sh / install.ps1 from <dir>
//! cargo xtask installer-smoke-test --only links,profiles      # just those groups of cases
//! ```
//!
//! The groups are `core`, `no-terminal`, `claude-refused`, `claude`, `claude-variants`, `update`,
//! `downloads`, `options`, `dirs`, `keys`, `odd-path` (every installer); `links`, `profiles`,
//! `platform`, `prerequisites`, `terminal`, `ps1-on-unix` (Unix); `git-bash-home`, `user-path`,
//! `locked`, `stale`, `cpus`, `winget` (Windows).
//!
//! On Windows it runs install.ps1 under Windows PowerShell and PowerShell 7, the documented
//! `irm | iex` lines (from cmd.exe, and from Git Bash as the install skill runs it), and install.sh
//! under Git Bash; elsewhere install.sh under bash, piped and not (on a Mac also under the stock
//! `/bin/bash` 3.2). Each installer must succeed, report a matching checksum, install every
//! program, leave a folder whose own jev-claude programs find their root without `JEV_ROOT`, and
//! leave no staging folder beside it. Then the cases around that:
//!
//! - the installer's options and settings (`--help`, an unknown option, the retired `--rust` and
//!   `--node`, `-y`, yes and no in any letter case), and every kind of install folder (someone
//!   else's, relative, with a trailing slash, `~`, with spaces and non-ASCII letters);
//! - the release going wrong (an unreachable host, a 404, a 500, a transfer that breaks off, a
//!   missing or odd SHA256SUMS, a wrong checksum, a pinned version, a broken archive, a short one,
//!   a new jev-check that fails to run) and the earlier install surviving it;
//! - Claude Code, which is required: missing, installed with consent (a stub of Anthropic's
//!   installer, `JEV_CLAUDE_INSTALL_BASE`), refused in any letter case, found only in `~/.local/bin`,
//!   unable to report its version, and with nobody to ask (no terminal, or inside a Claude Code
//!   session, which a pseudo-terminal proves on Unix);
//! - prerequisites (curl, git) offered through stand-ins for `apt-get`, `sudo`, `brew`, `winget`,
//!   which only record their command line, and the platform and CPU mapping through stand-ins for
//!   `uname`, `sysctl` and `ldd`;
//! - on Unix the links into `~/.local/bin` and the shell profile edits, on Windows the user PATH
//!   (in a scratch registry key), a locked install folder and the stale-folder sweep.
//!
//! The calling shell cannot change a run: every `JEV_*` variable and `CLAUDECODE` of it are
//! dropped, on Unix the installers run in a session of their own with no terminal (except where a
//! case needs one), and Claude Code is a stand-in on the PATH, since the installers require it. A
//! guard of do-nothing package managers leads every PATH, so nothing here can reach a real
//! package manager, and Anthropic's installer is never fetched. Everything happens under a
//! scratch folder: `HOME` and `USERPROFILE` point into it, and the real user PATH (the Windows
//! registry) is only touched through a scratch key, never directly.

mod cases;
mod installers;
mod platform;
mod run;
mod server;

use crate::TempDir;
use crate::package::{PROGRAMS, TARGETS, archive, host_target, release_version, sha256_file, stage_bundle};
use regex::Regex;
use run::{EXE, Installer, Run, Setup, create_dir_all, install, remove_dir_all, write_file};
use server::Server;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The checks run so far that failed, printed as they happen.
#[derive(Default)]
pub struct Checks {
    failures: Vec<String>,
}

impl Checks {
    fn check(&mut self, ok: bool, what: &str) {
        println!("{}  {what}", if ok { "  pass" } else { "  FAIL" });
        if !ok {
            self.failures.push(what.to_string());
        }
    }
}

/// One group of cases, run for an installer.
pub type Case = fn(&mut Ctx, &Installer) -> Result<(), String>;

/// What every case shares: the folders, the server, the checks so far.
pub struct Ctx<'a> {
    /// The scratch folder.
    pub work: &'a Path,
    /// The repository root.
    pub root: &'a Path,
    /// The release version in the archive under test.
    pub version: String,
    /// The archive's file name for this machine.
    pub asset: String,
    /// Everything is served from here.
    pub srv: Server,
    /// The checks so far.
    pub checks: Checks,
    /// "Claude Code ... 2.1.0" in an installer's report.
    pub claude_version: Regex,
    /// "Mode installed" in jev-check's report.
    pub mode_installed: Regex,
    /// The groups of cases to run (see `--only`); all of them when empty.
    pub only: Vec<String>,
}

impl Ctx<'_> {
    /// Is this group of cases to run?
    pub fn wants(&self, group: &str) -> bool {
        self.only.is_empty() || self.only.iter().any(|o| o == group)
    }

    /// The address of `name` on the local server.
    pub fn url(&self, name: &str) -> String {
        format!("{}/{name}", self.srv.url)
    }

    /// A fresh install of the good release.
    pub fn base(&self) -> Setup {
        Setup::new(&self.url("good"))
    }

    /// Runs the installer; the output of a run that fails a check is printed once.
    pub fn go(&self, installer: &Installer, setup: &Setup) -> Result<Run, String> {
        install(installer, self.work, setup)
    }

    /// One check, with no run to show.
    pub fn check(&mut self, ok: bool, what: &str) {
        self.checks.check(ok, what);
    }

    /// One check about a run; when it fails the run's output is printed (once).
    pub fn on(&mut self, run: &Run, ok: bool, what: &str) {
        self.checks.check(ok, what);
        if !ok && !run.shown.replace(true) {
            println!("---- output ----\n{}\n---- end of output ----", run.output.trim_end());
        }
    }

    /// The run exited 0.
    pub fn exits_0(&mut self, run: &Run, what: &str) {
        let got = run.status.map_or_else(|| "killed".to_string(), |code| code.to_string());
        self.on(run, run.status == Some(0), &format!("{what} (exit {got})"));
    }

    /// The run ended by itself with a non-zero exit code (a kill after the time limit is not that).
    pub fn exits_nonzero(&mut self, run: &Run, what: &str) {
        let got = run.status.map_or_else(|| "killed".to_string(), |code| code.to_string());
        self.on(run, matches!(run.status, Some(code) if code != 0), &format!("{what} (exit {got})"));
    }

    /// The output has `needle`.
    pub fn says(&mut self, run: &Run, needle: &str, what: &str) {
        self.on(run, run.output.contains(needle), &format!("{what} (\"{needle}\")"));
    }

    /// The output does not have `needle`.
    pub fn never_says(&mut self, run: &Run, needle: &str, what: &str) {
        self.on(run, !run.output.contains(needle), &format!("{what} (no \"{needle}\")"));
    }

    /// The output matches `pattern`.
    pub fn matches(&mut self, run: &Run, pattern: &str, what: &str) {
        let found = Regex::new(pattern).is_ok_and(|re| re.is_match(&run.output));
        self.on(run, found, &format!("{what} (/{pattern}/)"));
    }

    /// No `<dir>.new.*` or `<dir>.old.*` folder is left beside `dir`.
    pub fn no_leftovers(&mut self, run: &Run, dir: &Path, what: &str) {
        let left = leftover_siblings(dir);
        self.on(run, left.is_empty(), &format!("{what}: leaves no staging folder beside the install (found {left:?})"));
        // Cleared, so the next case reports only its own.
        if let Some(parent) = dir.parent() {
            for name in &left {
                remove_dir_all(&parent.join(name));
            }
        }
    }

    /// Is a jev-claude install at `dir`?
    pub fn installed(&self, dir: &Path) -> bool {
        PROGRAMS.iter().all(|p| dir.join("bin").join(format!("{p}{EXE}")).is_file())
            && dir.join("package.json").is_file()
    }
}

/// The staging folders an installer makes beside `dir` (`<dir>.new.<pid>`, `<dir>.old.<pid>`, with
/// `.1`, `.2` after them when a name is taken), still there.
pub fn leftover_siblings(dir: &Path) -> Vec<String> {
    let (Some(parent), Some(leaf)) = (dir.parent(), dir.file_name().and_then(|n| n.to_str())) else {
        return Vec::new();
    };
    std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&format!("{leaf}.new.")) || n.starts_with(&format!("{leaf}.old.")))
        .collect()
}

/// A folder and everything in it, copied.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    create_dir_all(to)?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("cannot read {}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = to.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)
                .map_err(|e| format!("cannot copy {} to {}: {e}", entry.path().display(), target.display()))?;
        }
    }
    Ok(())
}

/// The SHA256SUMS a release publishes: one line per archive, as `sha256sum jev-claude-*.tar.gz`
/// writes them, with this machine's last and the others' hashes made up.
fn published_sums(asset: &str, sha256: &str) -> String {
    let mut targets = TARGETS.to_vec();
    targets.sort_unstable();
    let mut text = String::new();
    for target in targets {
        let name = format!("jev-claude-{target}.tar.gz");
        if name != asset {
            let _ = writeln!(text, "{}  {name}", "ab".repeat(32));
        }
    }
    let _ = writeln!(text, "{sha256}  {asset}");
    text
}

/// Writes a release under `dir`: the archive and its SHA256SUMS at `latest/download/`.
fn write_release(dir: &Path, asset: &str, archive_file: &Path, sums: &str) -> Result<(), String> {
    let downloads = dir.join("latest").join("download");
    create_dir_all(&downloads)?;
    std::fs::copy(archive_file, downloads.join(asset)).map_err(|e| e.to_string())?;
    write_file(&downloads.join("SHA256SUMS"), sums, false)
}

/// Stages the good release's unpacked folder, changed by `edit`, as a release of its own.
fn variant(
    work: &Path,
    srv: &Path,
    unpacked: &Path,
    target: &str,
    name: &str,
    edit: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let stage = work.join(format!("stage-{name}"));
    copy_tree(unpacked, &stage)?;
    edit(&stage)?;
    let downloads = srv.join(name).join("latest").join("download");
    let (file, sha256) = archive(&stage, &downloads, target)?;
    let asset = file.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    write_file(&downloads.join("SHA256SUMS"), &published_sums(&asset, &sha256), false)
}

/// The two stubs of Anthropic's installers, one per shell, as served at `claude-good` and `claude-bad`.
fn write_claude_stubs(work: &Path, srv: &Path) -> Result<(), String> {
    // On Windows the stub installer leaves a claude.exe, a copy of this program that acts as Claude Code.
    let exe = if cfg!(windows) { run::stand_in(&work.join("claude-stand-in"), "claude")? } else { PathBuf::new() };
    let exe = exe.display().to_string().replace('\'', "''");
    let good = srv.join("claude-good");
    create_dir_all(&good)?;
    write_file(
        &good.join("install.sh"),
        "#!/bin/sh\nmkdir -p \"$HOME/.local/bin\"\nprintf '#!/bin/sh\\necho \"2.1.0 (Claude Code)\"\\n' > \"$HOME/.local/bin/claude\"\nchmod +x \"$HOME/.local/bin/claude\"\necho stub-claude-installer-ran\n",
        false,
    )?;
    write_file(
        &good.join("install.ps1"),
        &format!(
            "$d = Join-Path $env:USERPROFILE '.local\\bin'\nNew-Item -ItemType Directory -Force $d | Out-Null\nCopy-Item -LiteralPath '{exe}' -Destination (Join-Path $d 'claude.exe') -Force\n'stub-claude-installer-ran'\n"
        ),
        false,
    )?;
    // The same, leaving no claude behind and failing.
    let bad = srv.join("claude-bad");
    create_dir_all(&bad)?;
    write_file(&bad.join("install.sh"), "#!/bin/sh\necho stub-claude-installer-ran\nexit 1\n", false)?;
    write_file(&bad.join("install.ps1"), "'stub-claude-installer-ran'\nexit 1\n", false)
}

/// `cargo xtask installer-smoke-test [--bin-dir <dir> | --archive <file>] [--installers <dir>]`.
/// With neither of the first two, packages the repository's `rust/target/release`; the installers
/// are the repository's own unless `--installers` names another folder (to try a changed copy).
pub fn main(args: &[String]) -> Result<(), String> {
    let root = crate::repo_root();
    let mut bin_dir = None;
    let mut archive_file = None;
    let mut installers_dir = None;
    let mut only: Vec<String> = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().map(PathBuf::from).ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--bin-dir" => bin_dir = Some(value()?),
            "--archive" => archive_file = Some(value()?),
            "--installers" => installers_dir = Some(value()?),
            "--only" => only.extend(value()?.to_string_lossy().split(',').map(str::to_string)),
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    let target = host_target()?;
    let asset = format!("jev-claude-{target}.tar.gz");
    // An archive for another machine cannot be installed here: that is an error, not a pass.
    let archive_file = match archive_file {
        Some(file) => {
            let file = crate::absolute(&file)?;
            if file.file_name().is_none_or(|name| name.to_string_lossy() != asset) {
                return Err(format!(
                    "{} is not this machine's archive ({asset}); run the smoke test on a {target} machine",
                    file.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
            Some(file)
        }
        None => None,
    };
    let installers_dir = crate::absolute(&installers_dir.unwrap_or_else(|| root.clone()))?;
    let (sh, ps1) = (installers_dir.join("install.sh"), installers_dir.join("install.ps1"));

    let work_dir = TempDir::new("jev-installer-smoke-")?;
    let work = work_dir.path();
    let srv_dir = work.join("srv");
    let good = srv_dir.join("good");
    let downloads = good.join("latest").join("download");
    create_dir_all(&downloads)?;

    if let Some(file) = &archive_file {
        std::fs::copy(file, downloads.join(&asset)).map_err(|e| format!("cannot copy {}: {e}", file.display()))?;
    } else {
        let bin_dir = crate::absolute(&bin_dir.unwrap_or_else(|| root.join("rust").join("target").join("release")))?;
        let stage_dir = work.join("stage");
        stage_bundle(&bin_dir, &stage_dir, &target, &root)?;
        archive(&stage_dir, &downloads, &target)?;
    }
    let good_archive = downloads.join(&asset);
    let sha256 = sha256_file(&good_archive)?;
    let version = release_version(&root)?;

    // The good release also under its tag, for a pinned version; the SHA256SUMS lists every platform.
    write_file(&downloads.join("SHA256SUMS"), &published_sums(&asset, &sha256), false)?;
    let pinned = good.join("download").join(format!("v{version}"));
    create_dir_all(&pinned)?;
    std::fs::copy(&good_archive, pinned.join(&asset)).map_err(|e| e.to_string())?;
    write_file(&pinned.join("SHA256SUMS"), &published_sums(&asset, &sha256), false)?;

    // The good archive, unpacked, is what the variant releases are made from.
    let unpacked = work.join("unpacked");
    create_dir_all(&unpacked)?;
    let tar = Command::new("tar")
        .args(["-xzf", &asset, "-C"])
        .arg(&unpacked)
        .current_dir(&downloads)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("tar failed: {e}"))?;
    if !tar.status.success() {
        return Err(format!("tar failed: {}", String::from_utf8_lossy(&tar.stderr).trim_end()));
    }

    // A release whose checksum does not match.
    write_release(&srv_dir.join("bad"), &asset, &good_archive, &format!("{}  {asset}\n", "0".repeat(64)))?;
    // Only the checksums, only the archive, and checksums with no entry for this machine.
    let sums_only = srv_dir.join("sumsonly").join("latest").join("download");
    create_dir_all(&sums_only)?;
    write_file(&sums_only.join("SHA256SUMS"), &published_sums(&asset, &sha256), false)?;
    let no_sums = srv_dir.join("nosums").join("latest").join("download");
    create_dir_all(&no_sums)?;
    std::fs::copy(&good_archive, no_sums.join(&asset)).map_err(|e| e.to_string())?;
    write_release(
        &srv_dir.join("noentry"),
        &asset,
        &good_archive,
        &format!("{}  jev-claude-other.tar.gz\n", "cd".repeat(32)),
    )?;
    // CRLF line ends, an upper-case hash and the binary-mode "*" in front of the name.
    write_release(
        &srv_dir.join("crlf"),
        &asset,
        &good_archive,
        &format!("{}  jev-claude-other.tar.gz\r\n{} *{asset}\r\n", "ab".repeat(32), sha256.to_uppercase()),
    )?;
    // Bytes that are no archive at all, with a checksum that matches them.
    let garbage = srv_dir.join("garbage").join("latest").join("download");
    create_dir_all(&garbage)?;
    let junk = "not a gzip archive\n".repeat(8);
    write_file(&garbage.join(&asset), &junk, false)?;
    write_file(&garbage.join("SHA256SUMS"), &format!("{}  {asset}\n", sha256_file(&garbage.join(&asset))?), false)?;

    // Releases made from the good one.
    variant(work, &srv_dir, &unpacked, &target, "old", |stage| {
        let file = stage.join("package.json");
        let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
        let older = text.replace(&format!("\"version\": \"{version}\""), "\"version\": \"0.0.1\"");
        if older == text {
            return Err("package.json has no version to change".into());
        }
        write_file(&file, &older, false)
    })?;
    variant(work, &srv_dir, &unpacked, &target, "badcheck", |stage| {
        // A jev-check that cannot run: the new install must not replace a working one.
        run::stand_in(&stage.join("bin"), "jev-check").map(drop)
    })?;
    if cfg!(unix) {
        // A jev-check that takes its time, so an installer can be stopped while it runs.
        variant(work, &srv_dir, &unpacked, &target, "slowcheck", |stage| {
            write_file(&stage.join("bin").join("jev-check"), "#!/bin/sh\nsleep 60\n", true)
        })?;
    }
    variant(work, &srv_dir, &unpacked, &target, "short", |stage| {
        std::fs::remove_file(stage.join("bin").join(format!("jev-legend{EXE}"))).map_err(|e| e.to_string())
    })?;
    variant(work, &srv_dir, &unpacked, &target, "notrelease", |stage| {
        std::fs::remove_file(stage.join("package.json")).map_err(|e| e.to_string())
    })?;
    // The scripts, and the stubs of Anthropic's installer.
    let scripts = srv_dir.join("scripts");
    create_dir_all(&scripts)?;
    for (from, name) in [(&sh, "install.sh"), (&ps1, "install.ps1")] {
        if from.is_file() {
            std::fs::copy(from, scripts.join(name)).map_err(|e| e.to_string())?;
        }
    }
    write_claude_stubs(work, &srv_dir)?;

    let real_path_before = platform::real_user_path();
    let srv = server::serve(srv_dir)?;
    let list = installers::installers(&sh, &ps1, &format!("{}/scripts", srv.url))?;
    let mut ctx = Ctx {
        work,
        root: &root,
        version,
        asset,
        srv,
        checks: Checks::default(),
        claude_version: Regex::new(r"ok\s+Claude Code\s.*2\.1\.0").map_err(|e| e.to_string())?,
        mode_installed: Regex::new(r"Mode\s+installed").map_err(|e| e.to_string())?,
        only,
    };

    for entry in &list {
        println!("\n{}", entry.label);
        if entry.shell == run::Shell::Bash {
            // Which bash this is matters: a stock Mac has 3.2, a runner's PATH has a newer one.
            let version = Command::new(&entry.command).arg("--version").stdin(Stdio::null()).output().ok();
            let first =
                version.map(|o| String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or_default().to_string());
            println!("  ({})", first.unwrap_or_else(|| "bash version unknown".into()));
        }
        cases::core(&mut ctx, entry)?;
        if !entry.piped {
            cases::full(&mut ctx, entry)?;
        }
    }
    // The cases that are about this machine, not one installer: run with the first installer of the kind.
    if let Some(first) = list.iter().find(|e| !e.piped && e.shell == run::Shell::Bash) {
        println!("\n{} (this machine)", first.label);
        platform::on_this_machine(&mut ctx, first)?;
    }
    platform::windows_only(&mut ctx, &list)?;
    platform::other_systems(&mut ctx, &list)?;

    if real_path_before.is_some() {
        let unchanged = platform::real_user_path() == real_path_before;
        ctx.check(unchanged, "the real Windows user PATH is as it was before the run");
    }

    let failed = ctx.checks.failures.len();
    if failed == 0 {
        println!("\nall installer checks passed");
        Ok(())
    } else {
        println!("\nfailed checks:");
        for failure in &ctx.checks.failures {
            println!("  - {failure}");
        }
        Err(format!("\n{failed} check(s) failed"))
    }
}
