//! Packages the Rust programs into the archive the installers download:
//!
//! ```text
//! cargo xtask package --bin-dir target/release --out ../dist [--target windows-x64] [--stage-only]
//! ```
//!
//! The archive unpacks into one folder laid out like a clone as far as the programs care
//! (SPEC.md 2.4 finds the root by walking up from bin/ to the nearest .claude/skills/jev-calibrate):
//!
//! ```text
//! bin/                       the seven programs
//! .claude/skills/            /jev-calibrate, /jev-explain, /jev-legend, calling those programs
//! package.json               just the name and the release version (SPEC.md 2.3)
//! .env.example  LICENSE
//! ```
//!
//! The skills are the repository's own, which already call the programs by name: they are copied,
//! less the part of /jev-calibrate that re-tunes the router, which needs a clone. A skill that would
//! not work from an installed copy fails the build. `--stage-only` leaves the folder and skips the tar.

use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The seven programs every release carries.
pub const PROGRAMS: [&str; 7] =
    ["jev-claude", "jev-statusline", "jev-explain", "jev-legend", "jev-check", "jev-update-check", "jev-proxy-host"];

/// Where the re-tuning part of /jev-calibrate starts; it and everything after it are left out.
const RETUNE_HEADING: &str = "\n## Repository: re-calibrate";
/// The sentence of /jev-calibrate's description that only holds in a clone.
const RETUNE_SENTENCE: &str = " In the router's own repository, `retune` re-tunes it for new Claude models.";

/// The archive names (`jev-claude-<target>.tar.gz`) the release can carry: the only ones the
/// installers ask for. install.sh lists them in its platform check, install.ps1 asks for
/// `windows-x64`, and the `binaries` matrix in ci.yml builds each one (tests/package.rs ties the
/// four together).
pub const TARGETS: [&str; 5] = ["linux-x64", "linux-arm64", "macos-x64", "macos-arm64", "windows-x64"];

/// The release target of the machine running this: the platform name used in archive names, as
/// the installers pick it. Windows on ARM runs the x64 programs through emulation, so it takes
/// `windows-x64` (there is no ARM64 build).
pub fn host_target() -> Result<String, String> {
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64" | "aarch64") => "windows-x64",
        ("macos", "x86_64") => "macos-x64",
        ("macos", "aarch64") => "macos-arm64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        (os, arch) => return Err(format!("no release target for {os}/{arch}")),
    };
    Ok(target.to_string())
}

/// One repository skill as an installed copy ships it. The skill calls one of the programs by name
/// (on PATH after install, and the same line runs under Bash or PowerShell), and `allowed-tools`
/// lets exactly that program run in either shell. Re-tuning (`retune`) edits the repository's
/// sources, so that section is left out. Fails when the skill does not look like that, still
/// names a Node script, or still mentions re-tuning, so a changed skill fails the build instead of
/// shipping broken.
pub fn packaged_skill(text: &str, skill: &str) -> Result<String, String> {
    let program = text
        .split('\n')
        .filter_map(|line| line.strip_prefix("!`"))
        .map(|call| call.split([' ', '`']).next().unwrap_or_default())
        .find(|program| !program.is_empty())
        .ok_or_else(|| format!("{skill}: no !`<program>` line found"))?;
    if !PROGRAMS.contains(&program) {
        return Err(format!("{skill}: calls {program}, which is not a shipped program"));
    }
    let allowed = format!("allowed-tools: Bash({program} *) PowerShell({program} *)");
    if !text.split('\n').any(|line| line == allowed) {
        return Err(format!("{skill}: allowed-tools is not \"{allowed}\""));
    }
    let mut out = text.to_string();
    // Re-tuning edits the repository; an installed copy only ever reports.
    if let Some(retune) = out.find(RETUNE_HEADING) {
        out = format!("{}\n", out[..retune].trim_end());
        out = out.replacen(RETUNE_SENTENCE, "", 1);
    }
    if out.contains(".mjs") || out.contains("node/") {
        return Err(format!("{skill}: still refers to the Node implementation"));
    }
    // A renamed heading or reworded sentence would otherwise ship the clone-only part silently.
    if out.contains(RETUNE_HEADING) || out.contains("re-tunes it for new Claude models") {
        return Err(format!(
            "{skill}: the re-tuning part was not left out (was its heading{RETUNE_HEADING:?} or the description \
             sentence reworded?)"
        ));
    }
    Ok(out)
}

/// The release version, from `<root>/package.json` (SPEC.md 2.3).
pub fn release_version(root: &Path) -> Result<String, String> {
    let file = root.join("package.json");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    crate::bump::package_version(&text)
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to).map(drop).map_err(|e| format!("cannot copy {} to {}: {e}", from.display(), to.display()))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn create_dir_all(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("cannot create {}: {e}", path.display()))
}

/// Builds the folder that the archive unpacks into, from the repository at `root`. `bin_dir` holds
/// the built programs; the target says whether they carry `.exe`. Returns the release version.
pub fn stage_bundle(bin_dir: &Path, stage_dir: &Path, target: &str, root: &Path) -> Result<String, String> {
    let exe = if target.starts_with("windows") { ".exe" } else { "" };
    match std::fs::remove_dir_all(stage_dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("cannot remove {}: {e}", stage_dir.display())),
    }
    create_dir_all(&stage_dir.join("bin"))?;
    for program in PROGRAMS {
        let from = bin_dir.join(format!("{program}{exe}"));
        let to = stage_dir.join("bin").join(format!("{program}{exe}"));
        if std::fs::copy(&from, &to).is_err() {
            return Err(format!(
                "{} is missing: build with \"cargo build --release --locked\" in rust/ first",
                from.display()
            ));
        }
        #[cfg(unix)]
        if exe.is_empty() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("cannot make {} executable: {e}", to.display()))?;
        }
    }

    let skills_from = root.join(".claude").join("skills");
    let entries = std::fs::read_dir(&skills_from).map_err(|e| format!("cannot read {}: {e}", skills_from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", skills_from.display()))?;
        // A skill is a folder; a stray file beside them (.DS_Store) is not one.
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let skill = entry.file_name().to_string_lossy().into_owned();
        let source = entry.path().join("SKILL.md");
        let text = std::fs::read_to_string(&source).map_err(|e| format!("cannot read {}: {e}", source.display()))?;
        let to = stage_dir.join(".claude").join("skills").join(&skill);
        create_dir_all(&to)?;
        write(&to.join("SKILL.md"), &packaged_skill(&text.replace("\r\n", "\n"), &skill)?)?;
    }

    let version = release_version(root)?;
    // The JSON.stringify(..., null, 2) layout the programs have always read.
    write(
        &stage_dir.join("package.json"),
        &format!("{{\n  \"name\": \"jev-router\",\n  \"version\": \"{version}\",\n  \"private\": true\n}}\n"),
    )?;
    copy(&root.join(".env.example"), &stage_dir.join(".env.example"))?;
    copy(&root.join("LICENSE"), &stage_dir.join("LICENSE"))?;
    Ok(version)
}

/// The SHA-256 of a file, as lowercase hex.
pub fn sha256_file(file: &Path) -> Result<String, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(&bytes) {
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

/// Writes `<out_dir>/jev-claude-<target>.tar.gz` from the staged folder with the system `tar`;
/// returns its path and SHA-256.
pub fn archive(stage_dir: &Path, out_dir: &Path, target: &str) -> Result<(PathBuf, String), String> {
    create_dir_all(out_dir)?;
    let name = format!("jev-claude-{target}.tar.gz");
    // The archive name is relative to out_dir: GNU tar reads "C:\..." as a host name.
    let result = Command::new("tar")
        .arg("-czf")
        .arg(&name)
        .arg("-C")
        .arg(stage_dir)
        .arg(".")
        .current_dir(out_dir)
        .output()
        .map_err(|e| format!("tar failed: {e}"))?;
    if !result.status.success() {
        return Err(format!("tar failed: {}", String::from_utf8_lossy(&result.stderr).trim_end()));
    }
    let file = out_dir.join(name);
    let sha256 = sha256_file(&file)?;
    Ok((file, sha256))
}

/// `cargo xtask package [--bin-dir <dir>] [--out <dir>] [--target <platform>] [--stage-only]`.
/// Relative paths are taken from the current directory; the defaults are the repository's
/// `rust/target/release` and `dist`.
pub fn main(args: &[String]) -> Result<(), String> {
    let root = crate::repo_root();
    let mut bin_dir = root.join("rust").join("target").join("release");
    let mut out = root.join("dist");
    let mut target = None;
    let mut stage_only = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--stage-only" => stage_only = true,
            "--bin-dir" => bin_dir = PathBuf::from(value()?),
            "--out" => out = PathBuf::from(value()?),
            "--target" => target = Some(value()?),
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    let target = match target {
        Some(target) => target,
        None => host_target()?,
    };
    if !TARGETS.contains(&target.as_str()) {
        return Err(format!("no release target {target}; expected one of {}", TARGETS.join(", ")));
    }
    let out = crate::absolute(&out)?;
    let stage_dir = out.join(format!("jev-claude-{target}"));
    let version = stage_bundle(&crate::absolute(&bin_dir)?, &stage_dir, &target, &root)?;
    println!("staged jev-claude {version} for {target} in {}", stage_dir.display());
    if stage_only {
        return Ok(());
    }
    let (file, sha256) = archive(&stage_dir, &out, &target)?;
    let _ = std::fs::remove_dir_all(&stage_dir);
    println!("{sha256}  {}", file.display());
    Ok(())
}
