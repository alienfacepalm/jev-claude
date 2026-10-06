//! Port of node/test/explain.test.mjs.

mod common;

use common::*;
use jev_router::explain::format_explanation;
use jev_router::reasons::{long_reason, short_reason};
use regex::Regex;

/// "formats the last routing decision"
#[test]
fn formats_the_last_routing_decision() {
    let output = format_explanation(&json(
        r#"{
          "prompt": "Explain the router architecture", "tier": "sonnet", "confidence": 0.94, "reason": "jev",
          "jev": {
            "request": { "state": { "session": { "current_model": "haiku", "context_tokens": 6200 } } },
            "response": { "answers": { "model": { "choice": "claude-sonnet-5-5", "confidence": 0.94 } } }
          },
          "metrics": { "taskComplexity": 0.82, "reasoningRequired": 0.91, "toolComplexity": 0.64, "contextSize": 0.31 }
        }"#,
    ));
    for needle in [
        "Task complexity     0.82",
        "Prompt: Explain the router",
        "Current model: HAIKU",
        "Context tokens: 6200",
        "Recommended tier: SONNET",
        "Selected model: SONNET",
        "Confidence: 94%",
    ] {
        assert!(output.contains(needle), "{needle} in\n{output}");
    }
    // The sentence wraps inside the box rather than being cut at its edge.
    assert!(Regex::new("Decision: the router's\\s*\u{2502}\n\u{2502} recommendation").unwrap().is_match(&output));
}

/// "shows Jev's own pick when policy overruled it"
#[test]
fn shows_jevs_own_pick_when_policy_overruled_it() {
    let output = format_explanation(&json(
        r#"{ "tier": "opus", "model": "claude-opus-5-5", "confidence": 0.97,
             "reason": "downgrade-not-worth-cache-rebuild/no-change",
             "jev": { "response": { "answers": { "model": { "choice": "claude-haiku-4-5-20251001" } } } } }"#,
    ));
    assert!(output.contains("Recommended tier: HAIKU"));
    assert!(output.contains("Selected model: CLAUDE-OPUS-5-5"));
}

/// "reads the recommendation from sessions recorded before the rename"
#[test]
fn reads_the_recommendation_from_sessions_recorded_before_the_rename() {
    let old =
        json(r#"{ "tier": "opus", "jev": { "response": { "answers": { "model_tier": { "choice": "sonnet" } } } } }"#);
    assert!(format_explanation(&old).contains("Recommended tier: SONNET"));
}

/// "Claude skill pre-approves its read-only explanation command"
#[test]
fn claude_skill_pre_approves_its_read_only_explanation_command() {
    let skill = std::fs::read_to_string(repo_root().join(".claude/skills/jev-explain/SKILL.md")).unwrap();
    assert!(skill.lines().any(|l| l.trim_end() == "allowed-tools: Bash(node *)"));
}

/// "says a held decision in words a person reads, not the reason code"
#[test]
fn says_a_held_decision_in_words_a_person_reads() {
    assert_eq!(short_reason(Some("downgrade-not-worth-cache-rebuild/no-change")), Some("keeping the cache"));
    assert!(long_reason(Some("downgrade-not-worth-cache-rebuild")).contains("re-read the whole conversation"));
    assert_eq!(short_reason(Some("jev-unavailable")), Some("router offline"));
    assert_eq!(short_reason(Some("jev+unavailable")), Some("nearest available"));
}

/// "the status line stays quiet where the reason is obvious or not actionable"
#[test]
fn the_status_line_stays_quiet_where_the_reason_is_obvious() {
    assert_eq!(short_reason(Some("low-confidence-default")), None);
    assert_eq!(short_reason(Some("override")), None);
    assert!(long_reason(Some("low-confidence-default")).contains("unsure"));
    assert!(long_reason(Some("override")).contains("named this model"));
}

/// "an ordinary recommendation adds nothing to the status line"
#[test]
fn an_ordinary_recommendation_adds_nothing_to_the_status_line() {
    assert_eq!(short_reason(Some("jev")), None);
    assert_eq!(short_reason(Some("jev/no-change")), None);
    assert_eq!(long_reason(Some("jev")), "the router's recommendation");
}
