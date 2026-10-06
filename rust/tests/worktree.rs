//! Port of node/test/worktree.test.mjs.

mod common;

use common::*;
use jev_router::jsjson::Value;
use jev_router::jsstr::JsStr;
use jev_router::worktree::{Location, git_branch, location_info};
use std::cell::RefCell;
use std::process::Command;

fn never(_: &Value) -> Option<JsStr> {
    panic!("the branch should not have been looked up")
}

fn loc(branch: &str, worktree: Value) -> Option<Location> {
    Some(Location { branch: s(branch), worktree })
}

/// "outside a git checkout there is nothing to show"
#[test]
fn outside_a_git_checkout_there_is_nothing_to_show() {
    assert_eq!(location_info(&json(r#"{"workspace":{"current_dir":"/nowhere"}}"#), &|_| None), None);
    assert_eq!(location_info(&json("{}"), &|_| None), None);
    assert_eq!(location_info(&Value::Undefined, &|_| None), None);
}

/// "the main working tree has a branch and no worktree"
#[test]
fn the_main_working_tree_has_a_branch_and_no_worktree() {
    let input = json(r#"{"workspace":{"current_dir":"/repo"}}"#);
    let lookup = |dir: &Value| dir.is_str("/repo").then(|| JsStr::from("master"));
    assert_eq!(location_info(&input, &lookup), loc("master", Value::Null));
}

/// "a worktree session carries its own name and branch, with no git call"
#[test]
fn a_worktree_session_carries_its_own_name_and_branch() {
    let input = json(
        r#"{"worktree":{"name":"my-feature","branch":"worktree-my-feature","path":"/r/.claude/worktrees/my-feature"}}"#,
    );
    assert_eq!(location_info(&input, &never), loc("worktree-my-feature", s("my-feature")));
}

/// "a linked worktree has only a name, so the branch is read from git in the current directory"
#[test]
fn a_linked_worktree_reads_its_branch_from_git() {
    let asked = RefCell::new(Vec::new());
    let input = json(r#"{"workspace":{"current_dir":"/wt/feature-xyz","git_worktree":"feature-xyz"}}"#);
    let lookup = |dir: &Value| {
        asked.borrow_mut().push(dir.clone());
        Some(JsStr::from("feature/xyz"))
    };
    assert_eq!(location_info(&input, &lookup), loc("feature/xyz", s("feature-xyz")));
    assert_eq!(asked.into_inner(), vec![s("/wt/feature-xyz")]);
}

/// "a worktree session without a branch (hook-based) falls back to git"
#[test]
fn a_worktree_session_without_a_branch_falls_back_to_git() {
    let input = json(r#"{"worktree":{"name":"scratch","path":"/wt/scratch"}}"#);
    let lookup = |dir: &Value| dir.is_str("/wt/scratch").then(|| JsStr::from("main-2"));
    assert_eq!(location_info(&input, &lookup), loc("main-2", s("scratch")));
}

/// "a detached HEAD in a worktree still names the worktree"
#[test]
fn a_detached_head_in_a_worktree_still_names_the_worktree() {
    let input = json(r#"{"workspace":{"current_dir":"/wt/x","git_worktree":"x"}}"#);
    assert_eq!(location_info(&input, &|_| Some(JsStr::new())), loc("", s("x")));
}

/// "gitBranch: a branch, a detached HEAD, and no checkout"
#[test]
fn git_branch_a_branch_a_detached_head_and_no_checkout() {
    let dir = temp_dir("jev-worktree-test-");
    let dir_value = s(&dir.to_string_lossy());
    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    };
    assert_eq!(git_branch(&dir_value), None, "not a repository");
    git(&["init", "-q", "-b", "topic/a"]);
    assert_eq!(git_branch(&dir_value), Some(JsStr::from("topic/a")), "works before the first commit");
    git(&["commit", "-q", "--allow-empty", "-m", "x"]);
    git(&["checkout", "-q", "--detach"]);
    assert_eq!(git_branch(&dir_value), Some(JsStr::new()), "detached");
    assert_eq!(git_branch(&Value::Undefined), None);
    assert_eq!(git_branch(&s(&dir.join("missing").to_string_lossy())), None);
    let _ = std::fs::remove_dir_all(&dir);
}
