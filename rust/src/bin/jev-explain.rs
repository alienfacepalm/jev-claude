//! The explanation panel behind /jev-explain (SPEC 13).
use jev_router::explain::{format_agents, format_explanation};
use jev_router::jsjson::Value;
use jev_router::status::{main_decision, read_status};
use std::io::Write;

fn main() {
    jev_router::init();
    let id = std::env::args().nth(1).map_or(Value::Undefined, Value::from);
    let status = read_status(&id);
    let main = main_decision(&status);
    let agents = format_agents(&status, jev_router::timefmt::now_ms());
    let shown = if main.truthy() && status.get("manual").truthy() {
        let mut m = main.spread_of();
        m.insert("manual", Value::Bool(true));
        Value::Object(m)
    } else {
        main
    };
    let mut text = format!("{}\n", format_explanation(&shown));
    if !agents.is_empty() {
        text.push_str(&agents);
        text.push('\n');
    }
    let _ = std::io::stdout().write_all(text.as_bytes());
}
