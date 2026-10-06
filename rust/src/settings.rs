//! The saved default model (SPEC 9.2; `node/src/settings.mjs`).

use crate::config::AUTO_MODEL;
use crate::fsx::write_with_mode;
use crate::jsjson::{self, Object, Value};
use crate::osdirs::home_dir;
use crate::status::{ensure_dir, status_dir};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// `<home>/.claude/settings.json`, fixed at start.
pub static USER_SETTINGS: LazyLock<PathBuf> = LazyLock::new(|| home_dir().join(".claude").join("settings.json"));

/// `<status dir>/saved-model.json`.
pub fn saved_model_memo() -> PathBuf {
    status_dir().join("saved-model.json")
}

fn read_json(file: &Path) -> Option<Value> {
    jsjson::parse_bytes(&std::fs::read(file).ok()?).ok()
}

/// `JSON.parse(...).model`; None where Node throws (unreadable, not JSON, or `null`).
fn model_of(file: &Path) -> Option<Value> {
    let v = read_json(file)?;
    if v.is_nullish() {
        return None;
    }
    Some(v.get("model").clone())
}

/// `readSavedModel(file, memo)`: `Value::Undefined` stands for `undefined`.
pub fn read_saved_model(file: &Path, memo: &Path) -> Value {
    let Some(model) = model_of(file) else { return Value::Undefined };
    if model.is_str(AUTO_MODEL) {
        return model_of(memo).unwrap_or(Value::Undefined);
    }
    let write = || -> std::io::Result<()> {
        if memo == saved_model_memo() {
            ensure_dir()?;
        }
        let mut o = Object::new();
        o.insert("model", if model.is_undefined() { Value::Null } else { model.clone() });
        write_with_mode(memo, &jsjson::to_bytes(&Value::Object(o)), 0o600)
    };
    let _ = write();
    model
}

/// `restoreSavedModel(previous, file)`: whether it wrote.
pub fn restore_saved_model(previous: &Value, file: &Path) -> bool {
    let Some(mut settings) = read_json(file) else { return false };
    if settings.is_nullish() || !settings.get("model").is_str(AUTO_MODEL) {
        return false;
    }
    let Some(o) = settings.as_object_mut() else { return false };
    if previous.is_nullish() {
        o.remove("model");
    } else {
        o.insert("model", previous.clone());
    }
    let Some(mut text) = jsjson::stringify_pretty(&settings) else { return false };
    text.push_str("\n");
    std::fs::write(file, text.as_bytes()).is_ok()
}
