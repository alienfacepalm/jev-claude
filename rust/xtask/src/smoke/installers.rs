//! The installers to run on this machine.

use super::run::{Installer, Shell, find_on_path};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Git Bash's `bash.exe`, from the path of a `git.exe` in the same Git for Windows install:
/// `<root>\cmd\git.exe`, `<root>\bin\git.exe` and `<root>\mingw64\bin\git.exe` all lead to
/// `<root>\bin\bash.exe`.
pub fn bash_beside_git(git_exe: &Path) -> Option<PathBuf> {
    let dir = git_exe.parent()?;
    let up = dir.parent()?;
    let root = match up.file_name().and_then(OsStr::to_str).map(str::to_ascii_lowercase).as_deref() {
        Some("mingw64" | "mingw32" | "clangarm64") => up.parent()?,
        _ => up,
    };
    Some(root.join("bin").join("bash.exe"))
}

/// Git Bash: Git for Windows' default folder, else the install that `git` on the PATH belongs to.
/// Never the `bash` on the PATH, which on Windows may be the WSL launcher.
fn git_bash() -> Option<PathBuf> {
    let fixed = Path::new(r"C:\Program Files\Git\bin\bash.exe");
    if fixed.exists() {
        return Some(fixed.to_path_buf());
    }
    let out = Command::new("where").arg("git").stdin(Stdio::null()).output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| bash_beside_git(Path::new(line.trim())))
        .find(|bash| bash.exists())
}

/// Is there a PowerShell 7 (`pwsh`)?
fn have_pwsh() -> bool {
    Command::new("pwsh")
        .args(["-NoProfile", "-Command", "exit 0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Shells that were expected and are missing: noted, and an error on CI, where they are always there.
fn check_shells(missing: &[&str], on_ci: bool) -> Result<(), String> {
    for shell in missing {
        println!("skipped: {shell} not found");
    }
    if on_ci && !missing.is_empty() {
        return Err(format!("expected shell missing on this CI runner: {}", missing.join(", ")));
    }
    Ok(())
}

/// The installers to run here: `sh` and `ps1` are the scripts under test, `scripts_url` the address
/// where `ps1` is served (for the one-liners the README gives).
///
/// On CI (`GITHUB_ACTIONS=true`) a shell that should be there but is not is an error, not a skip,
/// so a changed runner image cannot quietly drop half of the Windows runs.
pub fn installers(sh: &Path, ps1: &Path, scripts_url: &str) -> Result<Vec<Installer>, String> {
    let mut list = Vec::new();
    let mut missing: Vec<&str> = Vec::new();
    if cfg!(windows) {
        let run: [&OsStr; 5] =
            ["-NoProfile".as_ref(), "-ExecutionPolicy".as_ref(), "Bypass".as_ref(), "-File".as_ref(), ps1.as_os_str()];
        // A single-quoted PowerShell string: a quote inside it is doubled.
        let quoted = ps1.display().to_string().replace('\'', "''");
        let piped_command = format!("Get-Content -Raw '{quoted}' | Invoke-Expression");
        let piped: [&OsStr; 3] = ["-NoProfile".as_ref(), "-Command".as_ref(), piped_command.as_ref()];
        // Windows PowerShell 5.1 started from PowerShell 7 would inherit its module path and lose its own cmdlets.
        let mut desktop =
            Installer::new("install.ps1 (Windows PowerShell)", "powershell.exe", &run, Shell::PowerShell, ps1);
        desktop.remove.push("PSModulePath");
        list.push(desktop);
        let mut desktop_piped = Installer::new(
            "install.ps1 piped to iex (Windows PowerShell)",
            "powershell.exe",
            &piped,
            Shell::PowerShell,
            ps1,
        );
        desktop_piped.remove.push("PSModulePath");
        desktop_piped.piped = true;
        list.push(desktop_piped);
        if have_pwsh() {
            list.push(Installer::new("install.ps1 (PowerShell 7)", "pwsh", &run, Shell::PowerShell, ps1));
            let mut pwsh_piped =
                Installer::new("install.ps1 piped to iex (PowerShell 7)", "pwsh", &piped, Shell::PowerShell, ps1);
            pwsh_piped.piped = true;
            list.push(pwsh_piped);
        } else {
            missing.push("PowerShell 7 (pwsh)");
        }
        // The README's line for cmd.exe, as typed: cmd.exe must leave the quoted `|` to PowerShell.
        let mut cmd_line = Installer::new("irm | iex from cmd.exe (README)", "cmd.exe", &[], Shell::PowerShell, ps1);
        cmd_line.remove.push("PSModulePath");
        cmd_line.piped = true;
        cmd_line.raw_args = Some(format!(
            "/d /s /c \"powershell -NoProfile -ExecutionPolicy Bypass -Command \"irm {scripts_url}/install.ps1 | iex\"\""
        ));
        list.push(cmd_line);
        if let Some(bash) = git_bash() {
            list.push(Installer::new("install.sh (Git Bash)", &bash, &[sh.as_os_str()], Shell::Bash, sh));
            let mut piped =
                Installer::new("install.sh piped to bash (Git Bash)", &bash, &["-s".as_ref()], Shell::Bash, sh);
            piped.stdin = Some(sh.to_path_buf());
            piped.piped = true;
            list.push(piped);
            // The line the install skill runs from a Bash tool: PowerShell's own quoting inside bash's.
            let line = format!(
                "powershell -NoProfile -Command '$env:JEV_ASSUME_YES = \"1\"; irm {scripts_url}/install.ps1 | iex'"
            );
            let mut skill = Installer::new(
                "irm | iex from Git Bash (install skill)",
                &bash,
                &["-c".as_ref(), line.as_ref()],
                Shell::PowerShell,
                ps1,
            );
            skill.remove.push("PSModulePath");
            skill.piped = true;
            list.push(skill);
        } else {
            missing.push("Git Bash");
        }
    } else {
        let bash = find_on_path("bash").ok_or("bash is not on the PATH")?;
        list.push(Installer::new("install.sh (bash)", &bash, &[sh.as_os_str()], Shell::Bash, sh));
        let mut piped = Installer::new("install.sh piped to bash", &bash, &["-s".as_ref()], Shell::Bash, sh);
        piped.stdin = Some(sh.to_path_buf());
        piped.piped = true;
        list.push(piped);
        // A stock Mac has only /bin/bash 3.2, which is what `curl ... | bash` gets there; the bash
        // first on a runner's PATH is usually a newer one.
        if cfg!(target_os = "macos") && Path::new("/bin/bash").exists() && bash != Path::new("/bin/bash") {
            list.push(Installer::new(
                "install.sh (/bin/bash, stock macOS)",
                "/bin/bash",
                &[sh.as_os_str()],
                Shell::Bash,
                sh,
            ));
            let mut piped32 = Installer::new(
                "install.sh piped to /bin/bash (stock macOS)",
                "/bin/bash",
                &["-s".as_ref()],
                Shell::Bash,
                sh,
            );
            piped32.stdin = Some(sh.to_path_buf());
            piped32.piped = true;
            list.push(piped32);
        }
    }
    check_shells(&missing, std::env::var("GITHUB_ACTIONS").is_ok_and(|v| v == "true"))?;
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::{bash_beside_git, check_shells};
    use std::path::Path;

    #[test]
    fn a_missing_shell_is_a_skip_on_a_developer_machine_and_an_error_on_ci() {
        assert!(check_shells(&[], true).is_ok());
        assert!(check_shells(&["Git Bash"], false).is_ok());
        let error = check_shells(&["PowerShell 7 (pwsh)", "Git Bash"], true).unwrap_err();
        assert!(error.contains("PowerShell 7 (pwsh), Git Bash"), "{error}");
    }

    #[test]
    fn git_bash_lies_beside_every_git_exe_of_a_git_for_windows_install() {
        for git in
            ["C:/Git/cmd/git.exe", "C:/Git/bin/git.exe", "C:/Git/mingw64/bin/git.exe", "C:/Git/MinGW64/bin/git.exe"]
        {
            assert_eq!(bash_beside_git(Path::new(git)).as_deref(), Some(Path::new("C:/Git/bin/bash.exe")), "{git}");
        }
        assert_eq!(bash_beside_git(Path::new("git.exe")), None);
    }
}
