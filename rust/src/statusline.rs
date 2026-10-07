//! The status line Claude Code runs (SPEC 12; `node/bin/jev-statusline.mjs`).

use crate::icons::Icons;
use crate::jsjson::{self, Value};
use crate::jsstr::{JsStr, number_to_string};
use crate::model_names::short_name;
use crate::reasons::short_reason;
use crate::status::{agent_view_now, calibration_file, read_calibration, read_status};
use crate::worktree::{git_branch, location_info};
use regex::bytes::Regex;
use std::fmt::Write as _;
use std::sync::LazyLock;

const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";
const MAX_BRANCH: usize = 28;

fn color(tier: &Value) -> &'static str {
    match tier.as_str().and_then(JsStr::as_str) {
        Some("haiku") => "\x1b[32m",
        Some("sonnet") => "\x1b[36m",
        Some("opus") => "\x1b[35m",
        Some("fable") => "\x1b[33m",
        _ => "",
    }
}

static STATUS_TIER: LazyLock<Regex> = LazyLock::new(|| Regex::new("claude-([a-z]+)-").unwrap());

/// The status line's own `tierOf`: the `/claude-([a-z]+)-/` capture.
fn status_tier_of(model: &Value) -> Value {
    let text = if model.is_nullish() { JsStr::new() } else { model.to_js_string() };
    match STATUS_TIER.captures(text.as_bytes()) {
        Some(c) => Value::String(JsStr::from_wtf8(c[1].to_vec())),
        None => Value::Undefined,
    }
}

/// `shortName(model)`: `exec` converts its argument to a string, `model ?? ""` first.
fn short_of(model: &Value) -> Option<String> {
    let text = if model.is_nullish() { JsStr::new() } else { model.to_js_string() };
    short_name(text.as_bytes())
}

fn s(v: &Value) -> String {
    v.to_js_string().to_string_lossy()
}

fn icon(mark: &str) -> String {
    format!("{BOLD}{mark}{RESET}")
}

fn main_line(entry: &Value, icons: &Icons) -> String {
    let c = color(entry.get("tier"));
    let confidence = entry.get("confidence");
    let p = if confidence.is_nullish() {
        String::new()
    } else {
        let pct = crate::jsstr::math_round(confidence.to_number() * 100.0);
        format!(" {DIM}({}%){RESET}", number_to_string(pct))
    };
    let effort = entry.get("effort");
    let level = if effort.truthy() {
        format!(" {DIM}\u{00B7}{RESET} {} {}", icon(icons.effort), s(effort))
    } else {
        String::new()
    };
    let reason = entry.get("reason").as_str().map(JsStr::to_string_lossy);
    let why = match short_reason(reason.as_deref()) {
        Some(said) => format!(" {DIM}({said}){RESET}"),
        None => String::new(),
    };
    let name = short_of(entry.get("model")).unwrap_or_else(|| s(entry.get("model").or(entry.get("tier"))));
    format!("{} {c}{name}{RESET}{p}{level}{why}", icon(icons.model))
}

/// `clip(text, 28)` for a branch value of any type.
fn clip(v: &Value) -> String {
    match v {
        Value::String(t) if t.utf16_len() > MAX_BRANCH => {
            let mut cut = t.slice_units(MAX_BRANCH - 1);
            cut.push_str("\u{2026}");
            cut.to_string_lossy()
        }
        other => s(other),
    }
}

/// Renders the status line for Claude Code's stdin JSON, with a branch lookup. Fails where Node
/// throws: stdin holding the JSON literal `null`, whose `session_id` Node cannot read.
pub fn render_with(stdin: &[u8], icons: &Icons, branch_of: &dyn Fn(&Value) -> Option<JsStr>) -> Result<String, String> {
    let text = String::from_utf8_lossy(stdin);
    let text = if text.is_empty() { "{}".into() } else { text };
    let input = jsjson::parse(&text).unwrap_or_else(|_| Value::Object(jsjson::Object::default()));
    if input.is_nullish() {
        return Err("TypeError: Cannot read properties of null (reading 'session_id')".to_string());
    }

    let status = read_status(input.get("session_id"));
    let empty = Value::from("");
    let dir_value = input.get("workspace").get("current_dir").or(input.get("cwd")).or(&empty);
    let dir_text = dir_value.to_js_string();
    let dir = dir_text.as_bytes().rsplit(|c| *c == b'/' || *c == b'\\').next().unwrap_or(&[]).to_vec();
    let dir = JsStr::from_wtf8(dir);
    let pct = crate::jsjson::round_value(input.get("context_window").get("used_percentage").or(&Value::Number(0.0)));
    let view = agent_view_now(&status);

    let main = &view.main;
    let routed = if main.get("manual").truthy() || (!main.truthy() && status.get("manual").truthy()) {
        let empty = Value::from("");
        let shown = input.get("model").get("display_name").or(main.get("model")).or(&empty);
        let mut line = JsStr::from(format!("{DIM}\u{261E} manual{RESET} "));
        line.push_js(&shown.to_js_string());
        line.trim_end().to_string_lossy()
    } else if main.truthy() {
        main_line(main, icons)
    } else if status.truthy() {
        main_line(&status, icons)
    } else {
        format!("{DIM}jev: waiting for first prompt{RESET}")
    };

    let mut agents = String::new();
    if !view.subagents.is_empty() {
        let shown = &view.subagents[..view.subagents.len().min(3)];
        let mut names: Vec<String> = shown
            .iter()
            .map(|a| {
                let tier = a.get("tier").or(&status_tier_of(a.get("model"))).clone();
                let c = color(&tier);
                // U+261E, not the emoji U+23F8, which terminals draw double-width in one cell.
                let picked = if a.get("manual").truthy() { "\u{261E} " } else { "" };
                let name = short_of(a.get("model"))
                    .unwrap_or_else(|| s(a.get("tier").or(a.get("model")).or(&Value::from("?"))));
                format!("{c}{picked}{name}{RESET}")
            })
            .collect();
        if view.subagents.len() > shown.len() {
            names.push(format!("{DIM}+{}{RESET}", view.subagents.len() - shown.len()));
        }
        agents = format!(" {DIM}\u{00B7}{RESET} {} {}", icon(icons.agents), names.join(&format!("{DIM},{RESET}")));
    }

    let calibration = read_calibration(&calibration_file());
    let notice = if calibration.newer.is_empty() {
        String::new()
    } else {
        let more =
            if calibration.newer.len() > 1 { format!(" +{}", calibration.newer.len() - 1) } else { String::new() };
        format!(" {DIM}\u{00B7}{RESET} \x1b[33mnew {}{more}: /jev-calibrate{RESET}", s(&calibration.newer[0]))
    };

    let loc = location_info(&input, branch_of);
    let mut where_ = String::new();
    let mut worktree = Value::Undefined;
    if let Some(loc) = &loc {
        if !loc.branch.is_nullish() {
            let b = if loc.branch.truthy() { loc.branch.clone() } else { Value::from("(detached)") };
            let _ = write!(where_, " {DIM}\u{00B7}{RESET} {} \x1b[34m{}{RESET}", icon(icons.branch), clip(&b));
        }
        if loc.worktree.truthy() {
            let _ =
                write!(where_, " {DIM}\u{00B7}{RESET} {} \x1b[32m{}{RESET}", icon(icons.worktree), s(&loc.worktree));
        }
        worktree = loc.worktree.clone();
    }
    let dir_part = if !dir.is_empty() && !matches!(&worktree, Value::String(w) if *w == dir) {
        format!(" {DIM}\u{00B7}{RESET} {} {}", icon(icons.dir), dir.to_string_lossy())
    } else {
        String::new()
    };

    // The whole path last, dimmed (SPEC 12).
    let full_path = if dir_text.is_empty() {
        String::new()
    } else {
        format!(" {DIM}\u{00B7} {}{RESET}", dir_text.to_string_lossy())
    };

    Ok(format!(
        "{routed}{agents}{dir_part}{where_} {DIM}\u{00B7}{RESET} {} {}%{notice}{full_path}\n",
        icon(icons.context),
        number_to_string(pct)
    ))
}

/// Renders with the real git lookup.
pub fn render(stdin: &[u8], icons: &Icons) -> Result<String, String> {
    render_with(stdin, icons, &git_branch)
}
