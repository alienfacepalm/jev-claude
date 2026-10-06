//! Port of node/test/settings.test.mjs.

mod common;

use common::*;
use jev_router::jsjson::{self, Value};
use jev_router::settings::{read_saved_model, restore_saved_model};
use std::path::{Path, PathBuf};

fn file_with(settings: &str) -> PathBuf {
    let file = temp_dir("jev-settings-").join("settings.json");
    let pretty = jsjson::stringify_pretty(&json(settings)).unwrap();
    std::fs::write(&file, pretty.as_bytes()).unwrap();
    file
}

fn model_in(file: &Path) -> Value {
    json(&std::fs::read_to_string(file).unwrap()).get("model").clone()
}

fn memo_file() -> PathBuf {
    temp_dir("jev-memo-").join("saved-model.json")
}

/// "reads the saved model, ignoring a leftover sentinel"
#[test]
fn reads_the_saved_model_ignoring_a_leftover_sentinel() {
    isolate_status();
    assert_eq!(read_saved_model(&file_with(r#"{"model":"opus"}"#), &memo_file()), s("opus"));
    assert_eq!(read_saved_model(&file_with(r#"{"model":"jev-router"}"#), &memo_file()), Value::Undefined);
    assert_eq!(read_saved_model(&file_with("{}"), &memo_file()), Value::Undefined);
    let missing = std::env::temp_dir().join("does-not-exist.json");
    assert_eq!(read_saved_model(&missing, &memo_file()), Value::Undefined);
}

/// "a sentinel left by a killed session resolves to the model from before it"
#[test]
fn a_sentinel_left_by_a_killed_session_resolves_to_the_model_from_before_it() {
    isolate_status();
    let memo = memo_file();
    assert_eq!(read_saved_model(&file_with(r#"{"model":"claude-opus-4-6"}"#), &memo), s("claude-opus-4-6"));
    // That session died with the sentinel saved; the next run must not treat it as "no model".
    let file = file_with(r#"{"model":"jev-router"}"#);
    let previous = read_saved_model(&file, &memo);
    assert_eq!(previous, s("claude-opus-4-6"));
    assert!(restore_saved_model(&previous, &file));
    assert_eq!(model_in(&file), s("claude-opus-4-6"));
}

/// "restores the previous model when the sentinel was saved"
#[test]
fn restores_the_previous_model_when_the_sentinel_was_saved() {
    let file = file_with(r#"{"model":"jev-router","permissions":{"deny":["Bash(rm*)"]}}"#);
    assert!(restore_saved_model(&s("opus"), &file));
    assert_eq!(model_in(&file), s("opus"));
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(json(&text).get("permissions"), &json(r#"{"deny":["Bash(rm*)"]}"#));
    assert!(text.ends_with("}\n"), "written indented with a trailing newline");
}

/// "removes the sentinel when there was no previous model"
#[test]
fn removes_the_sentinel_when_there_was_no_previous_model() {
    let file = file_with(r#"{"model":"jev-router"}"#);
    assert!(restore_saved_model(&Value::Undefined, &file));
    assert_eq!(model_in(&file), Value::Undefined);
}

/// "leaves a real model the user chose during the session alone"
#[test]
fn leaves_a_real_model_the_user_chose_alone() {
    let file = file_with(r#"{"model":"claude-opus-4-6"}"#);
    assert!(!restore_saved_model(&s("sonnet"), &file));
    assert_eq!(model_in(&file), s("claude-opus-4-6"));
}

/// "a missing or unreadable settings file is not an error"
#[test]
fn a_missing_or_unreadable_settings_file_is_not_an_error() {
    assert!(!restore_saved_model(&s("opus"), &std::env::temp_dir().join("nope").join("settings.json")));
}
