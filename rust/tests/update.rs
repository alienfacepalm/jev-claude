//! Port of node/test/update.test.mjs. These tests drive real git: a bare repository stands in
//! for GitHub, clones are made the way the installer makes them (including the shallow
//! `--depth 1` one), and the "upstream" moves on by real commits.

mod common;

use common::*;
use jev_router::jsjson::Value;
use jev_router::timefmt::{iso, parse_iso};
use jev_router::update::{
    CHECK_EVERY_MS, apply_update, check_for_update_now, compare_versions, is_check_due, needs_install, read_state,
    update_notice, write_state,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn pkg(version: &str) -> String {
    format!("{{\n  \"name\": \"jev-router\",\n  \"version\": \"{version}\"\n}}\n")
}

/// An origin at 0.1.0, a working copy that pushes to it, and helpers to release and clone.
struct Fixture {
    base: PathBuf,
    origin: PathBuf,
    upstream: PathBuf,
    _serial: std::sync::MutexGuard<'static, ()>,
}

/// Node runs a file's tests one after another; these share that, so dozens of git processes
/// never compete with the 5 s local git timeout the library applies.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl Fixture {
    fn new() -> Fixture {
        let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let base = temp_dir("jev-update-");
        let origin = base.join("origin.git");
        let upstream = base.join("upstream");
        git(&base, &["init", "--bare", "-b", "master", &origin.to_string_lossy()]);
        git(&base, &["clone", &origin.to_string_lossy(), &upstream.to_string_lossy()]);
        git(&upstream, &["checkout", "-b", "master"]);
        std::fs::write(upstream.join("package.json"), pkg("0.1.0")).unwrap();
        std::fs::write(upstream.join("pnpm-lock.yaml"), "lock: 1\n").unwrap();
        git(&upstream, &["add", "."]);
        git(&upstream, &["commit", "-m", "first"]);
        git(&upstream, &["push", "-u", "origin", "master"]);
        Fixture { base, origin, upstream, _serial: guard }
    }

    fn release(&self, version: &str, files: &[(&str, &str)]) {
        std::fs::write(self.upstream.join("package.json"), pkg(version)).unwrap();
        for (name, text) in files {
            std::fs::write(self.upstream.join(name), text).unwrap();
        }
        git(&self.upstream, &["add", "."]);
        git(&self.upstream, &["commit", "-m", &format!("release {version}")]);
        git(&self.upstream, &["push", "origin", "master"]);
    }

    fn clone_to(&self, name: &str, shallow: bool) -> PathBuf {
        let dir = self.base.join(name);
        if shallow {
            let url = format!("file:///{}", self.origin.to_string_lossy().replace('\\', "/").trim_start_matches('/'));
            git(&self.base, &["clone", "--depth", "1", &url, &dir.to_string_lossy()]);
        } else {
            git(&self.base, &["clone", &self.origin.to_string_lossy(), &dir.to_string_lossy()]);
        }
        dir
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn version(dir: &Path) -> String {
    json(&std::fs::read_to_string(dir.join("package.json")).unwrap()).get("version").as_str().unwrap().to_string_lossy()
}

fn status_of(v: &Value) -> String {
    v.get("status").as_str().unwrap().to_string_lossy()
}

fn reason_of(v: &Value) -> String {
    v.get("reason").as_str().unwrap().to_string_lossy()
}

fn level_with_origin(shallow: bool) {
    let f = Fixture::new();
    let install = f.clone_to("install", shallow);
    let found = check_for_update_now(&install);
    assert_eq!(found.get("available"), &Value::Bool(false));
    assert_eq!(update_notice(Some(&found), Some(&version(&install))), None);
}

fn release_found_and_applied(shallow: bool) {
    let f = Fixture::new();
    let install = f.clone_to("install", shallow);
    f.release("0.2.0", &[("new-file.txt", "hello\n")]);

    let found = check_for_update_now(&install);
    assert_eq!(found.get("available"), &Value::Bool(true));
    assert_eq!(found.get("latest"), &s("0.2.0"));
    assert_eq!(
        update_notice(Some(&found), Some(&version(&install))).as_deref(),
        Some("[jev] Update available: 0.1.0 -> 0.2.0. Run `jev-claude --update`.")
    );
    assert_eq!(version(&install), "0.1.0", "looking must not change the install");

    let applied = apply_update(&install, None);
    assert_eq!(applied, json(r#"{"status":"updated","from":"0.1.0","to":"0.2.0"}"#));
    assert_eq!(version(&install), "0.2.0");
    assert_eq!(std::fs::read_to_string(install.join("new-file.txt")).unwrap(), "hello\n");
    assert_eq!(git(&install, &["status", "--porcelain"]), "", "a clean checkout afterwards");

    assert_eq!(check_for_update_now(&install).get("available"), &Value::Bool(false), "nothing further to find");
    assert_eq!(apply_update(&install, None), json(r#"{"status":"current","version":"0.2.0"}"#));
}

/// "an install that is level with origin has no update (a full clone)"
#[test]
fn level_install_has_no_update_full_clone() {
    level_with_origin(false);
}

/// "an install that is level with origin has no update (a shallow clone, as the installer makes)"
#[test]
fn level_install_has_no_update_shallow_clone() {
    level_with_origin(true);
}

/// "a release upstream is found, announced, and applied by fast-forward (a full clone)"
#[test]
fn release_found_announced_and_applied_full_clone() {
    release_found_and_applied(false);
}

/// "a release upstream is found, announced, and applied by fast-forward (a shallow clone, as the installer makes)"
#[test]
fn release_found_announced_and_applied_shallow_clone() {
    release_found_and_applied(true);
}

/// "changed dependencies ask for an install; unchanged ones do not"
#[test]
fn changed_dependencies_ask_for_an_install() {
    let f = Fixture::new();
    let install = f.clone_to("install", false);
    f.release("0.1.1", &[("notes.txt", "docs only\n")]);
    let calls = RefCell::new(Vec::new());
    let recorded = |root: &Path| {
        calls.borrow_mut().push(root.to_path_buf());
        0
    };

    assert_eq!(status_of(&apply_update(&install, Some(&recorded))), "updated");
    assert!(calls.borrow().is_empty(), "a change that leaves the lockfile alone installs nothing");

    f.release("0.2.0", &[("pnpm-lock.yaml", "lock: 2\n")]);
    assert_eq!(status_of(&apply_update(&install, Some(&recorded))), "updated");
    assert_eq!(*calls.borrow(), vec![install.clone()], "a new lockfile installs once, in the install folder");

    f.release("0.3.0", &[("pnpm-lock.yaml", "lock: 3\n")]);
    let failing = |_: &Path| 1;
    let failed = apply_update(&install, Some(&failing));
    assert_eq!(status_of(&failed), "failed", "a failed install is reported, not hidden");
    assert!(reason_of(&failed).contains("exited with 1"));

    assert!(!needs_install(&["README.md", "src/proxy.mjs"]));
    assert!(!needs_install(&["package.json"]), "a version bump alone is not a dependency change");
    assert!(needs_install(&["README.md", "pnpm-lock.yaml"]));
}

/// "a copy with local changes is left exactly as it is"
#[test]
fn a_copy_with_local_changes_is_left_exactly_as_it_is() {
    let f = Fixture::new();
    let install = f.clone_to("install", false);
    std::fs::write(install.join("pnpm-lock.yaml"), "lock: 1\nmy edit\n").unwrap();
    f.release("0.2.0", &[]);

    assert_eq!(check_for_update_now(&install).get("available"), &Value::Bool(false), "no notice");
    let applied = apply_update(&install, None);
    assert_eq!(status_of(&applied), "refused");
    assert!(reason_of(&applied).contains("local changes"));
    assert!(std::fs::read_to_string(install.join("pnpm-lock.yaml")).unwrap().contains("my edit"), "the edit survives");
    assert_eq!(version(&install), "0.1.0");
}

/// "a development clone ahead of origin is not touched"
#[test]
fn a_development_clone_ahead_of_origin_is_not_touched() {
    let f = Fixture::new();
    let install = f.clone_to("install", false);
    std::fs::write(install.join("mine.txt"), "work in progress\n").unwrap();
    git(&install, &["add", "."]);
    git(&install, &["commit", "-m", "my own commit"]);
    let head = git(&install, &["rev-parse", "HEAD"]);

    let level = apply_update(&install, None);
    assert_eq!(status_of(&level), "current", "ahead of origin with nothing new upstream: nothing to do");
    assert_eq!(git(&install, &["rev-parse", "HEAD"]), head);

    f.release("0.2.0", &[]);
    assert_eq!(check_for_update_now(&install).get("available"), &Value::Bool(false), "diverged");
    let applied = apply_update(&install, None);
    assert_eq!(status_of(&applied), "refused");
    assert!(reason_of(&applied).contains("commits that origin/master does not"));
    assert_eq!(git(&install, &["rev-parse", "HEAD"]), head, "no merge, no rewrite");
}

/// "a detached checkout is refused"
#[test]
fn a_detached_checkout_is_refused() {
    let f = Fixture::new();
    let install = f.clone_to("install", false);
    git(&install, &["checkout", "--detach"]);
    f.release("0.2.0", &[]);
    let applied = apply_update(&install, None);
    assert_eq!(status_of(&applied), "refused");
    assert!(reason_of(&applied).contains("not on a branch"));
}

/// "an unreachable origin is reported, never thrown, and the install is unchanged"
#[test]
fn an_unreachable_origin_is_reported() {
    let f = Fixture::new();
    let install = f.clone_to("install", false);
    std::fs::remove_dir_all(&f.origin).unwrap();

    assert_eq!(check_for_update_now(&install).get("available"), &Value::Bool(false));
    let applied = apply_update(&install, None);
    assert_eq!(status_of(&applied), "refused");
    assert!(reason_of(&applied).contains("could not reach origin"));
    assert_eq!(version(&install), "0.1.0");
}

/// "folders that are not a clone of their own are refused"
#[test]
fn folders_that_are_not_a_clone_of_their_own_are_refused() {
    let f = Fixture::new();
    let plain = f.base.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let refused_plain = apply_update(&plain, None);
    assert_eq!(status_of(&refused_plain), "refused");
    assert!(reason_of(&refused_plain).contains("not a git clone"));

    // An install copied into some other repository must not be updated through that repository.
    let outer = f.clone_to("outer", false);
    let inner = outer.join("vendored").join("jev-claude");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(inner.join("package.json"), pkg("0.0.1")).unwrap();
    let refused_inner = apply_update(&inner, None);
    assert_eq!(status_of(&refused_inner), "refused");
    assert!(reason_of(&refused_inner).contains("not a git clone of its own"));
}

/// "the check is due when missing, stale, unreadable, or from the future"
#[test]
fn the_check_is_due_when_missing_stale_unreadable_or_from_the_future() {
    let now = parse_iso("2026-10-04T12:00:00.000Z").unwrap();
    let at = |ms: f64| json(&format!(r#"{{"checkedAt":"{}"}}"#, iso(now - ms)));
    assert!(is_check_due(None, now, CHECK_EVERY_MS));
    assert!(is_check_due(Some(&json("{}")), now, CHECK_EVERY_MS));
    assert!(is_check_due(Some(&json(r#"{"checkedAt":"yesterday-ish"}"#)), now, CHECK_EVERY_MS));
    assert!(!is_check_due(Some(&at(CHECK_EVERY_MS - 60_000.0)), now, CHECK_EVERY_MS), "just inside the window");
    assert!(is_check_due(Some(&at(CHECK_EVERY_MS)), now, CHECK_EVERY_MS), "exactly at the window");
    assert!(is_check_due(Some(&at(-60_000.0)), now, CHECK_EVERY_MS), "a clock that went backwards");
}

/// "versions compare as numbers, not text"
#[test]
fn versions_compare_as_numbers_not_text() {
    assert!(compare_versions("0.10.0", "0.9.9") > 0.0, "0.10 is newer than 0.9");
    assert!(compare_versions("0.6.5", "0.6.6") < 0.0);
    assert_eq!(compare_versions("1.0", "1.0.0"), 0.0);
    assert!(compare_versions("0.7.0-beta.1", "0.6.9") > 0.0, "a pre-release tag is ignored");
}

/// "a notice appears only for a genuinely newer version"
#[test]
fn a_notice_appears_only_for_a_genuinely_newer_version() {
    let found = json(r#"{"available":true,"latest":"0.7.0"}"#);
    assert!(update_notice(Some(&found), Some("0.6.5")).unwrap().contains("0.6.5 -> 0.7.0"));
    assert_eq!(update_notice(Some(&found), Some("0.7.0")), None, "already installed some other way");
    assert_eq!(update_notice(Some(&found), Some("0.8.0")), None, "a development copy ahead of the release");
    assert_eq!(update_notice(Some(&json(r#"{"available":false,"latest":"0.7.0"}"#)), Some("0.6.5")), None);
    assert_eq!(update_notice(None, Some("0.6.5")), None);
    assert_eq!(update_notice(Some(&found), None), None, "an unreadable local version says nothing");
}

/// "the state file round-trips and a damaged one reads as no state"
#[test]
fn the_state_file_round_trips_and_a_damaged_one_reads_as_no_state() {
    let dir = temp_dir("jev-update-state-");
    let file = dir.join("nested").join("update.json");
    assert_eq!(read_state(&file), None, "missing");
    let state = json(r#"{"checkedAt":"2026-10-04T12:00:00.000Z","available":true,"latest":"0.7.0"}"#);
    write_state(&state, &file);
    assert_eq!(read_state(&file), Some(state));
    std::fs::write(&file, "{ not json").unwrap();
    assert_eq!(read_state(&file), None, "damaged");
    std::fs::write(&file, "42").unwrap();
    assert_eq!(read_state(&file), None, "valid JSON that is not a state object");
    let _ = std::fs::remove_dir_all(&dir);
}
