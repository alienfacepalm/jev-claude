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
    v.get("version").as_str().map(super::jsstr::JsStr::to_string_lossy)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory under the system temp directory, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!(
                "jev-repo-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            // Canonical, so a symlinked temp directory (macOS /var -> /private/var) compares equal.
            Scratch(std::fs::canonicalize(dir).unwrap())
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn marker(root: &Path) -> PathBuf {
        MARKER.iter().fold(root.to_path_buf(), |p, part| p.join(part))
    }

    #[test]
    fn no_marker_means_no_root() {
        let s = Scratch::new("none");
        let deep = s.0.join("a").join("b");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(find_root_from(&deep), None);
    }

    #[test]
    fn the_nearest_ancestor_with_the_marker_wins() {
        let s = Scratch::new("nearest");
        let (outer, inner) = (s.0.join("a"), s.0.join("a").join("b"));
        for root in [&outer, &inner] {
            let file = marker(root);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, "skill").unwrap();
        }
        let deep = inner.join("c").join("d");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(find_root_from(&deep), Some(inner.clone()), "the nearest one");
        assert_eq!(find_root_from(&inner), Some(inner), "the start itself counts");
        std::fs::create_dir_all(outer.join("c")).unwrap();
        assert_eq!(find_root_from(&outer.join("c")), Some(outer), "an ancestor further up");
    }

    #[test]
    fn a_directory_named_skill_md_is_not_a_marker() {
        let s = Scratch::new("dir");
        std::fs::create_dir_all(marker(&s.0)).unwrap();
        assert_eq!(find_root_from(&s.0), None);
    }

    #[test]
    fn version_in_reads_only_a_string_version() {
        let s = Scratch::new("version");
        assert_eq!(version_in(&s.0), None, "no package.json");
        for text in [r#"{"version":1}"#, "not json", "null", r#"{"name":"jev-router"}"#] {
            std::fs::write(s.0.join("package.json"), text).unwrap();
            assert_eq!(version_in(&s.0), None, "{text}");
        }
        std::fs::write(s.0.join("package.json"), r#"{"name":"jev-router","version":"1.2.3"}"#).unwrap();
        assert_eq!(version_in(&s.0).as_deref(), Some("1.2.3"));
    }
}
