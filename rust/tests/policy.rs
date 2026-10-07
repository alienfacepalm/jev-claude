//! Port of node/test/policy.test.mjs.

// Each test's doc comment is the Node test title, quoted verbatim so the two suites can be
// compared line by line; Markdown backticks would change the titles.
#![allow(clippy::doc_markdown)]

mod common;

use common::*;
use jev_router::config::{available_tiers, questions, should_use_exact_model};
use jev_router::envx::{EnvMap, map};
use jev_router::jsjson::Value;
use jev_router::policy::{Decision, decide, detect_override, detect_override_str};

const ALL: [&str; 4] = ["haiku", "sonnet", "opus", "fable"];

fn jev(choice: &str, confidence: f64) -> Value {
    json(&format!(r#"{{"choice":"{choice}","confidence":{confidence}}}"#))
}
fn sure(choice: &str) -> Value {
    jev(choice, 0.95)
}
fn unsure(choice: &str) -> Value {
    jev(choice, 0.2)
}

fn run(prompt: &str, current: &str, available: &[&str], jev: Option<&Value>, ctx: f64) -> Decision {
    decide(&s(prompt), jev, current, available, ctx)
}

fn base(current: &str, jev: Option<&Value>) -> Decision {
    run("refactor the parser", current, &ALL, jev, 0.0)
}

/// "score rubrics contain only API-valid descriptions"
#[test]
fn score_rubrics_contain_only_api_valid_descriptions() {
    for (_, q) in questions() {
        if q.get("type").is_str("score") {
            let criteria = q.get("criteria").as_array().unwrap();
            assert!(criteria.iter().all(|d| d.as_str().is_some()));
            assert!(criteria.len() <= 10);
        }
    }
}

/// "follows a confident Jev answer"
#[test]
fn follows_a_confident_jev_answer() {
    assert_eq!(
        base("sonnet", Some(&sure("opus"))),
        Decision { tier: "opus".into(), reason: "jev".into(), changed: true }
    );
}

/// "an explicit user override beats Jev"
#[test]
fn an_explicit_user_override_beats_jev() {
    let out = run("use haiku to fix this typo", "sonnet", &ALL, Some(&sure("opus")), 0.0);
    assert_eq!(out.tier, "haiku");
    assert_eq!(out.reason, "override");
}

/// "a sub-agent report quoting an override phrase does not force a model"
#[test]
fn a_sub_agent_report_quoting_an_override_phrase_does_not_force_a_model() {
    let prompt =
        std::fs::read_to_string(repo_root().join("conformance/fixtures/subagent-handback-prompt.txt")).unwrap();
    assert_eq!(detect_override(&s(&prompt)), None);
    let out = run(&prompt, "opus", &ALL, Some(&sure("sonnet")), 0.0);
    assert_eq!(out.tier, "sonnet", "Jev's confident answer is acted on, not the quoted phrase");
    assert_eq!(out.reason, "jev");
}

/// "detectOverride only fires on a real instruction"
#[test]
fn detect_override_only_fires_on_a_real_instruction() {
    assert_eq!(detect_override_str("switch to opus"), Some("opus"));
    assert_eq!(detect_override_str("use haiku"), Some("haiku"));
    assert_eq!(detect_override_str("use the strong model"), Some("opus"));
    assert_eq!(detect_override_str("Use Claude Haiku for this one"), Some("haiku"));
    assert_eq!(detect_override_str("the opus of his career"), None);
}

/// "detectOverride ignores ordinary prose that mentions a tier word"
#[test]
fn detect_override_ignores_ordinary_prose() {
    for prompt in [
        "help me with fast fourier transform code",
        "replace the polling loop with long polling",
        "the test only fails on fast CI runners",
        "write tests with long input strings",
        "turn on fast refresh in vite",
        "refactor this to rely on strong typing",
        "use haiku-style commit messages",
        "use long variable names",
    ] {
        assert_eq!(detect_override_str(prompt), None, "{prompt}");
    }
}

/// "a tier named in a negated instruction is not a request for it"
#[test]
fn a_tier_named_in_a_negated_instruction_is_not_a_request_for_it() {
    for prompt in [
        "do not use fable",
        "never use fable for this",
        "please don't switch to fable",
        "we cannot use opus on this account",
        "Don't ever use Opus here",
        "do NOT route to sonnet",
    ] {
        assert_eq!(detect_override_str(prompt), None, "{prompt}");
    }
}

/// "a negated tier does not hide a real instruction beside it"
#[test]
fn a_negated_tier_does_not_hide_a_real_instruction_beside_it() {
    assert_eq!(detect_override_str("don't use haiku, use opus"), Some("opus"));
    assert_eq!(detect_override_str("do not use fable. use sonnet for the tests"), Some("sonnet"));
    assert_eq!(detect_override_str("never use haiku for this but use opus"), Some("opus"));
}

/// "a tier named in a question is asking about it, not asking for it"
#[test]
fn a_tier_named_in_a_question_is_asking_about_it_not_asking_for_it() {
    assert_eq!(detect_override_str("why does the planner use opus?"), None);
    assert_eq!(detect_override_str("should we switch to haiku for the lint step?"), None);
    assert_eq!(detect_override_str("Use opus for the migration. Is that too slow?"), Some("opus"));
    assert_eq!(detect_override_str("what does the router do when I say\nuse opus"), Some("opus"));
}

/// "a negated or questioned tier lets Jev decide"
#[test]
fn a_negated_or_questioned_tier_lets_jev_decide() {
    let out = run("do not use fable", "sonnet", &ALL, Some(&sure("opus")), 0.0);
    assert_eq!(out.tier, "opus");
    assert_eq!(out.reason, "jev");
}

/// "keeps the current model when Jev is unreachable"
#[test]
fn keeps_the_current_model_when_jev_is_unreachable() {
    let out = base("sonnet", None);
    assert_eq!(out.tier, "sonnet");
    assert!(!out.changed);
    assert!(out.reason.contains("jev-unavailable"));
}

/// "ignores a tier name Jev invented"
#[test]
fn ignores_a_tier_name_jev_invented() {
    assert_eq!(base("sonnet", Some(&sure("mystery-9"))).tier, "sonnet");
}

/// "an unsure pick of Opus runs one tier lower, on the default"
#[test]
fn an_unsure_pick_of_opus_runs_one_tier_lower() {
    let out = base("haiku", Some(&unsure("opus")));
    assert_eq!(out.tier, "sonnet");
    assert_eq!(out.reason, "low-confidence-default");
}

/// "never downgrades on a low-confidence answer"
#[test]
fn never_downgrades_on_a_low_confidence_answer() {
    let out = base("sonnet", Some(&unsure("haiku")));
    assert_eq!(out.tier, "sonnet", "an unsure downgrade is not a reason to leave the default");
    assert!(out.reason.contains("low-confidence-default"));
}

/// "a middling answer is not followed down to a weaker model"
#[test]
fn a_middling_answer_is_not_followed_down() {
    let middling = base("sonnet", Some(&jev("haiku", 0.45)));
    assert_eq!(middling.tier, "sonnet");
    assert!(middling.reason.contains("low-confidence-default"));
    assert_eq!(base("sonnet", Some(&jev("haiku", 0.78))).tier, "haiku");
}

/// "an unsure answer keeps Opus when Opus is already in use"
#[test]
fn an_unsure_answer_keeps_opus_when_opus_is_in_use() {
    let out = base("opus", Some(&unsure("haiku")));
    assert_eq!(out.tier, "opus");
    assert!(out.reason.contains("no-change"));
}

/// "keeps a tier stronger than the default on a low-confidence answer"
#[test]
fn keeps_a_tier_stronger_than_the_default() {
    let out = base("fable", Some(&unsure("haiku")));
    assert_eq!(out.tier, "fable");
    assert!(out.reason.contains("no-change"));
}

/// "an answer without a confidence is treated as unsure"
#[test]
fn an_answer_without_a_confidence_is_treated_as_unsure() {
    let out = base("haiku", Some(&json(r#"{"choice":"haiku"}"#)));
    assert_eq!(out.tier, "sonnet");
    assert_eq!(out.reason, "low-confidence-default");
}

/// "an unsure answer runs one tier below its pick"
#[test]
fn an_unsure_answer_runs_one_tier_below_its_pick() {
    assert_eq!(base("sonnet", Some(&unsure("opus"))).tier, "sonnet");
    assert_eq!(base("sonnet", Some(&unsure("fable"))).tier, "opus");
    assert_eq!(base("haiku", Some(&unsure("sonnet"))).tier, "sonnet", "never below the default");
}

/// "a low-confidence answer cannot reach fable"
#[test]
fn a_low_confidence_answer_cannot_reach_fable() {
    let out = base("haiku", Some(&unsure("fable")));
    assert_eq!(out.tier, "opus", "one step below fable, never fable itself");
    assert_eq!(out.reason, "low-confidence-default");
}

/// "still allows a confident upgrade to fable"
#[test]
fn still_allows_a_confident_upgrade_to_fable() {
    assert_eq!(base("sonnet", Some(&sure("fable"))).tier, "fable");
}

/// "refuses a downgrade once the cache rebuild costs more than it saves"
#[test]
fn refuses_a_downgrade_once_the_cache_rebuild_costs_more() {
    let out = run("refactor the parser", "opus", &ALL, Some(&sure("haiku")), 80000.0);
    assert_eq!(out.tier, "opus");
    assert!(out.reason.contains("cache-rebuild"));
}

/// "allows the same downgrade early in a conversation"
#[test]
fn allows_the_same_downgrade_early_in_a_conversation() {
    assert_eq!(base("opus", Some(&sure("haiku"))).tier, "haiku");
}

/// "substitutes upward when the chosen tier is unavailable"
#[test]
fn substitutes_upward_when_the_chosen_tier_is_unavailable() {
    let out = run("refactor the parser", "haiku", &["haiku", "opus"], Some(&sure("sonnet")), 0.0);
    assert_eq!(out.tier, "opus");
    assert!(out.reason.contains("unavailable"));
}

/// "never substitutes upward into paid fable"
#[test]
fn never_substitutes_upward_into_paid_fable() {
    let out = run("refactor the parser", "haiku", &["haiku", "fable"], Some(&sure("opus")), 0.0);
    assert_eq!(out.tier, "haiku");
}

/// "accepts exact model changes within the same tier"
#[test]
fn accepts_exact_model_changes_within_the_same_tier() {
    assert!(should_use_exact_model("jev/no-change", Some("opus"), "opus"));
    assert!(!should_use_exact_model("low-confidence-default/no-change", Some("opus"), "opus"));
}

/// "fable is on offer by default and can be switched off"
#[test]
fn fable_is_on_offer_by_default_and_can_be_switched_off() {
    assert!(available_tiers(&EnvMap::new()).contains(&"fable"));
    assert!(available_tiers(&map(&[("JEV_ALLOW_FABLE", "1")])).contains(&"fable"));
    for off in ["0", "false", "No", " off "] {
        assert!(!available_tiers(&map(&[("JEV_ALLOW_FABLE", off)])).contains(&"fable"), "{off}");
    }
    assert_eq!(available_tiers(&map(&[("JEV_ALLOW_FABLE", "0")])), ["haiku", "sonnet", "opus"]);
}
