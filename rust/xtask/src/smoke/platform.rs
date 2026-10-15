//! The cases about this machine rather than one installer: links and shell profiles, the platform
//! mapping, prerequisites offered through a package manager, a terminal to ask on, and what only
//! Windows has (the user PATH in the registry, a locked folder, stale folders, other CPUs).

#[cfg(unix)]
use super::run::steering_variables;
use super::run::{
    Installer, PathSpec, Run, Setup, Shell, create_dir_all, find_on_path, have_script, logging_stub, remove_dir_all,
    write_file,
};
use super::{Case, Ctx};
#[cfg(unix)]
use crate::package::PROGRAMS;
use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

/// Numbers the scratch registry keys of one run.
static KEY_SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

const MARKER: &str = "# jev-claude (Rust edition)";

/// The cases about this machine, run with the plain bash installer.
pub fn on_this_machine(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    if cfg!(windows) {
        if ctx.wants("git-bash-home") {
            git_bash_home(ctx, e)?;
        }
        return Ok(());
    }
    let groups: [(&str, Case); 5] = [
        ("links", links),
        ("profiles", profiles),
        ("platform", platform_mapping),
        ("prerequisites", prerequisites),
        ("terminal", terminal),
    ];
    for (name, case) in groups {
        if ctx.wants(name) {
            case(ctx, e)?;
        }
    }
    Ok(())
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// The installer's links into `~/.local/bin`: its own are replaced, anyone else's are left alone,
/// and a program run through a link finds its root.
#[cfg_attr(not(unix), allow(clippy::unnecessary_wraps))]
fn links(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let elsewhere = ctx.work.join("elsewhere");
        create_dir_all(&elsewhere)?;
        write_file(&elsewhere.join("x"), "x", false)?;
        let foreign = elsewhere.join("x");
        let seeded = foreign.clone();
        let setup = ctx.base().seed(move |home| {
            let bin = home.join(".local").join("bin");
            create_dir_all(&bin)?;
            write_file(&bin.join("jev-legend"), "mine", false)?;
            symlink(&seeded, bin.join("jev-explain")).map_err(|e| e.to_string())?;
            // The retired Node edition's link: replaced.
            symlink("/old/node/bin/jev-check.mjs", bin.join("jev-check")).map_err(|e| e.to_string())
        });
        let run = ctx.go(e, &setup)?;
        ctx.exits_0(&run, "with links of someone else's in ~/.local/bin the install succeeds");
        let bin = run.home.join(".local").join("bin");
        for program in PROGRAMS {
            if program == "jev-legend" || program == "jev-explain" {
                continue;
            }
            let target = std::fs::read_link(bin.join(program)).ok();
            let want = run.install_dir.join("bin").join(program);
            ctx.on(&run, target.as_deref() == Some(want.as_path()), &format!("{program} is linked to the install"));
        }
        let mine = read(&bin.join("jev-legend")) == "mine"
            && std::fs::symlink_metadata(bin.join("jev-legend")).is_ok_and(|m| !m.file_type().is_symlink());
        ctx.on(&run, mine, "a program of the user's own, not a link, is left alone");
        let theirs = std::fs::read_link(bin.join("jev-explain")).ok().as_deref() == Some(foreign.as_path());
        ctx.on(&run, theirs, "a link to somewhere else is left alone");
        ctx.says(&run, "jev-legend is not a link, so it was left alone", "and says so for the file");
        ctx.says(&run, "jev-explain is a link to", "and says so for the link");
        ctx.says(&run, "linked the programs into", "and says where it linked the others");
        // A program started through its link finds the install it belongs to (no JEV_ROOT).
        let mut own = Command::new(bin.join("jev-check"));
        for name in steering_variables() {
            own.env_remove(name);
        }
        let own = own.env("HOME", &run.home).stdin(Stdio::null()).output();
        let mode_installed = ctx.mode_installed.clone();
        let ran = own.is_ok_and(|o| o.status.success() && mode_installed.is_match(&String::from_utf8_lossy(&o.stdout)));
        ctx.on(&run, ran, "jev-check run through its link finds the install");

        // A bin folder that cannot be made: the install's own folder goes on the PATH instead.
        let afile = ctx.work.join("afile");
        write_file(&afile, "not a folder", false)?;
        let setup = ctx.base().env("JEV_BIN_DIR", afile.join("bin"));
        let run = ctx.go(e, &setup)?;
        ctx.exits_0(&run, "a bin folder that cannot be made does not stop the install");
        ctx.matches(&run, r"warn\s+Links\s+could not create the links in", "and warns about the links");
        ctx.says(&run, "Putting the install's own folder on your PATH instead", "and says where the PATH goes instead");
        ctx.on(&run, read(&afile) == "not a folder", "and leaves the file alone");
    }
    #[cfg(not(unix))]
    let _ = (ctx, e);
    Ok(())
}

/// Which shell profile gets the PATH line, and that it is written once.
fn profiles(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let work = ctx.work.to_path_buf();
    let line_in = |home: &Path| format!("export PATH=\"{}:$PATH\"", home.join(".local").join("bin").display());

    // zsh: ZDOTDIR's .zshrc.
    let zd = work.join("zd");
    remove_dir_all(&zd);
    create_dir_all(&zd)?;
    let good = ctx.url("good");
    let zsh = || Setup::new(&good).modifying().env("SHELL", "/bin/zsh").env("ZDOTDIR", &zd);
    let run = ctx.go(e, &zsh())?;
    ctx.exits_0(&run, "zsh: the install succeeds");
    let text = read(&zd.join(".zshrc"));
    ctx.on(
        &run,
        text.matches(MARKER).count() == 1 && text.contains(&line_in(&run.home)),
        "zsh: ZDOTDIR/.zshrc gets one marker and the PATH line",
    );
    ctx.on(&run, !run.home.join(".zshrc").exists(), "zsh: ~/.zshrc is not made when ZDOTDIR is set");
    ctx.says(&run, "Open a new terminal first", "zsh: says to open a new terminal");
    let run = ctx.go(e, &zsh().over().keeping_home())?;
    ctx.exits_0(&run, "zsh again: the install succeeds");
    ctx.says(&run, "already has a jev-claude line", "zsh again: says the profile already has the line");
    ctx.on(&run, read(&zd.join(".zshrc")).matches(MARKER).count() == 1, "zsh again: still one marker");

    // fish.
    let run = ctx.go(e, &ctx.base().modifying().env("SHELL", "/usr/bin/fish"))?;
    let fish = read(&run.home.join(".config").join("fish").join("config.fish"));
    let want = format!("fish_add_path \"{}\"", run.home.join(".local").join("bin").display());
    ctx.on(&run, fish.contains(&want), "fish: config.fish gets a fish_add_path line");

    // bash: .bashrc on Linux; on a Mac the first login file that exists, else a new .bash_profile.
    let bash = || Setup::new(&good).modifying().env("SHELL", "/bin/bash");
    if cfg!(target_os = "macos") {
        let run = ctx.go(e, &bash().seed(|home| write_file(&home.join(".profile"), "# mine\n", false)))?;
        ctx.on(
            &run,
            read(&run.home.join(".profile")).contains(MARKER),
            "bash on a Mac: an existing .profile gets the line",
        );
        ctx.on(&run, !run.home.join(".bash_profile").exists(), "bash on a Mac: and no .bash_profile is made beside it");
        let run = ctx.go(e, &bash())?;
        ctx.on(
            &run,
            read(&run.home.join(".bash_profile")).contains(MARKER),
            "bash on a Mac: with none, .bash_profile is made",
        );
    } else {
        let run = ctx.go(e, &bash())?;
        ctx.on(&run, read(&run.home.join(".bashrc")).contains(MARKER), "bash on Linux: .bashrc gets the line");
    }

    // Any other shell: .profile.
    let run = ctx.go(e, &ctx.base().modifying().env("SHELL", "/bin/sh"))?;
    ctx.on(&run, read(&run.home.join(".profile")).contains(MARKER), "another shell: .profile gets the line");

    // A profile that cannot be written is reported, with the line to add by hand.
    let zd2 = work.join("zd2");
    remove_dir_all(&zd2);
    create_dir_all(&zd2.join(".zshrc"))?;
    let run = ctx.go(e, &ctx.base().modifying().env("SHELL", "/bin/zsh").env("ZDOTDIR", &zd2))?;
    ctx.exits_0(&run, "a profile that cannot be written does not stop the install");
    ctx.matches(&run, r"warn\s+Profile\s+could not write to", "and is reported");
    ctx.says(&run, "Add this line to your shell profile yourself", "and the line to add is shown");
    remove_dir_all(&zd);
    remove_dir_all(&zd2);
    Ok(())
}

/// A stand-in `uname` that answers `-s` or `-m` as given and passes everything else to the real one.
fn uname_stub(kernel: Option<&str>, machine: Option<&str>) -> Result<String, String> {
    // Git Bash has its own, whatever the Windows PATH holds.
    let real = if cfg!(windows) {
        std::path::PathBuf::from("/usr/bin/uname")
    } else {
        find_on_path("uname").ok_or("uname is not on the PATH")?
    };
    let mut text = String::from("#!/bin/sh\ncase \"$1\" in\n");
    if let Some(kernel) = kernel {
        let _ = writeln!(text, "  -s) echo {kernel} ;;");
    }
    if let Some(machine) = machine {
        let _ = writeln!(text, "  -m) echo {machine} ;;");
    }
    let _ = write!(text, "  *) exec '{}' \"$@\" ;;\nesac\n", real.display());
    Ok(text)
}

/// The kernel and CPU the installer maps to an archive name.
fn platform_mapping(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let stubbed = |name: &str, text: String| PathSpec::clean().stubbed(vec![(name.to_string(), text)]);
    let no_download = |ctx: &mut Ctx, run: &Run, since: usize| {
        let asked: Vec<String> =
            ctx.srv.since(since).into_iter().filter(|l| l.contains(".tar.gz") || l.contains("SHA256SUMS")).collect();
        ctx.on(run, asked.is_empty(), &format!("and downloads nothing (asked for {asked:?})"));
    };

    let before = ctx.srv.count();
    let run = ctx.go(e, &ctx.base().path(stubbed("uname", uname_stub(Some("FreeBSD"), None)?)))?;
    ctx.exits_nonzero(&run, "an unsupported kernel fails the install");
    ctx.matches(&run, r"FAIL\s+Platform\s+FreeBSD", "and names the kernel");
    ctx.says(&run, "has no prebuilt programs", "and says there are no prebuilt programs");
    ctx.says(&run, "Nothing was installed", "and says nothing was installed");
    let none = !run.install_dir.exists();
    ctx.on(&run, none, "and leaves nothing installed");
    no_download(ctx, &run, before);

    let before = ctx.srv.count();
    let run = ctx.go(e, &ctx.base().path(stubbed("uname", uname_stub(None, Some("riscv64"))?)))?;
    ctx.exits_nonzero(&run, "an unsupported CPU fails the install");
    ctx.says(&run, "riscv64 has no prebuilt programs", "and names the CPU");
    no_download(ctx, &run, before);

    // A shell under Rosetta reports x86_64 on an Apple-silicon Mac; the native programs are meant.
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        let sysctl = find_on_path("sysctl").ok_or("sysctl is not on the PATH")?;
        let rosetta = vec![
            ("uname".to_string(), uname_stub(None, Some("x86_64"))?),
            (
                "sysctl".to_string(),
                format!(
                    "#!/bin/sh\nif [ \"$*\" = '-n sysctl.proc_translated' ]; then echo 1; else exec '{}' \"$@\"; fi\n",
                    sysctl.display()
                ),
            ),
        ];
        let run = ctx.go(e, &ctx.base().path(PathSpec::clean().stubbed(rosetta)))?;
        ctx.exits_0(&run, "under Rosetta the install succeeds");
        ctx.matches(&run, r"ok\s+Platform\s+macos-arm64", "and installs the Apple-silicon programs");
    }

    // The Linux programs are static: a musl system takes the same archive.
    if cfg!(target_os = "linux") {
        let run =
            ctx.go(e, &ctx.base().path(stubbed("ldd", "#!/bin/sh\necho 'musl libc (x86_64)'\nexit 1\n".into())))?;
        ctx.exits_0(&run, "on a system that reports musl the install succeeds");
        ctx.says(&run, "musl libc: the programs are static", "and says the programs are static");
    }
    Ok(())
}

/// The user running these is not root (the cases that expect a normal user are skipped for root).
fn is_root() -> bool {
    find_on_path("id")
        .and_then(|id| Command::new(id).arg("-u").stdin(Stdio::null()).output().ok())
        .is_some_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
}

/// Missing curl or git, offered through a package manager that is a stand-in recording its command line.
fn prerequisites(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let Some(curl) = find_on_path("curl") else {
        println!("skipped: prerequisites (curl is not on the PATH, so there is none to hide)");
        return Ok(());
    };
    let log = ctx.work.join("pm.log");
    let apt = format!(
        "#!/bin/sh\necho \"apt-get $*\" >> '{}'\ncase \"$*\" in *install*curl*) ln -sf '{}' \"$(dirname \"$0\")/curl\" ;; esac\n",
        log.display(),
        curl.display()
    );
    let sudo = logging_stub("sudo", &log);
    let lines = |ctx: &Ctx| -> Vec<String> { read(&ctx.work.join("pm.log")).lines().map(str::to_string).collect() };
    let fresh_log = || {
        let _ = std::fs::remove_file(ctx.work.join("pm.log"));
    };
    let _ = &fresh_log;
    let missing_curl = |stubs: Vec<(String, String)>| PathSpec::clean().hiding(&["curl", "wget", "git"]).stubbed(stubs);

    if is_root() {
        println!("skipped: prerequisites through sudo (these checks need a user who is not root)");
    } else {
        // No yes and no terminal: the exact command is printed, and nothing is run.
        let _ = std::fs::remove_file(&log);
        let spec = missing_curl(vec![("apt-get".into(), apt.clone()), ("sudo".into(), sudo.clone())]);
        let run = ctx.go(e, &ctx.base().path(spec))?;
        ctx.exits_nonzero(&run, "without curl or wget and no yes the install stops");
        ctx.matches(&run, r"FAIL\s+Download tool\s+neither curl nor wget was found", "and reports the download tool");
        ctx.says(
            &run,
            "To install (uses sudo, which may ask for your password): sudo apt-get update && sudo apt-get install -y curl ca-certificates",
            "and prints the exact command",
        );
        ctx.says(&run, "There is no terminal to ask on", "and says there is no terminal to ask on");
        ctx.on(&run, lines(ctx).is_empty(), "and runs no package manager");
        let none = !run.install_dir.exists();
        ctx.on(&run, none, "and leaves nothing installed");

        // With a yes: the same command runs, and the install goes on; git, missing too, is offered and tried.
        let _ = std::fs::remove_file(&log);
        let spec = missing_curl(vec![("apt-get".into(), apt.clone()), ("sudo".into(), sudo.clone())]);
        let run = ctx.go(e, &ctx.base().path(spec).env("JEV_ASSUME_YES", "1"))?;
        ctx.exits_0(&run, "with a yes and no curl the install goes on");
        ctx.says(
            &run,
            "Running: sudo apt-get update && sudo apt-get install -y curl ca-certificates",
            "and runs the command it showed",
        );
        ctx.matches(&run, r"ok\s+Prerequisites\s+installed curl", "and reports curl installed");
        let want: Vec<String> = [
            "sudo apt-get update",
            "apt-get update",
            "sudo apt-get install -y curl ca-certificates",
            "apt-get install -y curl ca-certificates",
            "sudo apt-get update",
            "apt-get update",
            "sudo apt-get install -y git",
            "apt-get install -y git",
        ]
        .map(str::to_string)
        .to_vec();
        let got = lines(ctx);
        ctx.on(&run, got == want, &format!("and the package manager got exactly the command shown (got {got:?})"));
        ctx.matches(
            &run,
            r"warn\s+git\s+could not be installed; the status line will not show the branch \(optional\)",
            "and a git that did not install is only a warning",
        );
        let programs = ctx.installed(&run.install_dir);
        ctx.on(&run, programs, "and installs jev-claude");

        // git alone missing, nobody to ask: a warning and the command to run later.
        let _ = std::fs::remove_file(&log);
        let spec = PathSpec::clean()
            .hiding(&["git"])
            .stubbed(vec![("apt-get".into(), apt.clone()), ("sudo".into(), sudo.clone())]);
        let run = ctx.go(e, &ctx.base().path(spec))?;
        ctx.exits_0(&run, "without git and nobody to ask the install succeeds");
        ctx.matches(&run, r"warn\s+git\s+not found", "and warns that git is missing");
        ctx.says(
            &run,
            "To install it later: sudo apt-get update && sudo apt-get install -y git",
            "and shows how to install it later",
        );
        ctx.on(&run, lines(ctx).is_empty(), "and runs no package manager");

        // As a normal user with no sudo: the command is for root to run.
        let _ = std::fs::remove_file(&log);
        let spec = missing_curl(vec![("apt-get".into(), apt.clone())]);
        let run = ctx.go(e, &ctx.base().path(spec).env("JEV_ASSUME_YES", "1"))?;
        ctx.exits_nonzero(&run, "without curl, sudo and root the install stops");
        ctx.says(
            &run,
            "needs root and sudo is not installed; as root run: apt-get update && apt-get install -y curl ca-certificates",
            "and shows the command to run as root",
        );
        ctx.on(&run, lines(ctx).is_empty(), "and runs no package manager");
    }

    // Homebrew refuses to run as root.
    let Some(id) = find_on_path("id") else { return Ok(()) };
    let _ = std::fs::remove_file(&log);
    let root_id = format!("#!/bin/sh\nif [ \"$1\" = -u ]; then echo 0; else exec '{}' \"$@\"; fi\n", id.display());
    let spec = missing_curl(vec![("brew".into(), logging_stub("brew", &log)), ("id".into(), root_id)]);
    let run = ctx.go(e, &ctx.base().path(spec).env("JEV_ASSUME_YES", "1"))?;
    ctx.exits_nonzero(&run, "as root with only Homebrew the install stops");
    ctx.says(&run, "Homebrew will not run as root", "and says Homebrew will not run as root");
    ctx.on(&run, lines(ctx).is_empty(), "and runs no package manager");
    Ok(())
}

/// Nobody to ask: no terminal, or a Claude Code session; and, under a pseudo-terminal, somebody to ask.
fn terminal(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    if !have_script() {
        println!("skipped: the pseudo-terminal cases (script(1) is not installed)");
        return Ok(());
    }
    let pty = |setup: Setup, typed: &str| {
        let mut setup = setup.pty(typed);
        setup.timeout = std::time::Duration::from_secs(30);
        setup
    };

    // Inside a Claude Code session nothing is asked, even with a terminal to ask on.
    let run =
        ctx.go(e, &pty(ctx.base().path(PathSpec::no_claude()).unset("JEV_INSTALL_CLAUDE").env("CLAUDECODE", "1"), ""))?;
    ctx.exits_nonzero(&run, "inside a Claude Code session with no Claude Code the install fails");
    ctx.says(&run, "There is no terminal to ask on", "and says there is nobody to ask, though there is a terminal");
    ctx.never_says(&run, "Run it now?", "and asks nothing");
    let quick = run.elapsed.as_secs() < 25;
    ctx.on(&run, quick, "and does not wait");
    let none = !run.install_dir.exists();
    ctx.on(&run, none, "and leaves nothing installed");

    // The same session, with Claude Code and no keys: they are not asked for either.
    let mut setup = ctx.base();
    setup.keys = super::run::Keys::Env { jev: None, anthropic: None };
    let run = ctx.go(e, &pty(setup, ""))?;
    ctx.exits_0(&run, "inside a Claude Code session with no keys the install succeeds");
    ctx.says(&run, "Add your Jev key later", "and leaves the key for later");
    ctx.never_says(&run, "Paste it here", "and does not ask for the Jev key");
    ctx.never_says(&run, "Paste the key", "and does not ask for the Anthropic key");
    let quick = run.elapsed.as_secs() < 25;
    ctx.on(&run, quick, "and does not wait");

    // Control: outside a session the same terminal is asked, so the checks above mean something.
    let run = ctx.go(e, &pty(ctx.base().path(PathSpec::no_claude()).unset("JEV_INSTALL_CLAUDE"), "n\n"))?;
    ctx.exits_nonzero(&run, "with a terminal and an answer of no the install stops");
    ctx.says(&run, "Run it now? [y/N]", "and asks whether to install Claude Code");
    ctx.says(&run, "Claude Code is required. Install it", "and says Claude Code is required");
    let none = !run.install_dir.exists();
    ctx.on(&run, none, "and leaves nothing installed");
    Ok(())
}

/// Git Bash takes its home from USERPROFILE, which Windows programs share with it.
fn git_bash_home(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let other = ctx.work.join("other-home");
    create_dir_all(&other)?;
    let run = ctx.go(e, &ctx.base().env("HOME", &other))?;
    ctx.exits_0(&run, "Git Bash with a HOME that is not the profile folder: the install succeeds");
    let suffix = run
        .output
        .lines()
        .any(|l| l.contains("Using the Jev key already in") && l.trim_end().ends_with("/home/.jev-router.env"));
    ctx.on(&run, suffix, "and reads the settings from the USERPROFILE folder");
    ctx.on(&run, !other.join(".jev-router.env").exists(), "and writes none in HOME");
    Ok(())
}

/// What only a non-Windows machine can show: install.ps1 refuses to run there.
pub fn other_systems(ctx: &mut Ctx, list: &[Installer]) -> Result<(), String> {
    if cfg!(windows) || !ctx.wants("ps1-on-unix") {
        return Ok(());
    }
    let Some(pwsh) = find_on_path("pwsh") else {
        println!("skipped: install.ps1 on a system that is not Windows (no pwsh)");
        return Ok(());
    };
    let Some(any) = list.first() else { return Ok(()) };
    println!("\ninstall.ps1 under PowerShell on a system that is not Windows");
    let ps1 = any.script.with_file_name("install.ps1");
    let installer = Installer::new(
        "install.ps1 (pwsh, not Windows)",
        pwsh,
        &["-NoProfile".as_ref(), "-File".as_ref(), ps1.as_os_str()],
        Shell::PowerShell,
        &ps1,
    );
    let run = ctx.go(&installer, &ctx.base())?;
    ctx.exits_nonzero(&run, "install.ps1 on a system that is not Windows fails");
    ctx.says(&run, "install.ps1 is for Windows", "and says it is for Windows");
    ctx.says(&run, "Nothing was installed", "and says nothing was installed");
    let none = !run.install_dir.exists();
    ctx.on(&run, none, "and leaves nothing installed");
    ctx.never_says(&run, "checksum matches", "and downloads nothing");
    Ok(())
}

/// A PowerShell for the registry helpers below.
fn powershell(script: &str) -> Option<String> {
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env_remove("PSModulePath")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).replace('\r', ""))
}

/// The user PATH as the registry holds it (%VAR% unexpanded) and its value kind.
fn registry_path(subkey: &str) -> Option<(String, String)> {
    let script = format!(
        "$k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('{subkey}'); if ($k) {{ $k.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames); $k.GetValueKind('Path').ToString() }}"
    );
    let out = powershell(&script)?;
    let mut lines = out.lines();
    Some((lines.next()?.to_string(), lines.next()?.to_string()))
}

/// The real user PATH, for proving that no run touched it.
pub fn real_user_path() -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    registry_path("Environment").map(|(value, kind)| format!("{kind}:{value}"))
}

/// Seeds a scratch registry key with a user PATH.
fn seed_registry(subkey: &str, value: &str) -> bool {
    let value = value.replace('\'', "''");
    powershell(&format!(
        "$k = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('{subkey}'); $k.SetValue('Path', '{value}', [Microsoft.Win32.RegistryValueKind]::ExpandString); $k.Close()"
    ))
    .is_some()
}

/// Deletes the scratch key, however a case ends.
struct ScratchKey(String);

impl Drop for ScratchKey {
    fn drop(&mut self) {
        let _ = powershell(&format!("[Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree('{}', $false)", self.0));
    }
}

/// The Windows user PATH, a locked install folder, stale folders and the other CPUs.
pub fn windows_only(ctx: &mut Ctx, list: &[Installer]) -> Result<(), String> {
    if !cfg!(windows) {
        return Ok(());
    }
    for e in list.iter().filter(|e| !e.piped) {
        println!("\n{} (Windows)", e.label);
        if ctx.wants("user-path") {
            user_path(ctx, e)?;
        }
        if ctx.wants("locked") {
            locked_folder(ctx, e)?;
        }
        if e.shell == Shell::PowerShell {
            if ctx.wants("stale") {
                stale_folders(ctx, e)?;
            }
            if ctx.wants("cpus") {
                other_cpus(ctx, e)?;
            }
            if ctx.wants("winget") {
                winget_git(ctx, e)?;
            }
        } else if ctx.wants("cpus") {
            git_bash_arm(ctx, e)?;
        }
    }
    Ok(())
}

/// The user PATH in the registry: added, kept in place, moved, and Claude Code's folder appended.
fn user_path(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    if !e.mentions("JEV_TEST_USER_ENV_KEY") {
        ctx.check(
            false,
            "the installer takes JEV_TEST_USER_ENV_KEY, the test hook that points it at a scratch registry key",
        );
        return Ok(());
    }
    let subkey = format!(
        "Software\\jev-claude-smoke\\{}-{}\\Environment",
        std::process::id(),
        KEY_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let guard = ScratchKey(subkey.rsplit_once('\\').map_or(subkey.clone(), |(parent, _)| parent.to_string()));
    let good = ctx.url("good");
    let modifying = || Setup::new(&good).modifying().env("JEV_TEST_USER_ENV_KEY", subkey.as_str());
    let bin_of = |run: &Run| {
        let dir = run.install_dir.join("bin").display().to_string();
        if e.shell == Shell::Bash { dir.replace('/', "\\") } else { dir }
    };

    if !seed_registry(&subkey, "%USERPROFILE%\\tools;C:\\x") {
        ctx.check(false, "can seed a scratch registry key");
        return Ok(());
    }
    let run = ctx.go(e, &modifying())?;
    let bin = bin_of(&run);
    let (value, kind) = registry_path(&subkey).unwrap_or_default();
    ctx.exits_0(&run, "the user PATH: an install adds its bin folder");
    ctx.says(&run, &format!("added {bin} to your Windows user PATH"), "and says so");
    ctx.on(
        &run,
        value == format!("{bin};%USERPROFILE%\\tools;C:\\x"),
        &format!("and puts it first and keeps %VAR% entries unexpanded (got {value})"),
    );
    ctx.on(&run, kind == "ExpandString", &format!("and keeps the value an expandable string (got {kind})"));
    ctx.says(&run, "Open a new terminal first", "and says to open a new terminal");

    let run = ctx.go(e, &modifying().over())?;
    ctx.says(&run, &format!("{bin} is already first on your Windows user PATH"), "again: says it is already first");
    ctx.on(&run, registry_path(&subkey).unwrap_or_default().0 == value, "again: leaves the value alone");

    seed_registry(&subkey, &format!("C:\\x;{bin}\\;%USERPROFILE%\\tools"));
    let run = ctx.go(e, &modifying().over())?;
    ctx.says(&run, &format!("moved {bin} to the front"), "when it is behind another folder: moves it to the front");
    let (value, _) = registry_path(&subkey).unwrap_or_default();
    let count = value.split(';').filter(|p| p.trim_end_matches('\\').eq_ignore_ascii_case(&bin)).count();
    ctx.on(
        &run,
        value == format!("{bin};C:\\x;%USERPROFILE%\\tools") && count == 1,
        &format!("and leaves it there once (got {value})"),
    );

    // Claude Code in its own folder, which Anthropic's installer leaves off the PATH: appended last.
    if e.shell == Shell::PowerShell {
        seed_registry(&subkey, "C:\\x");
        let local = |home: &Path| super::run::stand_in(&home.join(".local").join("bin"), "claude").map(drop);
        let run = ctx.go(e, &modifying().path(PathSpec::no_claude()).seed(local))?;
        let local_bin = run.home.join(".local").join("bin").display().to_string();
        let (value, _) = registry_path(&subkey).unwrap_or_default();
        ctx.on(
            &run,
            value.ends_with(&format!(";{local_bin}")),
            &format!("Claude Code's folder is appended last (got {value})"),
        );
        ctx.says(&run, &format!("added {local_bin} to your Windows user PATH too"), "and says so");
    }
    drop(guard);
    Ok(())
}

/// A running jev-claude holds its folder: the update must fail cleanly and change nothing.
fn locked_folder(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    if !cfg!(windows) {
        return Ok(());
    }
    let good = ctx.go(e, &ctx.base())?;
    let held_file = good.install_dir.join("bin").join(format!("jev-check{}", super::run::EXE));
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    // Read sharing only: no FILE_SHARE_DELETE, so the folder cannot be moved while this is open.
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let handle = options.open(&held_file).map_err(|err| format!("cannot hold {}: {err}", held_file.display()))?;
    let before = super::cases::snapshot(&good.install_dir);
    let run = ctx.go(e, &ctx.base().over())?;
    ctx.exits_nonzero(&run, "an update while a program of it is running fails");
    ctx.says(&run, "Could not replace", "and says to close the running programs");
    let intact = super::cases::snapshot(&good.install_dir) == before;
    ctx.on(&run, intact, "and keeps the earlier install intact");
    ctx.no_leftovers(&run, &good.install_dir, "a locked install folder");
    drop(handle);
    let run = ctx.go(e, &ctx.base().over())?;
    ctx.exits_0(&run, "once it is released the update succeeds");
    Ok(())
}

/// install.ps1 clears stale staging folders of its own beside the install, and only those.
fn stale_folders(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let good = ctx.go(e, &ctx.base())?;
    let work = ctx.work.to_path_buf();
    let stale_download = work.join("install.new.999");
    create_dir_all(&stale_download)?;
    write_file(&stale_download.join(".download"), "", false)?;
    // A copy of the install as an old backup.
    let stale_old = work.join("install.old.998");
    super::copy_tree(&good.install_dir, &stale_old)?;
    let recent = work.join("install.new.997");
    create_dir_all(&recent)?;
    write_file(&recent.join(".download"), "", false)?;
    let unrelated = work.join("install.new.996");
    create_dir_all(&unrelated)?;
    write_file(&unrelated.join("notes.txt"), "mine", false)?;
    for old in [&stale_download, &stale_old, &unrelated] {
        let path = old.display().to_string().replace('\'', "''");
        powershell(&format!("(Get-Item -LiteralPath '{path}').LastWriteTime = (Get-Date).AddMinutes(-20)"));
    }
    let run = ctx.go(e, &ctx.base().over())?;
    ctx.exits_0(&run, "with stale folders beside the install it succeeds");
    ctx.on(&run, !stale_download.exists(), "and removes an old unfinished download");
    ctx.on(&run, !stale_old.exists(), "and removes an old backup of an install");
    ctx.on(&run, recent.exists(), "and keeps a recent folder, which another run may be using");
    ctx.on(&run, unrelated.exists(), "and keeps an old folder that is neither");
    for dir in [&recent, &unrelated] {
        remove_dir_all(dir);
    }
    Ok(())
}

/// Windows on ARM takes the x64 programs by emulation from Windows 11; other CPUs have none.
fn other_cpus(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let build: u32 = powershell("(Get-ItemPropertyValue -LiteralPath 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion' -Name 'CurrentBuildNumber')")
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0);
    let run = ctx.go(e, &ctx.base().env("PROCESSOR_ARCHITEW6432", "ARM64"))?;
    if build >= 22000 {
        ctx.exits_0(&run, "an ARM64 PC with Windows 11 installs the x64 programs");
        ctx.says(&run, "runs it through emulation", "and says it runs through emulation");
    } else {
        ctx.exits_nonzero(&run, "an ARM64 PC with Windows 10 fails the install");
        ctx.says(&run, "cannot run the x64 programs", "and says why");
    }
    let run = ctx.go(e, &ctx.base().env("PROCESSOR_ARCHITEW6432", "IA64"))?;
    ctx.exits_nonzero(&run, "a CPU with no prebuilt programs fails the install");
    ctx.says(&run, "Windows IA64 has no prebuilt programs", "and names the CPU");
    let none = !run.install_dir.exists();
    ctx.on(&run, none, "and leaves nothing installed");
    Ok(())
}

/// Git Bash on an ARM PC reports aarch64; the x64 programs are meant.
fn git_bash_arm(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let stub = vec![("uname".to_string(), uname_stub(None, Some("aarch64"))?)];
    let run = ctx.go(e, &ctx.base().path(PathSpec::clean().stubbed(stub)))?;
    ctx.exits_0(&run, "Git Bash reporting aarch64 installs the x64 programs");
    ctx.matches(&run, r"ok\s+Platform\s+windows-x64", "and reports windows-x64");
    Ok(())
}

/// git offered through winget, a stand-in that records its command line.
fn winget_git(ctx: &mut Ctx, e: &Installer) -> Result<(), String> {
    let log = ctx.work.join("winget.log");
    let _ = std::fs::remove_file(&log);
    let stub = format!(">> \"{}\" echo winget %*\r\n@exit /b 0\r\n", log.display());
    let empty = ctx.work.join("empty-program-files");
    create_dir_all(&empty)?;
    let spec = PathSpec::clean().hiding(&["git"]).stubbed(vec![("winget.cmd".into(), stub)]);
    let setup =
        ctx.base().path(spec).env("JEV_ASSUME_YES", "1").env("ProgramW6432", &empty).env("ProgramFiles", &empty);
    let run = ctx.go(e, &setup)?;
    ctx.exits_0(&run, "without git and with a yes the install succeeds");
    let command = "winget install --id Git.Git -e --accept-package-agreements --accept-source-agreements";
    ctx.says(&run, &format!("To install it: {command}"), "and shows the winget command");
    let logged = read(&log);
    ctx.on(
        &run,
        logged.lines().any(|l| l.trim() == command),
        &format!("and runs exactly that command (got {logged:?})"),
    );
    ctx.says(
        &run,
        "could not be installed; the status line will not show the branch (optional)",
        "and a git that did not install is only a warning",
    );
    Ok(())
}
