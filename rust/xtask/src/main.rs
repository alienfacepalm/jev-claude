//! `cargo xtask <command>`: the developer and CI tasks of jev-claude. See the crate documentation.

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <command>

  bump-version [<before>] [<after>] [--dry-run]
      writes the next release version to package.json and prints it (the commits since the last v*
      tag; with no tag, since <before>; with neither, just <after>)
  package [--bin-dir <dir>] [--out <dir>] [--target <platform>] [--stage-only]
      packages built programs as jev-claude-<platform>.tar.gz
  installer-smoke-test [--bin-dir <dir> | --archive <file>] [--installers <dir>] [--only <group,...>]
      installs a release archive with the real installers in a throwaway home (--installers: use
      the install.sh and install.ps1 in <dir>; --only: run just those groups of cases)";

/// The stand-in programs the installer smoke test makes by copying this executable (see
/// [`xtask::smoke`]): what the copy does depends on its own file name, so one portable binary plays
/// Claude Code (or a broken one) and a `jev-check` that fails on every operating system.
fn stand_in() -> Option<ExitCode> {
    let exe = std::env::current_exe().ok()?;
    match exe.file_stem()?.to_str()? {
        // Claude Code answers `--version`; a `.broken` file beside the copy makes it fail silently instead.
        "claude" if exe.with_file_name(".broken").exists() => Some(ExitCode::FAILURE),
        "claude" => {
            println!("2.1.0 (Claude Code)");
            Some(ExitCode::SUCCESS)
        }
        "jev-check" => {
            println!("broken");
            Some(ExitCode::FAILURE)
        }
        _ => None,
    }
}

fn main() -> ExitCode {
    if let Some(code) = stand_in() {
        return code;
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("bump-version") => xtask::bump::main(&args[1..]),
        Some("package") => xtask::package::main(&args[1..]),
        Some("installer-smoke-test") => xtask::smoke::main(&args[1..]),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
