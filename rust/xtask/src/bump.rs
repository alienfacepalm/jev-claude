//! The next release version, from the commits being released. Run by the `release` job of
//! .github/workflows/ci.yml on every push to master, once CI has passed:
//!
//! ```text
//! cargo xtask bump-version <before-sha> <after-sha>
//! ```
//!
//! prints the new version, after writing it to the root package.json (`--dry-run` only prints it).
//!
//! Commit subjects follow Conventional Commits (<https://www.conventionalcommits.org>):
//! - "BREAKING CHANGE" in a message, or "!" before the colon ("feat!: ...")  -> major
//! - any "feat: ..." / "feat(scope): ..." subject                            -> minor
//! - anything else (fix:, docs:, chore:, or no prefix)                         -> patch
//!
//! Release commits made by the workflow itself are ignored.

use regex::Regex;
use std::ops::Range;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::LazyLock;

static RELEASE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^chore\(release\):").unwrap());
static BREAKING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"BREAKING[ -]CHANGE").unwrap());
// `\w` as JavaScript reads it: ASCII letters, digits and `_`.
static BANG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_]+(\([^)]*\))?!:").unwrap());
static FEAT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^feat(\([^)]*\))?:").unwrap());

/// The bump the messages call for: `"major"`, `"minor"` or `"patch"`.
pub fn bump_level<S: AsRef<str>>(messages: &[S]) -> &'static str {
    let real: Vec<&str> =
        messages.iter().map(AsRef::as_ref).filter(|m| !m.trim().is_empty() && !RELEASE.is_match(m.trim())).collect();
    if real.iter().any(|m| BREAKING.is_match(m) || BANG.is_match(m.trim())) {
        return "major";
    }
    if real.iter().any(|m| FEAT.is_match(m.trim())) {
        return "minor";
    }
    "patch"
}

/// `version` raised by `level`, with the lower parts reset. A pre-release or build suffix is
/// dropped: `1.2.3-rc.1` is followed by `1.2.4`.
pub fn next_version(version: &str, level: &str) -> Result<String, String> {
    let mut parts = version.split(['.', '+', '-']).map(str::parse::<u64>);
    let mut part = || match parts.next() {
        Some(Ok(n)) => Ok(n),
        _ => Err(format!("{version:?} is not a MAJOR.MINOR.PATCH version")),
    };
    let (major, minor, patch) = (part()?, part()?, part()?);
    Ok(match level {
        "major" => format!("{}.0.0", major + 1),
        "minor" => format!("{major}.{}.0", minor + 1),
        _ => format!("{major}.{minor}.{}", patch + 1),
    })
}

/// `git <args>` in `cwd`, its standard output when it exits 0. Standard error is dropped.
fn git(cwd: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    command.args(args).stdin(Stdio::null()).stderr(Stdio::null());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let out = command.output().map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {} failed ({})", args.join(" "), out.status));
    }
    String::from_utf8(out.stdout).map_err(|_| format!("git {} printed something that is not UTF-8", args.join(" ")))
}

/// Full messages of the commits being released: everything since the last release tag, so commits
/// from a push whose run failed are not lost. Without a tag, the pushed range `before..after`; a
/// branch's first push has no `before` (git sends all zeros), so only `after` itself counts then.
pub fn messages_between(before: Option<&str>, after: &str, cwd: Option<&Path>) -> Result<Vec<String>, String> {
    let range: Vec<String> = match git(cwd, &["describe", "--tags", "--abbrev=0", "--match", "v[0-9]*", after]) {
        Ok(tag) => vec![format!("{}..{after}", tag.trim())],
        Err(_) => match before {
            Some(before) if !before.is_empty() && !before.bytes().all(|b| b == b'0') => {
                vec![format!("{before}..{after}")]
            }
            _ => vec![after.to_string(), "-1".to_string()],
        },
    };
    let mut args = vec!["log", "--format=%B%x00"];
    args.extend(range.iter().map(String::as_str));
    Ok(git(cwd, &args)?.split('\0').filter(|m| !m.trim().is_empty()).map(str::to_string).collect())
}

/// Where the top-level `"version"` string of a package.json sits: the byte range of its contents,
/// between the quotes. Only the top-level object's own key counts, never one nested deeper.
pub fn version_span(text: &str) -> Option<Range<usize>> {
    let bytes = text.as_bytes();
    // The open containers, innermost last: `{` or `[`.
    let mut stack: Vec<u8> = Vec::new();
    // Inside an object, whether the next string is a key.
    let mut want_key = false;
    let mut i = 0;
    // The end of the string starting at `from` (the index just past its closing quote).
    let string_end = |from: usize| -> Option<usize> {
        let mut j = from + 1;
        while j < bytes.len() {
            match bytes[j] {
                b'\\' => j += 2,
                b'"' => return Some(j + 1),
                _ => j += 1,
            }
        }
        None
    };
    let skip_ws = |mut j: usize| {
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        j
    };
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                stack.push(b'{');
                want_key = true;
                i += 1;
            }
            b'[' => {
                stack.push(b'[');
                want_key = false;
                i += 1;
            }
            b'}' | b']' => {
                stack.pop();
                want_key = false;
                i += 1;
            }
            b',' => {
                want_key = stack.last() == Some(&b'{');
                i += 1;
            }
            b'"' => {
                let end = string_end(i)?;
                if want_key && stack.len() == 1 && &text[i..end] == "\"version\"" {
                    let colon = skip_ws(end);
                    if bytes.get(colon) != Some(&b':') {
                        return None;
                    }
                    let value = skip_ws(colon + 1);
                    if bytes.get(value) != Some(&b'"') {
                        return None;
                    }
                    return Some(value + 1..string_end(value)? - 1);
                }
                want_key = false;
                i = end;
            }
            _ => i += 1,
        }
    }
    None
}

/// The `version` of a package.json's text. A version holding an escape or a control character is
/// not one a release would carry, and is refused.
pub fn package_version(text: &str) -> Result<String, String> {
    let span = version_span(text).ok_or("package.json has no top-level \"version\" string")?;
    let version = &text[span];
    if version.is_empty() || version.chars().any(|c| c == '\\' || c.is_control()) {
        return Err(format!("package.json has an unusable version: {version:?}"));
    }
    Ok(version.to_string())
}

/// `text` with its top-level version replaced by `version`, everything else untouched.
pub fn with_version(text: &str, version: &str) -> Result<String, String> {
    let span = version_span(text).ok_or("package.json has no top-level \"version\" string")?;
    Ok(format!("{}{version}{}", &text[..span.start], &text[span.end..]))
}

/// `cargo xtask bump-version [<before>] [<after>] [--dry-run]`: works out the next version from
/// the commits being released (from the last `v*` tag; with no tag, from `<before>`; with neither, just `<after>`), writes it to the root package.json unless `--dry-run`, and prints
/// only the version (the release job reads it from standard output).
pub fn main(args: &[String]) -> Result<(), String> {
    let mut positional = Vec::new();
    let mut write = true;
    for arg in args {
        match arg.as_str() {
            "--dry-run" => write = false,
            "--write" => write = true,
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => positional.push(other),
        }
    }
    if positional.len() > 2 {
        return Err("usage: cargo xtask bump-version [<before>] [<after>] [--dry-run]".into());
    }
    let before = positional.first().copied();
    let after = positional.get(1).copied().unwrap_or("HEAD");
    let file = crate::repo_root().join("package.json");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let version = next_version(&package_version(&text)?, bump_level(&messages_between(before, after, None)?))?;
    if write {
        std::fs::write(&file, with_version(&text, &version)?)
            .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    }
    println!("{version}");
    Ok(())
}
