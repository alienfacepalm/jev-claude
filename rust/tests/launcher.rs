//! The launcher (SPEC 10): `is_claude_subcommand`, and the real `jev-claude` program run against a
//! stand-in `claude` found on PATH. Two stand-ins: a native one (a shell script on Unix, a copied
//! program as `claude.exe` on Windows), the form Anthropic's installer puts on PATH, which needs
//! nothing else; and an npm-style script, which needs `node` on PATH as an npm-installed Claude Code
//! does, so the tests that use it skip with a note without node.

// Each test's doc comment is its title as a plain sentence; Markdown backticks would change them.
#![allow(clippy::doc_markdown)]

mod common;

use common::*;
use jev_router::jsjson::Value;
use jev_router::launch::is_claude_subcommand;
use std::process::Command;

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
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
    launch_in(args, Stand::Npm, &[]).0
}

/// Which stand-in `claude` the launcher finds.
#[derive(Clone, Copy, PartialEq)]
enum Stand {
    /// The npm-style script ([`fake_claude`]), run through node.
    Npm,
    /// The native stand-in ([`native_claude`]), with only the system folders after it on PATH.
    #[cfg(unix)]
    Native,
}

/// [`launch`] with a choice of stand-in and extra variables; also returns the scratch directory,
/// which holds the status directory (`status/`) the launcher wrote to.
fn launch_in(args: &[&str], stand: Stand, extra: &[(&str, &str)]) -> (Seen, TempDir) {
    let base = temp_dir("jev-launcher-");
    let (bin, home, cwd) = (base.join("bin"), base.join("home"), base.join("cwd"));
    for dir in [&bin, &home, &cwd] {
        std::fs::create_dir(dir).unwrap();
    }
    let path = match stand {
        Stand::Npm => {
            fake_claude(&bin);
            path_with(&bin)
        }
        #[cfg(unix)]
        Stand::Native => {
            native_claude(&bin);
            std::env::join_paths([bin.clone(), "/usr/bin".into(), "/bin".into()]).unwrap()
        }
    };
    let mut command = Command::new(env!("CARGO_BIN_EXE_jev-claude"));
    command
        .args(args)
        .current_dir(&cwd)
        .env("PATH", path)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("JEV_API_KEY", "test-key")
        .env("JEV_STATUS_DIR", base.join("status"))
        .env_remove("JEV_ROOT")
        .env_remove("ANTHROPIC_BASE_URL")
        .env_remove("ANTHROPIC_MODEL")
        .env_remove("TYPESAFE_API_KEY")
        .env_remove("JEV_NO_STATUSLINE");
    for (k, v) in extra {
        command.env(k, v);
    }
    let out = command.output().unwrap();
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let seen = match stand {
        Stand::Npm => parse(&out.stdout, &out.stderr),
        #[cfg(unix)]
        Stand::Native => parse_native(&out.stdout, &out.stderr),
    };
    (seen, base)
}

/// What [`native_claude`] printed: `arg:<value>` and `env:<NAME>=<value>` lines.
#[cfg(unix)]
fn parse_native(stdout: &[u8], stderr: &[u8]) -> Seen {
    let text = String::from_utf8_lossy(stdout);
    let mut seen = Seen {
        args: vec![],
        base_url: None,
        model: None,
        key: None,
        stderr: String::from_utf8_lossy(stderr).into_owned(),
    };
    for line in text.lines() {
        if let Some(arg) = line.strip_prefix("arg:") {
            seen.args.push(arg.to_string());
        } else if let Some((name, value)) = line.strip_prefix("env:").and_then(|l| l.split_once('=')) {
            let value = (value != "<unset>").then(|| value.to_string());
            match name {
                "ANTHROPIC_BASE_URL" => seen.base_url = value,
                "ANTHROPIC_MODEL" => seen.model = value,
                "JEV_API_KEY" => seen.key = value,
                _ => {}
            }
        }
    }
    seen
}

/// The session assertions shared by both stand-ins: `--add-dir <repo>`, `--settings <file>` at the
/// end, the proxy's address, the routing model, and no Jev key.
fn assert_session(seen: &Seen) {
    let n = seen.args.len();
    assert!(n >= 4, "{:?}", seen.args);
    assert_eq!(seen.args[n - 4], "--add-dir");
    let root = std::fs::canonicalize(&seen.args[n - 3]).unwrap();
    assert_eq!(root, std::fs::canonicalize(repo_root()).unwrap());
    assert_eq!(seen.args[n - 2], "--settings");
    let url = seen.base_url.as_deref().expect("ANTHROPIC_BASE_URL");
    let port = url.strip_prefix("http://127.0.0.1:").unwrap_or_else(|| panic!("{url}"));
    assert!(!port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()), "{url}");
    assert_eq!(seen.model.as_deref(), Some("jev-router"));
    assert_eq!(seen.key, None, "the key is the launcher's alone");
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
    if !have_node("a_session_launch_gets_add_dir_the_settings_file_and_the_proxy") {
        return;
    }
    assert_session(&launch(&[]));
}

/// A native claude session launch needs no node
#[cfg(unix)]
#[test]
fn a_native_claude_session_launch_needs_no_node() {
    let (seen, _base) = launch_in(&[], Stand::Native, &[]);
    assert_session(&seen);
    let (seen, _base) = launch_in(&["mcp", "list"], Stand::Native, &[]);
    assert_eq!(seen.args, ["mcp", "list"]);
    assert_eq!(seen.base_url, None);
    assert_eq!(seen.model, None);
    assert_eq!(seen.key, None, "the key is stripped from the subcommand's environment too");
}

/// A native claude.exe is found through PATHEXT and run directly
#[cfg(windows)]
#[test]
fn a_native_claude_exe_is_found_through_pathext_and_run_directly() {
    use jev_router::launch::{LaunchSpec, launch_spec};
    let base = temp_dir("jev-launcher-exe-");
    let (bin, later, home, cwd) = (base.join("bin"), base.join("later"), base.join("home"), base.join("cwd"));
    for dir in [&bin, &later, &home, &cwd] {
        std::fs::create_dir(dir).unwrap();
    }
    // jev-check stands in for Claude Code: it prints the routing state the child was started with.
    let exe = bin.join("claude.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_jev-check"), &exe).unwrap();
    // A .cmd in a later folder must not win over the .exe found first.
    std::fs::write(later.join("claude.cmd"), "@ECHO off\r\necho wrong claude\r\n").unwrap();
    assert_eq!(launch_spec(&exe), LaunchSpec { command: exe.clone(), prefix: vec![], shim: None });
    let system = std::env::var_os("SystemRoot").map_or_else(|| "C:\\Windows".into(), std::path::PathBuf::from);
    let path = std::env::join_paths([bin.clone(), later.clone(), system.join("System32")]).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_jev-claude"))
        .current_dir(&cwd)
        .env("PATH", path)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("JEV_API_KEY", "test-key")
        .env("JEV_STATUS_DIR", base.join("status"))
        .env_remove("JEV_ROOT")
        .env_remove("ANTHROPIC_BASE_URL")
        .env_remove("ANTHROPIC_MODEL")
        .env_remove("TYPESAFE_API_KEY")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("Jev Router setup check"), "the copied program ran: {stdout}");
    assert!(stdout.contains("on - Jev Router picks a model"), "the child got the routing environment: {stdout}");
    assert!(!stdout.contains("test-key"), "{stdout}");
    assert!(!stdout.contains("wrong claude"), "{stdout}");
}

/// The settings file's status line runs through the shell
#[cfg(unix)]
#[test]
fn the_settings_file_status_line_runs_through_the_shell() {
    let (seen, base) = launch_in(&[], Stand::Native, &[]);
    let n = seen.args.len();
    assert_eq!(seen.args[n - 2], "--settings");
    let file = std::path::PathBuf::from(&seen.args[n - 1]);
    assert_eq!(file, base.join("status").join("settings.json"));
    let command = status_line_command(&file);
    let mut sh = Command::new("sh");
    sh.args(["-c", &command]);
    run_status_line(&base, sh);
}

/// The settings file's status line runs through the shells Claude Code uses on Windows
#[cfg(windows)]
#[test]
fn the_settings_file_status_line_runs_through_cmd_and_git_bash() {
    if !have_node("the_settings_file_status_line_runs_through_cmd_and_git_bash") {
        return;
    }
    let (seen, base) = launch_in(&[], Stand::Npm, &[]);
    let n = seen.args.len();
    let file = std::path::PathBuf::from(&seen.args[n - 1]);
    let command = status_line_command(&file);
    // cmd.exe takes its command line as written, not with the C runtime's quoting.
    let mut cmd = Command::new("cmd");
    std::os::windows::process::CommandExt::raw_arg(&mut cmd, format!("/d /s /c \"{command}\""));
    run_status_line(&base, cmd);
    match jev_router::launch::resolve_command("bash", None, None, true) {
        Some(bash) => {
            let mut bash = Command::new(bash);
            bash.args(["-c", &command]);
            run_status_line(&base, bash);
        }
        None => eprintln!("note: Git Bash leg skipped: `bash` is not on PATH"),
    }
}

/// The statusLine command in the settings file the launcher wrote: a quoted path to the
/// `jev-statusline` beside `jev-claude`.
fn status_line_command(file: &std::path::Path) -> String {
    let settings = jev_router::jsjson::parse_bytes(&std::fs::read(file).unwrap()).unwrap();
    let line = settings.get("statusLine");
    assert_eq!(text(line.get("type")).as_deref(), Some("command"));
    let command = text(line.get("command")).expect("a command");
    let inner = command.strip_prefix('"').and_then(|c| c.strip_suffix('"')).unwrap_or_else(|| panic!("{command}"));
    let name = if cfg!(windows) { "jev-statusline.exe" } else { "jev-statusline" };
    let want = std::path::Path::new(env!("CARGO_BIN_EXE_jev-claude")).with_file_name(name);
    assert_eq!(std::fs::canonicalize(inner).unwrap(), std::fs::canonicalize(want).unwrap());
    command
}

/// Runs the status line command through a shell with a session on stdin, as Claude Code does.
fn run_status_line(base: &std::path::Path, mut shell: Command) {
    use std::io::Write;
    let name = shell.get_program().to_string_lossy().into_owned();
    let mut child = shell
        .env("JEV_STATUS_DIR", base.join("status"))
        .env("HOME", base.join("home"))
        .env("USERPROFILE", base.join("home"))
        .env("JEV_ICONS", "text")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"session_id":"shell-check","workspace":{"current_dir":"/work/proj"},"context_window":{"used_percentage":8}}"#)
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{name}: {}", String::from_utf8_lossy(&out.stderr));
    assert!(!out.stdout.is_empty(), "{name}: the status line printed nothing");
}

/// JEV_NO_STATUSLINE=1 writes no settings
#[cfg(unix)]
#[test]
fn jev_no_statusline_writes_no_settings() {
    let (seen, base) = launch_in(&[], Stand::Native, &[("JEV_NO_STATUSLINE", "1")]);
    assert!(!seen.args.iter().any(|a| a == "--settings"), "{:?}", seen.args);
    assert!(!base.join("status").join("settings.json").exists());
    assert_eq!(seen.model.as_deref(), Some("jev-router"), "routing is still on");
}

/// A prompt that starts with a subcommand's name is still a session
#[test]
fn a_prompt_that_starts_with_a_subcommands_name_is_still_a_session() {
    if !have_node("a_prompt_that_starts_with_a_subcommands_name_is_still_a_session") {
        return;
    }
    let seen = launch(&["update the docs"]);
    assert_eq!(seen.args[0], "update the docs");
    assert!(seen.args.iter().any(|a| a == "--add-dir"), "{:?}", seen.args);
    assert!(seen.base_url.is_some_and(|u| u.starts_with("http:")));
}

/// Claude subcommands run untouched: no --add-dir, no proxy, no routing model, no key
#[test]
fn claude_subcommands_run_untouched() {
    if !have_node("claude_subcommands_run_untouched") {
        return;
    }
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
    if !have_node("a_subcommand_without_a_key_does_not_announce_that_routing_is_off") {
        return;
    }
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

/// A malformed ANTHROPIC_BASE_URL stops the launcher before Claude Code starts
#[cfg(unix)]
#[test]
fn a_malformed_anthropic_base_url_stops_the_launcher_before_claude_code_starts() {
    let base = temp_dir("jev-launcher-badurl-");
    let (bin, home) = (base.join("bin"), base.join("home"));
    for dir in [&bin, &home] {
        std::fs::create_dir(dir).unwrap();
    }
    native_claude(&bin);
    let out = Command::new(env!("CARGO_BIN_EXE_jev-claude"))
        .current_dir(&home)
        .env("PATH", std::env::join_paths([bin.clone(), "/usr/bin".into(), "/bin".into()]).unwrap())
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("JEV_API_KEY", "test-key")
        .env("JEV_STATUS_DIR", base.join("status"))
        .env("ANTHROPIC_BASE_URL", "localhost:4000")
        .env_remove("JEV_ROOT")
        .env_remove("ANTHROPIC_MODEL")
        .env_remove("JEV_DEBUG")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "[jev] invalid upstream URL (ANTHROPIC_BASE_URL): localhost:4000\n"
    );
    assert!(out.stdout.is_empty(), "Claude Code never ran: {}", String::from_utf8_lossy(&out.stdout));
}

/// The default status directory is per user on Unix
// On Linux and WSL the temp directory is the shared /tmp: a single `jev-claude` there, owner-only,
// would lock out every user but the first.
#[cfg(unix)]
#[test]
fn the_default_status_directory_is_per_user() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let base = temp_dir("jev-launcher-tmpdir-");
    let (bin, home, tmp) = (base.join("bin"), base.join("home"), base.join("tmp"));
    for dir in [&bin, &home, &tmp] {
        std::fs::create_dir(dir).unwrap();
    }
    native_claude(&bin);
    let out = Command::new(env!("CARGO_BIN_EXE_jev-claude"))
        .current_dir(&home)
        .env("PATH", std::env::join_paths([bin.clone(), "/usr/bin".into(), "/bin".into()]).unwrap())
        .env("HOME", &home)
        .env("TMPDIR", &tmp)
        .env("JEV_API_KEY", "test-key")
        .env_remove("JEV_STATUS_DIR")
        .env_remove("TMP")
        .env_remove("TEMP")
        .env_remove("JEV_ROOT")
        .env_remove("ANTHROPIC_BASE_URL")
        .env_remove("ANTHROPIC_MODEL")
        .env_remove("JEV_NO_STATUSLINE")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let uid = std::fs::metadata(&home).unwrap().uid();
    let dir = tmp.join(format!("jev-claude-{uid}"));
    assert!(dir.join("settings.json").is_file(), "the settings file is in {}", dir.display());
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    assert!(!tmp.join("jev-claude").exists(), "no shared folder");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let settings = dir.join("settings.json");
    assert!(stdout.lines().any(|l| l == format!("arg:{}", settings.display())), "{stdout}");
}
