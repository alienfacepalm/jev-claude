//! The read-only setup report behind /jev-calibrate (SPEC 13): the real `jev-check` program, run
//! with a scratch home and status directory. SPEC 13 leaves the rows' wording to this edition, so
//! these tests are what pins it.

// Each test's doc comment is its title as a plain sentence; Markdown backticks would change them.
#![allow(clippy::doc_markdown)]

mod common;

use common::*;
use jev_router::config::TIERS;
use std::process::Command;

/// Width of the label column.
const LABEL: usize = 13;

/// Runs jev-check with `calibration` (JSON text, or None for no file) and extra variables.
fn report(calibration: Option<&str>, env: &[(&str, &str)]) -> String {
    let base = temp_dir("jev-check-");
    let (home, status, root) = (base.join("home"), base.join("status"), base.join("root"));
    for dir in [&home, &status, &root] {
        std::fs::create_dir(dir).unwrap();
    }
    if let Some(text) = calibration {
        std::fs::write(status.join("calibration.json"), text).unwrap();
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_jev-check"));
    command
        .current_dir(&home)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("JEV_STATUS_DIR", &status)
        .env("JEV_ROOT", &root);
    for name in [
        "ANTHROPIC_CUSTOM_MODEL_OPTION",
        "ANTHROPIC_MODEL",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_API_KEY",
        "JEV_ALLOW_FABLE",
    ] {
        command.env_remove(name);
    }
    for (k, v) in env {
        command.env(k, v);
    }
    let out = command.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// The text of the row labelled `label` (the first one; "" for a continuation row).
fn row<'a>(report: &'a str, label: &str) -> &'a str {
    let prefix = format!("{label:<LABEL$}");
    report
        .lines()
        .find(|l| l.starts_with(&prefix) && (label.is_empty() || l.len() > LABEL))
        .and_then(|l| l.get(LABEL..))
        .unwrap_or_else(|| panic!("no {label:?} row in:\n{report}"))
}

fn has_row(report: &str, label: &str) -> bool {
    report.lines().any(|l| l.starts_with(&format!("{label:<LABEL$}")))
}

fn tuned_for() -> String {
    TIERS.iter().map(|t| format!("{} {}", t.name, t.id)).collect::<Vec<_>>().join(", ")
}

const NOT_READ: &str = "not read yet - Claude Code has not loaded the model list. Run /jev-calibrate again shortly.";

/// No calibration file says "not read yet"
#[test]
fn no_calibration_says_not_read_yet() {
    for calibration in [None, Some("null"), Some(r#"{"models":["claude-opus-5-5"],"newer":[]}"#)] {
        let r = report(calibration, &[]);
        assert_eq!(r.lines().next(), Some("Jev Router setup check (read-only: nothing was changed)"));
        assert_eq!(row(&r, "Your account"), NOT_READ, "{calibration:?}");
        assert!(!has_row(&r, "Newer models"), "{calibration:?}:\n{r}");
        assert_eq!(row(&r, "Tuned for"), tuned_for());
        assert!(r.ends_with('\n'));
    }
}

/// Null models join as empty, never as "null"
#[test]
fn null_models_join_as_empty() {
    let r = report(Some(r#"{"models":[null,"claude-opus-4-1"],"newer":[null,"claude-opus-6"],"at":0}"#), &[]);
    assert_eq!(row(&r, "Your account"), ", claude-opus-4-1 (as of 1970-01-01 00:00:00 UTC)");
    assert!(!r.contains("null"), "{r}");
    assert!(row(&r, "Newer models").starts_with(", claude-opus-6 - routing already uses them"), "{r}");
    assert_eq!(row(&r, ""), "not offered to this account: haiku, sonnet, fable (routing steps around them)");
}

/// The Newer models row points at the installer
#[test]
fn the_newer_models_row_points_at_the_installer() {
    let r = report(Some(r#"{"newer":["claude-test-9"],"models":["claude-test-9"],"at":1}"#), &[]);
    let line = row(&r, "Newer models");
    assert_eq!(
        line,
        "claude-test-9 - routing already uses them, but the router was tuned on the versions before. \
         Run the jev-claude installer again to get tuning for them."
    );
    assert!(!line.contains("jev-router"), "jev-router on npm is another project: {line}");
}

/// A full list has no "not offered" row and no newer models
#[test]
fn a_full_list_has_no_not_offered_row_and_no_newer_models() {
    let models: Vec<String> = TIERS.iter().map(|t| format!("\"{}\"", t.id)).collect();
    let r = report(Some(&format!(r#"{{"models":[{}],"newer":[],"at":0}}"#, models.join(","))), &[]);
    let ids: Vec<&str> = TIERS.iter().map(|t| t.id).collect();
    assert_eq!(row(&r, "Your account"), format!("{} (as of 1970-01-01 00:00:00 UTC)", ids.join(", ")));
    assert!(!r.contains("not offered"), "{r}");
    assert_eq!(row(&r, "Newer models"), "none - the router is tuned for the newest models your account offers");

    let r = report(Some(r#"{"models":[],"newer":[],"at":0}"#), &[]);
    assert_eq!(row(&r, "Your account"), "no Claude models listed (as of 1970-01-01 00:00:00 UTC)");
    assert_eq!(row(&r, ""), "not offered to this account: haiku, sonnet, opus, fable (routing steps around them)");
}

/// The Routing row: off without the custom model, on with it, pinned by ANTHROPIC_MODEL
#[test]
fn routing_rows() {
    let r = report(None, &[]);
    assert!(row(&r, "Routing").starts_with("off - no JEV_API_KEY found"), "{r}");
    let on = "on - Jev Router picks a model for each turn";
    assert_eq!(row(&report(None, &[("ANTHROPIC_CUSTOM_MODEL_OPTION", "jev-router")]), "Routing"), on);
    let r = report(None, &[("ANTHROPIC_CUSTOM_MODEL_OPTION", "jev-router"), ("ANTHROPIC_MODEL", "jev-router")]);
    assert_eq!(row(&r, "Routing"), on);
    let r = report(None, &[("ANTHROPIC_CUSTOM_MODEL_OPTION", "jev-router"), ("ANTHROPIC_MODEL", "claude-opus-5-5")]);
    assert_eq!(
        row(&r, "Routing"),
        "available, but this session started on claude-opus-5-5 because ANTHROPIC_MODEL is set. Choose Jev Router in /model to route."
    );
}

/// The Claude row: sign-in, API key, and the auth token winning over both
#[test]
fn claude_rows() {
    assert_eq!(row(&report(None, &[]), "Claude"), "your Claude Code sign-in");
    let r = report(None, &[("ANTHROPIC_API_KEY", "x")]);
    assert!(row(&r, "Claude").starts_with("API key (ANTHROPIC_API_KEY), billed per token"), "{r}");
    let r = report(None, &[("ANTHROPIC_AUTH_TOKEN", "x"), ("ANTHROPIC_API_KEY", "x")]);
    assert_eq!(row(&r, "Claude"), "auth token (ANTHROPIC_AUTH_TOKEN)");
}

/// The Fable row follows JEV_ALLOW_FABLE
#[test]
fn fable_rows() {
    assert!(row(&report(None, &[]), "Fable").starts_with("offered when the work calls for it"));
    for off in ["0", " Off "] {
        assert_eq!(
            row(&report(None, &[("JEV_ALLOW_FABLE", off)]), "Fable"),
            "off (JEV_ALLOW_FABLE is set to turn it off)"
        );
    }
}

/// An empty root folder is an installed copy
#[test]
fn an_empty_jev_root_folder_reports_installed() {
    let r = report(None, &[]);
    assert_eq!(row(&r, "Mode"), "installed");
}
