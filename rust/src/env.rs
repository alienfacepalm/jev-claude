//! Loading jev's settings files (SPEC 9.1; `node/src/env.mjs`), with Node's `util.parseEnv`
//! ported line for line from `conformance/reference/node_dotenv_parse_content.cc`.

use crate::envx::{Env, EnvMut};
use std::collections::BTreeMap;
use std::path::Path;

/// Keys only jev itself reads, removed from the environment handed to Claude Code.
pub const PRIVATE_KEYS: [&str; 2] = ["JEV_API_KEY", "TYPESAFE_API_KEY"];

const PROJECT_KEYS: [&str; 6] =
    ["JEV_API_KEY", "TYPESAFE_API_KEY", "JEV_DEBUG", "JEV_ALLOW_FABLE", "JEV_NO_STATUSLINE", "JEV_ICONS"];

/// `^JEV_(?:[A-Z]+_)?(?:FORCE_)?EFFORT$`.
fn is_effort_key(key: &str) -> bool {
    let Some(rest) = key.strip_prefix("JEV_") else { return false };
    let Some(rest) = rest.strip_suffix("EFFORT") else { return false };
    if rest.is_empty() || rest == "FORCE_" {
        return true;
    }
    // `[A-Z]+_` optionally followed by `FORCE_`.
    let body = rest.strip_suffix("FORCE_").filter(|b| !b.is_empty() && b.ends_with('_'));
    let candidates = [Some(rest), body];
    candidates.into_iter().flatten().any(|c| {
        c.strip_suffix('_').is_some_and(|name| !name.is_empty() && name.bytes().all(|b| b.is_ascii_uppercase()))
    })
}

fn is_project_key(key: &str) -> bool {
    PROJECT_KEYS.contains(&key) || is_effort_key(key)
}

/// `trim_spaces`: strips only space, tab, and newline.
fn trim_spaces(input: &[u8]) -> &[u8] {
    let is = |c: &u8| matches!(c, b' ' | b'\t' | b'\n');
    let Some(start) = input.iter().position(|c| !is(c)) else { return &[] };
    let end = input.iter().rposition(|c| !is(c)).unwrap();
    &input[start..=end]
}

fn find(hay: &[u8], c: u8, from: usize) -> Option<usize> {
    hay.get(from..)?.iter().position(|x| *x == c).map(|p| p + from)
}

/// `Dotenv::ParseContent` over a file's text. A repeated key keeps its last value.
pub fn parse_env(input: &str) -> BTreeMap<String, String> {
    let mut store: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    let lines: Vec<u8> = input.bytes().filter(|c| *c != b'\r').collect();
    let mut content: &[u8] = trim_spaces(&lines);

    while !content.is_empty() {
        if content[0] == b'\n' || content[0] == b'#' {
            match find(content, b'\n', 0) {
                Some(newline) => content = &content[newline + 1..],
                None => content = &[],
            }
            continue;
        }

        let equal_or_newline = content.iter().position(|c| *c == b'=' || *c == b'\n');
        match equal_or_newline {
            None => break,
            Some(i) if content[i] == b'\n' => {
                content = trim_spaces(&content[i + 1..]);
                continue;
            }
            Some(_) => {}
        }
        let equal = equal_or_newline.unwrap();

        let mut key = &content[..equal];
        content = &content[equal + 1..];
        key = trim_spaces(key);

        if content.is_empty() || content[0] == b'\n' {
            store.insert(key.to_vec(), Vec::new());
            continue;
        }

        content = trim_spaces(content);

        if key.is_empty() {
            continue;
        }

        if key.starts_with(b"export ") {
            key = trim_spaces(&key[7..]);
        }

        if content.is_empty() {
            store.insert(key.to_vec(), Vec::new());
            break;
        }

        let closing_double = if content[0] == b'"' { find(content, b'"', 1) } else { None };
        if let Some(closing) = closing_double {
            let value = &content[1..closing];
            let mut multi = Vec::with_capacity(value.len());
            let mut i = 0;
            while i < value.len() {
                if value[i] == b'\\' && value.get(i + 1) == Some(&b'n') {
                    multi.push(b'\n');
                    i += 2;
                } else {
                    multi.push(value[i]);
                    i += 1;
                }
            }
            store.insert(key.to_vec(), multi);
            match find(content, b'\n', closing + 1) {
                Some(newline) => content = &content[newline + 1..],
                None => content = &[],
            }
            continue;
        }

        if matches!(content[0], b'\'' | b'"' | b'`') {
            let quote = content[0];
            match find(content, quote, 1) {
                None => match find(content, b'\n', 0) {
                    Some(newline) => {
                        store.insert(key.to_vec(), content[..newline].to_vec());
                        content = &content[newline + 1..];
                    }
                    None => {
                        store.insert(key.to_vec(), content.to_vec());
                        break;
                    }
                },
                Some(closing) => {
                    store.insert(key.to_vec(), content[1..closing].to_vec());
                    match find(content, b'\n', closing + 1) {
                        Some(newline) => content = &content[newline + 1..],
                        None => content = &[],
                    }
                    continue;
                }
            }
        } else {
            match find(content, b'\n', 0) {
                Some(newline) => {
                    let mut value = &content[..newline];
                    if let Some(hash) = find(value, b'#', 0) {
                        value = &value[..hash];
                    }
                    store.insert(key.to_vec(), trim_spaces(value).to_vec());
                    content = &content[newline + 1..];
                }
                None => {
                    let mut value = content;
                    if let Some(hash) = find(value, b'#', 0) {
                        value = &content[..hash];
                    }
                    store.insert(key.to_vec(), trim_spaces(value).to_vec());
                    content = &[];
                }
            }
        }

        content = trim_spaces(content);
    }

    store
        .into_iter()
        .map(|(k, v)| (String::from_utf8_lossy(&k).into_owned(), String::from_utf8_lossy(&v).into_owned()))
        .collect()
}

fn read(file: &Path) -> BTreeMap<String, String> {
    match std::fs::read(file) {
        Ok(bytes) => parse_env(&String::from_utf8_lossy(&bytes)),
        Err(_) => BTreeMap::new(),
    }
}

/// `loadEnv({cwd, home, env})`: existing variables win, then the project `.env` (allow-listed
/// keys), then `~/.jev-router.env`, then `~/.jev-claude.env`.
pub fn load_env(cwd: &Path, home: &Path, env: &mut dyn EnvMut) {
    let project = read(&cwd.join(".env")).into_iter().filter(|(k, _)| is_project_key(k));
    let all: Vec<(String, String)> =
        project.chain(read(&home.join(".jev-router.env"))).chain(read(&home.join(".jev-claude.env"))).collect();
    for (key, value) in all {
        if !value.is_empty() && env.get(&key).is_none() {
            env.set(&key, &value);
        }
    }
}

/// `loadEnv()` for this process: the current directory and home.
pub fn load_process_env() {
    let cwd = std::env::current_dir().unwrap_or_default();
    let home = crate::osdirs::home_dir();
    load_env(&cwd, &home, &mut crate::envx::ProcessEnv);
}

/// `childEnv(env)`: a copy without the private keys.
pub fn child_env(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    env.iter().filter(|(k, _)| !PRIVATE_KEYS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// Whether `env` lacks a key, for `??=` (an empty-but-set value counts as present).
pub fn has(env: &dyn Env, key: &str) -> bool {
    env.get(key).is_some()
}
