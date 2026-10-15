//! `cargo xtask package`: the skills an installed copy ships, the archive's name (and that the
//! installers and ci.yml ask for the same names), and the command line as CI runs it. What the
//! staged folder holds is tested in rust/tests/bundle.rs, against the programs cargo just built.

use std::path::{Path, PathBuf};
use std::process::Command;
use xtask::TempDir;
use xtask::package::{PROGRAMS, TARGETS, host_target, packaged_skill, sha256_file};

fn skills() -> PathBuf {
    xtask::repo_root().join(".claude").join("skills")
}

fn skill_text(name: &str) -> String {
    std::fs::read_to_string(skills().join(name).join("SKILL.md")).unwrap().replace("\r\n", "\n")
}

#[test]
fn every_skill_in_the_repository_ships_as_one_that_runs_a_native_program_and_nothing_else() {
    let mut names: Vec<String> = std::fs::read_dir(skills())
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.file_type().unwrap().is_dir())
        .map(|e| e.file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["jev-calibrate", "jev-explain", "jev-legend"]);
    for name in &names {
        let out = packaged_skill(&skill_text(name), name).unwrap();
        let call = out.lines().find_map(|line| line.strip_prefix("!`")).unwrap();
        let program = call.split([' ', '`']).next().unwrap();
        assert!(PROGRAMS.contains(&program), "{name} calls {program}, which is not one of the shipped programs");
        let allowed = format!("allowed-tools: Bash({program} *) PowerShell({program} *)");
        assert!(out.lines().any(|line| line == allowed), "{name}");
        // The same test as rust/tests/bundle.rs makes of the staged folder.
        assert!(!out.contains(".mjs") && !out.contains("node/"), "{name} still needs Node");
    }
}

#[test]
fn skills_other_than_jev_calibrate_ship_exactly_as_the_repository_has_them() {
    for name in ["jev-explain", "jev-legend"] {
        assert_eq!(packaged_skill(&skill_text(name), name).unwrap(), skill_text(name));
    }
    let out = packaged_skill(&skill_text("jev-explain"), "jev-explain").unwrap();
    assert!(out.contains("!`jev-explain \"${CLAUDE_SESSION_ID}\" ; echo \"\"`"), "{out}");
    assert!(out.contains("Return the report below verbatim in a plain text code block"), "{out}");
}

#[test]
fn the_installed_jev_calibrate_only_reports_re_tuning_needs_a_clone_of_the_repository() {
    let out = packaged_skill(&skill_text("jev-calibrate"), "jev-calibrate").unwrap();
    assert!(out.contains("!`jev-check ; echo \"\"`"), "{out}");
    for gone in ["\n## Repository: re-calibrate", "cargo ", "config.rs", "calibration-cases"] {
        assert!(!out.contains(gone), "{gone:?} is still in:\n{out}");
    }
    assert!(out.contains("Report only (the default)"), "{out}");
    assert!(out.contains("re-tuning is done in\na clone of the router's repository"), "{out}");
    assert!(out.lines().any(|l| l == "description: Check Jev Router's setup and models (read-only)."), "{out}");
    assert!(out.ends_with(".\n") && !out.ends_with("\n\n"), "ends with one newline");
}

#[test]
fn a_skill_that_calls_no_shipped_program_lost_its_allowed_tools_or_names_a_node_script_fails_the_build() {
    let legend = skill_text("jev-legend");
    let call = legend.lines().find(|l| l.starts_with("!`")).unwrap().to_string();
    let error = |text: String| packaged_skill(&text, "jev-legend").unwrap_err();
    assert!(error(legend.replace(&call, "echo")).contains("no !`<program>` line"));
    assert!(error(legend.replace("!`jev-legend ;", "!`jev-nope ;")).contains("not a shipped"));
    let allowed = legend.lines().find(|l| l.starts_with("allowed-tools: ")).unwrap().to_string();
    assert!(error(legend.replace(&allowed, "allowed-tools: Bash(*)")).contains("allowed-tools"));
    let extra = format!("{legend}\nThen run: node node/bin/jev-legend.mjs\n");
    assert!(error(extra).contains("still refers to the Node implementation"));
}

#[test]
fn a_renamed_re_tuning_heading_fails_the_build_instead_of_shipping_the_clone_only_part() {
    let calibrate = skill_text("jev-calibrate");
    let renamed = calibrate.replace("## Repository: re-calibrate", "## Re-calibrating the repository");
    let error = packaged_skill(&renamed, "jev-calibrate").unwrap_err();
    assert!(error.contains("re-tuning part was not left out"), "{error}");
}

/// The archive this machine's installer downloads, by an (OS, CPU) table of its own.
#[test]
fn host_target_names_the_archive_the_installers_download() {
    let expected = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64" | "aarch64") => "windows-x64", // ARM PCs run the x64 programs by emulation
        ("macos", "x86_64") => "macos-x64",
        ("macos", "aarch64") => "macos-arm64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        other => panic!("no release target for {other:?}"),
    };
    assert_eq!(host_target().unwrap(), expected);
    assert!(TARGETS.contains(&expected));
}

fn repo_file(path: &str) -> String {
    let file = xtask::repo_root().join(path);
    std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()))
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names.dedup();
    names
}

/// The archive names are spelled out in four places that must agree: `package`, install.sh's
/// platform check, install.ps1's download, and the build matrix in ci.yml.
#[test]
fn release_targets_match_the_installers_and_ci() {
    let want = sorted(TARGETS.iter().map(ToString::to_string).collect());

    // install.sh: the `case` line that lists the targets it supports.
    let sh = repo_file("install.sh");
    let line = sh
        .lines()
        .find(|l| l.contains("linux-x64") && l.trim_end().ends_with(')'))
        .expect("install.sh lists the targets");
    let listed = sorted(line.trim().trim_end_matches(')').split('|').map(|t| t.trim().to_string()).collect());
    assert_eq!(listed, want, "install.sh: {line}");

    // install.ps1: it only ever asks for the x64 programs (an ARM PC runs them by emulation).
    let ps1 = repo_file("install.ps1");
    assert!(ps1.contains("'jev-claude-windows-x64.tar.gz'"), "install.ps1 asks for another archive");
    let windows: Vec<&str> = TARGETS.iter().copied().filter(|t| t.starts_with("windows")).collect();
    assert_eq!(windows, ["windows-x64"]);

    // ci.yml: every build matrix lists each target once, with the Rust triple that builds it.
    let triples = [
        ("linux-x64", "x86_64-unknown-linux"),
        ("linux-arm64", "aarch64-unknown-linux"),
        ("macos-x64", "x86_64-apple-darwin"),
        ("macos-arm64", "aarch64-apple-darwin"),
        ("windows-x64", "x86_64-pc-windows"),
    ];
    let ci = repo_file(".github/workflows/ci.yml");
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in ci.lines().map(str::trim) {
        if let Some(entry) = line.strip_prefix("- { target: ") {
            let target = entry.split(',').next().unwrap().trim().to_string();
            let triple =
                entry.split("triple: ").nth(1).and_then(|t| t.split([',', ' ', '}']).next()).unwrap_or_default();
            let (_, prefix) =
                triples.iter().find(|(t, _)| *t == target).unwrap_or_else(|| panic!("ci.yml builds {target}"));
            assert!(triple.starts_with(prefix), "ci.yml builds {target} with {triple}");
            current.push(target);
        } else if !(line.is_empty() || line.starts_with('#') || current.is_empty()) {
            groups.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    assert!(!groups.is_empty(), "ci.yml has no build matrix of targets");
    for group in groups {
        assert_eq!(sorted(group.clone()), want, "a ci.yml build matrix: {group:?}");
    }
}

/// `cargo xtask package <args>` run in `cwd`.
fn package(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("package")
        .args(args)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

/// Seven stand-in programs named for Windows, whatever this machine is.
fn fake_programs(dir: &Path, skip: Option<&str>) {
    std::fs::create_dir_all(dir).unwrap();
    for program in PROGRAMS {
        if skip != Some(program) {
            std::fs::write(dir.join(format!("{program}.exe")), program).unwrap();
        }
    }
}

#[test]
fn package_from_relative_paths_writes_a_windows_archive_removes_the_stage_and_prints_its_sha256() {
    let tmp = TempDir::new("xtask-package-").unwrap();
    fake_programs(&tmp.path().join("built"), None);
    let out = package(tmp.path(), &["--bin-dir", "built", "--target", "windows-x64", "--out", "dist"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    let archive = tmp.path().join("dist").join("jev-claude-windows-x64.tar.gz");
    assert!(archive.is_file());
    assert!(!tmp.path().join("dist").join("jev-claude-windows-x64").exists(), "the stage folder is removed");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let version = xtask::package::release_version(&xtask::repo_root()).unwrap();
    assert!(stdout.contains(&format!("staged jev-claude {version} for windows-x64")), "{stdout}");
    // The last line is what the release job publishes: the archive's SHA-256, two spaces, its path.
    let last = stdout.lines().rev().find(|l| !l.trim().is_empty()).unwrap();
    let (sha, path) = last.split_once("  ").unwrap_or_else(|| panic!("no `<sha256>  <file>` line in {stdout}"));
    assert!(sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{sha}");
    assert_eq!(sha, sha256_file(&archive).unwrap());
    assert!(Path::new(path).is_absolute() && path.ends_with("jev-claude-windows-x64.tar.gz"), "{path}");

    let listing = Command::new("tar")
        .arg("-tzf")
        .arg("jev-claude-windows-x64.tar.gz")
        .current_dir(tmp.path().join("dist"))
        .output()
        .unwrap();
    assert!(listing.status.success());
    let names: Vec<String> =
        String::from_utf8_lossy(&listing.stdout).lines().map(|l| l.trim_end_matches('\r').to_string()).collect();
    let mut wanted: Vec<String> = PROGRAMS.iter().map(|p| format!("./bin/{p}.exe")).collect();
    wanted.extend(
        ["./package.json", "./.env.example", "./LICENSE"]
            .into_iter()
            .chain([
                "./.claude/skills/jev-calibrate/SKILL.md",
                "./.claude/skills/jev-explain/SKILL.md",
                "./.claude/skills/jev-legend/SKILL.md",
            ])
            .map(str::to_string),
    );
    for name in wanted {
        assert!(names.contains(&name), "{name} is not in {names:?}");
    }
    let bare: Vec<&String> = names
        .iter()
        .filter(|n| n.starts_with("./bin/") && n.len() > "./bin/".len() && Path::new(n.as_str()).extension().is_none())
        .collect();
    assert!(bare.is_empty(), "{bare:?}");
}

#[test]
fn package_stage_only_keeps_the_folder_and_writes_no_archive() {
    let tmp = TempDir::new("xtask-package-").unwrap();
    fake_programs(&tmp.path().join("built"), None);
    let out = package(tmp.path(), &["--bin-dir", "built", "--target", "windows-x64", "--out", "dist", "--stage-only"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stage = tmp.path().join("dist").join("jev-claude-windows-x64");
    assert!(stage.join("bin").join("jev-claude.exe").is_file());
    assert!(std::fs::read_to_string(stage.join("package.json")).unwrap().contains("\"name\": \"jev-router\""));
    assert!(!tmp.path().join("dist").join("jev-claude-windows-x64.tar.gz").exists());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.lines().any(|l| l.split_once("  ").is_some_and(|(sha, _)| sha.len() == 64)), "{stdout}");
}

#[test]
fn package_rejects_a_target_that_is_not_a_release_target_an_unknown_option_and_a_missing_value() {
    let tmp = TempDir::new("xtask-package-").unwrap();
    fake_programs(&tmp.path().join("built"), None);
    for (args, message) in [
        (vec!["--bin-dir", "built", "--target", "linux-amd64", "--out", "dist"], "linux-amd64"),
        (vec!["--nope"], "unexpected argument --nope"),
        (vec!["--bin-dir", "built", "--out"], "--out needs a value"),
    ] {
        let out = package(tmp.path(), &args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success() && stderr.contains(message), "{args:?}: {stderr}");
        assert!(out.stdout.is_empty(), "{args:?} printed {}", String::from_utf8_lossy(&out.stdout));
    }
    assert!(!tmp.path().join("dist").exists(), "nothing was staged");
}

#[test]
fn package_names_the_program_that_was_not_built_and_how_to_build_it() {
    let tmp = TempDir::new("xtask-package-").unwrap();
    fake_programs(&tmp.path().join("built"), Some("jev-legend"));
    let out = package(tmp.path(), &["--bin-dir", "built", "--target", "windows-x64", "--out", "dist"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success() && stderr.contains("jev-legend.exe") && stderr.contains("cargo build --release --locked"),
        "{stderr}"
    );
}
