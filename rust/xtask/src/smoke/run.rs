//! How one installer is started: the installers to run on this machine, the PATH, home and
//! environment each run gets, and the stand-in programs on that PATH.

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The executable suffix of this machine's programs.
pub const EXE: &str = std::env::consts::EXE_SUFFIX;
/// How long one installer run may take before it is killed and counted as failed.
pub const TIMEOUT: Duration = Duration::from_secs(240);

/// Which language an installer is written in; it decides the options it takes and the messages it
/// prints.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shell {
    /// install.sh, under bash on macOS, Linux, WSL or Git Bash.
    Bash,
    /// install.ps1, under Windows PowerShell or PowerShell 7.
    PowerShell,
}

/// One installer to run on this machine. The piped form of each is the one the README tells people
/// to type, `curl ... | bash` and `irm ... | iex`: the script arrives on stdin or as a string, so no
/// child it starts may read stdin and swallow the rest of it, and there is no script path to find
/// the clone by. The one-liner forms go through `cmd.exe` and Git Bash's quoting as well.
pub struct Installer {
    /// What the log calls it.
    pub label: String,
    /// The program that is started.
    pub command: PathBuf,
    /// Its arguments.
    pub args: Vec<OsString>,
    /// Variables the run must not inherit.
    pub remove: Vec<&'static str>,
    /// A file piped to the installer's standard input.
    pub stdin: Option<PathBuf>,
    /// The language of the installer.
    pub shell: Shell,
    /// The script reaches the shell as text, so it can take no options and the heavy cases skip it.
    pub piped: bool,
    /// The installer script this runs (what the test hooks are looked for in).
    pub script: PathBuf,
    /// The whole argument text, passed to Windows as it is written (for the `cmd.exe` one-liner).
    pub raw_args: Option<String>,
}

impl Installer {
    /// An installer that runs `script` through `command`.
    pub fn new(label: &str, command: impl Into<PathBuf>, args: &[&OsStr], shell: Shell, script: &Path) -> Installer {
        Installer {
            label: label.to_string(),
            command: command.into(),
            args: args.iter().map(|a| (*a).to_os_string()).collect(),
            remove: Vec::new(),
            stdin: None,
            shell,
            piped: false,
            script: script.to_path_buf(),
            raw_args: None,
        }
    }

    /// Can this run be given options (`--help`, `-y`)?
    pub fn takes_options(&self) -> bool {
        self.raw_args.is_none() && !(self.piped && self.shell == Shell::PowerShell)
    }

    /// `sh` for a bash installer, `ps` for a PowerShell one.
    pub fn pick<'a>(&self, sh: &'a str, ps: &'a str) -> &'a str {
        if self.shell == Shell::Bash { sh } else { ps }
    }

    /// Does the installer script mention `needle` (a test hook, for one)?
    pub fn mentions(&self, needle: &str) -> bool {
        std::fs::read_to_string(&self.script).is_ok_and(|text| text.contains(needle))
    }
}

/// The first `name` on this process's PATH, as an absolute path.
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(format!("{name}{EXE}"))).find(|file| file.is_file())
}

/// The names of the old Node edition's programs, which must not be on a clean PATH.
pub const OTHER_EDITION: [&str; 4] = ["jev-claude", "jev-claude.exe", "jev-claude.cmd", "jev-claude.ps1"];
/// The package managers install.sh knows; none of the real ones may ever be reached.
pub const PACKAGE_MANAGERS: [&str; 7] = ["brew", "apt-get", "dnf", "yum", "zypper", "apk", "pacman"];

/// Writes a text file, executable on Unix when asked.
pub fn write_file(path: &Path, text: &str, executable: bool) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    Ok(())
}

/// `create_dir_all` with a message.
pub fn create_dir_all(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("cannot create {}: {e}", path.display()))
}

/// Removes a folder and everything in it; a folder that is not there is fine.
pub fn remove_dir_all(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}

/// A copy of this program as `<dir>/<name>{EXE}`. `main` makes it act as Claude Code (`claude`) or
/// as a `jev-check` that fails, depending on its own file name; `<dir>/.broken` beside a `claude`
/// makes it fail without a word.
pub fn stand_in(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| format!("cannot find this program: {e}"))?;
    if me.file_stem().and_then(OsStr::to_str) != Some("xtask") {
        return Err("the stand-in programs are copies of the xtask program: run this through cargo xtask".into());
    }
    create_dir_all(dir)?;
    let to = dir.join(format!("{name}{EXE}"));
    std::fs::copy(&me, &to).map_err(|e| format!("cannot copy {} to {}: {e}", me.display(), to.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    Ok(to)
}

/// What stands in for Claude Code on a PATH.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ClaudeOnPath {
    /// A native build that answers `--version`.
    #[default]
    Native,
    /// The npm package's `claude.cmd` (Windows only; elsewhere the same as [`Native`](Self::Native)).
    Npm,
    /// A `claude` that cannot report its version (exits 1 without a word).
    Broken,
    /// None at all: any PATH folder that holds a claude is dropped.
    Absent,
}

/// Which PATH an installer runs with.
#[derive(Clone, Default)]
pub struct PathSpec {
    /// The stand-in Claude Code, first on the PATH.
    pub claude: ClaudeOnPath,
    /// A stand-in for another jev-claude in front, as on a machine that still has the retired Node
    /// edition.
    pub shadow: bool,
    /// Tools to hide from the inherited PATH (by name).
    pub hide: Vec<String>,
    /// Stand-ins placed first, as (file name, text). Giving any replaces the default guard and
    /// hides every real package manager and `sudo`.
    pub stubs: Option<Vec<(String, String)>>,
}

impl PathSpec {
    /// No jev-claude anywhere, as on a new machine: the installer must not trip over finding nothing.
    pub fn clean() -> PathSpec {
        PathSpec::default()
    }

    /// Another jev-claude in front, as on a machine that still has the retired Node edition.
    pub fn shadowed() -> PathSpec {
        PathSpec { shadow: true, ..PathSpec::default() }
    }

    /// No Claude Code at all, as on a machine that never installed it.
    pub fn no_claude() -> PathSpec {
        PathSpec { claude: ClaudeOnPath::Absent, ..PathSpec::default() }
    }

    /// The same, with a different Claude Code.
    pub fn with_claude(mut self, claude: ClaudeOnPath) -> PathSpec {
        self.claude = claude;
        self
    }

    /// The same, hiding these tools.
    pub fn hiding(mut self, names: &[&str]) -> PathSpec {
        self.hide.extend(names.iter().map(|n| (*n).to_string()));
        self
    }

    /// The same, with these stand-ins first (see [`PathSpec::stubs`]).
    pub fn stubbed(mut self, stubs: Vec<(String, String)>) -> PathSpec {
        self.stubs = Some(stubs);
        self
    }
}

/// A shell script that records its command line in `log`, then does nothing; `sudo` runs its
/// arguments afterwards, like the real one.
pub fn logging_stub(name: &str, log: &Path) -> String {
    let log = log.display();
    if name == "sudo" {
        format!("#!/bin/sh\necho \"sudo $*\" >> '{log}'\nexec \"$@\"\n")
    } else {
        format!("#!/bin/sh\necho \"{name} $*\" >> '{log}'\n")
    }
}

/// The folder holding the PATH's stand-ins for one run, made fresh under `work`.
fn claude_dir(base: &Path, claude: ClaudeOnPath) -> Result<Option<PathBuf>, String> {
    let dir = base.join("claude-code");
    match claude {
        ClaudeOnPath::Absent => return Ok(None),
        ClaudeOnPath::Native | ClaudeOnPath::Npm | ClaudeOnPath::Broken => create_dir_all(&dir)?,
    }
    let broken = claude == ClaudeOnPath::Broken;
    if cfg!(windows) {
        if claude == ClaudeOnPath::Npm {
            write_file(&dir.join("claude.cmd"), "@echo 2.1.0 (Claude Code)\r\n", false)?;
        } else {
            stand_in(&dir, "claude")?;
            if broken {
                write_file(&dir.join(".broken"), "", false)?;
            }
        }
    } else {
        let text = if broken { "#!/bin/sh\nexit 1\n" } else { "#!/bin/sh\necho '2.1.0 (Claude Code)'\n" };
        write_file(&dir.join("claude"), text, true)?;
    }
    Ok(Some(dir))
}

fn has_any(dir: &Path, names: &[&str]) -> bool {
    names.iter().any(|name| dir.join(name).exists())
}

/// Symlinks every program of `dirs` (the first of each name) into `farm`, less the `excluded` names.
#[cfg(unix)]
fn link_farm(farm: &Path, dirs: &[PathBuf], excluded: &[String]) -> Result<(), String> {
    create_dir_all(farm)?;
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if excluded.iter().any(|x| OsStr::new(x) == name) {
                continue;
            }
            // The first of each name wins, as on a PATH; a second one fails to link and is skipped.
            let _ = std::os::unix::fs::symlink(entry.path(), farm.join(&name));
        }
    }
    Ok(())
}

/// The PATH to run an installer with, built fresh under `work`: the stand-ins first, then the
/// inherited PATH less whatever the spec hides. By default a guard of do-nothing package managers
/// and `sudo` is first, so no run can reach a real package manager, whatever it is asked to do.
pub fn build_path(spec: &PathSpec, work: &Path, current: &OsStr) -> Result<OsString, String> {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let base = work.join(format!("path-{}", SEQ.fetch_add(1, Ordering::Relaxed)));
    create_dir_all(&base)?;
    let mut dirs: Vec<PathBuf> = Vec::new();

    let custom = spec.stubs.is_some();
    if cfg!(unix) || custom {
        let stub_dir = base.join("stubs");
        create_dir_all(&stub_dir)?;
        if let Some(list) = &spec.stubs {
            for (name, text) in list {
                write_file(&stub_dir.join(name), text, true)?;
            }
        } else {
            let log = work.join("pm-guard.log");
            for name in PACKAGE_MANAGERS.iter().chain(&["sudo"]) {
                write_file(&stub_dir.join(name), &logging_stub(name, &log), true)?;
            }
        }
        dirs.push(stub_dir);
    }
    if let Some(dir) = claude_dir(&base, spec.claude)? {
        dirs.push(dir);
    }
    if spec.shadow {
        let other = base.join("other-edition");
        create_dir_all(&other)?;
        // A pnpm shim of the retired Node edition: a short script that names the .mjs file.
        let shim = "#!/bin/sh\n# pnpm shim for the retired Node edition\nexec node \"$HOME/jev-claude/node/bin/jev-claude.mjs\" \"$@\"\n";
        write_file(&other.join("jev-claude"), shim, true)?;
        write_file(&other.join("jev-claude.cmd"), "@exit /b 0\r\n", false)?;
        dirs.push(other);
    }

    let claude_names = ["claude", "claude.exe", "claude.cmd"];
    let farm_wanted = !spec.hide.is_empty() || custom;
    let mut kept: Vec<PathBuf> = std::env::split_paths(current)
        .filter(|dir| !dir.as_os_str().is_empty() && !has_any(dir, &OTHER_EDITION))
        .collect();
    if spec.claude == ClaudeOnPath::Absent && !(farm_wanted && cfg!(unix)) {
        kept.retain(|dir| !has_any(dir, &claude_names));
    }
    if farm_wanted && cfg!(unix) {
        #[cfg(unix)]
        {
            let mut excluded: Vec<String> = spec.hide.clone();
            excluded.extend(OTHER_EDITION.iter().map(|n| (*n).to_string()));
            if custom {
                excluded.extend(PACKAGE_MANAGERS.iter().map(|n| (*n).to_string()));
                excluded.push("sudo".into());
            }
            if spec.claude == ClaudeOnPath::Absent {
                excluded.extend(claude_names.iter().map(|n| (*n).to_string()));
            }
            let farm = base.join("farm");
            link_farm(&farm, &kept, &excluded)?;
            kept = vec![farm];
        }
    } else if farm_wanted {
        // Windows: a folder that holds a hidden tool is dropped whole.
        let hidden: Vec<String> = spec
            .hide
            .iter()
            .flat_map(|n| [n.clone(), format!("{n}.exe"), format!("{n}.cmd"), format!("{n}.bat")])
            .collect();
        let names: Vec<&str> = hidden.iter().map(String::as_str).collect();
        kept.retain(|dir| !has_any(dir, &names));
    }
    dirs.extend(kept);
    std::env::join_paths(dirs).map_err(|e| format!("cannot build a PATH: {e}"))
}

/// The settings file `Prewritten` keys put in the scratch home.
pub const PREWRITTEN_KEYS: &str = "JEV_API_KEY=smoke-test\nANTHROPIC_API_KEY=smoke-test\n";

/// How the keys reach the installer.
#[derive(Clone, Default)]
pub enum Keys {
    /// A settings file with both keys already in it: the installer has nothing to ask.
    #[default]
    Prewritten,
    /// No settings file; these go in the environment (and the run is marked as inside a Claude Code
    /// session, so no prompt can wait for an answer).
    Env {
        /// `JEV_API_KEY`.
        jev: Option<String>,
        /// `ANTHROPIC_API_KEY`.
        anthropic: Option<String>,
    },
}

/// Something done to the scratch home before a run.
pub type Seed = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

/// Everything about one run except which installer it is.
#[derive(Clone)]
pub struct Setup {
    /// `JEV_RELEASE_URL`.
    pub url: String,
    /// Empty the install folder first.
    pub fresh: bool,
    /// The PATH.
    pub path: PathSpec,
    /// Options after the installer's own arguments.
    pub args: Vec<String>,
    /// Variables set after the defaults.
    pub env: Vec<(String, OsString)>,
    /// Variables removed after the defaults.
    pub unset: Vec<String>,
    /// The keys.
    pub keys: Keys,
    /// `JEV_INSTALL_DIR`; by default `<place>/install`.
    pub dir: Option<OsString>,
    /// The folder holding `home` and `install`; by default the work folder.
    pub place: Option<PathBuf>,
    /// The folder the installer is started in; by default the work folder.
    pub cwd: Option<PathBuf>,
    /// Keep the scratch home from an earlier run.
    pub keep_home: bool,
    /// Leave `JEV_NO_MODIFY_PATH` unset, so the installer edits profiles or the user PATH.
    pub modify_path: bool,
    /// Done to the home before the run.
    pub seeds: Vec<Seed>,
    /// Text typed into a pseudo-terminal the installer runs in (Unix only); without it there is none.
    pub pty: Option<String>,
    /// Stop the installer with SIGTERM (its whole session, Unix) once this text appears in its output.
    pub interrupt_at: Option<String>,
    /// How long the run may take.
    pub timeout: Duration,
}

impl Setup {
    /// A fresh install of the release at `url`, with Claude Code present and both keys set.
    pub fn new(url: &str) -> Setup {
        Setup {
            url: url.to_string(),
            fresh: true,
            path: PathSpec::clean(),
            args: Vec::new(),
            env: Vec::new(),
            unset: Vec::new(),
            keys: Keys::default(),
            dir: None,
            place: None,
            cwd: None,
            keep_home: false,
            modify_path: false,
            seeds: Vec::new(),
            pty: None,
            interrupt_at: None,
            timeout: TIMEOUT,
        }
    }

    /// Install over what is there.
    pub fn over(mut self) -> Setup {
        self.fresh = false;
        self
    }

    /// With this PATH.
    pub fn path(mut self, path: PathSpec) -> Setup {
        self.path = path;
        self
    }

    /// With a variable set.
    pub fn env(mut self, name: &str, value: impl Into<OsString>) -> Setup {
        self.env.push((name.to_string(), value.into()));
        self
    }

    /// With a variable removed.
    pub fn unset(mut self, name: &str) -> Setup {
        self.unset.push(name.to_string());
        self
    }

    /// With options after the installer's own arguments.
    pub fn args(mut self, args: &[&str]) -> Setup {
        self.args.extend(args.iter().map(|a| (*a).to_string()));
        self
    }

    /// With `JEV_INSTALL_DIR` set to this text.
    pub fn dir(mut self, dir: impl Into<OsString>) -> Setup {
        self.dir = Some(dir.into());
        self
    }

    /// Leaving the PATH to the installer (no `JEV_NO_MODIFY_PATH`): it edits a profile or the user PATH.
    pub fn modifying(mut self) -> Setup {
        self.modify_path = true;
        self
    }

    /// Keeping the scratch home of the run before.
    pub fn keeping_home(mut self) -> Setup {
        self.keep_home = true;
        self
    }

    /// Run in a pseudo-terminal, typing `typed` into it (Unix).
    pub fn pty(mut self, typed: &str) -> Setup {
        self.pty = Some(typed.to_string());
        self
    }

    /// Terminated (SIGTERM, to its whole session) once `text` has appeared in its output.
    pub fn interrupted_at(mut self, text: &str) -> Setup {
        self.interrupt_at = Some(text.to_string());
        self
    }

    /// With something done to the home first.
    pub fn seed(mut self, seed: impl Fn(&Path) -> Result<(), String> + Send + Sync + 'static) -> Setup {
        self.seeds.push(Arc::new(seed));
        self
    }
}

/// What one installer run did.
pub struct Run {
    /// Where the install went (unless the run chose another folder).
    pub install_dir: PathBuf,
    /// The scratch home.
    pub home: PathBuf,
    /// The exit code; `None` when it was killed (or timed out).
    pub status: Option<i32>,
    /// Standard output and standard error, interleaved as they arrived.
    pub output: String,
    /// How long it took.
    pub elapsed: Duration,
    /// Whether the output has been printed (once, for the first failed check).
    pub shown: std::cell::Cell<bool>,
}

/// The environment of this process less anything that could steer a run: every `JEV_*` variable
/// (an install folder, a bin folder to link into, a pinned version, a yes to every question) and
/// `CLAUDECODE`, which makes the installers skip their key prompts.
pub fn steering_variables() -> Vec<OsString> {
    std::env::vars_os()
        .map(|(name, _)| name)
        .filter(|name| {
            let name = name.to_string_lossy().to_ascii_uppercase();
            name.starts_with("JEV_") || name == "CLAUDECODE"
        })
        .collect()
}

/// Starts `command` in a session of its own on Unix: with no controlling terminal, an installer
/// cannot open /dev/tty to ask anything, as in CI or a piped install with no terminal.
#[allow(unsafe_code)]
fn detach(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe extern "C" {
            fn setsid() -> i32;
        }
        // SAFETY: the closure runs in the forked child before exec and calls only setsid(2), which
        // is async-signal-safe and touches no memory of this process.
        unsafe {
            command.pre_exec(|| {
                if setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(not(unix))]
    let _ = command;
}

/// Runs `command` to the end or until `timeout`, collecting both output streams into one.
pub fn run_collecting(
    mut command: Command,
    input: Option<Vec<u8>>,
    timeout: Duration,
    interrupt_at: Option<&str>,
) -> Result<(Option<i32>, String), String> {
    command.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() });
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    detach(&mut command);
    let mut child = command.spawn().map_err(|e| format!("cannot start {:?}: {e}", command.get_program()))?;
    let output = Arc::new(Mutex::new(Vec::new()));
    let mut readers = Vec::new();
    let streams: [Option<Box<dyn Read + Send>>; 2] = [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
    ];
    for mut stream in streams.into_iter().flatten() {
        let output = Arc::clone(&output);
        readers.push(std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = stream.read(&mut buf) {
                if n == 0 {
                    break;
                }
                output.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        }));
    }
    if let (Some(input), Some(mut pipe)) = (input, child.stdin.take()) {
        // Written from a thread: a script larger than the pipe buffer would otherwise block here
        // until the installer reads it, while nothing reads the installer's output.
        readers.push(std::thread::spawn(move || {
            let _ = pipe.write_all(&input);
        }));
    }
    let started = Instant::now();
    let mut interrupted = interrupt_at.is_none();
    let status = loop {
        if !interrupted && String::from_utf8_lossy(&output.lock().unwrap()).contains(interrupt_at.unwrap_or_default()) {
            interrupted = true;
            // The installer is the leader of a session of its own (see `detach`): its process group is its pid.
            let _ = Command::new("sh").arg("-c").arg(format!("kill -TERM -- -{}", child.id())).status();
        }
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) if started.elapsed() < timeout => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    for reader in readers {
        let _ = reader.join();
    }
    let output = String::from_utf8_lossy(&output.lock().unwrap()).replace('\r', "");
    Ok((status, output))
}

/// `inner` run in a pseudo-terminal made by `script(1)`, so the installer has a /dev/tty to ask on.
fn in_pty(inner: &Command) -> Command {
    let mut outer = Command::new("script");
    if cfg!(target_os = "macos") {
        outer.args(["-q", "/dev/null"]).arg(inner.get_program()).args(inner.get_args());
    } else {
        let quote = |s: &OsStr| format!("'{}'", s.to_string_lossy().replace('\'', "'\\''"));
        let line: Vec<String> = std::iter::once(inner.get_program()).chain(inner.get_args()).map(quote).collect();
        outer.arg("-qec").arg(line.join(" ")).arg("/dev/null");
    }
    for (name, value) in inner.get_envs() {
        match value {
            Some(value) => outer.env(name, value),
            None => outer.env_remove(name),
        };
    }
    if let Some(dir) = inner.get_current_dir() {
        outer.current_dir(dir);
    }
    outer
}

/// Is `script(1)` there, to give an installer a terminal?
pub fn have_script() -> bool {
    cfg!(unix) && find_on_path("script").is_some()
}

/// One run of `installer` as `setup` describes it, in a scratch home under `work`.
pub fn install(installer: &Installer, work: &Path, setup: &Setup) -> Result<Run, String> {
    let place = setup.place.clone().unwrap_or_else(|| work.to_path_buf());
    let home = place.join("home");
    let install_dir = place.join("install");
    if setup.fresh {
        remove_dir_all(&install_dir);
    }
    if !setup.keep_home {
        remove_dir_all(&home);
    }
    create_dir_all(&home)?;
    if matches!(setup.keys, Keys::Prewritten) {
        let file = home.join(".jev-router.env");
        write_file(&file, PREWRITTEN_KEYS, false)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600));
        }
    }
    for seed in &setup.seeds {
        seed(&home)?;
    }

    let mut command = Command::new(&installer.command);
    command.args(&installer.args);
    if !setup.args.is_empty() {
        if !installer.takes_options() {
            return Err(format!("{} cannot take options", installer.label));
        }
        // `bash -s -- --help`: the script comes from stdin, so what follows are its arguments.
        if installer.stdin.is_some() {
            command.arg("--");
        }
        command.args(&setup.args);
    }
    if let Some(raw) = &installer.raw_args {
        if !cfg!(windows) {
            return Err(format!("{raw}: raw arguments are Windows only"));
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.raw_arg(raw);
        }
    }
    command.current_dir(setup.cwd.as_deref().unwrap_or(work));
    for name in steering_variables() {
        command.env_remove(name);
    }
    for name in ["ANTHROPIC_API_KEY", "ZDOTDIR"] {
        command.env_remove(name);
    }
    command
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("SHELL", "/bin/bash")
        .env("JEV_RELEASE_URL", &setup.url)
        .env("JEV_INSTALL_DIR", setup.dir.clone().unwrap_or_else(|| install_dir.clone().into_os_string()))
        .env("JEV_INSTALL_CLAUDE", "0")
        .env("PATH", build_path(&setup.path, work, &std::env::var_os("PATH").unwrap_or_default())?);
    if !setup.modify_path {
        command.env("JEV_NO_MODIFY_PATH", "1");
    }
    if cfg!(windows) {
        // Windows PowerShell 5.1 expands "~" from these, not from USERPROFILE.
        let text = home.to_string_lossy().into_owned();
        if text.as_bytes().get(1) == Some(&b':') {
            let (drive, rest) = text.split_at(2);
            command.env("HOMEDRIVE", drive).env("HOMEPATH", rest);
        }
    }
    if let Keys::Env { jev, anthropic } = &setup.keys {
        command.env("CLAUDECODE", "1");
        if let Some(jev) = jev {
            command.env("JEV_API_KEY", jev);
        }
        if let Some(anthropic) = anthropic {
            command.env("ANTHROPIC_API_KEY", anthropic);
        }
    }
    for (name, value) in &setup.env {
        command.env(name, value);
    }
    for name in &setup.unset {
        command.env_remove(name);
    }
    for name in &installer.remove {
        command.env_remove(name);
    }
    let input = match &installer.stdin {
        Some(file) => Some(std::fs::read(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?),
        None => None,
    };
    let started = Instant::now();
    let (status, output) = match &setup.pty {
        Some(typed) => {
            if !have_script() {
                return Err("script(1) is not installed".into());
            }
            run_collecting(in_pty(&command), Some(typed.clone().into_bytes()), setup.timeout, None)?
        }
        None => run_collecting(command, input, setup.timeout, setup.interrupt_at.as_deref())?,
    };
    Ok(Run { install_dir, home, status, output, elapsed: started.elapsed(), shown: std::cell::Cell::new(false) })
}
