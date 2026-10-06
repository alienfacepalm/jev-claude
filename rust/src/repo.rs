//! The repository root and the release version (SPEC 2.3, 2.4).

use crate::envx;
use crate::jsjson;
use crate::osdirs::simplify;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const MARKER: &[&str] = &[".claude", "skills", "jev-calibrate", "SKILL.md"];

/// The nearest ancestor of `start` (itself included) holding the jev-calibrate skill.
pub fn find_root_from(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        let mut marker = d.to_path_buf();
        for part in MARKER {
            marker.push(part);
        }
        if marker.is_file() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

fn resolve_root() -> Option<PathBuf> {
    if let Some(root) = envx::get("JEV_ROOT").filter(|r| !r.is_empty()) {
        return Some(PathBuf::from(root));
    }
    let exe = std::env::current_exe().ok()?;
    let exe = simplify(std::fs::canonicalize(&exe).unwrap_or(exe));
    find_root_from(exe.parent()?)
}

static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();

/// The repository root, resolved once.
pub fn root() -> Option<&'static Path> {
    ROOT.get_or_init(resolve_root).as_deref()
}

/// The release version from `<root>/package.json`, or `0.0.0`.
pub fn release_version() -> String {
    root().and_then(version_in).unwrap_or_else(|| "0.0.0".to_string())
}

/// `version` from a root's `package.json`, when it is a string.
pub fn version_in(root: &Path) -> Option<String> {
    let text = std::fs::read(root.join("package.json")).ok()?;
    let v = jsjson::parse_bytes(&text).ok()?;
    v.get("version").as_str().map(|s| s.to_string_lossy())
}
