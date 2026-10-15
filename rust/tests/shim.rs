//! The Lattis shim (`shim/claude`, a POSIX sh script) run under `/bin/sh` against stub programs:
//! it hands plain launches to the real `claude`, Lattis's to the native launcher, and never picks
//! itself, under another name in another PATH folder, as either of them.
#![cfg(unix)]

mod common;

use common::*;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A stub that prints its name, the `JEV_SHIM` marker and its arguments.
fn stub(file: &Path, name: &str) {
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, format!("#!/bin/sh\necho \"{name} JEV_SHIM=${{JEV_SHIM:-}} $*\"\n")).unwrap();
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Layout {
    _base: TempDir,
    home: PathBuf,
    shim: PathBuf,
    link: PathBuf,
    real: PathBuf,
    native: PathBuf,
}

/// The shim in `shim/`, a symlink to it in `link/`, and stub `claude` and `jev-claude` programs.
fn layout() -> Layout {
    let base = temp_dir("jev-shim-sh-");
    let (home, shim, link) = (base.join("home"), base.join("shim"), base.join("link"));
    for dir in [&home, &shim, &link] {
        std::fs::create_dir(dir).unwrap();
    }
    std::fs::copy(repo_root().join("shim").join("claude"), shim.join("claude")).unwrap();
    std::fs::set_permissions(shim.join("claude"), std::fs::Permissions::from_mode(0o755)).unwrap();
    symlink(shim.join("claude"), link.join("claude")).unwrap();
    let (real, native) = (base.join("real"), base.join("native"));
    stub(&real.join("claude"), "REAL");
    stub(&native.join("jev-claude"), "NATIVE");
    Layout { _base: base, home, shim, link, real, native }
}

fn path_of(dirs: &[&Path]) -> String {
    let mut parts: Vec<String> = dirs.iter().map(|d| d.to_string_lossy().into_owned()).collect();
    parts.extend(["/usr/bin".to_string(), "/bin".to_string()]);
    parts.join(":")
}

/// Runs `sh <flags> <entry> a` with only `env`, and returns its output; a run that does not end
/// within 10 s (the shim calling itself forever) fails the test.
fn run(entry: &Path, flags: &[&str], env: &[(&str, &str)]) -> String {
    let mut cmd = Command::new("/bin/sh");
    cmd.args(flags).arg(entry).arg("a").env_clear().stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{} never finished: it keeps starting itself", entry.display());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim_end().to_string()
}

/// A plain launch reaches the real claude when another PATH folder holds a link to the shim
#[test]
fn a_link_to_the_shim_in_another_path_folder_is_not_the_real_claude() {
    let l = layout();
    let home = l.home.to_string_lossy().into_owned();
    for (entry, order) in [(&l.shim, [&l.shim, &l.link]), (&l.link, [&l.shim, &l.link]), (&l.link, [&l.link, &l.shim])]
    {
        let path = path_of(&[order[0], order[1], &l.real]);
        let got = run(&entry.join("claude"), &[], &[("HOME", &home), ("PATH", &path)]);
        assert_eq!(got, "REAL JEV_SHIM= a", "from {}", entry.display());
    }
}

/// `JEV_REAL_CLAUDE` naming a link to the shim falls back to the real claude on PATH
#[test]
fn jev_real_claude_naming_the_shim_falls_back_to_path() {
    let l = layout();
    let (home, link) = (l.home.to_string_lossy().into_owned(), l.link.join("claude").to_string_lossy().into_owned());
    let path = path_of(&[&l.shim, &l.link, &l.real]);
    let got = run(&l.shim.join("claude"), &[], &[("HOME", &home), ("PATH", &path), ("JEV_REAL_CLAUDE", &link)]);
    assert_eq!(got, "REAL JEV_SHIM= a");
}

/// A Lattis launch goes to the native launcher, never to a jev-claude that is the shim itself
#[test]
fn a_lattis_launch_goes_to_the_native_launcher() {
    let l = layout();
    let fake = l.link.join("jev-claude");
    symlink(l.shim.join("claude"), &fake).unwrap();
    let home = l.home.to_string_lossy().into_owned();
    let path = path_of(&[&l.shim, &l.link, &l.real, &l.native]);
    for config in ["/x/lattis/claude", "/x/Application Support/Lattis/claude"] {
        let env = [("HOME", home.as_str()), ("PATH", path.as_str()), ("CLAUDE_CONFIG_DIR", config)];
        assert_eq!(run(&l.shim.join("claude"), &[], &env), "NATIVE JEV_SHIM=1 a", "{config}");
    }
    // The launcher's own call back to `claude` carries the marker and reaches the real one.
    let env = [("HOME", home.as_str()), ("PATH", path.as_str()), ("JEV_SHIM", "1"), ("LATTIS_AGENT_TOKEN", "t")];
    assert_eq!(run(&l.link.join("claude"), &[], &env), "REAL JEV_SHIM=1 a");
}

/// The shim runs under `set -u`, with `HOME` and `JEV_SHIM` unset
#[test]
fn the_shim_runs_under_set_u() {
    let l = layout();
    let path = path_of(&[&l.shim, &l.real, &l.native]);
    assert_eq!(run(&l.shim.join("claude"), &["-u"], &[("PATH", &path)]), "REAL JEV_SHIM= a");
    let env = [("PATH", path.as_str()), ("LATTIS_AGENT_TOKEN", "t")];
    assert_eq!(run(&l.shim.join("claude"), &["-u"], &env), "NATIVE JEV_SHIM=1 a");
}

/// With no real claude anywhere, the shim says so
#[test]
fn with_no_real_claude_the_shim_says_so() {
    let l = layout();
    let home = l.home.to_string_lossy().into_owned();
    let path = path_of(&[&l.shim, &l.link]);
    let got = run(&l.shim.join("claude"), &[], &[("HOME", &home), ("PATH", &path)]);
    assert!(got.starts_with("[jev] shim: the real Claude Code was not found"), "{got}");
}
