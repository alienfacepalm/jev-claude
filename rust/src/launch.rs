//! Finding and starting Claude Code (SPEC 10.2; `node/src/launch.mjs`).

use crate::envx;
use regex::bytes::Regex;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

/// The names of the `claude` CLI's own subcommands, as of Claude Code 2.1.292 (SPEC 10.3).
const CLAUDE_SUBCOMMANDS: [&str; 21] = [
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
];

/// `isClaudeSubcommand(args)`: whether `args` run a `claude` subcommand, that is, the first
/// argument is exactly one of its names (case-sensitive, the whole argument).
pub fn is_claude_subcommand(args: &[String]) -> bool {
    args.first().is_some_and(|first| CLAUDE_SUBCOMMANDS.contains(&first.as_str()))
}

/// `resolveCommand(name, {exts, path, win})`.
pub fn resolve_command(name: &str, exts: Option<&[&str]>, path: Option<&str>, win: bool) -> Option<PathBuf> {
    let path = path.map_or_else(|| envx::get("PATH").unwrap_or_default(), str::to_string);
    let pathext;
    let suffixes: Vec<&str> = if win {
        if let Some(e) = exts {
            e.to_vec()
        } else {
            pathext = envx::get("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());
            pathext.split(';').collect()
        }
    } else {
        vec![""]
    };
    for dir in path.split(if win { ';' } else { ':' }) {
        if dir.is_empty() {
            continue;
        }
        let dir = dir.strip_prefix('"').unwrap_or(dir);
        let dir = dir.strip_suffix('"').unwrap_or(dir);
        for ext in &suffixes {
            let file = Path::new(dir).join(format!("{name}{ext}"));
            if win {
                if std::fs::metadata(&file).is_ok() {
                    return Some(file);
                }
            } else if is_executable(&file) {
                return Some(file);
            }
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(file: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(file).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(file: &Path) -> bool {
    std::fs::metadata(file).is_ok()
}

static SHIM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i-u)"%~?dp0%?\\([^"]+?\.[cm]?js)""#).unwrap());

/// The Node script an npm `.cmd` shim launches, or None.
pub fn shim_script(file: &Path) -> Option<PathBuf> {
    let text = std::fs::read(file).ok()?;
    let text = String::from_utf8_lossy(&text);
    let c = SHIM.captures(text.as_bytes())?;
    let rel = String::from_utf8_lossy(&c[1]).into_owned();
    let mut script = file.parent()?.to_path_buf();
    for part in rel.split('\\') {
        if part == ".." {
            script.pop();
        } else if !part.is_empty() && part != "." {
            script.push(part);
        }
    }
    std::fs::metadata(&script).ok()?;
    Some(script)
}

const META: &[u8] = b"()[]%!^\"`<>&|;, *?";

fn caret_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        if c.is_ascii() && META.contains(&(c as u8)) {
            out.push('^');
        }
        out.push(c);
    }
    out
}

/// Quotes one argument for a command line that cmd.exe parses and a batch file re-parses.
pub fn quote_for_cmd(arg: &str) -> String {
    // MSVCRT quoting: double the backslashes before each quote and escape the quote, then
    // double a trailing run of backslashes.
    let mut quoted = String::new();
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2));
                quoted.push_str("\\\"");
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                quoted.push(c);
                backslashes = 0;
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    caret_escape(&caret_escape(&format!("\"{quoted}\"")))
}

/// How to start a resolved executable.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchSpec {
    /// The program to run.
    pub command: PathBuf,
    /// Arguments placed before the user's (for example a shim's script path).
    pub prefix: Vec<String>,
    /// A `.cmd`/`.bat` shim with no script to run directly, run through cmd.exe verbatim.
    pub shim: Option<PathBuf>,
}

fn has_ext(file: &Path, exts: &[&str]) -> bool {
    let s = file.to_string_lossy().to_ascii_lowercase();
    exts.iter().any(|e| s.ends_with(e))
}

/// `launchSpec(file)`. Node runs a shim's script with its own executable; this port uses `node`
/// from PATH and falls back to the cmd.exe route when there is none.
pub fn launch_spec(file: &Path) -> LaunchSpec {
    if has_ext(file, &[".ps1"]) {
        return LaunchSpec {
            command: "powershell.exe".into(),
            prefix: vec![
                "-NoProfile".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-File".into(),
                file.to_string_lossy().into_owned(),
            ],
            shim: None,
        };
    }
    if has_ext(file, &[".cmd", ".bat"]) {
        if let (Some(script), Some(node)) = (shim_script(file), resolve_command("node", None, None, cfg!(windows))) {
            return LaunchSpec { command: node, prefix: vec![script.to_string_lossy().into_owned()], shim: None };
        }
        let comspec = envx::get("ComSpec").unwrap_or_else(|| "cmd.exe".to_string());
        return LaunchSpec { command: comspec.into(), prefix: vec![], shim: Some(file.to_path_buf()) };
    }
    LaunchSpec { command: file.to_path_buf(), prefix: vec![], shim: None }
}

/// The `Command` for a launch spec with `args`, never through an implicit shell.
pub fn command_for(spec: &LaunchSpec, args: &[String]) -> Command {
    let mut cmd = Command::new(&spec.command);
    match &spec.shim {
        None => {
            cmd.args(&spec.prefix).args(args);
        }
        Some(shim) => {
            let mut line = caret_escape(&shim.to_string_lossy());
            for a in args {
                line.push(' ');
                line.push_str(&quote_for_cmd(a));
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.raw_arg("/d /s /c").raw_arg(format!("\"{line}\""));
            }
            #[cfg(not(windows))]
            {
                cmd.args(["/d", "/s", "/c", &format!("\"{line}\"")]);
            }
        }
    }
    cmd
}
