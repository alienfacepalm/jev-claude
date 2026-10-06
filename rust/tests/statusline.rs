//! Port of node/test/statusline.test.mjs: runs the real `jev-statusline` binary the way Claude
//! Code does.

mod common;

use common::*;
use jev_router::jsjson::Value;
use jev_router::status::{Agent, write_decision};
use jev_router::timefmt::now_ms;
use regex::Regex;
use std::io::Write;
use std::process::{Command, Stdio};

fn main_agent() -> Agent {
    Agent { key: "main".into(), label: "main".into(), main: true }
}

fn decision(text: &str) -> jev_router::jsjson::Object {
    let mut d = obj(&json(text));
    d.insert("at", Value::Number(now_ms()));
    d
}

/// Runs the status line with this binary's status directory and returns its text without colours.
fn render(session: &str, workspace: &str, extra: &str, icons: &str) -> String {
    let dir = isolate_status();
    let mut input = json(&format!(
        r#"{{"session_id":"{session}","workspace":{{"current_dir":"/work/proj"}},"context_window":{{"used_percentage":8}}}}"#
    ));
    let ws = json(workspace);
    let o = input.as_object_mut().unwrap();
    let mut merged = o.get("workspace").unwrap().spread_of();
    merged.spread(&ws.spread_of());
    o.insert("workspace", Value::Object(merged));
    o.spread(&json(extra).spread_of());
    let mut child = Command::new(env!("CARGO_BIN_EXE_jev-statusline"))
        .env("JEV_STATUS_DIR", dir)
        .env("JEV_ICONS", icons)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&jev_router::jsjson::to_bytes(&input)).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    Regex::new("\x1b\\[[0-9;]*m").unwrap().replace_all(&text, "").trim().to_string()
}

fn sid(name: &str) -> String {
    format!("{name}-{}", std::process::id())
}

/// "symbols replace the words, and the branch uses the Powerline glyph"
#[test]
fn symbols_replace_the_words_and_the_branch_uses_the_powerline_glyph() {
    let id = sid("statusline-symbols");
    write_decision(
        &s(&id),
        &decision(r#"{"tier":"sonnet","model":"claude-sonnet-5-5","confidence":0.94,"effort":"high","reason":"jev"}"#),
        Some(&main_agent()),
    )
    .unwrap();
    let line = render(&id, "{}", r#"{"worktree":{"name":"login-fix","branch":"fix/login"}}"#, "symbols");
    assert_eq!(
        line,
        "\u{25C6} Sonnet 5.5 (94%) \u{00B7} \u{25D4} high \u{00B7} \u{2750} proj \u{00B7} \u{E0A0} fix/login \u{00B7} \u{2302} login-fix \u{00B7} \u{2261} 8%"
    );
}

/// "shows the effort the turn ran at next to the model and confidence"
#[test]
fn shows_the_effort_the_turn_ran_at() {
    let id = sid("statusline-effort");
    write_decision(
        &s(&id),
        &decision(r#"{"tier":"sonnet","model":"claude-sonnet-5-5","confidence":0.94,"effort":"high","reason":"jev"}"#),
        Some(&main_agent()),
    )
    .unwrap();
    assert_eq!(
        render(&id, "{}", "{}", "text"),
        "model Sonnet 5.5 (94%) \u{00B7} effort high \u{00B7} dir proj \u{00B7} ctx 8%"
    );
}

/// "shows a higher effort when Claude Code asked for one"
#[test]
fn shows_a_higher_effort_when_claude_code_asked_for_one() {
    let id = sid("statusline-xhigh");
    write_decision(
        &s(&id),
        &decision(r#"{"tier":"opus","model":"claude-opus-5-5","confidence":0.91,"effort":"xhigh","reason":"jev"}"#),
        Some(&main_agent()),
    )
    .unwrap();
    assert!(render(&id, "{}", "{}", "text").starts_with("model Opus 5.5 (91%) \u{00B7} effort xhigh \u{00B7} "));
}

/// "says nothing about effort for Haiku, which takes none"
#[test]
fn says_nothing_about_effort_for_haiku() {
    let id = sid("statusline-haiku");
    write_decision(
        &s(&id),
        &decision(
            r#"{"tier":"haiku","model":"claude-haiku-4-5-20251001","confidence":0.97,"effort":null,"reason":"jev"}"#,
        ),
        Some(&main_agent()),
    )
    .unwrap();
    let line = render(&id, "{}", "{}", "text");
    assert!(line.starts_with("model Haiku 4.5 (97%) \u{00B7} dir proj"), "{line}");
    assert!(!line.contains("effort"));
}

/// "a session recorded before effort was tracked still renders"
#[test]
fn a_session_recorded_before_effort_was_tracked_still_renders() {
    let id = sid("statusline-old");
    write_decision(
        &s(&id),
        &decision(r#"{"tier":"sonnet","model":"claude-sonnet-5-5","confidence":0.8,"reason":"jev"}"#),
        Some(&main_agent()),
    )
    .unwrap();
    let line = render(&id, "{}", "{}", "text");
    assert!(line.starts_with("model Sonnet 5.5 (80%)"));
    assert!(!line.contains("effort"));
}

/// "inside a worktree, the branch and the worktree are each named"
#[test]
fn inside_a_worktree_the_branch_and_the_worktree_are_each_named() {
    let line =
        render(&sid("statusline-worktree"), "{}", r#"{"worktree":{"name":"login-fix","branch":"fix/login"}}"#, "text");
    assert!(
        line.ends_with(" \u{00B7} dir proj \u{00B7} branch fix/login \u{00B7} worktree login-fix \u{00B7} ctx 8%"),
        "{line}"
    );
}

/// "a worktree named like the directory is not said twice"
#[test]
fn a_worktree_named_like_the_directory_is_not_said_twice() {
    let line =
        render(&sid("statusline-samename"), r#"{"current_dir":"/work/COR-1","git_worktree":"COR-1"}"#, "{}", "text");
    assert!(line.ends_with(" \u{00B7} worktree COR-1 \u{00B7} ctx 8%"), "{line}");
    assert!(!line.contains("dir"));
}

/// "a long branch name is cut with an ellipsis"
#[test]
fn a_long_branch_name_is_cut_with_an_ellipsis() {
    let extra = r#"{"worktree":{"name":"wt","branch":"COR-1263/multi-edit-inspection-sync"}}"#;
    let line = render(&sid("statusline-longbranch"), "{}", extra, "text");
    assert!(line.contains(" \u{00B7} branch COR-1263/multi-edit-inspect\u{2026} \u{00B7} "), "{line}");
    assert!(!line.contains("inspection-sync"));
}

/// "a linked worktree whose branch cannot be read still names the worktree"
#[test]
fn a_linked_worktree_whose_branch_cannot_be_read_still_names_the_worktree() {
    // /work/proj is not a repository, so there is no branch to look up.
    let line = render(&sid("statusline-linked"), r#"{"git_worktree":"scratch"}"#, "{}", "text");
    assert!(line.contains(" \u{00B7} dir proj \u{00B7} worktree scratch \u{00B7} "), "{line}");
    assert!(!line.contains("branch"));
}

/// "the main working tree shows its branch and no worktree"
#[test]
fn the_main_working_tree_shows_its_branch_and_no_worktree() {
    let dir = temp_dir("jev-statusline-repo-");
    let dir = jev_router::osdirs::simplify(std::fs::canonicalize(&dir).unwrap());
    let ok = Command::new("git").args(["init", "-q", "-b", "main-line"]).current_dir(&dir).status().unwrap();
    assert!(ok.success());
    let ws = jev_router::jsjson::to_bytes(&Value::Object({
        let mut o = jev_router::jsjson::Object::new();
        o.insert("current_dir", s(&dir.to_string_lossy()));
        o
    }));
    let line = render(&sid("statusline-main"), &String::from_utf8(ws).unwrap(), "{}", "text");
    assert!(line.ends_with(" \u{00B7} branch main-line \u{00B7} ctx 8%"), "{line}");
    assert!(!line.contains("worktree"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// "a directory that is not a git checkout shows neither a branch nor a worktree"
#[test]
fn a_directory_that_is_not_a_git_checkout_shows_neither() {
    let line = render(&sid("statusline-nogit"), "{}", "{}", "text");
    assert!(!line.contains("branch") && !line.contains("worktree"), "{line}");
}

/// "a space separates the sub-agents symbol from the first model name"
#[test]
fn a_space_separates_the_sub_agents_symbol_from_the_first_model_name() {
    let id = sid("statusline-agents");
    write_decision(
        &s(&id),
        &decision(r#"{"tier":"sonnet","model":"claude-sonnet-5-5","confidence":0.94,"effort":"high","reason":"jev"}"#),
        Some(&main_agent()),
    )
    .unwrap();
    write_decision(
        &s(&id),
        &decision(r#"{"tier":"haiku","model":"claude-haiku-4-5-20251001","confidence":0.9,"reason":"jev"}"#),
        Some(&Agent { key: "a1".into(), label: "a1".into(), main: false }),
    )
    .unwrap();
    let line = render(&id, "{}", "{}", "symbols");
    assert!(line.contains("\u{2726} Haiku 4.5"), "{line}");
}

/// Not a Node test: Node throws reading `session_id` of a JSON `null` on stdin, so the status
/// line writes an error to stderr, nothing to stdout, and exits 1.
#[test]
fn a_json_null_on_stdin_fails_as_node_does() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_jev-statusline"))
        .env("JEV_STATUS_DIR", isolate_status())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"null").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty(), "nothing on stdout");
    assert!(String::from_utf8_lossy(&out.stderr).contains("session_id"), "an error on stderr");
}
