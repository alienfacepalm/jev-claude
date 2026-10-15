//! Asks the real Jev about a few sample prompts and prints its picks, with no assertions: a quick
//! look at live routing after a change. Needs `JEV_API_KEY` (or `TYPESAFE_API_KEY`), from the
//! environment or the settings files the programs read, and spends Jev credit. Never part of a
//! test run, which never calls the network:
//!
//! ```text
//! cargo run --manifest-path rust/Cargo.toml --release --example live_routing
//! ```

use jev_router::envx;
use jev_router::proxy::{claude_models, newest_per_tier};
use jev_router::router::{RouteArgs, ask_jev};

/// One sample per tier, cheapest first.
const PROMPTS: [&str; 4] = [
    "fix the typo 'recieve' in README.md",
    "add a unit test for the existing formatDate helper",
    "users intermittently get logged out after deploy, figure out why",
    "migrate the entire monorepo from webpack to vite",
];

#[tokio::main]
async fn main() {
    jev_router::init();
    jev_router::env::load_process_env();
    let key = envx::get("JEV_API_KEY").or_else(|| envx::get("TYPESAFE_API_KEY"));
    if key.is_none_or(|k| k.trim().is_empty()) {
        eprintln!(
            "live_routing: no Jev API key. Set JEV_API_KEY (or TYPESAFE_API_KEY) in the environment or in \
             ~/.jev-router.env; this asks the real Jev and spends its credit."
        );
        std::process::exit(1);
    }

    let models = newest_per_tier(&claude_models(&[]).unwrap_or_else(|e| {
        eprintln!("live_routing: {e}");
        std::process::exit(1);
    }));
    let current = models.iter().find(|m| m.tier == "sonnet").map(|m| m.id.clone()).unwrap_or_default();
    for prompt in PROMPTS {
        let args =
            RouteArgs { prompt: prompt.into(), current: current.clone(), context_tokens: 0.0, models: models.clone() };
        let Some(answer) = ask_jev(args).await else {
            println!("FAIL  {prompt}");
            continue;
        };
        let choice = answer.get("choice").to_js_string().to_string_lossy();
        let confidence = answer.get("confidence").to_number();
        let ms = answer.get("ms").to_js_string().to_string_lossy();
        println!("{choice:<26} conf={confidence:.2} {ms:>5}ms | {prompt}");
    }
}
