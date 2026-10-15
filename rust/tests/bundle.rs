//! The release archive that installs without Node.js: the release packager (`cargo xtask package`,
//! the `xtask` crate) stages the programs this crate just built, and the staged folder must work on
//! its own.

mod common;

use common::*;
use jev_router::repo;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The folder the archive unpacks into, made from the programs cargo just built.
fn stage(base: &Path) -> PathBuf {
    let bin_dir = Path::new(env!("CARGO_BIN_EXE_jev-claude")).parent().unwrap().to_path_buf();
    let target = xtask::package::host_target().unwrap();
    let stage = base.join(format!("jev-claude-{target}"));
    xtask::package::stage_bundle(&bin_dir, &stage, &target, &repo_root()).unwrap();
    stage
}

fn program(stage: &Path, name: &str) -> PathBuf {
    stage.join("bin").join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// All seven programs are packaged, and the folder carries the root marker and the release version
#[test]
fn the_bundle_holds_every_program_and_finds_its_own_root() {
    let base = temp_dir("jev-bundle-");
    let stage = stage(base.path());
    for name in
        ["jev-claude", "jev-statusline", "jev-explain", "jev-legend", "jev-check", "jev-update-check", "jev-proxy-host"]
    {
        assert!(program(&stage, name).is_file(), "{name} is missing from the bundle");
    }
    // The programs walk up from bin/ (SPEC 2.4), so no JEV_ROOT is needed beside this folder.
    assert_eq!(repo::find_root_from(&stage.join("bin")).as_deref(), Some(stage.as_path()));
    let root_version = repo::version_in(&repo_root()).expect("the repository's own version");
    assert_eq!(repo::version_in(&stage), Some(root_version), "the bundle carries the release version (SPEC 2.3)");
}

/// The skills in the bundle call the native programs, and every program they call is in the bundle
#[test]
fn the_bundles_skills_call_native_programs_that_exist() {
    let base = temp_dir("jev-bundle-skills-");
    let stage = stage(base.path());
    let skills = stage.join(".claude").join("skills");
    let mut seen = Vec::new();
    for entry in std::fs::read_dir(&skills).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        let text = std::fs::read_to_string(skills.join(&name).join("SKILL.md")).unwrap();
        assert!(!text.contains(".mjs") && !text.contains("node/"), "{name} still needs Node:\n{text}");
        let call = text.lines().find_map(|l| l.strip_prefix("!`")).unwrap_or_else(|| panic!("{name} has no command"));
        let program_name = call.split([' ', '`']).next().unwrap();
        assert!(program(&stage, program_name).is_file(), "{name} calls {program_name}, which is not in the bundle");
        let allowed = text.lines().find(|l| l.starts_with("allowed-tools:")).unwrap();
        assert!(
            allowed.contains(&format!("Bash({program_name} *)"))
                && allowed.contains(&format!("PowerShell({program_name} *)")),
            "{name}: {allowed}"
        );
        seen.push(name);
    }
    seen.sort();
    assert_eq!(seen, ["jev-calibrate", "jev-explain", "jev-legend"]);
}

/// Stages the bundle under `base`, then runs jev-check and the launcher from it with no clone and no
/// `JEV_ROOT`: jev-check reports an installed copy, and the launcher hands Claude Code the bundle as
/// --add-dir, whole. Everything (the bundle, home, working folder) lives under `base`.
fn run_bundle_from(base: &Path) {
    let stage = stage(base);
    let (bin, home, cwd) = (base.join("fakebin"), base.join("home"), base.join("cwd"));
    for dir in [&bin, &home, &cwd] {
        std::fs::create_dir(dir).unwrap();
    }
    fake_claude(&bin);

    let check = Command::new(program(&stage, "jev-check"))
        .current_dir(&cwd)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("JEV_STATUS_DIR", base.join("status"))
        .env_remove("JEV_ROOT")
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&check.stdout);
    assert!(check.status.success(), "{report}{}", String::from_utf8_lossy(&check.stderr));
    let mode = report.lines().find(|l| l.starts_with("Mode")).unwrap_or_else(|| panic!("{report}"));
    assert!(mode.trim_end().ends_with("installed"), "{mode}");

    let out = Command::new(program(&stage, "jev-claude"))
        .arg("-p")
        .arg("hello")
        .current_dir(&cwd)
        .env("PATH", path_with(&bin))
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("JEV_STATUS_DIR", base.join("status"))
        .env_remove("JEV_ROOT")
        .env_remove("JEV_API_KEY")
        .env_remove("TYPESAFE_API_KEY")
        .env_remove("ANTHROPIC_BASE_URL")
        .env_remove("ANTHROPIC_MODEL")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let seen = jev_router::jsjson::parse_bytes(&out.stdout).unwrap();
    let args: Vec<String> =
        seen.get("args").as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string_lossy()).collect();
    let at = args.iter().position(|a| a == "--add-dir").unwrap_or_else(|| panic!("{args:?}"));
    assert_eq!(
        std::fs::canonicalize(&args[at + 1]).unwrap(),
        std::fs::canonicalize(&stage).unwrap(),
        "--add-dir names the bundle, so Claude Code loads its skills"
    );
}

/// Run from the bundle, jev-check reports an installed copy, and the launcher hands Claude Code the bundle as --add-dir
#[test]
fn the_bundled_programs_run_with_no_clone_and_no_jev_root() {
    if !have_node("the_bundled_programs_run_with_no_clone_and_no_jev_root") {
        return;
    }
    let base = temp_dir("jev-bundle-run-");
    run_bundle_from(base.path());
}

/// The same from a folder whose name has a space and non-ASCII letters, as a Windows user folder can
/// (the installers are tested with such a folder too, in `cargo xtask installer-smoke-test`)
#[test]
fn the_bundled_programs_run_from_a_folder_with_a_space_and_non_ascii_letters() {
    if !have_node("the_bundled_programs_run_from_a_folder_with_a_space_and_non_ascii_letters") {
        return;
    }
    let base = temp_dir("jev-bundle-odd-");
    let odd = base.join("Jev Test \u{dc}n\u{ef} dir");
    std::fs::create_dir(&odd).unwrap();
    assert!(odd.to_string_lossy().contains(' ') && !odd.to_string_lossy().is_ascii());
    run_bundle_from(&odd);
}

/// jev-check reports a source checkout (rust/Cargo.toml beside a .git directory), and only that
#[test]
fn jev_check_tells_a_source_checkout_from_an_installed_copy() {
    let base = temp_dir("jev-check-mode-");
    let root = base.join("root");
    let mode = |root: &Path| {
        let out = Command::new(env!("CARGO_BIN_EXE_jev-check"))
            .env("JEV_ROOT", root)
            .env("HOME", &*base)
            .env("USERPROFILE", &*base)
            .env("JEV_STATUS_DIR", base.join("status"))
            .output()
            .unwrap();
        let report = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(out.status.success(), "{report}{}", String::from_utf8_lossy(&out.stderr));
        let line = report.lines().find(|l| l.starts_with("Mode")).unwrap_or_else(|| panic!("{report}"));
        line.trim_end().rsplit(' ').next().unwrap().to_string()
    };
    std::fs::create_dir_all(root.join("rust")).unwrap();
    std::fs::write(root.join("rust").join("Cargo.toml"), "[package]\n").unwrap();
    assert_eq!(mode(&root), "installed", "no .git directory");
    std::fs::create_dir(root.join(".git")).unwrap();
    assert_eq!(mode(&root), "repository");
    std::fs::remove_file(root.join("rust").join("Cargo.toml")).unwrap();
    assert_eq!(mode(&root), "installed", "no rust/Cargo.toml");
}
