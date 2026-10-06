//! The read-only setup report behind /jev-calibrate (SPEC 13).
use jev_router::config::{AUTO_MODEL, TIERS, fable_allowed, tier_of_str};
use jev_router::envx::{self, ProcessEnv};
use jev_router::jsjson::join_array;
use jev_router::status::{calibration_file, read_calibration};
use std::io::Write;

fn row(label: &str, text: &str) -> String {
    format!("{label:<13}{text}")
}

fn main() {
    jev_router::init();
    let repository = jev_router::repo::root()
        .is_some_and(|r| r.join(".git").exists() && r.join("node").join("scripts").join("calibrate.mjs").exists());
    let routing = envx::get("ANTHROPIC_CUSTOM_MODEL_OPTION").as_deref() == Some(AUTO_MODEL);
    let calibration = read_calibration(&calibration_file());
    let mut lines = vec!["Jev Router setup check (read-only: nothing was changed)".to_string(), String::new()];

    let model = envx::get("ANTHROPIC_MODEL").unwrap_or_default();
    let pinned = !model.is_empty() && model != AUTO_MODEL;
    lines.push(row(
        "Routing",
        &if !routing {
            "off - no JEV_API_KEY found. Add JEV_API_KEY=... to ~/.jev-router.env and restart jev-claude.".to_string()
        } else if pinned {
            format!(
                "available, but this session started on {model} because ANTHROPIC_MODEL is set. Choose Jev Router in /model to route."
            )
        } else {
            "on - Jev Router picks a model for each turn".to_string()
        },
    ));
    lines.push(row(
        "Claude",
        if envx::truthy("ANTHROPIC_AUTH_TOKEN") {
            "auth token (ANTHROPIC_AUTH_TOKEN)"
        } else if envx::truthy("ANTHROPIC_API_KEY") {
            "API key (ANTHROPIC_API_KEY), billed per token - if you approved it when Claude Code asked; otherwise your sign-in. Change it with 'Use custom API key' in /config."
        } else {
            "your Claude Code sign-in"
        },
    ));
    let tuned: Vec<String> = TIERS.iter().map(|t| format!("{} {}", t.name, t.id)).collect();
    lines.push(row("Tuned for", &tuned.join(", ")));

    match calibration.at {
        None => lines.push(row(
            "Your account",
            "not read yet - Claude Code has not loaded the model list. Run /jev-calibrate again shortly.",
        )),
        Some(at) => {
            // `models.join(", ") || "no Claude models listed"`: null elements join as empty.
            let joined = join_array(&calibration.models, ", ").to_string_lossy();
            let listed = if joined.is_empty() { "no Claude models listed".to_string() } else { joined };
            lines.push(row("Your account", &format!("{listed} (as of {})", jev_router::timefmt::display_utc(at))));
            let offered: Vec<Option<&str>> =
                calibration.models.iter().map(|m| m.as_str().and_then(|s| tier_of_str(s.as_bytes()))).collect();
            let missing: Vec<&str> =
                TIERS.iter().filter(|t| !offered.contains(&Some(t.name))).map(|t| t.name).collect();
            if !missing.is_empty() {
                lines.push(row(
                    "",
                    &format!("not offered to this account: {} (routing steps around them)", missing.join(", ")),
                ));
            }
            let newer = &calibration.newer;
            lines.push(row(
                "Newer models",
                &if newer.is_empty() {
                    "none - the router is tuned for the newest models your account offers".to_string()
                } else {
                    format!(
                        "{} - routing already uses them, but the router was tuned on the versions before. Update jev-router to get tuning for them.",
                        join_array(newer, ", ").to_string_lossy()
                    )
                },
            ));
        }
    }
    lines.push(row(
        "Fable",
        if fable_allowed(&ProcessEnv) {
            "offered when the work calls for it (bills extra usage credits; JEV_ALLOW_FABLE=0 turns it off)"
        } else {
            "off (JEV_ALLOW_FABLE is set to turn it off)"
        },
    ));
    lines.push(row("Mode", if repository { "repository" } else { "installed" }));
    let _ = std::io::stdout().write_all(format!("{}\n", lines.join("\n")).as_bytes());
}
