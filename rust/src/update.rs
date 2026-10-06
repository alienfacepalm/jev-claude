//! The update library (SPEC 14; `node/src/update.mjs`).

use crate::fsx::temp_name;
use crate::jsjson::{self, Object, Value};
use crate::jsstr::trim_str;
use crate::osdirs::{home_dir, simplify};
use crate::process::{RunOptions, run_with_timeout};
use crate::timefmt::{iso, now_ms, parse_iso};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

/// `<home>/.jev-router/update.json`, fixed at start.
pub static UPDATE_FILE: LazyLock<PathBuf> = LazyLock::new(|| home_dir().join(".jev-router").join("update.json"));

/// How long a check stays fresh.
pub const CHECK_EVERY_MS: f64 = 6.0 * 60.0 * 60.0 * 1000.0;

const FETCH_TIMEOUT: Duration = Duration::from_millis(20_000);
const LOCAL_TIMEOUT: Duration = Duration::from_millis(5_000);

/// Runs `git -C <root> args...`; Ok(trimmed stdout) or Err(message).
fn git(root: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let root_s = root.to_string_lossy().into_owned();
    let mut all = vec!["-C", root_s.as_str()];
    all.extend_from_slice(args);
    let opts = RunOptions {
        timeout,
        env: vec![("GIT_TERMINAL_PROMPT".into(), "0".into())],
        capture_stderr: true,
        ..Default::default()
    };
    let command = format!("git {}", all.join(" "));
    let out = run_with_timeout("git", &all, &opts).map_err(|e| format!("spawn git {e}"))?;
    if out.timed_out {
        return Err(format!("Command failed: {command} (timed out)"));
    }
    if !out.success {
        return Err(format!("Command failed: {command}\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(trim_str(&String::from_utf8_lossy(&out.stdout)).to_string())
}

/// The state file, when it holds an object (arrays count, as `typeof` says "object").
pub fn read_state(file: &Path) -> Option<Value> {
    let v = jsjson::parse_bytes(&std::fs::read(file).ok()?).ok()?;
    matches!(v, Value::Object(_) | Value::Array(_)).then_some(v)
}

pub fn write_state(state: &Value, file: &Path) {
    let _ = (|| -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temp = temp_name(file);
        std::fs::write(&temp, jsjson::to_bytes(state))?;
        std::fs::rename(&temp, file)
    })();
}

/// Whether the last check is missing, unreadable, from the future, or at least `every_ms` old.
pub fn is_check_due(state: Option<&Value>, now: f64, every_ms: f64) -> bool {
    let at = state.and_then(|s| s.get("checkedAt").as_str()).and_then(|s| s.as_str().and_then(parse_iso));
    match at {
        None => true,
        Some(at) => now - at >= every_ms || at > now,
    }
}

/// `Number.parseInt(n, 10) || 0` for one dotted part.
fn parse_int(part: &str) -> f64 {
    let t = crate::jsstr::trim_str(part);
    let (sign, digits) = match t.as_bytes().first() {
        Some(b'-') => (-1.0, &t[1..]),
        Some(b'+') => (1.0, &t[1..]),
        _ => (1.0, t),
    };
    let run: String = digits.chars().take_while(char::is_ascii_digit).collect();
    if run.is_empty() {
        return 0.0;
    }
    let v = sign * run.parse::<f64>().unwrap_or(0.0);
    if v == 0.0 { 0.0 } else { v }
}

/// Compares dotted release numbers; negative when `a` is older. Pre-release tags are ignored.
pub fn compare_versions(a: &str, b: &str) -> f64 {
    let parts = |v: &str| -> Vec<f64> { v.split('-').next().unwrap_or("").split('.').map(parse_int).collect() };
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        let diff = x.get(i).copied().unwrap_or(0.0) - y.get(i).copied().unwrap_or(0.0);
        if diff != 0.0 {
            return diff;
        }
    }
    0.0
}

/// The one line printed at launch, or None.
pub fn update_notice(state: Option<&Value>, current: Option<&str>) -> Option<String> {
    let state = state?;
    let current = current.filter(|c| !c.is_empty())?;
    if !state.get("available").truthy() || !state.get("latest").truthy() {
        return None;
    }
    let latest = state.get("latest").to_js_string().to_string_lossy();
    if compare_versions(&latest, current) <= 0.0 {
        return None;
    }
    Some(format!("[jev] Update available: {current} -> {latest}. Run `jev-claude --update`."))
}

/// The `version` in `<root>/package.json`.
pub fn installed_version(root: &Path) -> Value {
    let Ok(bytes) = std::fs::read(root.join("package.json")) else { return Value::Null };
    match jsjson::parse_bytes(&bytes) {
        Ok(v) if !v.is_nullish() => v.get("version").or(&Value::Null).clone(),
        _ => Value::Null,
    }
}

/// What `inspectClone` found.
#[derive(Debug, Clone, PartialEq)]
pub enum CloneCheck {
    Ok { branch: String, head: String, remote: String, behind: bool },
    Refused(String),
}

fn canonical(p: &Path) -> Option<String> {
    let real = simplify(std::fs::canonicalize(p).ok()?);
    let s = real.to_string_lossy().replace('\\', "/");
    let s = s.trim_end_matches('/').to_string();
    Some(if cfg!(windows) { s.to_lowercase() } else { s })
}

fn first_line(message: &str) -> String {
    message.split('\n').next().unwrap_or("").to_string()
}

/// Whether `root` is a clean clone on a branch this tool may fast-forward.
pub fn inspect_clone(root: &Path) -> CloneCheck {
    match git(root, &["rev-parse", "--show-toplevel"], LOCAL_TIMEOUT) {
        Ok(top) => {
            let same = match (canonical(Path::new(&top)), canonical(root)) {
                (Some(a), Some(b)) => a == b,
                _ => return CloneCheck::Refused("this folder is not a git clone".into()),
            };
            if !same {
                return CloneCheck::Refused("this folder is not a git clone of its own".into());
            }
        }
        Err(_) => return CloneCheck::Refused("this folder is not a git clone".into()),
    }
    let branch = match git(root, &["symbolic-ref", "--short", "HEAD"], LOCAL_TIMEOUT) {
        Ok(b) => b,
        Err(_) => return CloneCheck::Refused("the checkout is not on a branch".into()),
    };
    let result = (|| -> Result<CloneCheck, String> {
        if !git(root, &["status", "--porcelain", "--untracked-files=no"], LOCAL_TIMEOUT)?.is_empty() {
            return Ok(CloneCheck::Refused("there are local changes in the checkout".into()));
        }
        git(root, &["fetch", "--quiet", "origin", &branch], FETCH_TIMEOUT)?;
        let head = git(root, &["rev-parse", "HEAD"], LOCAL_TIMEOUT)?;
        let remote = git(root, &["rev-parse", "FETCH_HEAD"], LOCAL_TIMEOUT)?;
        let ok = |behind| CloneCheck::Ok { branch: branch.clone(), head: head.clone(), remote: remote.clone(), behind };
        if head == remote {
            return Ok(ok(false));
        }
        let is_ancestor =
            |older: &str, newer: &str| git(root, &["merge-base", "--is-ancestor", older, newer], LOCAL_TIMEOUT).is_ok();
        if is_ancestor("FETCH_HEAD", "HEAD") {
            return Ok(ok(false));
        }
        if !is_ancestor("HEAD", "FETCH_HEAD") {
            return Ok(CloneCheck::Refused(format!("local {branch} has commits that origin/{branch} does not")));
        }
        Ok(ok(true))
    })();
    result.unwrap_or_else(|e| CloneCheck::Refused(format!("could not reach origin ({})", first_line(&e))))
}

fn version_at(root: &Path, git_ref: &str) -> Value {
    let Ok(text) = git(root, &["show", &format!("{git_ref}:package.json")], LOCAL_TIMEOUT) else {
        return Value::Null;
    };
    match jsjson::parse(&text) {
        Ok(v) if !v.is_nullish() => v.get("version").or(&Value::Null).clone(),
        _ => Value::Null,
    }
}

/// One update check, as the state file records it.
pub fn check_for_update(root: &Path, now: f64) -> Value {
    let mut o = Object::new();
    o.insert("checkedAt", iso(now).into());
    match inspect_clone(root) {
        CloneCheck::Ok { behind: true, remote, .. } => {
            let latest = version_at(root, "FETCH_HEAD");
            o.insert("available", Value::Bool(latest.truthy()));
            o.insert("latest", latest);
            o.insert("remote", remote.into());
        }
        _ => o.insert("available", Value::Bool(false)),
    }
    Value::Object(o)
}

pub fn check_for_update_now(root: &Path) -> Value {
    check_for_update(root, now_ms())
}

/// Whether these changed files mean the dependencies need installing again.
pub fn needs_install(changed: &[&str]) -> bool {
    changed.contains(&"pnpm-lock.yaml")
}

/// `applyUpdate(root, {install})`, as the object Node resolves.
pub fn apply_update(root: &Path, install: Option<&dyn Fn(&Path) -> i32>) -> Value {
    let mut o = Object::new();
    let (behind, _) = match inspect_clone(root) {
        CloneCheck::Refused(reason) => {
            o.insert("status", "refused".into());
            o.insert("reason", reason.into());
            return Value::Object(o);
        }
        CloneCheck::Ok { behind, remote, .. } => (behind, remote),
    };
    let from = installed_version(root);
    if !behind {
        o.insert("status", "current".into());
        o.insert("version", from);
        return Value::Object(o);
    }
    let result = (|| -> Result<Value, String> {
        let diff = git(root, &["diff", "--name-only", "HEAD", "FETCH_HEAD"], LOCAL_TIMEOUT)?;
        let changed: Vec<&str> = diff.split('\n').collect();
        git(root, &["merge", "--ff-only", "FETCH_HEAD"], FETCH_TIMEOUT)?;
        let to = installed_version(root);
        if needs_install(&changed) {
            let code = install.map_or(0, |f| f(root));
            if code != 0 {
                let mut f = Object::new();
                f.insert("status", "failed".into());
                f.insert("reason", format!("installing dependencies exited with {code}").into());
                f.insert("from", from.clone());
                f.insert("to", to);
                return Ok(Value::Object(f));
            }
        }
        let mut u = Object::new();
        u.insert("status", "updated".into());
        u.insert("from", from.clone());
        u.insert("to", to);
        Ok(Value::Object(u))
    })();
    result.unwrap_or_else(|e| {
        let mut f = Object::new();
        f.insert("status", "failed".into());
        f.insert("reason", first_line(&e).into());
        f.insert("from", from);
        Value::Object(f)
    })
}
