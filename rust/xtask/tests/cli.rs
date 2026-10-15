//! The `cargo xtask` command line as CI and people use it: what `bump-version` prints (the
//! release job reads the version from standard output and nothing else), the arguments each
//! command refuses, and that an installer smoke test of another machine's archive is an error and
//! not a pass.

use std::path::Path;
use std::process::{Command, Output, Stdio};
use xtask::TempDir;

/// `xtask <args>` run in `cwd`.
fn xtask(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask")).args(args).current_dir(cwd).stdin(Stdio::null()).output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn version_of(text: &str) -> (u64, u64, u64) {
    let mut parts = text.split('.').map(|p| p.trim().parse::<u64>().unwrap());
    (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap())
}

#[test]
fn bump_version_dry_run_prints_only_the_version_and_writes_nothing() {
    let package_json = xtask::repo_root().join("package.json");
    let before = std::fs::read(&package_json).unwrap();
    let current = xtask::bump::package_version(&String::from_utf8(before.clone()).unwrap()).unwrap();
    // No <before>: the range is the last tag to HEAD, or just HEAD in a shallow checkout (as CI has).
    let out = xtask(&xtask::repo_root(), &["bump-version", "--dry-run"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    // Exactly `MAJOR.MINOR.PATCH` and a newline: the release job matches it with ^[0-9]+\.[0-9]+\.[0-9]+$.
    let version = stdout.strip_suffix('\n').unwrap_or_else(|| panic!("no newline: {stdout:?}"));
    assert!(
        version.split('.').count() == 3
            && version.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())),
        "{stdout:?}"
    );
    assert!(version_of(version) > version_of(&current), "{version} is not after {current}");
    assert_eq!(std::fs::read(&package_json).unwrap(), before, "--dry-run changed package.json");
}

#[test]
fn bump_version_refuses_an_unknown_option_and_too_many_arguments_and_prints_nothing_on_stdout() {
    let cwd = TempDir::new("xtask-cli-").unwrap();
    for (args, message) in [
        (vec!["bump-version", "--bogus"], "unknown option --bogus"),
        (vec!["bump-version", "a", "b", "c", "--dry-run"], "usage:"),
    ] {
        let out = xtask(cwd.path(), &args);
        assert!(!out.status.success(), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?} printed {}", text(&out.stdout));
        assert!(text(&out.stderr).contains(message), "{args:?}: {}", text(&out.stderr));
    }
}

#[test]
fn an_unknown_command_prints_the_usage_and_fails_and_help_prints_it_and_succeeds() {
    let cwd = TempDir::new("xtask-cli-").unwrap();
    let out = xtask(cwd.path(), &["frobnicate"]);
    assert!(!out.status.success() && out.stdout.is_empty());
    assert!(text(&out.stderr).contains("usage: cargo xtask <command>"));
    let out = xtask(cwd.path(), &["--help"]);
    assert!(out.status.success());
    let stdout = text(&out.stdout);
    for command in ["bump-version", "package", "installer-smoke-test"] {
        assert!(stdout.contains(command), "{command} is not in the usage:\n{stdout}");
    }
}

#[test]
fn an_installer_smoke_test_of_another_machines_archive_fails_instead_of_passing() {
    let cwd = TempDir::new("xtask-cli-").unwrap();
    let other = if xtask::package::host_target().unwrap() == "linux-x64" { "macos-arm64" } else { "linux-x64" };
    let file = cwd.path().join(format!("jev-claude-{other}.tar.gz"));
    std::fs::write(&file, b"not an archive").unwrap();
    let out = xtask(cwd.path(), &["installer-smoke-test", "--archive", file.to_str().unwrap()]);
    let stderr = text(&out.stderr);
    assert!(!out.status.success(), "a skipped smoke test must not count as a pass: {}", text(&out.stdout));
    assert!(stderr.contains("is not this machine's archive"), "{stderr}");
    assert!(stderr.contains(&xtask::package::host_target().unwrap()), "{stderr}");
    // The name check comes before any work: nothing was installed or served.
    assert!(!text(&out.stdout).contains("install.sh"), "{}", text(&out.stdout));

    let out = xtask(cwd.path(), &["installer-smoke-test", "--archive", "jev-claude-nope-x64.tar.gz"]);
    assert!(
        !out.status.success() && text(&out.stderr).contains("is not this machine's archive"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn the_stand_ins_act_as_claude_code_and_a_failing_jev_check_by_their_file_name() {
    let dir = TempDir::new("xtask-cli-").unwrap();
    let me = Path::new(env!("CARGO_BIN_EXE_xtask"));
    let exe = std::env::consts::EXE_SUFFIX;
    let claude = dir.path().join(format!("claude{exe}"));
    std::fs::copy(me, &claude).unwrap();
    let out = Command::new(&claude).arg("--version").output().unwrap();
    assert!(out.status.success() && text(&out.stdout).trim() == "2.1.0 (Claude Code)", "{}", text(&out.stdout));
    // A `.broken` file beside it makes it exit 1 without a word.
    std::fs::write(dir.path().join(".broken"), "").unwrap();
    let out = Command::new(&claude).arg("--version").output().unwrap();
    assert!(!out.status.success() && out.stdout.is_empty());
    let check = dir.path().join(format!("jev-check{exe}"));
    std::fs::copy(me, &check).unwrap();
    let out = Command::new(&check).output().unwrap();
    assert!(!out.status.success() && text(&out.stdout).trim() == "broken");
}
