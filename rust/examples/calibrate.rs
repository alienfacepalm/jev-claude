//! Scores routing against the labelled prompts in `examples/calibration-cases.json`, so a change to the
//! tier guidance, costs, effort or thresholds in `src/config.rs` is judged against the version
//! before it rather than against one lucky run. Jev's confidence moves a few hundredths between
//! identical runs, so each prompt is asked more than once.
//!
//! Two scores, because they answer different questions. Whether Jev picked the intended tier is
//! what the guidance controls. Where the turn finally ran also depends on policy: an unsure pick
//! runs a tier lower by design, which is reported as `step`, not as a miss.
//!
//! Needs `JEV_API_KEY` (or `TYPESAFE_API_KEY`), from the environment or the settings files the
//! programs read. Calls the real Jev, never Claude, and spends Jev credit. A developer tool, not
//! part of any test run and not shipped:
//!
//! ```text
//! cargo run --manifest-path rust/Cargo.toml --release --example calibrate -- --runs 2
//! ```

use jev_router::config::{MIN_CONFIDENCE, UNCERTAIN_DEFAULT, available_tiers};
use jev_router::envx::{self, ProcessEnv};
use jev_router::jsjson::{self, Object, Value};
use jev_router::jsstr::JsStr;
use jev_router::policy::decide;
use jev_router::proxy::{claude_models, newest_per_tier};
use jev_router::router::{RouteArgs, ask_jev};
use std::fmt::Write;
use std::path::PathBuf;

/// The labelled prompts: `(intended tier, prompt)`, in file order.
fn cases() -> Vec<(String, String)> {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples").join("calibration-cases.json");
    let text = std::fs::read(&file).unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", file.display())));
    let parsed = jsjson::parse_bytes(&text).unwrap_or_else(|e| fail(&format!("{} is not JSON: {e}", file.display())));
    let groups = parsed.get("groups").as_array().unwrap_or_else(|| fail(&format!("{} has no groups", file.display())));
    let mut out = Vec::new();
    for group in groups {
        let want = group.get("want").as_str().map(JsStr::to_string_lossy);
        let (Some(want), Some(prompts)) = (want, group.get("prompts").as_array()) else {
            fail(&format!("{}: every group needs a want and prompts", file.display()));
        };
        for prompt in prompts {
            let prompt =
                prompt.as_str().unwrap_or_else(|| fail(&format!("{}: a prompt is not a string", file.display())));
            out.push((want.clone(), prompt.to_string_lossy()));
        }
    }
    out
}

fn fail(message: &str) -> ! {
    eprintln!("calibrate: {message}");
    std::process::exit(1);
}

/// `--runs N`, 2 when absent or not a positive whole number.
fn runs() -> u32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let at = args.iter().position(|a| a == "--runs");
    at.and_then(|i| args.get(i + 1)).and_then(|n| n.parse::<u32>().ok()).filter(|n| *n > 0).unwrap_or(2)
}

/// The first 60 UTF-16 units of a prompt: enough to tell the cases apart, one line per answer.
fn head(prompt: &str) -> String {
    JsStr::from(prompt).slice_units(60).to_string_lossy()
}

#[tokio::main]
async fn main() {
    jev_router::init();
    jev_router::env::load_process_env();
    let key = envx::get("JEV_API_KEY").or_else(|| envx::get("TYPESAFE_API_KEY"));
    if key.is_none_or(|k| k.trim().is_empty()) {
        fail(
            "no Jev API key. Set JEV_API_KEY (or TYPESAFE_API_KEY) in the environment or in ~/.jev-router.env; \
             this asks the real Jev and spends its credit.",
        );
    }
    let runs = runs();

    let tiers = available_tiers(&ProcessEnv);
    let catalog = claude_models(&[]).unwrap_or_else(|e| fail(&e));
    let models = newest_per_tier(&catalog.into_iter().filter(|m| tiers.contains(&m.tier.as_str())).collect::<Vec<_>>());
    let mut available: Vec<&str> = Vec::new();
    for m in &models {
        if !available.contains(&m.tier.as_str()) {
            available.push(m.tier.as_str());
        }
    }
    let current = UNCERTAIN_DEFAULT;
    let current_id = models.iter().find(|m| m.tier == current).map(|m| m.id.clone()).unwrap_or_default();
    // A case for a tier this machine cannot route to measures nothing.
    let all = cases();
    let cases: Vec<&(String, String)> = all.iter().filter(|(want, _)| available.contains(&want.as_str())).collect();
    let skipped = all.len() - cases.len();

    let (mut picks, mut landed, mut stepped, mut asked, mut failed) = (0, 0, 0, 0, 0);
    // Per intended tier, in first-seen order: (tier, picks, asked).
    let mut by_tier: Vec<(&str, u32, u32)> = Vec::new();
    let mut confidences: Vec<f64> = Vec::new();
    for run in 1..=runs {
        println!("--- run {run}");
        for (want, prompt) in &cases {
            asked += 1;
            let slot = by_tier.iter().position(|(t, _, _)| *t == want.as_str()).unwrap_or_else(|| {
                by_tier.push((want.as_str(), 0, 0));
                by_tier.len() - 1
            });
            by_tier[slot].2 += 1;
            let args = RouteArgs {
                prompt: JsStr::from(prompt.as_str()),
                current: current_id.clone(),
                context_tokens: 0.0,
                models: models.clone(),
            };
            let Some(answer) = ask_jev(args).await else {
                failed += 1;
                println!("FAIL want={want:<6} no answer from Jev | {}", head(prompt));
                continue;
            };
            let chosen = models.iter().find(|m| answer.get("choice").as_str() == Some(&m.id));
            let jev = chosen.map(|m| {
                let mut o = Object::new();
                o.insert("choice", m.tier.as_str().into());
                o.insert("confidence", answer.get("confidence").clone());
                Value::Object(o)
            });
            let prompt_value = Value::String(JsStr::from(prompt.as_str()));
            let tier = decide(&prompt_value, jev.as_ref(), current, &available, 0.0).tier;
            let confidence = answer.get("confidence").to_number();
            confidences.push(confidence);
            let picked = chosen.is_some_and(|m| m.tier == *want);
            if picked {
                picks += 1;
                by_tier[slot].1 += 1;
            }
            if tier == *want {
                landed += 1;
            } else if picked {
                stepped += 1;
            }
            let mark = if tier == *want {
                "ok  "
            } else if picked {
                "step"
            } else {
                "MISS"
            };
            let jev_tier = chosen.map_or("?", |m| m.tier.as_str());
            println!("{mark} want={want:<6} jev={jev_tier:<6} conf={confidence:.2} final={tier:<6} | {}", head(prompt));
        }
    }

    confidences.sort_by(f64::total_cmp);
    let median = confidences.get(confidences.len() / 2).map_or_else(|| "n/a".to_string(), |c| format!("{c:.2}"));
    let low = confidences.iter().filter(|c| **c < MIN_CONFIDENCE).count();
    let tiers =
        by_tier.iter().map(|(tier, picks, asked)| format!("{tier} {picks}/{asked}")).collect::<Vec<_>>().join(", ");
    let mut summary = format!(
        "\nJev picked the intended tier {picks}/{asked} ({tiers}) · ran there {landed}/{asked}, \
         {stepped} stepped down as unsure · median confidence {median} · \
         {low} below the {MIN_CONFIDENCE} bar"
    );
    if failed > 0 {
        let _ = write!(summary, " · {failed} unanswered");
    }
    if skipped > 0 {
        let _ = write!(summary, " · {skipped} cases skipped for unavailable tiers");
    }
    println!("{summary}");
}
