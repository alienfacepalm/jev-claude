//! Shared test support: an isolated status directory (Node's `isolate-status.mjs`), temporary
//! directories, the golden-case tagged encoding, and loopback HTTP servers.
#![allow(dead_code)]

use jev_router::jsjson::{self, Object, Value};
use jev_router::jsstr::JsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

static STATUS: OnceLock<PathBuf> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);

/// The repository root (the worktree holding `rust/`).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// A fresh, empty directory under the system temp directory.
pub fn temp_dir(prefix: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "{prefix}{}-{}-{n}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Points this test binary's status directory at a throwaway one, once.
pub fn isolate_status() -> &'static Path {
    let dir = STATUS.get_or_init(|| temp_dir("jev-status-test-"));
    jev_router::status::set_status_dir(dir.clone())
}

/// Parses JSON text written by these tests.
pub fn json(text: &str) -> Value {
    jsjson::parse(text).unwrap_or_else(|e| panic!("bad JSON in test: {e}: {text}"))
}

pub fn s(v: &str) -> Value {
    Value::from(v)
}

// ---------------------------------------------------------------------------------------------
// Golden cases (conformance/cases/README.md)

pub struct Case {
    pub file: String,
    pub name: String,
    pub input: Value,
    pub expected: Value,
}

/// Loads `conformance/cases/<file>.json` with its tags decoded.
pub fn load_cases(file: &str) -> Vec<Case> {
    let path = repo_root().join("conformance").join("cases").join(format!("{file}.json"));
    let text = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let Value::Array(items) = jsjson::parse_bytes(&text).unwrap() else { panic!("{file}: not an array") };
    items
        .into_iter()
        .map(|c| Case {
            file: file.to_string(),
            name: c.get("name").as_str().unwrap().to_string_lossy(),
            input: decode(c.get("input")),
            expected: decode(c.get("expected")),
        })
        .collect()
}

fn single_tag(o: &Object) -> Option<(&str, &Value)> {
    if o.len() != 1 {
        return None;
    }
    let (k, v) = o.iter().next()?;
    let k = k.as_str()?;
    ["$undefined", "$number", "$utf16", "$hex", "$throws", "$clock"].contains(&k).then_some((k, v))
}

/// Raw bytes from a `$hex` tag.
pub fn hex_bytes(v: &Value) -> Vec<u8> {
    let Value::Object(o) = v else { panic!("not a $hex tag") };
    let hex = o.get("$hex").and_then(Value::as_str).expect("$hex").to_string_lossy();
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
}

/// Decodes `$undefined`, `$number`, and `$utf16`; leaves `$hex`, `$throws`, `$clock` in place.
pub fn decode(v: &Value) -> Value {
    match v {
        Value::Object(o) => {
            if let Some((tag, inner)) = single_tag(o) {
                return match tag {
                    "$undefined" => Value::Undefined,
                    "$number" => Value::Number(match inner.as_str().unwrap().to_string_lossy().as_str() {
                        "NaN" => f64::NAN,
                        "Infinity" => f64::INFINITY,
                        "-Infinity" => f64::NEG_INFINITY,
                        "-0" => -0.0,
                        other => panic!("unknown $number {other}"),
                    }),
                    "$utf16" => {
                        let units: Vec<u16> =
                            inner.as_array().unwrap().iter().map(|u| u.as_number().unwrap() as u16).collect();
                        Value::String(JsStr::from_utf16(&units))
                    }
                    _ => v.clone(),
                };
            }
            let mut out = Object::new();
            for (k, item) in o.iter() {
                out.insert(k.clone(), decode(item));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(decode).collect()),
        other => other.clone(),
    }
}

pub fn is_throws(v: &Value) -> bool {
    matches!(v, Value::Object(o) if single_tag(o).is_some_and(|(k, _)| k == "$throws"))
}

fn is_clock(v: &Value) -> bool {
    matches!(v, Value::Object(o) if single_tag(o).is_some_and(|(k, _)| k == "$clock"))
}

/// Deep comparison with key order, the sign of zero, NaN equal to NaN, and `$clock` matching
/// any number. Returns the path of the first difference.
pub fn diff(expected: &Value, actual: &Value, path: &str) -> Option<String> {
    if is_clock(expected) {
        return (!matches!(actual, Value::Number(_)))
            .then(|| format!("{path}: expected a clock number, got {actual:?}"));
    }
    let mismatch = || Some(format!("{path}: expected {expected:?}, got {actual:?}"));
    match (expected, actual) {
        (Value::Number(a), Value::Number(b)) => {
            let same = (a.is_nan() && b.is_nan()) || (a == b && a.is_sign_negative() == b.is_sign_negative());
            if same { None } else { mismatch() }
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return mismatch();
            }
            a.iter().zip(b).enumerate().find_map(|(i, (x, y))| diff(x, y, &format!("{path}[{i}]")))
        }
        (Value::Object(a), Value::Object(b)) => {
            let ka: Vec<&JsStr> = a.keys().collect();
            let kb: Vec<&JsStr> = b.keys().collect();
            if ka != kb {
                return Some(format!("{path}: keys {ka:?} != {kb:?}"));
            }
            a.iter().zip(b.iter()).find_map(|((k, x), (_, y))| diff(x, y, &format!("{path}.{k}")))
        }
        _ => {
            if expected == actual {
                None
            } else {
                mismatch()
            }
        }
    }
}

/// Runs every case of a file through `run`, collecting failures; panics listing all of them.
pub fn check_cases(file: &str, run: &mut dyn FnMut(&Value) -> Result<Value, String>) -> usize {
    let cases = load_cases(file);
    let mut failures = Vec::new();
    for case in &cases {
        let got = run(&case.input);
        let problem = match (&got, is_throws(&case.expected)) {
            (Err(_), true) => None,
            (Ok(v), true) => Some(format!("expected a throw, got {v:?}")),
            (Err(e), false) => Some(format!("failed: {e}")),
            (Ok(v), false) => diff(&case.expected, v, "$"),
        };
        if let Some(p) = problem {
            failures.push(format!("{file} / {}: {p}", case.name));
        }
    }
    assert!(failures.is_empty(), "{} of {} cases failed:\n{}", failures.len(), cases.len(), failures.join("\n"));
    cases.len()
}

/// An object from a `Value` (panics otherwise).
pub fn obj(v: &Value) -> Object {
    v.as_object().cloned().unwrap_or_default()
}
