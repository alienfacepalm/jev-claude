//! The first-run setup offer (SPEC 11; `node/src/first-run.mjs`).

use crate::jsjson::{self, Object, Value};
use crate::jsstr::trim_str;
use crate::osdirs::home_dir;
use crate::timefmt::iso_now;
use std::io::{BufRead, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

/// `<home>/.jev-router/first-run.json`, fixed at start.
pub static FIRST_RUN_FILE: LazyLock<PathBuf> = LazyLock::new(|| home_dir().join(".jev-router").join("first-run.json"));

pub fn was_offered(file: &Path) -> bool {
    std::fs::read(file).is_ok()
}

pub fn mark_offered(accepted: bool, file: &Path) {
    let _ = (|| -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut o = Object::new();
        o.insert("offeredAt", iso_now().into());
        o.insert("accepted", Value::Bool(accepted));
        std::fs::write(file, jsjson::to_bytes(&Value::Object(o)))
    })();
}

/// Only a plain interactive start that has not been offered or shadowed qualifies.
pub fn should_offer(args: &[String], interactive: bool, offered: bool, shadowed: bool) -> bool {
    interactive && !offered && !shadowed && args.is_empty()
}

/// `path.resolve`: absolute, with `.` and `..` resolved lexically, no symlinks followed.
pub fn resolve_lexically(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(p) };
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Whether the launch directory defines its own `jev-calibrate` skill.
pub fn shadows_skill(cwd: &Path, root: Option<&Path>) -> bool {
    let Some(root) = root else { return false };
    let a = resolve_lexically(cwd).to_string_lossy().trim_end_matches(['\\', '/']).to_string();
    let b = resolve_lexically(root).to_string_lossy().trim_end_matches(['\\', '/']).to_string();
    a != b && cwd.join(".claude").join("skills").join("jev-calibrate").exists()
}

/// The answer to a yes/no question.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Answer {
    Yes,
    No,
    /// End of input or an input error.
    None,
}

/// Writes the question and reads one line: `n`/`no` declines, anything else accepts.
pub fn ask_yes_no(question: &str, input: &mut dyn BufRead, output: &mut dyn Write) -> Answer {
    let _ = output.write_all(question.as_bytes());
    let _ = output.flush();
    let mut line = Vec::new();
    match input.read_until(b'\n', &mut line) {
        Ok(0) | Err(_) => Answer::None,
        Ok(_) => {
            let text = String::from_utf8_lossy(&line);
            let text = text.strip_suffix('\n').unwrap_or(&text);
            let text = text.strip_suffix('\r').unwrap_or(text);
            let t = trim_str(text).to_ascii_lowercase();
            if t == "n" || t == "no" { Answer::No } else { Answer::Yes }
        }
    }
}
