//! Golden cases (SPEC 16.2): every file in `conformance/cases`, generated from the Node
//! functions themselves, checked against this implementation.

mod common;

use common::*;
use jev_router::config::{self, Model, effort_floor, fable_allowed, forced_effort};
use jev_router::envx::EnvMap;
use jev_router::jsjson::{self, Object, Value};
use jev_router::jsstr::{JsStr, math_round, number_to_string, to_fixed2, utf16_len_bytes};
use jev_router::{explain, icons, launch, legend, model_names, policy, proxy, reasons, status, update, worktree};
use std::cell::RefCell;

fn env_of(v: &Value) -> EnvMap {
    let mut m = EnvMap::new();
    if let Value::Object(o) = v {
        for (k, val) in o.iter() {
            if let Value::String(s) = val {
                m.insert(k.to_string_lossy(), s.to_string_lossy());
            }
        }
    }
    m
}

fn opt_str(v: Option<&str>) -> Value {
    v.map_or(Value::Null, Value::from)
}

fn models_value(models: &[Model]) -> Value {
    Value::Array(models.iter().map(Model::to_value).collect())
}

fn models_of(v: &Value) -> Vec<Model> {
    v.as_array().unwrap().iter().map(|m| Model::from_value(m).expect("model with a tier")).collect()
}

fn agent_value(a: &status::Agent) -> Value {
    Value::Object(a.to_object())
}

#[test]
fn detect_override() {
    check_cases("detect-override", &mut |i| Ok(opt_str(policy::detect_override(i.get("prompt")))));
}

#[test]
fn decide() {
    check_cases("decide", &mut |i| {
        let available: Vec<String> =
            i.get("available").as_array().unwrap().iter().map(|a| a.to_js_string().to_string_lossy()).collect();
        let available: Vec<&str> = available.iter().map(String::as_str).collect();
        let jev = i.get("jev");
        let jev = if jev.truthy() { Some(jev) } else { None };
        let ctx = i.get("contextTokens");
        let ctx = if ctx.is_undefined() { 0.0 } else { ctx.to_number() };
        let d =
            policy::decide(i.get("prompt"), jev, &i.get("current").to_js_string().to_string_lossy(), &available, ctx);
        let mut o = Object::new();
        o.insert("tier", d.tier.into());
        o.insert("reason", d.reason.into());
        o.insert("changed", Value::Bool(d.changed));
        Ok(Value::Object(o))
    });
}

#[test]
fn effort_settings() {
    check_cases("effort-floor", &mut |i| {
        Ok(opt_str(effort_floor(&i.get("name").to_js_string().to_string_lossy(), &env_of(i.get("env")))))
    });
    check_cases("forced-effort", &mut |i| {
        Ok(opt_str(forced_effort(&i.get("name").to_js_string().to_string_lossy(), &env_of(i.get("env")))))
    });
    check_cases("fable-allowed", &mut |i| Ok(Value::Bool(fable_allowed(&env_of(i.get("env"))))));
}

#[test]
fn new_turn_prompt() {
    check_cases("new-turn-prompt", &mut |i| {
        Ok(proxy::new_turn_prompt(i.get("body"))?.map_or(Value::Null, Value::String))
    });
}

#[test]
fn agent_label() {
    check_cases("agent-label", &mut |i| {
        let max = i.get("max");
        let max = if max.is_undefined() { 48 } else { max.to_number() as usize };
        Ok(Value::String(proxy::agent_label(i.get("body"), max)?))
    });
}

#[test]
fn apply_tier() {
    check_cases("apply-tier", &mut |i| {
        let mut body = i.get("body").clone();
        let model = i.get("model").as_str().cloned();
        proxy::apply_tier(
            &mut body,
            &i.get("tier").to_js_string().to_string_lossy(),
            model.as_ref(),
            &env_of(i.get("env")),
        );
        Ok(body)
    });
}

#[test]
fn sanitize_schema() {
    check_cases("sanitize-schema", &mut |i| {
        let mut node = i.get("node").clone();
        proxy::sanitize_schema(&mut node);
        Ok(node)
    });
}

#[test]
fn versions_and_models() {
    check_cases("version-of", &mut |i| {
        let id = i.get("id");
        let id = if id.is_undefined() { JsStr::new() } else { id.to_js_string() };
        let tier = i.get("tier").as_str().map(JsStr::to_string_lossy).unwrap_or_default();
        let (a, b) = proxy::version_of(id.as_bytes(), &tier);
        Ok(Value::Array(vec![Value::Number(a), Value::Number(b)]))
    });
    check_cases("short-name", &mut |i| {
        let m = i.get("model");
        let text = if m.is_nullish() { JsStr::new() } else { m.to_js_string() };
        Ok(model_names::short_name(text.as_bytes()).map_or(Value::Null, Value::from))
    });
    check_cases("claude-models", &mut |i| {
        Ok(models_value(&proxy::claude_models(i.get("catalog").as_array().unwrap())?))
    });
    check_cases("newest-per-tier", &mut |i| Ok(models_value(&proxy::newest_per_tier(&models_of(i.get("models"))))));
    check_cases("newer-than-calibrated", &mut |i| {
        let ids = proxy::newer_than_calibrated(i.get("catalog").as_array().unwrap())?;
        Ok(Value::Array(ids.into_iter().map(Value::String).collect()))
    });
}

#[test]
fn sessions_and_keys() {
    check_cases("session-of", &mut |i| Ok(proxy::session_of(i.get("body"))));
    check_cases("conversation-key", &mut |i| Ok(Value::String(proxy::conversation_key(i.get("body"))?)));
    check_cases("agent-of", &mut |i| {
        let mut mains = proxy::Mains::new();
        let mut results = Vec::new();
        for body in i.get("steps").as_array().unwrap() {
            let agent = proxy::agent_of(body, &mut mains)?;
            results.push(agent_value(&agent));
        }
        let entries: Vec<Value> =
            mains.entries().map(|(s, k)| Value::Array(vec![s.clone(), Value::String(k.clone())])).collect();
        let mut o = Object::new();
        o.insert("results", Value::Array(results));
        o.insert("mains", Value::Array(entries));
        Ok(Value::Object(o))
    });
}

#[test]
fn write_decision() {
    isolate_status();
    let mut first_write = true;
    check_cases("write-decision", &mut |i| {
        let mut files = Vec::new();
        for step in i.get("steps").as_array().unwrap() {
            let was_first = std::mem::take(&mut first_write);
            let id = step.get("id");
            let agent = step.get("agent");
            let agent = agent.as_object().map(|a| status::Agent {
                key: a.get("key").unwrap().to_js_string(),
                label: a.get("label").unwrap().to_js_string(),
                main: a.get("main").unwrap().truthy(),
            });
            if step.get("op").is_str("writeDecision") {
                status::write_decision(id, &obj(step.get("decision")), agent.as_ref())?;
            } else {
                status::mark_manual(id, step.get("model"), agent.as_ref());
            }
            if was_first {
                // The generator pins Date.now to 1.8e12 while it runs these steps, and Node's
                // first status write in a process prunes files older than a week by that clock,
                // which removes the file just written. Prune by the same clock to match.
                status::prune_stale(status::STALE_AFTER_MS, 1_800_000_000_000.0);
            }
            let mut f = Object::new();
            f.insert("file", status::read_status(id));
            files.push(Value::Object(f));
        }
        Ok(Value::Array(files))
    });
}

#[test]
fn views() {
    check_cases("agent-view", &mut |i| {
        let fresh = i.get("freshMs");
        let fresh = if fresh.is_undefined() { 90_000.0 } else { fresh.to_number() };
        let view = status::agent_view(i.get("status"), fresh, i.get("now").to_number());
        let mut o = Object::new();
        o.insert("main", view.main);
        o.insert("subagents", Value::Array(view.subagents));
        Ok(Value::Object(o))
    });
    check_cases("main-decision", &mut |i| Ok(status::main_decision(i.get("status"))));
}

fn icons_value(set: &icons::Icons) -> Value {
    let mut o = Object::new();
    for (k, v) in set.entries() {
        o.insert(k, v.into());
    }
    Value::Object(o)
}

#[test]
fn icons_reasons_legend() {
    check_cases("icons", &mut |i| {
        Ok(icons_value(&icons::icons_for(&env_of(i.get("env")), i.get("platform").is_str("win32"))))
    });
    check_cases("reasons", &mut |i| {
        let r = i.get("reason").as_str().map(JsStr::to_string_lossy);
        let mut o = Object::new();
        o.insert("short", opt_str(reasons::short_reason(r.as_deref())));
        o.insert("long", reasons::long_reason(r.as_deref()).into());
        o.insert("noChange", Value::Bool(reasons::is_no_change(r.as_deref())));
        Ok(Value::Object(o))
    });
    check_cases("format-legend", &mut |i| {
        let set = i.get("set");
        let leak =
            |k: &str| -> &'static str { Box::leak(set.get(k).to_js_string().to_string_lossy().into_boxed_str()) };
        let icons = icons::Icons {
            model: leak("model"),
            effort: leak("effort"),
            agents: leak("agents"),
            dir: leak("dir"),
            branch: leak("branch"),
            worktree: leak("worktree"),
            context: leak("context"),
        };
        Ok(Value::from(legend::format_legend(&icons)))
    });
}

#[test]
fn explanation_panels() {
    check_cases("format-explanation", &mut |i| Ok(Value::from(explain::format_explanation(i.get("status")))));
    check_cases("format-agents", &mut |i| {
        Ok(Value::from(explain::format_agents(i.get("status"), i.get("now").to_number())))
    });
}

#[test]
fn location_info() {
    check_cases("location-info", &mut |i| {
        let asked = RefCell::new(Vec::new());
        let branch = i.get("branch").clone();
        let lookup = |dir: &Value| {
            asked.borrow_mut().push(dir.clone());
            branch.as_str().cloned()
        };
        let result = worktree::location_info(i.get("input"), &lookup);
        let mut o = Object::new();
        o.insert(
            "result",
            match result {
                None => Value::Null,
                Some(l) => {
                    let mut r = Object::new();
                    r.insert("branch", l.branch);
                    r.insert("worktree", l.worktree);
                    Value::Object(r)
                }
            },
        );
        o.insert("asked", Value::Array(asked.into_inner()));
        Ok(Value::Object(o))
    });
}

#[test]
fn update_helpers() {
    check_cases("compare-versions", &mut |i| {
        Ok(Value::Number(update::compare_versions(
            &i.get("a").to_js_string().to_string_lossy(),
            &i.get("b").to_js_string().to_string_lossy(),
        )))
    });
    check_cases("update-notice", &mut |i| {
        let state = i.get("state");
        let current = i.get("currentVersion").as_str().map(JsStr::to_string_lossy);
        let state = if state.is_nullish() { None } else { Some(state) };
        Ok(update::update_notice(state, current.as_deref()).map_or(Value::Null, Value::from))
    });
    check_cases("is-check-due", &mut |i| {
        let every = i.get("everyMs");
        let every = if every.is_undefined() { update::CHECK_EVERY_MS } else { every.to_number() };
        let state = i.get("state");
        let state = if state.is_nullish() { None } else { Some(state) };
        Ok(Value::Bool(update::is_check_due(state, i.get("now").to_number(), every)))
    });
    check_cases("needs-install", &mut |i| {
        let files: Vec<String> =
            i.get("changedFiles").as_array().unwrap().iter().map(|f| f.to_js_string().to_string_lossy()).collect();
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        Ok(Value::Bool(update::needs_install(&files)))
    });
}

#[test]
fn quote_for_cmd() {
    check_cases("quote-for-cmd", &mut |i| {
        Ok(Value::from(launch::quote_for_cmd(&i.get("arg").to_js_string().to_string_lossy())))
    });
}

#[test]
fn parse_env() {
    check_cases("parse-env", &mut |i| {
        let text = i.get("text").to_js_string().to_string_lossy();
        let parsed = jev_router::env::parse_env(&text);
        // Compare as a map: order the result the way the expected object is ordered.
        let mut o = Object::new();
        for (k, v) in parsed {
            o.insert(k, v.into());
        }
        Ok(Value::Object(o))
    });
}

#[test]
fn json_stringify_and_parse() {
    check_cases("stringify", &mut |i| {
        let text = if i.get("indent").is_undefined() {
            jsjson::stringify(i.get("value"))
        } else {
            jsjson::stringify_pretty(i.get("value"))
        };
        let mut o = Object::new();
        match text {
            Some(t) => {
                let len = utf16_len_bytes(t.as_bytes()) as f64;
                o.insert("text", Value::String(t));
                o.insert("utf16Length", Value::Number(len));
            }
            None => {
                o.insert("text", Value::Undefined);
                o.insert("utf16Length", Value::Undefined);
            }
        }
        Ok(Value::Object(o))
    });
    check_cases("parse", &mut |i| {
        let v = jsjson::parse_bytes(&hex_bytes(i.get("bytes")))?;
        let mut o = Object::new();
        let text = jsjson::stringify(&v).map_or(Value::Undefined, Value::String);
        o.insert("value", v);
        o.insert("text", text);
        Ok(Value::Object(o))
    });
}

#[test]
fn math() {
    check_cases("math", &mut |i| {
        let v = i.get("value");
        let op = i.get("op").to_js_string().to_string_lossy();
        Ok(match op.as_str() {
            "MathRound" => Value::Number(math_round(v.to_number())),
            "toFixed2" => Value::from(to_fixed2(v.as_number().unwrap())),
            "ToNumber" => Value::Number(v.to_number()),
            "roundPercent" => Value::Number(math_round(v.to_number() * 100.0)),
            "toFixed2OfToNumber" => Value::from(to_fixed2(v.to_number())),
            other => return Err(format!("unknown op {other}")),
        })
    });
    // Number formatting is exercised by `stringify`; a direct spot check of the exponent rule:
    assert_eq!(number_to_string(1e21), "1e+21");
}

#[test]
fn config_tables_match_node() {
    // Every tier id and family in one place, as `jev-check` prints them.
    let ids: Vec<&str> = config::TIERS.iter().map(|t| t.id).collect();
    assert_eq!(ids, ["claude-haiku-4-5-20251001", "claude-sonnet-5-5", "claude-opus-5-5", "claude-fable-5-1"]);
}
