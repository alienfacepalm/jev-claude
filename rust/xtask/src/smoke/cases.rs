//! The cases that run for every installer: the core ones for each form of it, the full set for the
//! forms that can take options and settings.

use super::run::{
    ClaudeOnPath, EXE, Installer, Keys, PathSpec, Run, Setup, Shell, create_dir_all, steering_variables, write_file,
};
use super::{Case, Ctx};
use crate::package::{PROGRAMS, release_version};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn regex(pattern: &str) -> Result<Regex, String> {
    Regex::new(pattern).map_err(|e| e.to_string())
}

/// The shell profiles an installer may write, under a home.
pub const PROFILES: [&str; 6] =
    [".zshrc", ".bashrc", ".bash_profile", ".bash_login", ".profile", ".config/fish/config.fish"];

/// A home with the old Node edition's clone in it, as on a machine that has not removed it.
pub fn old_node_clone() -> impl Fn(&Path) -> Result<(), String> + Send + Sync + 'static {
    |home| {
        let bin = home.join("jev-claude").join("node").join("bin");
        create_dir_all(&bin)?;
        write_file(&bin.join("jev-claude.mjs"), "// the retired Node edition\n", false)
    }
}

/// A PATH that cannot reach a real package manager even when a run is told to answer yes: on
/// Windows a `winget` that only records its command line; elsewhere every PATH already starts
/// with the guard.
pub fn safe_yes(ctx: &Ctx, spec: PathSpec) -> PathSpec {
    if cfg!(windows) && spec.stubs.is_none() {
        let log = ctx.work.join("pm-guard.log");
        spec.stubbed(vec![("winget.cmd".into(), format!(">> \"{}\" echo winget %*\r\n@exit /b 0\r\n", log.display()))])
    } else {
        spec
    }
}

/// What a folder holds that a rollback must put back exactly: package.json and each program with its size.
pub fn snapshot(dir: &Path) -> String {
    let mut text =
        std::fs::read_to_string(dir.join("package.json")).unwrap_or_else(|e| format!("no package.json: {e}"));
    let mut files: Vec<String> = std::fs::read_dir(dir.join("bin"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| format!("{}:{}", e.file_name().to_string_lossy(), e.metadata().map_or(0, |m| m.len())))
        .collect();
    files.sort();
    text.push_str(&files.join("\n"));
    text
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Where a stand-in for Anthropic's installers (`install.sh` and `install.ps1`) is served.
fn claude_base(ctx: &Ctx, good: bool) -> String {
    ctx.url(if good { "claude-good" } else { "claude-bad" })
}

/// The cases every form of the installer runs: a fresh install, over the top with another
/// jev-claude on the PATH, without Claude Code, and a download that fails its checksum.
pub fn core(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    if !ctx.wants("core") {
        return Ok(());
    }
    let ignored = regex(r"\bignored\b")?;
    let windows = cfg!(windows);

    let done = ctx.go(e, &ctx.base())?;
    ctx.exits_0(&done, "exits 0");
    ctx.says(&done, "checksum matches", "reports the checksum matched");
    ctx.says(&done, "All required checks passed", "reports its dependency checks");
    let found = ctx.claude_version.is_match(&done.output);
    ctx.on(&done, found, "finds Claude Code and reports its version");
    ctx.on(&done, !ignored.is_match(&done.output), "has no legacy option to ignore");
    let mode_installed = ctx.mode_installed.clone();
    ctx.on(
        &done,
        mode_installed.is_match(&done.output),
        "runs the installed jev-check, which reports an installed copy",
    );
    for program in PROGRAMS {
        let there = done.install_dir.join("bin").join(format!("{program}{EXE}")).exists();
        ctx.on(&done, there, &format!("installs {program}"));
    }
    let skills = done.install_dir.join(".claude").join("skills").join("jev-calibrate").join("SKILL.md").exists();
    ctx.on(&done, skills, "installs the skills");
    let installed_version = release_version(&done.install_dir).unwrap_or_else(|e| e);
    let want = ctx.version.clone();
    ctx.on(&done, installed_version == want, &format!("installs release {want} (got {installed_version})"));
    ctx.no_leftovers(&done, &done.install_dir, "a fresh install");
    let untouched = if windows { "your Windows user PATH is untouched" } else { "your shell profile is untouched" };
    ctx.says(
        &done,
        &format!("JEV_NO_MODIFY_PATH=1, so {untouched}"),
        "with JEV_NO_MODIFY_PATH=1 says it left the PATH alone",
    );
    if !windows && e.shell == Shell::Bash {
        let wrote: Vec<&str> = PROFILES.iter().copied().filter(|p| done.home.join(p).exists()).collect();
        ctx.on(&done, wrote.is_empty(), &format!("and writes no shell profile (found {wrote:?})"));
    }

    // The installed programs, started on their own: no clone, no JEV_ROOT.
    let mut own = Command::new(done.install_dir.join("bin").join(format!("jev-check{EXE}")));
    for name in steering_variables() {
        own.env_remove(name);
    }
    let own = own.env("HOME", &done.home).env("USERPROFILE", &done.home).stdin(Stdio::null()).output();
    let ran = own.is_ok_and(|o| o.status.success() && mode_installed.is_match(&String::from_utf8_lossy(&o.stdout)));
    ctx.check(ran, "jev-check run by itself finds its root without JEV_ROOT");

    // Again over the top, with another jev-claude on the PATH and the old Node edition's clone in the
    // home: an update replaces the folder and says so, and warns about what the old edition left.
    let again = ctx.go(e, &ctx.base().over().path(PathSpec::shadowed()).seed(old_node_clone()))?;
    ctx.exits_0(&again, "a second run succeeds");
    ctx.says(&again, "reinstalled version", "and says it reinstalled the same version");
    ctx.says(&again, "already on your PATH", "and reports the other jev-claude on the PATH");
    ctx.says(&again, "Old Node edition", "and warns about the old Node edition");
    ctx.says(&again, "pnpm remove --global jev-router", "and says how to remove its pnpm link");
    let hint = again.output.lines().any(|l| {
        let l = l.trim_end();
        (l.contains("rm -rf") && l.ends_with("/jev-claude\""))
            || (l.contains("Remove-Item -Recurse -Force") && l.ends_with("jev-claude'"))
    });
    ctx.on(&again, hint, "and says how to remove the old clone");
    ctx.says(&again, "whichever folder comes first on your PATH wins", "and says which program wins until then");
    let kept = again.home.join("jev-claude").join("node").join("bin").join("jev-claude.mjs").exists();
    ctx.on(&again, kept, "and deletes nothing of the old edition");
    ctx.no_leftovers(&again, &again.install_dir, "an install over the top");

    // Claude Code is required: without it (and told not to install it) nothing may change.
    let bare = ctx.go(e, &ctx.base().path(PathSpec::no_claude()))?;
    ctx.exits_nonzero(&bare, "without Claude Code the install fails");
    ctx.says(&bare, "Claude Code is required", "and says Claude Code is required");
    let none = !bare.install_dir.exists();
    ctx.on(&bare, none, "and leaves nothing installed");
    ctx.no_leftovers(&bare, &bare.install_dir, "a refused Claude Code");

    // A download that does not match its published checksum must install nothing.
    let tampered = ctx.go(e, &Setup::new(&ctx.url("bad")))?;
    ctx.exits_nonzero(&tampered, "a wrong checksum fails the install");
    ctx.says(&tampered, "checksums differ", "and says why");
    let none = !tampered.install_dir.exists();
    ctx.on(&tampered, none, "and leaves nothing installed");
    ctx.no_leftovers(&tampered, &tampered.install_dir, "a wrong checksum");

    // Options that need no folder of their own.
    if e.takes_options() && e.piped {
        help_piped(ctx, e)?;
    }
    Ok(())
}

/// `bash -s -- --help`: the script comes from a pipe, so the usage cannot be read from the file.
fn help_piped(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let before = ctx.srv.count();
    let run = ctx.go(e, &ctx.base().args(&["--help"]))?;
    ctx.exits_0(&run, "piped --help exits 0");
    ctx.says(&run, "Usage: install.sh", "and prints the usage when the script comes from a pipe");
    let none = !run.install_dir.exists();
    ctx.on(&run, none, "and installs nothing");
    let quiet = ctx.srv.count() == before;
    ctx.on(&run, quiet, "and downloads nothing");
    Ok(())
}

/// The cases for an installer that is run from a file, so it can take options and settings.
pub fn full(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let groups: [(&str, Case); 10] = [
        ("no-terminal", no_terminal),
        ("claude-refused", claude_refused),
        ("claude", claude_cases),
        ("claude-variants", claude_variants),
        ("update", update_and_breakage),
        ("downloads", downloads),
        ("options", options),
        ("dirs", install_dirs),
        ("keys", keys),
        ("odd-path", odd_path),
    ];
    for (name, case) in groups {
        if ctx.wants(name) {
            case(ctx, e)?;
        }
    }
    Ok(())
}

/// Nobody to ask whether to install Claude Code: no terminal, so it says what to run, and stops.
fn no_terminal(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let run = ctx.go(e, &ctx.base().path(PathSpec::no_claude()).unset("JEV_INSTALL_CLAUDE"))?;
    ctx.exits_nonzero(&run, "with no terminal to ask on and no Claude Code the install fails");
    ctx.says(&run, "There is no terminal to ask on", "and says there is no terminal to ask on");
    ctx.never_says(&run, "Run it now?", "and does not wait on a question");
    let quick = run.elapsed.as_secs() < 60;
    ctx.on(&run, quick, "and ends at once instead of waiting for an answer");
    let none = !run.install_dir.exists();
    ctx.on(&run, none, "and leaves nothing installed");
    ctx.no_leftovers(&run, &run.install_dir, "no terminal");
    Ok(())
}

/// `JEV_INSTALL_CLAUDE=No`, in any letter case, means no.
fn claude_refused(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let run = ctx.go(e, &ctx.base().path(PathSpec::no_claude()).env("JEV_INSTALL_CLAUDE", "No"))?;
    ctx.exits_nonzero(&run, "JEV_INSTALL_CLAUDE=No fails the install");
    ctx.says(&run, "says not to install it", "and says the setting forbids installing Claude Code");
    let none = !run.install_dir.exists();
    ctx.on(&run, none, "and leaves nothing installed");
    Ok(())
}

/// What a run that installed Claude Code with consent must show.
fn consent_checks(ctx: &mut Ctx, e: &Installer, run: &Run, how: &str) {
    ctx.exits_0(run, &format!("{how}: exits 0"));
    ctx.says(run, "Running:", &format!("{how}: shows the command it runs"));
    ctx.says(run, "(output below)", &format!("{how}: marks where Anthropic's installer output starts"));
    ctx.says(run, "stub-claude-installer-ran", &format!("{how}: runs Anthropic's installer"));
    if cfg!(windows) {
        ctx.matches(run, r"ok\s+Claude Code", &format!("{how}: finds the Claude Code it just installed"));
    } else {
        let found = ctx.claude_version.is_match(&run.output);
        ctx.on(run, found, &format!("{how}: finds the Claude Code it just installed and reports 2.1.0"));
    }
    ctx.says(run, "Open a new terminal first", &format!("{how}: says to open a new terminal"));
    let programs = ctx.installed(&run.install_dir);
    ctx.on(run, programs, &format!("{how}: installs jev-claude"));
    let claude = run.home.join(".local").join("bin").join(format!("claude{EXE}")).is_file();
    ctx.on(run, claude, &format!("{how}: Claude Code is in ~/.local/bin"));
    if e.shell == Shell::PowerShell {
        ctx.says(
            run,
            "Claude Code's folder, which its installer leaves off the PATH",
            &format!("{how}: names the Claude Code folder for the PATH"),
        );
    }
}

/// Claude Code installed with consent, from a stand-in for Anthropic's installer, never the real one.
fn claude_cases(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    if !e.mentions("JEV_CLAUDE_INSTALL_BASE") {
        ctx.check(false, "the installer takes JEV_CLAUDE_INSTALL_BASE, the test hook that points Anthropic's installer at a stand-in");
        return Ok(());
    }
    let good = claude_base(ctx, true);
    let consenting =
        |ctx: &Ctx| ctx.base().path(safe_yes(ctx, PathSpec::no_claude())).env("JEV_CLAUDE_INSTALL_BASE", good.clone());

    let run = ctx.go(e, &consenting(ctx).env("JEV_INSTALL_CLAUDE", "1"))?;
    consent_checks(ctx, e, &run, "JEV_INSTALL_CLAUDE=1");
    let run = ctx.go(e, &consenting(ctx).unset("JEV_INSTALL_CLAUDE").env("JEV_ASSUME_YES", "1"))?;
    consent_checks(ctx, e, &run, "JEV_ASSUME_YES=1");
    if e.takes_options() {
        let option = e.pick("-y", "-Yes");
        let run = ctx.go(e, &consenting(ctx).unset("JEV_INSTALL_CLAUDE").args(&[option]))?;
        consent_checks(ctx, e, &run, option);
    }

    // Anthropic's installer fails and leaves no claude behind: the install stops, changing nothing.
    let bad = claude_base(ctx, false);
    let run = ctx.go(
        e,
        &ctx.base()
            .path(safe_yes(ctx, PathSpec::no_claude()))
            .env("JEV_INSTALL_CLAUDE", "1")
            .env("JEV_CLAUDE_INSTALL_BASE", bad),
    )?;
    ctx.exits_nonzero(&run, "a failed Claude Code install fails the install");
    ctx.says(
        &run,
        "Anthropic's installer did not leave a claude program",
        "and says Anthropic's installer left nothing",
    );
    ctx.says(&run, "Claude Code could not be installed", "and says Claude Code could not be installed");
    let none = !run.install_dir.exists();
    ctx.on(&run, none, "and leaves nothing installed");
    ctx.no_leftovers(&run, &run.install_dir, "a failed Claude Code install");
    let links: Vec<String> = std::fs::read_dir(run.home.join(".local").join("bin"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|f| f.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("jev-"))
        .collect();
    ctx.on(&run, links.is_empty(), &format!("and links nothing into ~/.local/bin (found {links:?})"));
    let profiles: Vec<&str> = PROFILES.iter().copied().filter(|p| run.home.join(p).exists()).collect();
    ctx.on(&run, profiles.is_empty(), &format!("and writes no shell profile (found {profiles:?})"));
    Ok(())
}

/// Claude Code that is only in ~/.local/bin, one that does not run, and the npm package.
fn claude_variants(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    // Anthropic's installer puts claude in ~/.local/bin, which a shell that was not restarted may not list.
    let local_bin = |home: &Path| -> Result<(), String> {
        let bin = home.join(".local").join("bin");
        create_dir_all(&bin)?;
        if cfg!(windows) {
            super::run::stand_in(&bin, "claude").map(drop)
        } else {
            write_file(&bin.join("claude"), "#!/bin/sh\necho '2.1.0 (Claude Code)'\n", true)
        }
    };
    let run = ctx.go(e, &ctx.base().path(PathSpec::no_claude()).seed(local_bin))?;
    ctx.exits_0(&run, "Claude Code found only in ~/.local/bin: exits 0");
    let found = ctx.claude_version.is_match(&run.output);
    ctx.on(&run, found, "and finds it and reports its version");
    ctx.says(&run, "native build", "and reports the native build");
    let programs = ctx.installed(&run.install_dir);
    ctx.on(&run, programs, "and installs jev-claude");
    if e.shell == Shell::PowerShell {
        ctx.says(&run, "which is not on your user PATH yet", "and says its folder is not on the Windows user PATH");
    }

    // A claude that cannot report its version does not count: it would fail the moment jev-claude starts it.
    let run = ctx.go(e, &ctx.base().path(PathSpec::clean().with_claude(ClaudeOnPath::Broken)))?;
    ctx.exits_nonzero(&run, "a claude that cannot report its version fails the install");
    ctx.matches(&run, r"FAIL\s+Claude Code\s+.*does not run", "and reports it as a failed check");
    let none = !run.install_dir.exists();
    ctx.on(&run, none, "and leaves nothing installed");
    ctx.no_leftovers(&run, &run.install_dir, "a claude that does not run");

    // The npm package of Claude Code is a script run by Node.js.
    if cfg!(windows) && e.shell == Shell::PowerShell {
        let run = ctx.go(e, &ctx.base().path(PathSpec::clean().with_claude(ClaudeOnPath::Npm)))?;
        ctx.exits_0(&run, "the npm package of Claude Code: exits 0");
        ctx.says(&run, "npm package", "and is reported as the npm package");
    }
    Ok(())
}

/// An update, and a release that cannot be installed: the earlier install must survive every one.
fn update_and_breakage(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    // 0.0.1 first, then the real one over it.
    let old = ctx.go(e, &Setup::new(&ctx.url("old")))?;
    ctx.exits_0(&old, "an older release installs");
    let up = ctx.go(e, &ctx.base().over())?;
    ctx.exits_0(&up, "and the release over it succeeds");
    let version = ctx.version.clone();
    ctx.says(&up, &format!("updated 0.0.1 -> {version}"), "and says which versions it went between");
    ctx.no_leftovers(&up, &up.install_dir, "an update");

    // From here a good install is in place, and each broken release must leave it exactly as it is.
    let before = snapshot(&up.install_dir);
    let cases: [(&str, &str, &[&str]); 4] = [
        (
            "badcheck",
            "a new jev-check that cannot run",
            &["jev-check failed to run", "The earlier install was put back."],
        ),
        ("short", "an archive missing a program", &["jev-legend is missing from the release archive"]),
        ("notrelease", "an archive that is no release", &["is not a jev-claude release"]),
        ("garbage", "bytes that are no archive", &["Could not unpack the archive."]),
    ];
    for (name, what, needles) in cases {
        let run = ctx.go(e, &Setup::new(&ctx.url(name)).over())?;
        ctx.exits_nonzero(&run, &format!("{what} fails the update"));
        for needle in needles {
            ctx.says(&run, needle, &format!("{what}: says why"));
        }
        let same = snapshot(&up.install_dir) == before;
        ctx.on(&run, same, &format!("{what}: keeps the earlier install exactly as it was"));
        ctx.no_leftovers(&run, &up.install_dir, what);
    }
    // Stopped half way (Ctrl-C, a closed terminal): the earlier install comes back, nothing is left beside it.
    if cfg!(unix) && e.shell == Shell::Bash {
        let run = ctx.go(e, &Setup::new(&ctx.url("slowcheck")).over().interrupted_at("Step 6 of 6"))?;
        ctx.exits_nonzero(&run, "an installer stopped while it checks the new install ends");
        ctx.says(&run, "The earlier install was put back.", "and puts the earlier install back");
        let same = snapshot(&up.install_dir) == before;
        ctx.on(&run, same, "and the earlier install is exactly as it was");
        ctx.no_leftovers(&run, &up.install_dir, "a stopped update");
    }
    // And with nothing installed yet, a broken archive leaves nothing.
    let run = ctx.go(e, &Setup::new(&ctx.url("garbage")))?;
    ctx.exits_nonzero(&run, "bytes that are no archive fail a first install");
    let none = !run.install_dir.exists();
    ctx.on(&run, none, "and leave nothing installed");
    ctx.no_leftovers(&run, &run.install_dir, "a broken first install");
    Ok(())
}

/// The release going wrong at the download and checksum steps, and the ways of pointing at one.
fn downloads(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let nothing = |ctx: &mut Ctx, run: &Run| {
        let none = !run.install_dir.exists();
        ctx.on(run, none, "and leaves nothing installed");
        ctx.no_leftovers(run, &run.install_dir, "a failed download");
    };

    let run = ctx.go(e, &Setup::new(&super::server::closed_port_url()?))?;
    ctx.exits_nonzero(&run, "an unreachable host fails the install");
    ctx.says(&run, "cannot reach", "and says the network cannot be reached");
    nothing(ctx, &run);

    let run = ctx.go(e, &Setup::new(&ctx.url("sumsonly")))?;
    ctx.exits_nonzero(&run, "a release with no archive fails the install");
    ctx.says(&run, "HTTP 404", "and says the address answered 404");
    ctx.says(&run, "check that the tag exists", "and tells how to check the pinned tag");
    nothing(ctx, &run);

    let run = ctx.go(e, &ctx.base().env("JEV_VERSION", "v9.9.9"))?;
    ctx.exits_nonzero(&run, "a pinned version that does not exist fails the install");
    ctx.says(&run, "HTTP 404", "and says the address answered 404");
    nothing(ctx, &run);

    let tag = format!("v{}", ctx.version);
    let run = ctx.go(e, &ctx.base().env("JEV_VERSION", tag.as_str()))?;
    ctx.exits_0(&run, "a pinned version that exists installs");
    ctx.says(&run, &format!("downloading release {tag}"), "and says it is downloading that release");
    ctx.says(&run, "checksum matches", "and verifies it");
    let installed = release_version(&run.install_dir).unwrap_or_else(|e| e);
    let want = ctx.version.clone();
    ctx.on(&run, installed == want, &format!("and installs {want} (got {installed})"));

    let run = ctx.go(e, &Setup::new(&ctx.url("status500")))?;
    ctx.exits_nonzero(&run, "a server that answers 500 fails the install");
    ctx.says(&run, "HTTP 500", "and says the download failed with HTTP 500");
    nothing(ctx, &run);

    let run = ctx.go(e, &Setup::new(&ctx.url("truncated")))?;
    ctx.exits_nonzero(&run, "a transfer that breaks off fails the install");
    if e.shell == Shell::Bash {
        ctx.says(&run, "interrupted", "and says the download was interrupted");
    }
    nothing(ctx, &run);

    let run = ctx.go(e, &Setup::new(&ctx.url("nosums")))?;
    ctx.exits_nonzero(&run, "a release with no SHA256SUMS fails the install");
    ctx.says(&run, "Could not fetch the release's SHA256SUMS", "and says the checksums could not be fetched");
    nothing(ctx, &run);

    let run = ctx.go(e, &Setup::new(&ctx.url("noentry")))?;
    ctx.exits_nonzero(&run, "a SHA256SUMS with no entry for this archive fails the install");
    ctx.says(&run, "has no entry for", "and says there is no entry");
    nothing(ctx, &run);

    let run = ctx.go(e, &Setup::new(&ctx.url("crlf")))?;
    ctx.exits_0(&run, "a SHA256SUMS with CRLF line ends, an upper-case hash and a * before the name installs");
    ctx.says(&run, "checksum matches", "and verifies it");

    let run = ctx.go(e, &Setup::new(&format!("{}/", ctx.url("good"))))?;
    ctx.exits_0(&run, "a release address with a trailing slash installs");

    // A mirror's password is never printed.
    let secret = ctx.srv.url.replacen("http://", "http://user:s3cret@", 1);
    let run = ctx.go(e, &Setup::new(&format!("{secret}/good")).env("JEV_VERSION", "v9.9.9"))?;
    ctx.exits_nonzero(&run, "a release address with a password, pinned to a missing version, fails");
    ctx.never_says(&run, "s3cret", "and never prints the password");
    Ok(())
}

/// Options and yes/no settings.
fn options(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let ignored = regex(r"\bignored\b")?;
    let help = e.pick("--help", "-Help");
    let before = ctx.srv.count();
    let run = ctx.go(e, &ctx.base().args(&[help]))?;
    ctx.exits_0(&run, &format!("{help} exits 0"));
    ctx.says(&run, "JEV_INSTALL_DIR", "and prints the settings");
    let none = !run.install_dir.exists();
    ctx.on(&run, none, "and installs nothing");
    let quiet = ctx.srv.count() == before;
    ctx.on(&run, quiet, "and downloads nothing");

    let bogus = e.pick("--bogus", "-Bogus");
    let run = ctx.go(e, &ctx.base().args(&[bogus]))?;
    ctx.exits_nonzero(&run, &format!("{bogus} fails"));
    ctx.says(&run, "Unknown option", "and says the option is unknown");
    let none = !run.install_dir.exists();
    ctx.on(&run, none, "and installs nothing");

    // The retired options and JEV_IMPL are ignored with a note; yes and no are read in any letter case.
    let legacy = [e.pick("--rust", "-Rust"), e.pick("--node", "-Node")];
    let run = ctx.go(
        e,
        &ctx.base()
            .args(&legacy)
            .env("JEV_IMPL", "node")
            .env("JEV_ASSUME_YES", "Yes")
            .env("JEV_NO_MODIFY_PATH", "TRUE")
            .path(safe_yes(ctx, PathSpec::clean())),
    )?;
    ctx.exits_0(&run, "the retired options and JEV_IMPL do not stop the install");
    ctx.on(&run, ignored.is_match(&run.output), "and are reported as ignored");
    ctx.says(&run, "has been retired", "and the Node edition is reported as retired");
    ctx.says(&run, "untouched", "and JEV_NO_MODIFY_PATH=TRUE counts as yes");

    let yes = e.pick("-y", "-Yes");
    let run = ctx.go(e, &ctx.base().args(&[yes]).path(safe_yes(ctx, PathSpec::clean())))?;
    ctx.exits_0(&run, &format!("{yes} installs"));
    ctx.says(&run, "checksum matches", "and verifies the download");
    Ok(())
}

/// Every kind of install folder: someone else's, a file, empty, trailing slash, relative, `~`.
fn install_dirs(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let work = ctx.work.to_path_buf();
    let programs_at = |dir: &Path| dir.join("bin").join(format!("jev-claude{EXE}")).is_file();

    // Someone else's folder is never touched.
    let notmine = work.join("notmine");
    create_dir_all(&notmine)?;
    write_file(&notmine.join("keep.txt"), "mine", false)?;
    let run = ctx.go(e, &ctx.base().over().dir(notmine.as_os_str()))?;
    ctx.exits_nonzero(&run, "a folder that is not a jev-claude install fails the install");
    ctx.says(&run, "exists but is not a jev-claude install", "and says so");
    let intact = read(&notmine.join("keep.txt")) == "mine" && !notmine.join("bin").exists();
    ctx.on(&run, intact, "and leaves the folder as it was");
    ctx.no_leftovers(&run, &notmine, "a folder that is not an install");
    if e.shell == Shell::PowerShell {
        ctx.matches(&run, r"FAIL\s+Install folder", "and reports the install folder as a failed check");
    }

    let in_the_way = work.join("notmine-file");
    write_file(&in_the_way, "x", false)?;
    let run = ctx.go(e, &ctx.base().over().dir(in_the_way.as_os_str()))?;
    ctx.exits_nonzero(&run, "a file where the install folder should be fails the install");
    ctx.says(&run, "exists but is not a jev-claude install", "and says so");
    let intact = read(&in_the_way) == "x";
    ctx.on(&run, intact, "and leaves the file as it was");

    // An empty folder is fine.
    let empty = work.join("empty-install");
    create_dir_all(&empty)?;
    let run = ctx.go(e, &ctx.base().over().dir(empty.as_os_str()))?;
    ctx.exits_0(&run, "an empty existing folder is installed into");
    let there = programs_at(&empty);
    ctx.on(&run, there, "and holds the programs");

    // A trailing separator is dropped before the staging folders are named.
    let slash = work.join("slash-install");
    let run = ctx.go(e, &ctx.base().over().dir(format!("{}/", slash.display())))?;
    ctx.exits_0(&run, "an install folder with a trailing slash is installed into");
    let there = programs_at(&slash);
    ctx.on(&run, there, "and holds the programs");
    ctx.no_leftovers(&run, &slash, "a trailing slash");

    // A relative folder means the folder the installer is run from.
    let run = ctx.go(e, &ctx.base().over().dir("rel-install"))?;
    ctx.exits_0(&run, "a relative install folder is installed into");
    let there = programs_at(&work.join("rel-install"));
    ctx.on(&run, there, "and means the folder it was run from");

    // ~ means the home folder (a shell does not expand it inside an environment variable).
    let run = ctx.go(e, &ctx.base().over().dir("~/tilde-install"))?;
    ctx.exits_0(&run, "~/dir as the install folder is installed into");
    let there = programs_at(&run.home.join("tilde-install"));
    ctx.on(&run, there, "and means the home folder");
    let stray = work.join("~").exists();
    ctx.on(&run, !stray, "and makes no folder named ~");

    for name in ["notmine", "notmine-file", "empty-install", "slash-install", "rel-install"] {
        let path = work.join(name);
        if path.is_dir() {
            super::run::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
}

/// The settings file: created from .env.example, keys saved over their placeholders, nothing echoed.
fn keys(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let spec = Keys::Env { jev: Some("k-smoke-jev-1".into()), anthropic: Some("k-smoke-anth-2".into()) };
    let mut setup = ctx.base();
    setup.keys = spec;
    let run = ctx.go(e, &setup)?;
    let file = run.home.join(".jev-router.env");
    let text = read(&file);
    let lines: Vec<&str> = text.lines().collect();
    ctx.exits_0(&run, "with no settings file and keys in the environment, the install succeeds");
    let from_example = lines.iter().any(|l| l.contains("TYPESAFE_API_KEY also works"));
    ctx.on(&run, from_example, "the settings file is made from .env.example");
    let one = lines.iter().filter(|l| **l == "JEV_API_KEY=k-smoke-jev-1").count() == 1
        && !lines.iter().any(|l| *l == "JEV_API_KEY=" || l.starts_with("# JEV_API_KEY="));
    ctx.on(&run, one, "the Jev key replaces its placeholder, once");
    let one = lines.iter().filter(|l| **l == "ANTHROPIC_API_KEY=k-smoke-anth-2").count() == 1
        && !lines.iter().any(|l| l.starts_with("# ANTHROPIC_API_KEY="));
    ctx.on(&run, one, "the Anthropic key replaces its commented placeholder, once");
    ctx.says(&run, "Saved your Jev key", "the install says the Jev key was saved");
    ctx.says(&run, "Saved your Anthropic API key", "and the Anthropic key");
    ctx.never_says(&run, "k-smoke-jev-1", "and never prints the Jev key");
    ctx.never_says(&run, "k-smoke-anth-2", "and never prints the Anthropic key");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).map_or(0, |m| m.permissions().mode() & 0o777);
        ctx.on(&run, mode == 0o600, &format!("the settings file is readable only by its owner (mode {mode:o})"));
    }
    if cfg!(windows) {
        let bytes = std::fs::read(&file).unwrap_or_default();
        ctx.on(
            &run,
            !bytes.starts_with(&[0xEF, 0xBB, 0xBF]) && String::from_utf8(bytes).is_ok(),
            "the settings file is UTF-8 without a byte-order mark",
        );
    }

    let mut setup = ctx.base();
    setup.keys = Keys::Env { jev: None, anthropic: None };
    let run = ctx.go(e, &setup)?;
    ctx.exits_0(&run, "with no key at all the install still succeeds");
    ctx.says(&run, "Add your Jev key later", "and says to add the key later");
    let text = read(&run.home.join(".jev-router.env"));
    let none = !text.lines().any(|l| l.starts_with("JEV_API_KEY=") && l.len() > "JEV_API_KEY=".len());
    ctx.on(&run, none, "and saves no key");

    // A settings file that is a link into a dotfiles folder stays a link.
    #[cfg(unix)]
    if e.shell == Shell::Bash {
        let dotfiles = ctx.work.join("dotfiles");
        super::run::remove_dir_all(&dotfiles);
        create_dir_all(&dotfiles)?;
        let example = ctx.root.join(".env.example");
        let target = dotfiles.join("env");
        let mut setup = ctx.base();
        setup.keys = Keys::Env { jev: Some("k-smoke-jev-3".into()), anthropic: None };
        let setup = setup.seed(move |home| {
            std::fs::copy(&example, &target).map_err(|e| format!("cannot copy .env.example: {e}"))?;
            std::os::unix::fs::symlink(&target, home.join(".jev-router.env")).map_err(|e| e.to_string())
        });
        let run = ctx.go(e, &setup)?;
        ctx.exits_0(&run, "a settings file that is a link: the install succeeds");
        let link =
            std::fs::symlink_metadata(run.home.join(".jev-router.env")).is_ok_and(|m| m.file_type().is_symlink());
        ctx.on(&run, link, "and the settings file is still a link");
        let kept = read(&dotfiles.join("env")).lines().filter(|l| *l == "JEV_API_KEY=k-smoke-jev-3").count() == 1;
        ctx.on(&run, kept, "and the key was written through it");
    }
    Ok(())
}

/// A home and install folder whose names have a space and non-ASCII letters.
fn odd_path(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let odd: PathBuf = ctx.work.join("Jev Test \u{dc}n\u{ef} dir");
    let mut setup = ctx.base();
    setup.place = Some(odd.clone());
    create_dir_all(&odd)?;
    let run = ctx.go(e, &setup)?;
    ctx.exits_0(&run, "a folder with a space and non-ASCII letters: the install succeeds");
    let programs = ctx.installed(&run.install_dir);
    ctx.on(&run, programs, "and holds every program");
    let installed = release_version(&run.install_dir).unwrap_or_else(|e| e);
    let want = ctx.version.clone();
    ctx.on(&run, installed == want, &format!("and the release {want} (got {installed})"));
    ctx.no_leftovers(&run, &run.install_dir, "a folder with spaces");
    let mut own = Command::new(run.install_dir.join("bin").join(format!("jev-check{EXE}")));
    for name in steering_variables() {
        own.env_remove(name);
    }
    let own = own.env("HOME", &run.home).env("USERPROFILE", &run.home).stdin(Stdio::null()).output();
    let mode_installed = ctx.mode_installed.clone();
    let ran = own.is_ok_and(|o| o.status.success() && mode_installed.is_match(&String::from_utf8_lossy(&o.stdout)));
    ctx.on(&run, ran, "and its jev-check finds its root there");
    super::run::remove_dir_all(&odd);
    Ok(())
}
