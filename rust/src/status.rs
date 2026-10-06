//! Per-session status files (SPEC 8; `node/src/status.mjs`).

use crate::envx;
use crate::fsx::{chmod, mkdir_all, temp_name, write_with_mode};
use crate::jsjson::{self, Object, Value};
use crate::jsstr::JsStr;
use crate::osdirs::temp_dir;
use crate::timefmt::now_ms;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

const DIR_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;

/// Files not updated for this long belong to finished sessions.
pub const STALE_AFTER_MS: f64 = 7.0 * 24.0 * 60.0 * 60.0 * 1000.0;
const MAX_AGENTS: usize = 12;

static DIR: OnceLock<PathBuf> = OnceLock::new();
static PRUNED: AtomicBool = AtomicBool::new(false);
static DUMPED: AtomicU64 = AtomicU64::new(0);
/// One lock around every status-file read-modify-write and the calibration write (3.10).
static LOCK: Mutex<()> = Mutex::new(());

fn resolve_dir() -> PathBuf {
    match envx::get("JEV_STATUS_DIR").filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => temp_dir().join("jev-claude"),
    }
}

/// The status directory, fixed at its first use (programs call [`crate::init`] at start).
pub fn status_dir() -> &'static Path {
    DIR.get_or_init(resolve_dir)
}

/// Points the status directory somewhere else before anything has used it (tests; Node's
/// `isolate-status.mjs`). Returns the directory in force.
pub fn set_status_dir(dir: PathBuf) -> &'static Path {
    let _ = DIR.set(dir);
    status_dir()
}

/// `SETTINGS_FILE`: the launcher's `--settings` file.
pub fn settings_file() -> PathBuf {
    status_dir().join("settings.json")
}

pub fn calibration_file() -> PathBuf {
    status_dir().join("calibration.json")
}

/// `fileFor(sessionId)`: None where Node's `.replace` would throw (a non-string id).
pub fn file_for(session_id: &Value) -> Option<PathBuf> {
    let id = session_id.as_str()?;
    let clean: Vec<u8> =
        id.as_bytes().iter().copied().filter(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-').collect();
    let name = format!("{}.json", String::from_utf8(clean).unwrap());
    Some(status_dir().join(name))
}

/// Creates the status directory, owner-only.
pub fn ensure_dir() -> std::io::Result<()> {
    mkdir_all(status_dir(), DIR_MODE)?;
    chmod(status_dir(), DIR_MODE)
}

/// Writes `text` to `file` through a unique temporary file, owner-only.
pub fn write_private(file: &Path, text: &[u8]) -> std::io::Result<()> {
    ensure_dir()?;
    let temp = temp_name(file);
    write_with_mode(&temp, text, FILE_MODE)?;
    if let Err(e) = std::fs::rename(&temp, file) {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    chmod(file, FILE_MODE)
}

/// Publishes a status. Does nothing for a falsy id; every error is swallowed.
pub fn write_status(session_id: &Value, status: &Value) {
    if !session_id.truthy() {
        return;
    }
    let Some(file) = file_for(session_id) else { return };
    if write_private(&file, &jsjson::to_bytes(status)).is_ok() && !PRUNED.swap(true, Ordering::SeqCst) {
        prune_stale(STALE_AFTER_MS, now_ms());
    }
}

/// An agent inside a session.
#[derive(Debug, Clone, PartialEq)]
pub struct Agent {
    pub key: JsStr,
    pub label: JsStr,
    pub main: bool,
}

impl Agent {
    pub fn to_object(&self) -> Object {
        let mut o = Object::new();
        o.insert("key", Value::String(self.key.clone()));
        o.insert("label", Value::String(self.label.clone()));
        o.insert("main", Value::Bool(self.main));
        o
    }
}

/// `[...value]` for an array spread: arrays, strings by code point, nullish as empty; anything
/// else is not iterable and throws.
pub fn iterate(v: &Value) -> Result<Vec<Value>, String> {
    match v {
        Value::Undefined | Value::Null => Ok(Vec::new()),
        Value::Array(a) => Ok(a.clone()),
        Value::String(s) => Ok(s
            .code_points()
            .map(|cp| {
                let mut one = JsStr::new();
                one.push_code_point(cp);
                Value::String(one)
            })
            .collect()),
        _ => Err("value is not iterable".to_string()),
    }
}

/// Publishes a routed decision and keeps the recent history (`writeDecision`).
pub fn write_decision(session_id: &Value, decision: &Object, agent: Option<&Agent>) -> Result<(), String> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous = read_status(session_id);
    let entry = match agent {
        Some(a) => {
            let mut e = decision.clone();
            e.insert("agent", Value::Object(a.to_object()));
            e
        }
        None => decision.clone(),
    };
    let mut history = iterate(previous.get("history").or(&Value::Undefined))?;
    history.push(Value::Object(entry));
    if history.len() > 20 {
        history.drain(..history.len() - 20);
    }
    let agents = match agent {
        Some(a) => {
            let d = |k: &str| decision.get(k).cloned().unwrap_or_default();
            let at = decision.get("at").filter(|v| !v.is_nullish()).cloned().unwrap_or(Value::Number(now_ms()));
            let mut e = Object::new();
            e.insert("label", Value::String(a.label.clone()));
            e.insert("main", Value::Bool(a.main));
            e.insert("tier", d("tier"));
            e.insert("model", d("model"));
            e.insert("confidence", d("confidence"));
            e.insert("effort", d("effort"));
            e.insert("reason", d("reason"));
            e.insert("at", at);
            Value::Object(merge_agent(previous.get("agents"), a, &e))
        }
        None => previous.get("agents").clone(),
    };
    let mut out = decision.clone();
    if agents.truthy() {
        out.insert("agents", agents);
    }
    out.insert("history", Value::Array(history));
    write_status(session_id, &Value::Object(out));
    Ok(())
}

/// Records that an agent runs a model the user chose (`markManual`).
pub fn mark_manual(session_id: &Value, model: &Value, agent: Option<&Agent>) {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous = read_status(session_id);
    let now = now_ms();
    let agents = match agent {
        Some(a) => {
            let mut e = Object::new();
            e.insert("label", Value::String(a.label.clone()));
            e.insert("main", Value::Bool(a.main));
            e.insert("model", model.clone());
            e.insert("manual", Value::Bool(true));
            e.insert("at", Value::Number(now));
            Value::Object(merge_agent(previous.get("agents"), a, &e))
        }
        None => previous.get("agents").clone(),
    };
    let manual = match agent {
        Some(a) if a.main => Value::Bool(true),
        Some(_) => previous.get("manual").or(&Value::Bool(false)).clone(),
        None => Value::Bool(true),
    };
    let mut out = previous.spread_of();
    if agents.truthy() {
        out.insert("agents", agents);
    }
    out.insert("manual", manual);
    out.insert("at", Value::Number(now));
    write_status(session_id, &Value::Object(out));
}

/// Stable sort, newest `at` first (`(b.at ?? 0) - (a.at ?? 0)`).
fn by_at_desc(a: &Value, b: &Value) -> std::cmp::Ordering {
    let at = |v: &Value| v.get("at").or(&Value::Number(0.0)).to_number();
    let d = at(b) - at(a);
    if d < 0.0 {
        std::cmp::Ordering::Less
    } else if d > 0.0 {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

/// Newest-wins merge of one agent, trimmed to the most recent `MAX_AGENTS` (`merge`).
pub fn merge_agent(existing: &Value, agent: &Agent, entry: &Object) -> Object {
    let mut agents = existing.or(&Value::Undefined).spread_of();
    let mut merged = agents.get_js(&agent.key).cloned().unwrap_or_default().spread_of();
    merged.spread(entry);
    agents.insert(agent.key.clone(), Value::Object(merged));
    if agents.len() > MAX_AGENTS {
        let mut ordered: Vec<(JsStr, Value)> =
            agents.iter().filter(|(_, v)| !v.get("main").truthy()).map(|(k, v)| (k.clone(), v.clone())).collect();
        ordered.sort_by(|(_, a), (_, b)| by_at_desc(a, b));
        for (stale, _) in ordered.into_iter().skip(MAX_AGENTS - 1) {
            agents.remove_js(&stale);
        }
    }
    agents
}

/// The main agent's entry and the live sub-agents, newest first (`agentView`).
pub struct AgentView {
    pub main: Value,
    pub subagents: Vec<Value>,
}

pub fn agent_view(status: &Value, fresh_ms: f64, now: f64) -> AgentView {
    let agents = status.get("agents").or(&Value::Undefined).spread_of();
    let entries: Vec<Value> = agents
        .iter()
        .map(|(k, a)| {
            let mut e = Object::new();
            e.insert("key", Value::String(k.clone()));
            e.spread(&a.spread_of());
            Value::Object(e)
        })
        .collect();
    let main = entries.iter().find(|a| a.get("main").truthy()).cloned().unwrap_or(Value::Null);
    let mut subagents: Vec<Value> = entries
        .into_iter()
        .filter(|a| !a.get("main").truthy() && now - a.get("at").or(&Value::Number(0.0)).to_number() <= fresh_ms)
        .collect();
    subagents.sort_by(by_at_desc);
    AgentView { main, subagents }
}

/// `agentView(status)` with the defaults (90 s, now).
pub fn agent_view_now(status: &Value) -> AgentView {
    agent_view(status, 90_000.0, now_ms())
}

/// The main thread's most recent decision, else the status itself (`mainDecision`).
pub fn main_decision(status: &Value) -> Value {
    if status.is_nullish() {
        return Value::Null;
    }
    let history = iterate(status.get("history")).unwrap_or_default();
    history.into_iter().rev().find(|d| d.get("agent").get("main").truthy()).unwrap_or_else(|| status.clone())
}

/// Records the newest model per tier and which are newer than calibrated.
pub fn write_calibration(newer: &[JsStr], models: &[JsStr], file: &Path) {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let list = |ids: &[JsStr]| Value::Array(ids.iter().cloned().map(Value::String).collect());
    let mut o = Object::new();
    o.insert("newer", list(newer));
    o.insert("models", list(models));
    o.insert("at", Value::Number(now_ms()));
    let _ = write_private(file, &jsjson::to_bytes(&Value::Object(o)));
}

/// What the last model list said.
#[derive(Debug, Clone, PartialEq)]
pub struct Calibration {
    pub newer: Vec<Value>,
    pub models: Vec<Value>,
    pub at: Option<f64>,
}

pub fn read_calibration(file: &Path) -> Calibration {
    let empty = Calibration { newer: vec![], models: vec![], at: None };
    let Ok(bytes) = std::fs::read(file) else { return empty };
    let Ok(v) = jsjson::parse_bytes(&bytes) else { return empty };
    // Destructuring `null` throws, so a file holding `null` reads as empty.
    if v.is_nullish() {
        return empty;
    }
    let models = v.get("models").as_array();
    let at = v.get("at").as_number();
    let known = models.is_some() && at.is_some();
    Calibration {
        newer: v.get("newer").as_array().cloned().unwrap_or_default(),
        models: if known { models.cloned().unwrap_or_default() } else { vec![] },
        at: if known { at } else { None },
    }
}

/// The latest status for a session, or `null` on any error (`readStatus`).
pub fn read_status(session_id: &Value) -> Value {
    let Some(file) = file_for(session_id) else { return Value::Null };
    match std::fs::read(&file) {
        Ok(bytes) => jsjson::parse_bytes(&bytes).unwrap_or(Value::Null),
        Err(_) => Value::Null,
    }
}

/// Saves a request body (`JEV_DUMP`). Returns the file written.
pub fn dump_body(body: &Value, setting: Option<&str>) -> Option<PathBuf> {
    let setting = setting.filter(|s| !s.is_empty())?;
    let lower = setting.to_ascii_lowercase();
    let dir = status_dir();
    let prefix = if matches!(lower.as_str(), "1" | "true" | "yes") { dir.join("dump") } else { PathBuf::from(setting) };
    let n = DUMPED.fetch_add(1, Ordering::SeqCst);
    let mut name = prefix.as_os_str().to_os_string();
    name.push(format!(".{}-{n}.json", now_ms() as u64));
    let file = PathBuf::from(name);
    let write = || -> std::io::Result<()> {
        // Node ensures the directory whenever the prefix starts with it as a string.
        if prefix.to_string_lossy().starts_with(&*dir.to_string_lossy()) {
            ensure_dir()?;
        }
        let text = jsjson::stringify_pretty(body).map(JsStr::into_bytes).unwrap_or_default();
        write_with_mode(&file, &text, FILE_MODE)
    };
    write().ok().map(|_| file)
}

/// Deletes status files untouched for `max_age_ms`; returns how many.
pub fn prune_stale(max_age_ms: f64, now: f64) -> usize {
    let mut removed = 0;
    let Ok(entries) = std::fs::read_dir(status_dir()) else { return 0 };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".json") || name == "settings.json" {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        let mtime_ms = mtime.duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0);
        if now - mtime_ms > max_age_ms && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}
