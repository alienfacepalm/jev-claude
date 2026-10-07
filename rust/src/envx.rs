//! Environment access.
//!
//! Node reads `process.env` and `loadEnv` writes into it. Rust 2024 makes `set_var` unsafe, so
//! the process environment here is the real one plus an overlay that `loadEnv` writes to; every
//! read goes through [`ProcessEnv`], and the launcher hands the overlay to Claude Code. Functions
//! that Node gives an `env` parameter take `&dyn Env`, so tests pass a plain map.

use std::collections::BTreeMap;
use std::sync::{LazyLock, RwLock};

/// Read access to an environment.
pub trait Env: Send + Sync {
    /// The value of `key`, or None when unset.
    fn get(&self, key: &str) -> Option<String>;
}

/// Write access, for `loadEnv`.
pub trait EnvMut: Env {
    /// Sets `key` to `value`.
    fn set(&mut self, key: &str, value: &str);
}

/// A plain map, as Node's tests pass `{ JEV_OPUS_EFFORT: "high" }`.
pub type EnvMap = BTreeMap<String, String>;

impl Env for EnvMap {
    fn get(&self, key: &str) -> Option<String> {
        BTreeMap::get(self, key).cloned()
    }
}

impl EnvMut for EnvMap {
    fn set(&mut self, key: &str, value: &str) {
        self.insert(key.to_string(), value.to_string());
    }
}

/// Builds an [`EnvMap`] from pairs.
pub fn map(pairs: &[(&str, &str)]) -> EnvMap {
    pairs.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect()
}

static OVERLAY: LazyLock<RwLock<Vec<(String, String)>>> = LazyLock::new(|| RwLock::new(Vec::new()));

fn same_key(a: &str, b: &str) -> bool {
    if cfg!(windows) { a.eq_ignore_ascii_case(b) } else { a == b }
}

/// `process.env`: the real environment with `loadEnv`'s additions. Names compare
/// case-insensitively on Windows, as Node's `process.env` does there.
#[derive(Clone, Copy, Default)]
pub struct ProcessEnv;

impl Env for ProcessEnv {
    fn get(&self, key: &str) -> Option<String> {
        if let Some((_, v)) = OVERLAY.read().unwrap().iter().find(|(k, _)| same_key(k, key)) {
            return Some(v.clone());
        }
        std::env::var_os(key).map(|v| v.to_string_lossy().into_owned())
    }
}

impl EnvMut for ProcessEnv {
    fn set(&mut self, key: &str, value: &str) {
        let mut o = OVERLAY.write().unwrap();
        match o.iter_mut().find(|(k, _)| same_key(k, key)) {
            Some(slot) => slot.1 = value.to_string(),
            None => o.push((key.to_string(), value.to_string())),
        }
    }
}

/// Values `loadEnv` added to the process environment, for the child process.
pub fn overlay() -> Vec<(String, String)> {
    OVERLAY.read().unwrap().clone()
}

/// `process.env[key]`.
pub fn get(key: &str) -> Option<String> {
    ProcessEnv.get(key)
}

/// `process.env[key]` is truthy (set and non-empty).
pub fn truthy(key: &str) -> bool {
    get(key).is_some_and(|v| !v.is_empty())
}

/// Sets a value in the process environment overlay (tests and `loadEnv`).
pub fn set(key: &str, value: &str) {
    ProcessEnv.set(key, value);
}

/// Removes a value from the overlay (it does not touch the real environment).
pub fn unset_overlay(key: &str) {
    OVERLAY.write().unwrap().retain(|(k, _)| !same_key(k, key));
}
