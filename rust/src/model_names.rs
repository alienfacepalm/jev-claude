//! A model's short display name with its version (SPEC 12.3; `node/src/model-names.mjs`).

use crate::jsstr::minor_after;
use regex::bytes::Regex;
use std::sync::LazyLock;

static SHORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"claude-([a-z]+)-([0-9]+)").unwrap());

/// `shortName(model)`: "Opus 5.5" for `claude-opus-5-5`, None for anything else.
pub fn short_name(model: &[u8]) -> Option<String> {
    let c = SHORT.captures(model)?;
    let family = std::str::from_utf8(&c[1]).ok()?;
    let major = std::str::from_utf8(&c[2]).ok()?;
    let end = c.get(2)?.end();
    let mut name = String::new();
    name.push_str(&family[..1].to_uppercase());
    name.push_str(&family[1..]);
    name.push(' ');
    name.push_str(major);
    if let Some(minor) = minor_after(model, end) {
        name.push('.');
        name.push_str(std::str::from_utf8(minor).ok()?);
    }
    Some(name)
}
