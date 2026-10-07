//! Port of node/test/launcher.test.mjs: `is_claude_subcommand`, and the real `jev-claude` program
//! run against a stand-in `claude` found on PATH. Needs `node` on PATH, as the other tests do.

mod common;

use common::*;
use jev_router::jsjson::Value;
use jev_router::launch::is_claude_subcommand;
use std::path::{Path, PathBuf};
use std::process::Command;

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

/// What the stand-in prints: the arguments and the variables that say whether a session was set up.
const FAKE_CLAUDE: &str = r#"process.stdout.write(JSON.stringify({
  args: process.argv.slice(2),
  env: Object.fromEntries(
    ["ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL", "JEV_API_KEY"].map((k) => [k, process.env[k] ?? null]),
  ),
}));
"#;

/// A `claude` the launcher finds on PATH: an npm-style `.cmd` shim on Windows (the form the launcher
/// runs directly through node), an executable script elsewhere.
fn fake_claude(dir: &Path) {
    #[cfg(windows)]
    {
        std::fs::write(dir.join("claude-fake.mjs"), FAKE_CLAUDE).unwrap();
        std::fs::write(dir.join("claude.cmd"), "@ECHO off\r\n\"node\"  \"%~dp0\\claude-fake.mjs\" %*\r\n").unwrap();
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let file = dir.join("claude");
        std::fs::write(&file, format!("#!/usr/bin/env node\n{FAKE_CLAUDE}")).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// PATH with `dir` first.
fn path_with(dir: &Path) -> std::ffi::OsString {
    let mut paths = vec![PathBuf::from(dir)];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    std::env::join_paths(paths).unwrap()
}

struct Seen {
    args: Vec<String>,
    base_url: Option<String>,
    model: Option<String>,
    key: Option<String>,
    stderr: String,
}

fn text(value: &Value) -> Option<String> {
    value.as_str().map(jev_router::jsstr::JsStr::to_string_lossy)
}

fn parse(stdout: &[u8], stderr: &[u8]) -> Seen {
    let seen =
        jev_router::jsjson::parse_bytes(stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(stdout)));
    let env = seen.get("env");
    Seen {
        args: seen.get("args").as_array().unwrap().iter().map(|a| text(a).unwrap()).collect(),
        base_url: text(env.get("ANTHROPIC_BASE_URL")),
        model: text(env.get("ANTHROPIC_MODEL")),
        key: text(env.get("JEV_API_KEY")),
        stderr: String::from_utf8_lossy(stderr).into_owned(),
    }
}

/// Runs the real launcher in an empty home and working directory, with a Jev key set.
fn launch(args: &[&str]) -> Seen {
    let base = temp_dir("jev-launcher-");
    let (bin, home, cwd) = (base.join("bin"), base.join("home"), base.join("cwd"));
    for dir in [&bin, &home, &cwd] {
        std::fs::create_dir(dir).unwrap();
    }
    fake_claude(&bin);
    let out = Command::new(env!("CARGO_BIN_EXE_jev-claude"))
        .args(args)
        .current_dir(&cwd)
        .env("PATH", path_with(&bin))
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("JEV_API_KEY", "test-key")
        .env("JEV_STATUS_DIR", base.join("status"))
        .env_remove("JEV_ROOT")
        .env_remove("ANTHROPIC_BASE_URL")
        .env_remove("ANTHROPIC_MODEL")
        .env_remove("TYPESAFE_API_KEY")
        .env_remove("JEV_NO_STATUSLINE")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    parse(&out.stdout, &out.stderr)
}

/// Only the first argument, spelled exactly, makes a claude subcommand
#[test]
fn only_the_first_argument_spelled_exactly_makes_a_claude_subcommand() {
    for name in ["mcp", "plugin", "plugins", "doctor", "update", "upgrade", "auth", "agents", "kill", "stop"] {
        assert!(is_claude_subcommand(&strings(&[name, "x"])), "{name}");
    }
    for name in [
        "agents",
        "attach",
        "auth",
        "auto-mode",
        "doctor",
        "gateway",
        "import",
        "install",
        "kill",
        "logs",
        "mcp",
        "plugin",
        "plugins",
        "purge",
        "respawn",
        "rm",
        "setup-token",
        "stop",
        "ultrareview",
        "update",
        "upgrade",
    ] {
        assert!(is_claude_subcommand(&strings(&[name])), "{name}");
    }
    assert!(!is_claude_subcommand(&[]));
    assert!(!is_claude_subcommand(&strings(&["update the docs"])), "a prompt that starts with the word");
    assert!(!is_claude_subcommand(&strings(&["-p", "mcp"])), "the word later on");
    assert!(!is_claude_subcommand(&strings(&["--model", "opus", "mcp", "list"])));
    assert!(!is_claude_subcommand(&strings(&["MCP"])));
    assert!(!is_claude_subcommand(&strings(&["--model", "opus"])));
}

/// A session launch gets --add-dir, the settings file and the proxy
#[test]
fn a_session_launch_gets_add_dir_the_settings_file_and_the_proxy() {
    let seen = launch(&[]);
    let n = seen.args.len();
    assert!(n >= 4, "{:?}", seen.args);
    assert_eq!(seen.args[n - 4], "--add-dir");
    let root = std::fs::canonicalize(&seen.args[n - 3]).unwrap();
    assert_eq!(root, std::fs::canonicalize(repo_root()).unwrap());
    assert_eq!(seen.args[n - 2], "--settings");
    let url = seen.base_url.expect("ANTHROPIC_BASE_URL");
    let port = url.strip_prefix("http://127.0.0.1:").unwrap_or_else(|| panic!("{url}"));
    assert!(!port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()), "{url}");
    assert_eq!(seen.model.as_deref(), Some("jev-router"));
    assert_eq!(seen.key, None, "the key is the launcher's alone");
}

/// A prompt that starts with a subcommand's name is still a session
#[test]
fn a_prompt_that_starts_with_a_subcommands_name_is_still_a_session() {
    let seen = launch(&["update the docs"]);
    assert_eq!(seen.args[0], "update the docs");
    assert!(seen.args.iter().any(|a| a == "--add-dir"), "{:?}", seen.args);
    assert!(seen.base_url.is_some_and(|u| u.starts_with("http:")));
}

/// Claude subcommands run untouched: no --add-dir, no proxy, no routing model, no key
#[test]
fn claude_subcommands_run_untouched() {
    for args in [vec!["mcp", "list"], vec!["plugin", "install", "x", "--scope", "user"], vec!["doctor"], vec!["update"]]
    {
        let seen = launch(&args);
        assert_eq!(seen.args, args);
        assert_eq!(seen.base_url, None, "{args:?}");
        assert_eq!(seen.model, None, "{args:?}");
        assert_eq!(seen.key, None, "the key is stripped from the subcommand's environment too");
    }
}

/// A subcommand without a key does not announce that routing is off
#[test]
fn a_subcommand_without_a_key_does_not_announce_that_routing_is_off() {
    // No key: a subcommand is not a session, so there is nothing to say about routing.
    let base = temp_dir("jev-launcher-nokey-");
    let bin = base.join("bin");
    std::fs::create_dir(&bin).unwrap();
    fake_claude(&bin);
    let out = Command::new(env!("CARGO_BIN_EXE_jev-claude"))
        .args(["mcp", "list"])
        .current_dir(base.path())
        .env("PATH", path_with(&bin))
        .env("HOME", base.path())
        .env("USERPROFILE", base.path())
        .env("JEV_STATUS_DIR", base.join("status"))
        .env_remove("JEV_ROOT")
        .env_remove("JEV_API_KEY")
        .env_remove("TYPESAFE_API_KEY")
        .env_remove("ANTHROPIC_BASE_URL")
        .env_remove("ANTHROPIC_MODEL")
        .output()
        .unwrap();
    let seen = parse(&out.stdout, &out.stderr);
    assert!(out.status.success(), "{}", seen.stderr);
    assert!(!seen.stderr.contains("no JEV_API_KEY"), "{}", seen.stderr);
    assert_eq!(seen.args, ["mcp", "list"]);
}
