//! `cargo xtask bump-version`: the bump rules, the commits a release counts, and the edit to
//! package.json.

use std::path::Path;
use std::process::Command;
use xtask::TempDir;
use xtask::bump::{bump_level, messages_between, next_version, package_version, with_version};

/// `git <args>` in `cwd`, with no user or system configuration (a signing or hook setting there
/// must not change what these commits are).
fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("HOME", cwd)
        .env("XDG_CONFIG_HOME", cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
fn a_release_counts_every_commit_since_the_last_tag_not_just_the_last_push() {
    let dir = TempDir::new("jev-bump-").unwrap();
    let cwd = dir.path();
    git(cwd, &["init", "-q"]);
    git(cwd, &["config", "user.email", "test@example.com"]);
    git(cwd, &["config", "user.name", "test"]);
    let commit = |message: &str| {
        git(cwd, &["commit", "-q", "--allow-empty", "-m", message]);
        git(cwd, &["rev-parse", "HEAD"])
    };
    let first = commit("chore(release): v0.4.0");
    git(cwd, &["tag", "v0.4.0"]);
    commit("feat: pushed first, but its run failed");
    let before = commit("docs: also in the failed push");
    let after = commit("fix: the push whose run succeeds");

    let messages = messages_between(Some(&before), &after, Some(cwd)).unwrap();
    assert_eq!(messages.len(), 3, "everything since v0.4.0");
    assert_eq!(bump_level(&messages), "minor", "the earlier feat still counts");

    git(cwd, &["tag", "-d", "v0.4.0"]);
    assert_eq!(
        messages_between(Some(&before), &after, Some(cwd)).unwrap().len(),
        1,
        "with no tag, only the pushed range"
    );
    let zeros = "0".repeat(40);
    assert_eq!(
        messages_between(Some(&zeros), &first, Some(cwd)).unwrap().len(),
        1,
        "a first push counts its last commit"
    );
    assert_eq!(messages_between(None, &first, Some(cwd)).unwrap().len(), 1, "so does one with no before at all");
}

#[test]
fn a_feature_is_a_minor_bump_anything_else_a_patch() {
    assert_eq!(bump_level(&["fix: keep the cache", "feat(statusline): show versions"]), "minor");
    assert_eq!(bump_level(&["fix: keep the cache", "docs: explain .env.example"]), "patch");
    assert_eq!(bump_level(&["Show sub-agent models with their version"]), "patch", "no prefix is a patch");
}

#[test]
fn a_breaking_change_is_a_major_bump_in_the_subject_or_the_body() {
    assert_eq!(bump_level(&["feat!: drop Node 18"]), "major");
    assert_eq!(bump_level(&["refactor(proxy)!: new status format"]), "major");
    assert_eq!(bump_level(&["feat: new config\n\nBREAKING CHANGE: JEV_TIER renamed"]), "major");
    assert_eq!(bump_level(&["fix: x\n\nBREAKING-CHANGE: also with a hyphen"]), "major");
}

#[test]
fn the_workflows_own_release_commits_do_not_count() {
    assert_eq!(bump_level(&["chore(release): v0.5.0"]), "patch");
    assert_eq!(bump_level(&["chore(release): v0.5.0", "feat: something"]), "minor");
}

#[test]
fn a_bump_resets_the_lower_parts_of_the_version() {
    assert_eq!(next_version("0.4.0", "patch").unwrap(), "0.4.1");
    assert_eq!(next_version("0.4.7", "minor").unwrap(), "0.5.0");
    assert_eq!(next_version("0.4.7", "major").unwrap(), "1.0.0");
    assert_eq!(next_version("1.2.3-rc.1", "patch").unwrap(), "1.2.4", "a pre-release suffix is dropped");
    assert!(next_version("one.two", "patch").is_err());
}

#[test]
fn the_new_version_replaces_only_the_top_level_version_in_package_json() {
    let text = "{\n  \"name\": \"jev-router\",\n  \"repository\": { \"version\": \"9.9.9\" },\n  \"description\": \"a \\\"version\\\": \\\"8.8.8\\\" string\",\n  \"version\": \"0.11.0\",\n  \"private\": true\n}\n";
    assert_eq!(package_version(text).unwrap(), "0.11.0");
    let bumped = with_version(text, "0.12.0").unwrap();
    assert_eq!(bumped, text.replace("\"version\": \"0.11.0\"", "\"version\": \"0.12.0\""));
    assert!(package_version("{\"name\": \"x\"}").is_err(), "no version");
    assert!(package_version("[\"version\", \"1.0.0\"]").is_err(), "not an object");
}

#[test]
fn the_repositorys_package_json_carries_a_version_the_bump_can_read() {
    let text = std::fs::read_to_string(xtask::repo_root().join("package.json")).unwrap();
    let version = package_version(&text).unwrap();
    assert!(next_version(&version, "patch").is_ok(), "{version}");
}
