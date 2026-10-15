//! The repository root and release version as the programs resolve them (SPEC 2.3, 2.4), and the
//! background update check (SPEC 14) run as a program: built programs are copied into a folder with
//! no root above it, and into a release-like folder that carries the root marker.

// Each test's doc comment is its title as a plain sentence; Markdown backticks would change them.
#![allow(clippy::doc_markdown)]

mod common;

use common::*;
use std::path::{Path, PathBuf};
use std::process::Command;

const PROGRAMS: [&str; 3] = ["jev-update-check", "jev-check", "jev-claude"];

/// A folder holding copies of the built programs in `bin/`; with `release`, also the root marker
/// and a package.json, as an unpacked release archive has. Returned canonical, so a symlinked temp
/// directory cannot make the walk up find a root by another spelling.
fn programs_in(base: &Path, name: &str, release: bool) -> PathBuf {
    let dir = std::fs::canonicalize(base).unwrap().join(name);
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for program in PROGRAMS {
        let built = Path::new(env!("CARGO_BIN_EXE_jev-claude"))
            .with_file_name(format!("{program}{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(&built, bin.join(built.file_name().unwrap())).unwrap();
    }
    if release {
        let skill = dir.join(".claude").join("skills").join("jev-calibrate");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "---\nname: jev-calibrate\n---\n").unwrap();
        std::fs::write(dir.join("package.json"), "{\"name\":\"jev-router\",\"version\":\"9.8.7\"}\n").unwrap();
    }
    assert_eq!(jev_router::repo::find_root_from(&bin).is_some(), release, "{}", dir.display());
    dir
}

fn program(dir: &Path, name: &str) -> PathBuf {
    dir.join("bin").join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// A command for a copied program with a scratch home and no JEV_ROOT.
fn run(dir: &Path, name: &str, home: &Path) -> Command {
    let mut command = Command::new(program(dir, name));
    command
        .current_dir(home)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("JEV_STATUS_DIR", home.join("status"))
        .env_remove("JEV_ROOT")
        .stdin(std::process::Stdio::null());
    command
}

/// jev-update-check does nothing without a root
#[test]
fn jev_update_check_does_nothing_without_a_root() {
    let base = temp_dir("jev-repo-loose-");
    let dir = programs_in(&base, "loose", false);
    let home = base.join("home");
    std::fs::create_dir(&home).unwrap();
    let out = run(&dir, "jev-update-check", &home).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert!(!home.join(".jev-router").exists(), "no update.json is written");
}

/// jev-update-check in a release folder records that there is no update
#[test]
fn jev_update_check_in_a_release_folder_reports_no_update() {
    let base = temp_dir("jev-repo-release-");
    let dir = programs_in(&base, "release", true);
    let home = base.join("home");
    std::fs::create_dir(&home).unwrap();
    let out = run(&dir, "jev-update-check", &home).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let state = std::fs::read(home.join(".jev-router").join("update.json")).expect("update.json is written");
    let state = jev_router::jsjson::parse_bytes(&state).unwrap();
    assert_eq!(state.get("available"), &jev_router::jsjson::Value::Bool(false));
    let at = state.get("checkedAt").as_str().expect("checkedAt").to_string_lossy();
    assert!(jev_router::timefmt::parse_iso(&at).is_some() && at.len() == 24 && at.ends_with('Z'), "{at}");
}

/// An empty JEV_ROOT falls back to the folder the program is in
#[test]
fn an_empty_jev_root_falls_back_to_the_programs_own_folder() {
    let base = temp_dir("jev-repo-empty-root-");
    let mode = |dir: &Path, root: Option<&str>| {
        let home = base.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let mut command = run(dir, "jev-check", &home);
        if let Some(root) = root {
            command.env("JEV_ROOT", root);
        }
        let out = command.output().unwrap();
        let report = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(out.status.success(), "{report}");
        report.lines().find(|l| l.starts_with("Mode")).unwrap_or_else(|| panic!("{report}")).to_string()
    };
    // Make the release folder look like a source checkout, so "repository" proves the root was found
    // by the walk up from bin/ rather than missing.
    let dir = programs_in(&base, "checkout", true);
    std::fs::create_dir(dir.join(".git")).unwrap();
    std::fs::create_dir(dir.join("rust")).unwrap();
    std::fs::write(dir.join("rust").join("Cargo.toml"), "[package]\n").unwrap();
    assert!(mode(&dir, None).ends_with("repository"), "found by the walk up");
    assert!(mode(&dir, Some("")).ends_with("repository"), "an empty JEV_ROOT is not a root");
    let elsewhere = base.join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    assert!(mode(&dir, Some(&elsewhere.to_string_lossy())).ends_with("installed"), "a set JEV_ROOT wins");
}

/// Without a root, jev-claude passes no --add-dir
#[cfg(unix)]
#[test]
fn without_a_root_jev_claude_passes_no_add_dir() {
    let base = temp_dir("jev-repo-noroot-claude-");
    let dir = programs_in(&base, "loose", false);
    let (fake, home) = (base.join("fake"), base.join("home"));
    for d in [&fake, &home] {
        std::fs::create_dir(d).unwrap();
    }
    native_claude(&fake);
    let out = run(&dir, "jev-claude", &home)
        .args(["-p", "hello"])
        .env("PATH", std::env::join_paths([fake.clone(), "/usr/bin".into(), "/bin".into()]).unwrap())
        .env("JEV_API_KEY", "test-key")
        .env_remove("TYPESAFE_API_KEY")
        .env_remove("ANTHROPIC_BASE_URL")
        .env_remove("ANTHROPIC_MODEL")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}{}", String::from_utf8_lossy(&out.stderr));
    let args: Vec<&str> = stdout.lines().filter_map(|l| l.strip_prefix("arg:")).collect();
    assert_eq!(&args[..2], ["-p", "hello"], "{args:?}");
    assert!(!args.contains(&"--add-dir"), "{args:?}");
    assert!(args.contains(&"--settings"), "still a session: {args:?}");
}
