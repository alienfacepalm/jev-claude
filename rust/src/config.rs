//! Every routing knob (SPEC 4; `node/src/config.mjs`). Prose sent to Jev is copied byte for byte.

use crate::envx::Env;
use crate::jsjson::{Object, Value};
use crate::jsstr::{JSWS, JsStr, trim_str};
use regex::bytes::Regex;
use std::sync::LazyLock;

/// One model tier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tier {
    /// Tier name used throughout routing: `haiku`, `sonnet`, `opus` or `fable`.
    pub name: &'static str,
    /// Default model id sent upstream for this tier.
    pub id: &'static str,
    /// Substring that identifies the tier's family inside a model id.
    pub family: &'static str,
    /// Whether the tier accepts `thinking` and thinking-related context edits.
    pub thinking: bool,
    /// Whether the tier accepts `output_config.effort`.
    pub effort: bool,
    /// Effort applied when a request names none (overridable by `JEV_<TIER>_EFFORT`).
    pub floor: Option<&'static str>,
}

/// Model tiers, cheapest first.
pub const TIERS: [Tier; 4] = [
    Tier {
        name: "haiku",
        id: "claude-haiku-4-5-20251001",
        family: "haiku",
        thinking: false,
        effort: false,
        floor: None,
    },
    Tier {
        name: "sonnet",
        id: "claude-sonnet-5-5",
        family: "sonnet",
        thinking: true,
        effort: true,
        floor: Some("high"),
    },
    Tier { name: "opus", id: "claude-opus-5-5", family: "opus", thinking: true, effort: true, floor: Some("medium") },
    Tier { name: "fable", id: "claude-fable-5-1", family: "fable", thinking: true, effort: true, floor: Some("high") },
];

/// Effort levels, lowest first.
pub const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// Tier names, cheapest first.
pub const TIER_NAMES: [&str; 4] = ["haiku", "sonnet", "opus", "fable"];

/// Sentinel model id offered as an extra row in Claude Code's /model picker.
pub const AUTO_MODEL: &str = "jev-router";

/// Below this Jev confidence the router does not trust Jev's choice.
pub const MIN_CONFIDENCE: f64 = 0.6;
/// The tier used when Jev is unsure or unreachable.
pub const UNCERTAIN_DEFAULT: &str = "sonnet";
/// Context size (estimated tokens) above which a routed turn never moves to a cheaper tier.
pub const DOWNGRADE_MAX_CONTEXT_TOKENS: f64 = 20000.0;
/// Per-attempt timeout for a Jev request.
pub const JEV_TIMEOUT_MS: u64 = 1500;
/// Overall deadline for a Jev answer, retries included.
pub const JEV_DEADLINE_MS: u64 = 3000;
/// How many times a failed Jev request is retried.
pub const JEV_MAX_RETRIES: u32 = 1;
/// The context window the status line measures usage against.
pub const CONTEXT_WINDOW_TOKENS: f64 = 200_000.0;

const COMPLEXITY_SCALE: [&str; 10] =
    ["None", "Very low", "Low", "Some", "Moderate", "Moderate to high", "High", "Very high", "Severe", "Extreme"];

/// The highest complexity score Jev reports (the last index of its complexity scale).
pub const COMPLEXITY_MAX_SCORE: f64 = (COMPLEXITY_SCALE.len() - 1) as f64;

/// The tier called `name`, if there is one.
pub fn tier_spec(name: &str) -> Option<&'static Tier> {
    TIERS.iter().find(|t| t.name == name)
}

/// `rankOf`: -1 when unknown.
pub fn rank_of(name: &str) -> i32 {
    TIER_NAMES.iter().position(|n| *n == name).map_or(-1, |i| i as i32)
}

/// The default model id for the tier called `name`.
pub fn id_of(name: &str) -> Option<&'static str> {
    tier_spec(name).map(|t| t.id)
}

/// Whether a request should be routed.
pub fn is_auto(model: &Value) -> bool {
    model.is_str(AUTO_MODEL)
}

/// Tier name for a model string, or None (non-strings have none).
pub fn tier_of(model: &Value) -> Option<&'static str> {
    let s = model.as_str()?;
    tier_of_str(s.as_bytes())
}

/// Tier name for a model id given as bytes: the first tier whose family appears in it.
pub fn tier_of_str(model: &[u8]) -> Option<&'static str> {
    TIERS.iter().find(|t| model.windows(t.family.len()).any(|w| w == t.family.as_bytes())).map(|t| t.name)
}

fn env_effort(env: &dyn Env, key: &str) -> Option<&'static str> {
    let v = env.get(key)?;
    let chosen = trim_str(&v).to_lowercase();
    EFFORTS.iter().find(|e| **e == chosen).copied()
}

/// The effort a tier is given when the request names none.
pub fn effort_floor(name: &str, env: &dyn Env) -> Option<&'static str> {
    let floor = tier_spec(name)?.floor?;
    Some(env_effort(env, &format!("JEV_{}_EFFORT", name.to_uppercase())).unwrap_or(floor))
}

/// An effort that replaces whatever the request carries, or None.
pub fn forced_effort(name: &str, env: &dyn Env) -> Option<&'static str> {
    if !tier_spec(name)?.effort {
        return None;
    }
    let tier_key = format!("JEV_{}_FORCE_EFFORT", name.to_uppercase());
    [tier_key.as_str(), "JEV_FORCE_EFFORT"].into_iter().find_map(|k| env_effort(env, k))
}

/// False only when `JEV_ALLOW_FABLE` says off.
pub fn fable_allowed(env: &dyn Env) -> bool {
    let v = env.get("JEV_ALLOW_FABLE").unwrap_or_default();
    let t = trim_str(&v).to_ascii_lowercase();
    !matches!(t.as_str(), "0" | "false" | "no" | "off")
}

/// Tier names this account may route to (`fable` only when `JEV_ALLOW_FABLE` allows it).
pub fn available_tiers(env: &dyn Env) -> Vec<&'static str> {
    let fable = fable_allowed(env);
    TIER_NAMES.iter().copied().filter(|n| *n != "fable" || fable).collect()
}

/// Whether policy accepted Jev's exact model.
pub fn should_use_exact_model(reason: &str, chosen_tier: Option<&str>, final_tier: &str) -> bool {
    (reason == "jev" || reason == "jev/no-change") && chosen_tier == Some(final_tier)
}

// ---------------------------------------------------------------------------------------------
// Override patterns (4.5)

/// A phrase in the user's prompt that pins a tier (SPEC 4.5).
pub struct OverridePattern {
    /// The tier the phrase asks for.
    pub tier: &'static str,
    /// The pattern, matched over the user's own words.
    pub re: Regex,
}

/// The override pattern with its trailing `(?![-\w])` removed; [`crate::jsstr::find_without_word_after`]
/// applies that check.
pub static OVERRIDE_PATTERNS: LazyLock<Vec<OverridePattern>> = LazyLock::new(|| {
    let names = [("haiku", "fast"), ("sonnet", "balanced"), ("opus", "strong"), ("fable", "long")];
    TIERS
        .iter()
        .zip(names)
        .map(|(t, (name, generic))| {
            let pattern = format!(
                r"(?-u:\b)(?i-u:use|switch to|switch over to|route to){JSWS}+(?:(?i-u:the){JSWS}+)?(?:(?i-u:claude)(?:-|{JSWS}))?(?:(?:(?i-u:{name}))|(?i-u:{generic}){JSWS}+(?i-u:model|tier))"
            );
            OverridePattern { tier: t.name, re: Regex::new(&pattern).unwrap() }
        })
        .collect()
});

// ---------------------------------------------------------------------------------------------
// Questions (4.4)

fn string_array(items: &[&str]) -> Value {
    Value::Array(items.iter().map(|s| Value::from(*s)).collect())
}

fn score(instructions: &str) -> Value {
    let mut o = Object::new();
    o.insert("type", "score".into());
    o.insert("instructions", instructions.into());
    o.insert("criteria", string_array(&COMPLEXITY_SCALE));
    Value::Object(o)
}

/// `QUESTIONS`, in order.
pub fn questions() -> Vec<(&'static str, Value)> {
    vec![
        (
            "task_complexity",
            score("How complex is the coding task overall, including ambiguity, scope, and blast radius?"),
        ),
        ("reasoning_required", score("How much reasoning is required to complete the request correctly in one pass?")),
        (
            "tool_complexity",
            score("How complex is the tool use required, from no tools to many coordinated or stateful operations?"),
        ),
    ]
}

struct Guidance {
    what: &'static str,
    signals: &'static [&'static str],
    not_for: &'static str,
}

fn guidance(tier: &str) -> Option<Guidance> {
    Some(match tier {
        "haiku" => Guidance {
            what: "Trivial, mechanical, or purely factual work.",
            signals: &["Rename, reformat, comment, or run one obvious command"],
            not_for: "Design judgement or multi-file reasoning.",
        },
        "sonnet" => Guidance {
            what: "Well-scoped everyday work, and documents and knowledge work, where it does well for well under Opus's cost.",
            signals: &[
                "Implement a specified function or change, add tests, or fix a bug whose cause is already known",
                "Write or edit documents, specs, summaries, or analysis",
            ],
            not_for: "Open-ended or multi-step coding, changes that must not break existing behaviour, unknown-cause debugging, or judgement calls: Opus scores 5-21 points higher on agentic coding.",
        },
        "opus" => Guidance {
            what: "Complex or open-ended coding and work that needs sustained judgement, where it clearly beats Sonnet, for about twice the cost per task.",
            signals: &[
                "Unknown-cause or intermittent bugs, multi-step changes across a codebase, cross-module design, API or behaviour-preserving changes, security, auth, concurrency, or migrations",
            ],
            not_for: "Well-scoped changes and routine document or knowledge work, where Sonnet does well for less.",
        },
        "fable" => Guidance {
            what: "Long-horizon autonomous work, the most demanding reasoning, and adversarial review that hardens a plan, spec, or design by hunting for how it fails.",
            signals: &[
                "Long-horizon autonomous work: a whole-repo migration, a large system built end to end from a spec, or a full deliverable such as financial analysis with spreadsheets and slides",
                "Adversarially review, red-team, stress-test, or poke holes in a plan, spec, or design to harden it",
                "A problem that has already defeated a strong model, such as a bug two attempts have missed",
            ],
            not_for: "Writing the plan or spec itself, ordinary code review, or security-focused analysis, where Fable's safety classifiers can decline.",
        },
        _ => return None,
    })
}

fn cost(tier: &str) -> Option<&'static str> {
    Some(match tier {
        "haiku" => "$1 / $5 per million input / output tokens; the cheapest, but it does no reasoning",
        "sonnet" => "$2 / $10 per million tokens; about 40-60% less per completed task than Opus",
        "opus" => "$4 / $20 per million tokens; about 1.6-2.6x Sonnet's cost per completed task",
        "fable" => "$10 / $50 per million tokens; the most expensive by far",
        _ => return None,
    })
}

/// One model on the menu: `{id, tier, releasedAt, description}`.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// Full model id, as the catalog lists it.
    pub id: JsStr,
    /// The tier the model belongs to.
    pub tier: String,
    /// The catalog's `released_at` value, kept as JSON for ordering.
    pub released_at: Value,
    /// The catalog description, when it has one.
    pub description: Option<JsStr>,
}

impl Model {
    /// The model as the JavaScript object Node builds (absent fields left out).
    pub fn to_value(&self) -> Value {
        let mut o = Object::new();
        o.insert("id", Value::String(self.id.clone()));
        o.insert("tier", self.tier.as_str().into());
        if !self.released_at.is_undefined() {
            o.insert("releasedAt", self.released_at.clone());
        }
        if let Some(d) = &self.description {
            o.insert("description", Value::String(d.clone()));
        }
        Value::Object(o)
    }

    /// A model object such as tests and callers build by hand; None without a string tier.
    pub fn from_value(v: &Value) -> Option<Model> {
        let tier = v.get("tier").as_str()?.to_string_lossy();
        Some(Model {
            id: v.get("id").to_js_string(),
            tier,
            released_at: v.get("releasedAt").clone(),
            description: v.get("description").as_str().cloned(),
        })
    }
}

const INSTRUCTIONS: [&str; 5] = [
    "Pick the cheapest exact model that can fully complete this coding request in one pass, without retrying on a stronger model.",
    "When you are unsure whether a cheaper model would get it right, pick the stronger one: a failed attempt wastes the whole turn and is rerun anyway, so it costs more than the difference. Speed does not matter.",
    "Each tier is offered as its newest version only. Judge required reasoning, not requested reply length.",
    "Reasoning effort is set per tier and is not something to choose between; judge only which model the work needs.",
    "Changing tier mid-conversation discards the prompt cache and re-reads the whole history, so prefer the current model where the work has not changed shape.",
];

/// A Jev choice from the exact models available.
pub fn question_for_models(models: &[Model]) -> Value {
    let mut criteria = Object::new();
    for m in models {
        let mut entry = Object::new();
        entry.insert("model", Value::String(m.description.clone().unwrap_or_else(|| m.id.clone())));
        entry.insert("cost", cost(&m.tier).map_or(Value::Undefined, Value::from));
        if let Some(g) = guidance(&m.tier) {
            entry.insert("what", g.what.into());
            entry.insert("signals", string_array(g.signals));
            entry.insert("not_for", g.not_for.into());
        }
        criteria.insert(m.id.clone(), Value::Object(entry));
    }
    let mut o = Object::new();
    o.insert("type", "choice".into());
    o.insert("instructions", string_array(&INSTRUCTIONS));
    o.insert("criteria", Value::Object(criteria));
    Value::Object(o)
}
