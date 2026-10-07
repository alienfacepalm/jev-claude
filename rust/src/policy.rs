//! Turns a Jev answer into the tier that runs (SPEC 7.6; `node/src/policy.mjs`).

use crate::config::{
    DOWNGRADE_MAX_CONTEXT_TOKENS, MIN_CONFIDENCE, OVERRIDE_PATTERNS, TIER_NAMES, UNCERTAIN_DEFAULT, rank_of,
};
use crate::jsjson::Value;
use crate::jsstr::{JSWS, JsStr, find_without_word_after};
use regex::bytes::Regex;
use std::sync::LazyLock;

/// A negated instruction verb (SPEC 7.6): `not use`, `don't switch to`, `never ever route to`.
/// ASCII-only `\b` and case folding (3.1); `\s` is the JSWS class.
fn negated_verb() -> Regex {
    let pattern = format!(
        r"(?:(?-u:\b)(?i-u:not|cannot)|(?i-u:n)['\x{{2019}}](?i-u:t)|(?-u:\b)(?i-u:never|no|avoid|without|dont)){JSWS}+(?:(?i-u:ever|really|actually|just|simply){JSWS}+)?(?i-u:use|switch to|switch over to|route to)"
    );
    Regex::new(&pattern).unwrap()
}

static OWN_WORDS: LazyLock<[Regex; 7]> = LazyLock::new(|| {
    [
        Regex::new(r"<agent-message(?s-u:.)*?</agent-message>").unwrap(),
        Regex::new(r"<system-reminder>(?s-u:.)*?</system-reminder>").unwrap(),
        Regex::new(r"```(?s-u:.)*?```").unwrap(),
        Regex::new(r"`(?-u:[^`\n])*`").unwrap(),
        Regex::new(r#""(?-u:[^"\n])*""#).unwrap(),
        negated_verb(),
        // A sentence that ends in a question mark: from the previous `.`, `!`, `?` or line break.
        Regex::new(r"(?-u:[^.!?\n])*\?").unwrap(),
    ]
});

/// The part of a prompt the user wrote themselves: `String(prompt ?? "")` with quoted and
/// injected text, negated instruction verbs and questions replaced by a space.
pub fn own_words(prompt: &Value) -> JsStr {
    let mut text = if prompt.is_nullish() { JsStr::new() } else { prompt.to_js_string() };
    for re in OWN_WORDS.iter() {
        text = text.replace_all(re, " ");
    }
    text
}

/// The tier the user named explicitly in the prompt, or None.
pub fn detect_override(prompt: &Value) -> Option<&'static str> {
    let text = own_words(prompt);
    OVERRIDE_PATTERNS.iter().find(|p| find_without_word_after(&p.re, text.as_bytes()).is_some()).map(|p| p.tier)
}

/// [`detect_override`] for a Rust string.
pub fn detect_override_str(prompt: &str) -> Option<&'static str> {
    detect_override(&Value::from(prompt))
}

/// Nearest tier the account can run.
pub fn clamp_to_available(tier: &str, available: &[&str]) -> Option<&'static str> {
    if let Some(t) = TIER_NAMES.iter().find(|t| **t == tier && available.contains(t)) {
        return Some(t);
    }
    let rank = rank_of(tier);
    let up = TIER_NAMES
        .iter()
        .enumerate()
        .find(|(i, t)| (*i as i32) > rank && available.contains(t) && (**t != "fable" || tier == "fable"));
    if let Some((_, t)) = up {
        return Some(t);
    }
    TIER_NAMES
        .iter()
        .enumerate()
        .filter(|(i, t)| (*i as i32) < rank && available.contains(t))
        .map(|(_, t)| *t)
        .next_back()
}

#[derive(Debug, Clone, PartialEq)]
/// What policy decided for a turn.
pub struct Decision {
    /// The tier the turn runs on.
    pub tier: String,
    /// Why, as the status line and decision log show it (for example `jev` or `override`).
    pub reason: String,
    /// Whether the tier differs from the previous turn's.
    pub changed: bool,
}

/// `decide({prompt, jev, current, available, contextTokens})`.
pub fn decide(prompt: &Value, jev: Option<&Value>, current: &str, available: &[&str], context_tokens: f64) -> Decision {
    let settle = |tier: &str, reason: &str| {
        let fin = clamp_to_available(tier, available).map_or(current.to_string(), str::to_string);
        let why = if fin == tier { reason.to_string() } else { format!("{reason}+unavailable") };
        let reason = if fin == current { format!("{why}/no-change") } else { why };
        Decision { changed: fin != current, tier: fin, reason }
    };

    if let Some(t) = detect_override(prompt) {
        return settle(t, "override");
    }
    let choice = jev.and_then(|j| j.get("choice").as_str()).and_then(|c| TIER_NAMES.iter().find(|t| *c == **t));
    let (Some(jev), Some(target)) = (jev, choice) else {
        return settle(current, "jev-unavailable");
    };
    // `!(confidence >= 0.6)`: NaN and anything non-numeric count as unsure.
    let confidence = jev.get("confidence").to_number();
    let sure = confidence >= MIN_CONFIDENCE;
    if !sure {
        let step = (rank_of(target) - 1).max(rank_of(UNCERTAIN_DEFAULT)).max(rank_of(current));
        return settle(TIER_NAMES[step as usize], "low-confidence-default");
    }
    if rank_of(target) < rank_of(current) && context_tokens > DOWNGRADE_MAX_CONTEXT_TOKENS {
        return settle(current, "downgrade-not-worth-cache-rebuild");
    }
    settle(target, "jev")
}
