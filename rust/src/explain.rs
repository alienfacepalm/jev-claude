//! The explanation panel (SPEC 13; `node/src/explain.mjs`).

use crate::config::tier_of;
use crate::jsjson::Value;
use crate::jsstr::{JSWS_RUN, JsStr, math_round, number_to_string, to_fixed2};
use crate::reasons::{is_no_change, long_reason};
use crate::status::agent_view;

const WIDTH: usize = 33;
const AGENT_WIDTH: usize = 52;

fn recommendation_of(status: &Value) -> JsStr {
    let answers = status.get("jev").get("response").get("answers");
    let choice = answers.get("model").get("choice").or(answers.get("model_tier").get("choice"));
    if !choice.truthy() {
        return status.get("tier").or(&Value::from("unknown")).to_js_string();
    }
    match tier_of(choice) {
        Some(t) => t.into(),
        None => choice.to_js_string(),
    }
}

fn boxed_row(text: &JsStr, width: usize) -> String {
    format!("\u{2502} {} \u{2502}", text.slice_units(width - 2).pad_end(width - 2))
}

fn row(text: impl Into<JsStr>) -> String {
    boxed_row(&text.into(), WIDTH)
}

fn agent_row(text: impl Into<JsStr>) -> String {
    boxed_row(&text.into(), AGENT_WIDTH)
}

fn metric(value: &Value) -> String {
    match value {
        Value::Number(n) if n.is_finite() => to_fixed2(*n),
        _ => "n/a".to_string(),
    }
}

fn wrapped(label: &str, value: &JsStr) -> Vec<String> {
    let mut text = JsStr::from(label);
    text.push_js(value);
    let words = text.replace_all(&JSWS_RUN, " ").trim().split_on(" ");
    let mut lines: Vec<JsStr> = Vec::new();
    for word in words {
        let fits = lines.last().map(|last| {
            let mut joined = last.clone();
            joined.push_str(" ");
            joined.push_js(&word);
            joined.utf16_len() <= WIDTH - 2
        });
        match fits {
            Some(true) => {
                let last = lines.last_mut().unwrap();
                last.push_str(" ");
                last.push_js(&word);
            }
            _ => lines.push(word),
        }
    }
    lines.iter().map(|l| boxed_row(l, WIDTH)).collect()
}

fn reason_text(reason: &Value) -> Option<String> {
    reason.as_str().map(JsStr::to_string_lossy)
}

fn decision_text(reason: &Value) -> JsStr {
    let r = reason_text(reason);
    let prefix = if is_no_change(r.as_deref()) { "kept this model - " } else { "" };
    format!("{prefix}{}", long_reason(r.as_deref())).into()
}

fn age(at: &Value, now: f64) -> String {
    let at = if at.is_nullish() { now } else { at.to_number() };
    let s = math_round((now - at) / 1000.0).max(0.0);
    // Math.max(0, NaN) is NaN, which falls through to hours.
    let s = if (now - at).is_nan() { f64::NAN } else { s };
    if s < 60.0 {
        format!("{}s", number_to_string(s))
    } else if s < 3600.0 {
        format!("{}m", number_to_string(math_round(s / 60.0)))
    } else {
        format!("{}h", number_to_string(math_round(s / 3600.0)))
    }
}

fn percent(confidence: &Value) -> String {
    format!("{}%", number_to_string(math_round(confidence.to_number() * 100.0)))
}

fn upper(v: &Value) -> JsStr {
    v.to_js_string().to_upper()
}

/// Every agent routed in this session (`formatAgents`); "" when there is none.
pub fn format_agents(status: &Value, now: f64) -> String {
    let view = agent_view(status, f64::INFINITY, now);
    let mut all = Vec::new();
    if view.main.truthy() {
        all.push(view.main.clone());
    }
    all.extend(view.subagents);
    if all.is_empty() {
        return String::new();
    }
    let rule = "\u{2500}".repeat(AGENT_WIDTH);
    let mut lines =
        vec![format!("\u{250C}{rule}\u{2510}"), agent_row("Jev Router \u{00B7} agents this session"), agent_row("")];
    for a in &all {
        let role = JsStr::from(if a.get("main").truthy() { "main" } else { "sub" }).pad_end(5);
        let model = upper(a.get("model").or(a.get("tier")).or(&Value::from("unknown"))).pad_end(22);
        let p: JsStr = if a.get("manual").truthy() {
            "manual".into()
        } else if a.get("confidence").is_nullish() {
            "".into()
        } else {
            percent(a.get("confidence")).into()
        };
        let mut text = role;
        text.push_str(" ");
        text.push_js(&model);
        text.push_str(" ");
        text.push_js(&p.pad_end(7));
        text.push_str(" ");
        text.push_str(&age(a.get("at"), now));
        lines.push(agent_row(text));
        let label = a.get("label");
        if label.truthy() && !label.is_str("main") {
            let mut t = JsStr::from("      ");
            t.push_js(&label.to_js_string());
            lines.push(agent_row(t));
        }
    }
    lines.push(format!("\u{2514}{rule}\u{2518}"));
    lines.join("\n")
}

/// The last routing decision as a panel (`formatExplanation`).
pub fn format_explanation(status: &Value) -> String {
    if !status.truthy() {
        return "Jev Router: no routing decision has been recorded for this session.".to_string();
    }
    if status.get("manual").truthy() {
        return "Jev Router: routing is paused because you selected a model manually.".to_string();
    }
    let m = status.get("metrics");
    let request = status.get("jev").get("request").get("state");
    let recommendation = recommendation_of(status);
    let rule = "\u{2500}".repeat(WIDTH);
    let line = |label: &str, value: JsStr| {
        let mut t = JsStr::from(label);
        t.push_js(&value);
        row(t)
    };
    let mut lines = vec![format!("\u{250C}{rule}\u{2510}"), row("Jev Router"), row(""), row("Jev request")];
    lines.extend(wrapped("Prompt: ", &status.get("prompt").or(&Value::from("not recorded")).to_js_string()));
    lines.push(line("Current model: ", upper(request.get("session").get("current_model").or(&Value::from("unknown")))));
    lines.push(line(
        "Context tokens: ",
        request.get("session").get("context_tokens").or(&Value::from("unknown")).to_js_string(),
    ));
    lines.push(row(""));
    lines.push(row("Jev response"));
    lines.push(row(format!("Task complexity     {}", metric(m.get("taskComplexity")))));
    lines.push(row(format!("Reasoning required  {}", metric(m.get("reasoningRequired")))));
    lines.push(row(format!("Tool complexity     {}", metric(m.get("toolComplexity")))));
    lines.push(row(format!("Context size        {}", metric(m.get("contextSize")))));
    lines.push(row(""));
    lines.push(line("Recommended tier: ", recommendation.to_upper()));
    lines.push(line("Selected model: ", upper(status.get("model").or(status.get("tier")).or(&Value::from("unknown")))));
    lines.push(row(""));
    let confidence = status.get("confidence");
    let shown = if confidence.is_nullish() { "n/a".to_string() } else { percent(confidence) };
    lines.push(row(format!("Confidence: {shown}")));
    lines.extend(wrapped("Decision: ", &decision_text(status.get("reason"))));
    lines.push(format!("\u{2514}{rule}\u{2518}"));
    lines.join("\n")
}
