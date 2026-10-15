# The Rust implementation

`rust/` is jev-claude: the launcher, the local routing proxy, the status line, and the helper
programs. It is the only implementation, and the one that ships. It follows
[`SPEC.md`](../SPEC.md), together with the golden cases in `conformance/cases/`; where the spec is
silent, this code decides. It replaced the Node.js edition, which is retired and available at the
git tag `v0.11.0`. The repository holds no other implementation, and no Python or JavaScript: the
developer and release tasks are the `cargo xtask` crate in `rust/xtask`, and the conformance
suite is Rust integration tests.

The Rust build is a set of native executables with no runtime to install. It starts in a few
milliseconds, and the proxy runs on Tokio and hyper. Most people should not build it: every
release carries ready-made programs, and the one-line installer sets them up, with no Rust
toolchain and no Node.js; see [Install a release](#install-a-release). Build from source to work
on the code, or on a platform that has no release archive.

- [Programs](#programs)
- [Install a release](#install-a-release)
- [Install the toolchain](#install-the-toolchain)
- [Build](#build)
- [Install the programs and put them on PATH](#install-the-programs-and-put-them-on-path)
- [Run jev-claude](#run-jev-claude)
- [Configuration](#configuration)
- [Tests](#tests)
- [Lint, format and docs](#lint-format-and-docs)
- [Calibration examples](#calibration-examples)
- [Conformance tests](#conformance-tests)
- [Release tasks (cargo xtask)](#release-tasks-cargo-xtask)
- [The full gate](#the-full-gate)
- [Troubleshooting](#troubleshooting)
- [Behaviour notes](#behaviour-notes)
- [Differences from the retired Node edition](#differences-from-the-retired-node-edition)
- [Source layout and dependencies](#source-layout-and-dependencies)

## Programs

`cargo build` produces seven executables, with `.exe` added on Windows:

| Binary | What it does | SPEC |
| --- | --- | --- |
| `jev-claude` | The launcher. It loads your settings, starts the proxy, and runs the real `claude` through it | 10 |
| `jev-statusline` | The status line command that Claude Code runs | 12 |
| `jev-explain <session id>` | The explanation panel for `/jev-explain` | 13 |
| `jev-legend` | The status line key for `/jev-legend` | 13 |
| `jev-check` | The read-only setup report for `/jev-calibrate` | 13 |
| `jev-update-check` | The update check. It writes `~/.jev-router/update.json`. The launcher does not start it yet (SPEC 1.2) | 14 |
| `jev-proxy-host` | Starts the proxy on its own, prints `PORT=<port>`, and runs until killed. The conformance tests use it | 16.3 |

The crate is `jev-router`: edition 2024, minimum supported Rust 1.85 (MSRV), and version
`0.0.0`. The release version lives only in the root `package.json`, which the programs read at
run time (SPEC 2.3).

## Install a release

The installers fetch a release archive, check it, and set it up. They install the prebuilt
programs: they never install Rust, and they never build anything. The command is the same on every
system, one line per shell family:

```bash
curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh | bash   # macOS, Linux, WSL, Git Bash
```

```powershell
irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex   # Windows PowerShell 5.1 or 7
```

```bat
powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex"
```

The last line is for Command Prompt (cmd.exe): it runs the same `install.ps1` through Windows
PowerShell. From a clone, run `./install.sh`, or `.\install.ps1` (`powershell -ExecutionPolicy
Bypass -File .\install.ps1` if scripts are blocked). `-h`/`--help` (`-Help` for `install.ps1`)
prints the settings from the script's header; piped through `| bash` or `| iex` there is no file to
read, so it points to the header on GitHub instead. The older `--rust` and `--node` options and `JEV_IMPL` are still accepted and
ignored, with an `info` line, so existing instructions keep working: there is one edition.

**What it checks first.** Every check is printed with `ok`, `info`, `warn` or `FAIL`. All of them
run before anything is changed, and all the failures are listed together with how to fix each:

| Check | Kind | If it is missing |
| --- | --- | --- |
| Platform | required | `windows-x64` (an ARM64 PC on Windows 11 runs it through emulation; Windows 10 on ARM cannot, and both installers stop there), `macos-arm64`, `macos-x64`, `linux-x64`, `linux-arm64`. The Linux programs are static (musl), so glibc and musl systems (Alpine) take the same archive; on musl the row says so. WSL counts as Linux and Git Bash as Windows. Anything else stops the installer: [build from source](#install-the-toolchain) |
| Download tool | required | `install.sh` uses `curl`, else `wget`, else offers to install `curl` with the package manager (`apt-get`, `dnf`, `yum`, `zypper`, `apk`, `pacman` or `brew`). `install.ps1` uses PowerShell's own web cmdlets, always present, and forces TLS 1.2 for its own downloads (Windows PowerShell 5.1 can default to older versions that GitHub refuses). The `irm` that fetches `install.ps1` itself runs before that; see [Troubleshooting](#troubleshooting) if it fails |
| `tar` | required | `install.sh` offers the package manager. `install.ps1` uses `System32\tar.exe` (Windows 10 version 1803 and later), else a `tar.exe` on PATH, else Git for Windows' GNU tar; with none of them it stops with the fix |
| Checksum tool | required | `sha256sum`, `shasum` or `openssl`, whichever is there; when none is, `install.sh` offers `coreutils` (which has `sha256sum`) through the package manager (a Mac always has `shasum`). PowerShell uses .NET |
| Network | required | any answer from the release host counts; silence is reported as a network problem. `install.ps1` only warns on HTTP 407 (a proxy that wants a sign-in) and tries anyway |
| Claude Code (`claude`) | **required** | jev-claude is a launcher for it, and nothing works without it, so the installer installs it for you with Anthropic's own installer (`curl -fsSL https://claude.ai/install.sh \| bash`, or `irm https://claude.ai/install.ps1 \| iex`), asking first unless `JEV_ASSUME_YES` or `JEV_INSTALL_CLAUDE=1`. If you decline (`JEV_INSTALL_CLAUDE=0`, or answer no), the installer stops before it changes anything and prints the command; if Anthropic's installer fails, it stops there with the same command (a missing `curl`, `tar` or checksum tool you agreed to install is installed before Claude Code), before any of jev-claude is installed. After installing, it looks for `claude` again (Anthropic's installer puts it in `~/.local/bin`, which the installer adds to its own PATH for the rest of the run) and reports whether it is the native build or the older npm package, which runs on Node.js. A `claude` that is found but whose `claude --version` fails is a `FAIL` row (`found at <path> but it does not run`) with the command to reinstall it. Under WSL a Claude Code on the Windows side (under `/mnt/<drive>`) does not count: WSL needs its own |
| Claude Code's libraries (Alpine) | required on musl | Claude Code's musl build needs `libgcc`, `libstdc++` and `ripgrep`. When Claude Code is missing or does not run on Alpine, `install.sh` offers `apk add --no-cache libgcc libstdc++ ripgrep` (another musl distribution is only told the names), and then offers to add `{"env": {"USE_BUILTIN_RIPGREP": "0"}}` to `~/.claude/settings.json` so Claude Code uses that `ripgrep`; with no one to ask, it prints the setting |
| git | optional | only the status line's branch and worktree names use it. The installer offers `brew install git`, `apt-get install -y git`, `dnf`, `apk`, `pacman`, or `winget install --id Git.Git -e`; declining is fine |
| Rust toolchain | never needed | printed as `info` if present; the installer never installs it |
| Node.js | not used | printed as `info` if present; never required, never installed |
| Install folder | required | the install folder must be missing, empty, or an earlier jev-claude install (its `package.json` names `jev-router` and its `bin` holds `jev-claude`). Any other folder is refused and never touched, in this first step, before anything is changed (and again just before the swap); set `JEV_INSTALL_DIR` to another folder |
| Old Node edition | `warn` only | a `jev-claude` on PATH that is a JavaScript file (`.mjs`, `.js`) or a `.cmd`, `.ps1` or `.bat` script, a pnpm shim naming a `.mjs` file, or `~/jev-claude/node/bin/jev-claude.mjs`; only reported, with how to remove it (see "The Node edition" below) |

**Installing a prerequisite.** For every missing item the installer can fix, it shows the exact
command first and asks `y/N` before it changes the system. `JEV_ASSUME_YES=1` (or `-y`, `--yes`;
`-Yes` for `install.ps1` run as a file) answers yes to every such question. It uses `sudo` only
when you are not root and `sudo` exists, and says so in the question; it never runs `sudo`
silently. With nobody to ask, and no `JEV_ASSUME_YES`, it does not install anything: it prints the
command to run, and fails if the item is required. "Nobody to ask" means a run with no terminal
(`install.sh` reads the answer from `/dev/tty`, so a piped script or redirected output still asks;
`install.ps1` does not ask when its input is redirected or a console was started with
`-NonInteractive`), or a Claude Code session (`CLAUDECODE` is set). It never
touches anything outside the item it is installing, and a failed install is reported with the
manual fix.

**What it does.** Numbered steps, each printed: the checks (and the prerequisites you agreed to);
the download (the URL is shown); the SHA-256 check (expected and actual are both shown, and a
mismatch installs nothing); the install; the PATH; and a run of the installed `jev-check`. The
archive is unpacked next to the destination and swapped in only when all seven programs are
there, so a failed unpack never leaves half an install, and a running `jev-claude` that blocks the
swap is reported, with a request to close it and run the installer again. The earlier install is
set aside, not deleted, until the new one's `jev-check` has run; if that fails, the earlier
install is put back. Keys are then asked for, as the root README's
[Install](../README.md#install) section describes, and saved to `~/.jev-router.env`, readable
only by you. A key is never printed or logged, and the installer never asks for one in an agent
or plugin session.

**Where things go.** `~/jev-claude-rust` (`JEV_INSTALL_DIR` changes it) holds:

```text
bin/                          the seven programs
.claude/skills/               /jev-calibrate, /jev-explain, /jev-legend
package.json                  name jev-router and the release version, nothing else (SPEC 1.2, 2.3)
.env.example  LICENSE
```

This is the same shape the programs look for in a clone (SPEC 2.4): they walk up from `bin/` to
the folder holding `.claude/skills/jev-calibrate/SKILL.md`, so `--add-dir` hands Claude Code the
skills and the programs report their version, with no `JEV_ROOT`. The skills in the archive are
the repository's own, copied by `cargo xtask package`, which drops the
`/jev-calibrate retune` section because re-tuning only makes sense in a source clone. They run
the native programs by name (`jev-check`, `jev-explain`, `jev-legend`), found on PATH, so the
same line runs under Bash and PowerShell, and no Node.js is involved.

**PATH.** The result is the same on every system: typing `jev-claude` in a new terminal works.

- macOS, Linux and WSL (`install.sh`): the seven programs are symlinked into `~/.local/bin`
  (`JEV_BIN_DIR` to change it). Links, not copies,
  because `jev-claude` finds `jev-statusline` beside itself and reads its version from the
  install's `package.json`. A link the installer did not make, or a file that is not a link, is
  left alone and named. If the folder cannot be written, the install's own `bin` goes on PATH
  instead. If the folder is not on PATH yet, one `export PATH` line (`fish_add_path` for fish),
  marked with a `# jev-claude (Rust edition)` comment, is added once to your login shell's file:
  `$ZDOTDIR/.zshrc` (default `~/.zshrc`) for zsh, `~/.bashrc` for bash on Linux and WSL, on macOS
  the first of `~/.bash_profile`, `~/.bash_login` and `~/.profile` that exists (else
  `~/.bash_profile`), fish's `config.fish`, or `~/.profile` for any other shell. A profile it
  cannot write is reported with the line to add yourself.
- Windows, from PowerShell (`install.ps1`) or from Git Bash (`install.sh`, which has no symlinks
  to use and so installs as Windows does): the install's own `bin` folder goes first on your
  Windows user PATH, which lives in the registry; `%VAR%` entries already there stay unexpanded,
  and running programs are told the environment changed. PowerShell, Command Prompt and Git Bash
  all read that setting when they start, so one run covers every Windows shell (Git Bash gets no
  profile line); open a new terminal to pick it up (Windows Terminal and VS Code give new tabs the
  PATH they started with, so restart the app if a new tab cannot find `jev-claude`). Both
  installers also add Claude Code's folder, `%USERPROFILE%\.local\bin`, to the end of the user PATH
  when Claude Code is there and the folder is on neither the user nor the system PATH, because
  Anthropic's Windows installer leaves it off. In Git Bash, `HOME` is taken from `USERPROFILE`, so
  both installers write the same settings file.

`JEV_NO_MODIFY_PATH=1` changes neither the profile nor the PATH and prints what to add.

**Settings.** `JEV_VERSION` (a tag such as `v0.11.0`; the default is the latest release),
`JEV_INSTALL_DIR`, `JEV_BIN_DIR` (`install.sh` only), `JEV_NO_MODIFY_PATH`, `JEV_INSTALL_CLAUDE`
(`1` installs Claude Code when it is missing without asking, `0` never offers),
`JEV_ASSUME_YES`, and `JEV_RELEASE_URL` (a mirror with the same layout: `latest/download/<archive>`
and `download/<tag>/<archive>`; an `https://` address or a `file://` folder on disk). The two
scripts take the same names. `JEV_API_KEY` and
`ANTHROPIC_API_KEY` save the keys without the prompt. The yes/no settings take `1`, `true` or
`yes` and `0`, `false` or `no`, in any letter case. `JEV_INSTALL_DIR` and `JEV_BIN_DIR` may be
relative or, in `install.sh`, start with `~`; they are made absolute before anything is written.

**Updating and removing.** Run the installer again: it reports `updated 0.10.0 -> 0.11.0` or
`reinstalled version 0.11.0`. To remove it, delete the install folder, the `jev-*` links in
`~/.local/bin`, the marked line in your shell profile (if there is one), and the folder's entry in
your user PATH. You can also delete the files the programs made: `~/.jev-router/` (first-run and
update-check state), `~/.jev-claude.log` (written only with `JEV_DEBUG`) and the status folder
(`JEV_STATUS_DIR`, by default `<temp>/jev-claude-<uid>`, or `<temp>\jev-claude` on Windows). Your
settings in `~/.jev-router.env` are yours and stay, and so does Claude Code.

**The Node edition.** Up to v0.11.0 the installer's default was the Node edition, a git clone in
`~/jev-claude` linked with `pnpm add --global`; `--rust` / `JEV_IMPL=rust` installed this edition
into `~/jev-claude-rust`. An earlier `--rust` install is updated in place by running the installer
again, and its profile line (marked `# jev-claude (Rust edition)`) is recognised and not
duplicated; `--rust` and `--node` (`-Rust` and `-Node` in `install.ps1`) and `JEV_IMPL` are
accepted and ignored, with an `info` row saying so. This installer does not touch a Node clone. If it finds
one (the `Old Node edition` row above), it prints how to remove it (`pnpm remove --global
jev-router`, then delete the clone: `rm -rf` in `install.sh`, `Remove-Item -Recurse -Force` in
`install.ps1`) and, in step 5, which `jev-claude` comes first on PATH, and never deletes anything
itself. The README's [Upgrading from the Node edition](../README.md#upgrading-from-the-node-edition)
has the commands for Bash, PowerShell and the Command Prompt. The new install is in `~/jev-claude-rust`, which is why the two do not collide,
and both read the same settings file. On Unix their status folders differ (the Node edition used
`<temp>/jev-claude`), so a saved-model memo of the old edition is not picked up.

**The archives.** Each release has `jev-claude-<platform>.tar.gz` for the five platforms above and
a `SHA256SUMS` file. The names carry no version, so `releases/latest/download/<name>` always
works. To install by hand: download the archive and `SHA256SUMS`, compare with `sha256sum -c`,
unpack anywhere, and put its `bin` on PATH. CI builds them from the release tag with the
programs linked statically (musl on Linux, the C runtime on Windows), so nothing else has to be
installed, and the Linux archives run on glibc and musl systems alike, Alpine included. It then
installs each one with the real installer, on a machine of its own architecture, before
publishing (the Intel Mac archive is built and installed on an Intel runner); each Linux archive is
also checked to be statically linked, and each Windows one to link the C runtime in.

**Also on the release page.** `install.sh` and `install.ps1` as they are at the release tag, and
`jev-claude-windows-x64.msi` (below). `SHA256SUMS` lists every file of the release; the installers
look their archive up there by exact file name.

**Unsupported platforms.** For a platform with no archive (32-bit systems, FreeBSD, other CPU
architectures) the installer stops with the reason. There is no source-build fallback in the
installer: build the programs as described in [Install the toolchain](#install-the-toolchain) and
[Install the programs and put them on PATH](#install-the-programs-and-put-them-on-path).

### The Windows installer (MSI)

The command-line installers above are the recommended way to install, on Windows too: they check the
machine, install Claude Code when it is missing, ask for the Jev key, and update with the same line.
For people who want a standard Windows setup wizard, each release also has
`jev-claude-windows-x64.msi` (x64; Windows 11 on ARM runs it through emulation, like the archive).

- **What it installs.** The same files as the Windows archive, in the same layout (`bin\` with the
  seven programs, `.claude\skills\`, `package.json`, `.env.example`, `LICENSE`), per user, into
  `%LocalAppData%\Programs\jev-claude`. No administrator prompt. It adds that folder's `bin` to your
  user PATH; open a new terminal for it to take effect.
- **What it does not do.** It does not install Claude Code, git or anything else, and does not ask
  for a key. Run `jev-check` afterwards: it reports what is missing, and `jev-claude` offers its
  setup check on the first launch. See the README's
  [Settings file and API key](../README.md#settings-file-and-api-key).
- **Signing.** It is not code-signed, so Windows SmartScreen shows "Windows protected your PC": More
  info, then Run anyway. Compare its hash with `SHA256SUMS` if you want to be sure.
- **Silent install.** `msiexec /i jev-claude-windows-x64.msi /qn` (add `/l*v install.log` for a log).
- **Update.** Install the newer MSI over it (double-click, or the `msiexec` line); the old version is
  replaced. A downgrade is refused.
- **Remove.** Settings > Apps > jev-claude > Uninstall, or `msiexec /x jev-claude-windows-x64.msi /qn`.
  It removes the files and the PATH entry. `~/.jev-router.env` and the other files listed in the
  README's [Removing](../README.md#removing) stay.
- **Do not mix the two.** An install by `install.ps1` lives in `~\jev-claude-rust`, an MSI install in
  `%LocalAppData%\Programs\jev-claude`, and each puts its own `bin` on PATH. Having both means two
  copies, and whichever PATH entry comes first runs. `install.ps1` does not update an MSI install and
  `jev-update-check` does not look at either. Pick one and remove the other.

CI builds it with WiX (`installer/windows/jev-claude.wxs`, `.github/scripts/build-msi.ps1`) from the
archive it has just checked, then installs it on a Windows runner, runs a program, installs it again,
uninstalls it, and checks that the files and the PATH entry are gone
(`.github/scripts/test-msi.ps1`), before a release can be tagged.

## Install the toolchain

Building from source is for contributors and for platforms that have no release archive; to use
jev-claude, [install a release](#install-a-release). You need Rust 1.85 or newer, installed through
[rustup](https://rustup.rs), plus a C compiler, because the `ring` TLS backend compiles a little C.
The tests also need `git` on `PATH`: they make real repositories. Node.js is optional: a few
launcher tests start an npm-style stand-in `claude` through `node`, as an npm-installed Claude Code
starts, and without `node` on `PATH` they print a note and skip. Nothing else needs it, and
running jev-claude, including its `/jev-*` skills, never does.

**Windows** (PowerShell):

```powershell
winget install --id Rustlang.Rustup
# The MSVC linker and C compiler (skip if Visual Studio with "Desktop development with C++" is installed)
winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
# Open a new terminal, then:
rustup default stable
rustup component add clippy rustfmt
rustup toolchain install 1.85      # only to check the MSRV
```

**macOS:**

```bash
xcode-select --install             # Apple's C compiler and linker
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"
rustup component add clippy rustfmt
rustup toolchain install 1.85      # only to check the MSRV
```

**Linux** (Debian/Ubuntu; use your distribution's equivalent of `build-essential`):

```bash
sudo apt-get install -y build-essential git curl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"
rustup component add clippy rustfmt
rustup toolchain install 1.85      # only to check the MSRV
```

Check the toolchain with `cargo --version`, which must print 1.85 or later.

## Build

Run these from the repository root. All of them work the same in Git Bash, PowerShell, and on
macOS and Linux:

```bash
cd rust
cargo build --release --locked
```

The programs land in `rust/target/release/`. `--locked` builds the exact dependency versions in
the committed `Cargo.lock`. A debug build (`cargo build`) goes to `rust/target/debug/` and is
what the tests use.

## Install the programs and put them on PATH

Two rules apply, whichever way you install:

1. **Keep the seven programs together.** `jev-claude` points Claude Code's status line at the
   `jev-statusline` program in its own directory. If you copy `jev-claude` somewhere on its own,
   the status line command points at a file that does not exist.
2. **Tell programs outside the clone where the clone is.** The programs find the repository by
   walking up from their own location to the nearest directory holding
   `.claude/skills/jev-calibrate/SKILL.md`. Inside `rust/target/release/` this always works.
   Anywhere else, set `JEV_ROOT` to the clone. Without it, the launcher cannot pass
   `--add-dir <clone>`, so the `/jev-*` skills are missing outside the clone. `jev-check` also
   reports `Mode installed` instead of `repository`, and the release version that the programs
   send to Jev falls back to `0.0.0`.

### Option A: use the build directory (recommended)

Put `rust/target/release` on `PATH`. Rebuilding updates it in place, and no `JEV_ROOT` is needed.

Git Bash, macOS, or Linux. Add the line to `~/.bashrc` or `~/.zshrc` to keep it:

```bash
export PATH="/path/to/jev-claude/rust/target/release:$PATH"
```

PowerShell for the current session:

```powershell
$env:Path = "C:\path\to\jev-claude\rust\target\release;$env:Path"
```

PowerShell for every new terminal (your user `Path`):

```powershell
$dir = "C:\path\to\jev-claude\rust\target\release"
[Environment]::SetEnvironmentVariable("Path", "$dir;" + [Environment]::GetEnvironmentVariable("Path", "User"), "User")
```

### Option B: `cargo install`

This copies all seven programs into one `bin` directory, which keeps them together:

```bash
cargo install --path rust --locked                          # into ~/.cargo/bin (already on PATH with rustup)
cargo install --path rust --locked --root ~/.local/jev-rust # or into ~/.local/jev-rust/bin
```

Then set `JEV_ROOT` to the clone permanently, because these copies live outside it:

```bash
echo 'export JEV_ROOT="/path/to/jev-claude"' >> ~/.bashrc          # Git Bash / Linux; ~/.zshrc on macOS
```

```powershell
[Environment]::SetEnvironmentVariable("JEV_ROOT", "C:\path\to\jev-claude", "User")
```

Run `cargo install` again after each `git pull` to update the copies.

### Next to other installs

If you also ran the installer, its `jev-claude` is on `PATH` as well (and so, if you never
removed it, is the retired Node edition's), and whichever directory comes first on `PATH` wins. To
see which one runs, use `command -v jev-claude` (Bash), `Get-Command jev-claude -All`
(PowerShell) or `where jev-claude` (Command Prompt). To keep the installed one as the default and still try your build, leave the build
directory off `PATH` and call it by path, or define a short name:

```bash
alias jev-claude-dev='/path/to/jev-claude/rust/target/release/jev-claude'
```

```powershell
function jev-claude-dev { & "C:\path\to\jev-claude\rust\target\release\jev-claude.exe" @args }
```

Every copy (your build and an installed release) shares the same files: settings, the status
directory, the decision log, and the update state. Use whichever you like, one session at a time or
side by side. (The retired Node edition shared the settings, the log and the update state too, but
on Unix kept its status files in `<temp>/jev-claude`, not the per-user folder below.)

## Run jev-claude

Give `jev-claude` the same arguments you would give `claude`:

```bash
jev-claude                         # interactive session
jev-claude -p "what is 2+2?"       # one-shot
jev-claude --resume                # any claude flag passes through
```

What it needs:

- **The real Claude Code CLI on `PATH`** as `claude`. On Windows, if Claude Code was installed as
  the older npm package, the launcher finds npm's `claude.cmd` shim and runs the script behind it
  with `node`; the native build is a plain executable.
- **A Jev API key** in `~/.jev-router.env` (see [Configuration](#configuration)). Without one,
  Claude Code starts without routing and the launcher says how to turn it on.
- **The `jev-*` programs on `PATH`, for the slash commands.** `/jev-calibrate`, `/jev-explain` and
  `/jev-legend` are skills in `.claude/skills/` that run the native programs `jev-check`,
  `jev-explain` and `jev-legend` by name, in a clone and in an
  [installed release](#install-a-release) alike. The status files they read have the same format
  in every copy. No Node.js is involved.

On the first interactive run, the launcher offers to check your setup with `/jev-calibrate`.
Then it starts the proxy on a loopback port and points Claude Code at it with
`ANTHROPIC_BASE_URL`. It adds **Jev Router** to `/model`, installs its status line (unless you
already have one or set `JEV_NO_STATUSLINE`), and waits for Claude Code to exit, then exits with
Claude Code's exit code.

A first argument that is a Claude Code subcommand (`jev-claude mcp list`, `plugin`, `doctor`, `update`, `auth`, and the rest of `claude --help`'s commands) is passed straight through: no proxy, no `--add-dir`, no status line, since those commands manage Claude Code and reject the extra flags.

You can also run the helper programs directly, for example
`jev-explain <session id>`, `jev-legend` or `jev-check`.

## Configuration

Settings come from three places. A variable already set in the environment always wins over the
files:

| Source | What it may set |
| --- | --- |
| The process environment | Anything |
| `~/.jev-router.env` (and the older `~/.jev-claude.env`) | Anything; this is your own settings file |
| `.env` in the directory you start `jev-claude` from | Only `JEV_API_KEY`, `TYPESAFE_API_KEY`, `JEV_DEBUG`, `JEV_ALLOW_FABLE`, `JEV_NO_STATUSLINE`, `JEV_ICONS`, and the `JEV_*_EFFORT` / `JEV_*_FORCE_EFFORT` keys, so a cloned repository can never redirect your traffic |

Start from the commented template:

```bash
cp .env.example ~/.jev-router.env                      # Git Bash, macOS, Linux
```

```powershell
Copy-Item .env.example "$HOME\.jev-router.env"
```

Then paste your key after `JEV_API_KEY=`.

| Variable | Effect |
| --- | --- |
| `JEV_API_KEY` (or `TYPESAFE_API_KEY`) | Turns routing on. It is sent only to Jev and is removed from Claude Code's environment |
| `ANTHROPIC_API_KEY` | Runs Claude Code on an API key instead of your sign-in. Set it only in your own file or shell |
| `JEV_ALLOW_FABLE` | `0`, `false`, `no` or `off` stops routing to Fable |
| `JEV_SONNET_EFFORT`, `JEV_OPUS_EFFORT`, `JEV_FABLE_EFFORT` | The effort a tier gets when Claude Code sends none (`low`, `medium`, `high`, `xhigh`, `max`) |
| `JEV_FORCE_EFFORT`, `JEV_<TIER>_FORCE_EFFORT` | An effort that replaces the one Claude Code sends; the per-tier key wins |
| `JEV_DEBUG` | Logs each routing decision to `~/.jev-claude.log` |
| `JEV_NO_STATUSLINE` | Leaves Claude Code's status line alone |
| `JEV_ICONS` | `symbols` or `text` (also `ascii`) for the status line labels; the default is symbols, except in a plain Windows console |
| `ANTHROPIC_BASE_URL` | When set before launch, the proxy forwards to it instead of `https://api.anthropic.com` |
| `ANTHROPIC_MODEL` | Starts the session on that model instead of Jev Router |
| `TYPESAFE_BASE_URL`, `TYPESAFE_DEFAULT_MODEL` | Point the router at another Jev endpoint or model |
| `JEV_ROOT` | The repository clone, for programs installed outside it |
| `JEV_STATUS_DIR` | The status directory; the default is `<temp>/jev-claude-<uid>` on Unix (your numeric user id, so users who share `/tmp` do not lock each other out) and `<temp>\jev-claude` on Windows |
| `JEV_DUMP` | `1` (or `true`, `yes`) saves each request body into the status directory; any other value is a path prefix. This is for debugging only, because bodies hold your conversation |

The root [README's Configuration section](../README.md#configuration) explains each setting in
more depth. The programs read the same files and variables as the retired Node edition did.

## Tests

```bash
cd rust
cargo test --workspace --locked
```

`--workspace` adds the tests of the `xtask` crate; plain `cargo test` in `rust/` runs the product
crate's tests alone. The tests need `git` on `PATH`; Node.js is optional (see
[Install the toolchain](#install-the-toolchain)). They run offline: every server they talk to is a
loopback fake. `cargo test --workspace` runs:

- one test file per unit-test file of the retired Node edition (the `node/test/` directory at tag
  `v0.11.0`), with each Node test title kept as the Rust test's doc comment (a handful were
  reworded, or were generated per case in Node): `explain`, `first_run`, `hardening`, `icons`,
  `launcher`, `legend`, `model_names`, `policy`, `proxy`, `proxy_routing`, `settings`,
  `statusline`, `update`, `worktree`. They use real temporary files, real
  git repositories, the real `jev-statusline` binary, real child processes, and loopback HTTP
  servers (a fake Jev and a fake Anthropic upstream). `hardening` also covers Rust-only paths: the
  launcher's child environment, run as a real child process, and a client that disconnects while
  Jev is being asked (SPEC 20.10). The exceptions: Node's `status.test.mjs` became `fsx.rs`, which
  tests the atomic file replace under Rust names; `bump-version` and `package-rust-release` became
  `rust/xtask/tests/bump.rs` and `package.rs`; `usage.test.mjs` and `env-cases.test.mjs` have no
  file of their own, because the usage meters and the env-file parser are covered by the
  `usage-meters` and `parse-env` golden cases in `tests/golden.rs` (the `parse-env` cases call the
  parser directly, not through `load_env`);
- the Rust-only `tests/check.rs` (the `jev-check` report), `repo.rs` (root and version
  resolution), and `router.rs` (the Jev client's retries and timeouts);
- `tests/bundle.rs`, which stages the programs cargo just built with the release packager
  (`xtask::package`, the code behind `cargo xtask package`) and checks the result as a user gets
  it: all seven programs, the root found from `bin/` with no `JEV_ROOT`, the release version,
  every skill calling a native program that is in the bundle, and the launcher passing the bundle
  as `--add-dir`;
- `tests/golden.rs`, which checks every file in `conformance/cases/` (SPEC 16.2) against this
  implementation and decodes the tagged encoding in `conformance/cases/README.md`.
  `every_case_file_is_checked` fails when a new case file appears without a test;
- the black-box conformance tests, `tests/conformance_*.rs` (see [Conformance tests](#conformance-tests));
- the unit tests inside `src/` (JSON, string and number semantics), and the `xtask` tests (the
  version bump rules and the packaged skills).

Every temporary directory is removed when its test ends. The status directory is one fixed
directory per test binary, `jev-status-test-rust-<binary>` under the system temp directory, and
each run empties it and reuses it.

To run one file or one test:

```bash
cargo test --locked --test proxy
cargo test --locked --test hardening a_client_that_leaves
```

## Lint, format and docs

The lint policy is in `rust/Cargo.toml` under `[lints]`, so plain `cargo clippy` and
`cargo build` apply it without extra flags:

- `unsafe_code = "deny"`. The only `unsafe` is three foreign-function calls that the standard
  library does not offer: the Windows profile directory and the Unix user id (for the
  `os.homedir` rules SPEC 3.8 takes from Node), and `kill(2)` to forward a signal to Claude Code.
  Each sits in one small function with `#[allow(unsafe_code)]` and a `SAFETY:` comment, which
  `clippy::undocumented_unsafe_blocks` enforces.
- `missing_docs = "warn"`: every public item of the library is documented.
- `clippy::all` and `clippy::pedantic` as warnings, with a short list of allows, each justified
  in `Cargo.toml`. The `as` casts are allowed because the crate reproduces JavaScript's number
  and UTF-16 arithmetic on purpose. `must_use_candidate` is allowed because the crate is
  internal. `missing_errors_doc` and `missing_panics_doc` are allowed because SPEC.md defines the
  errors. `too_many_lines`, `similar_names` and `many_single_char_names` are allowed because the
  code follows the original Node source (tag `v0.11.0`) line by line.
- `rustfmt.toml` sets a 120-column width, and `.cargo/config.toml` turns every rustdoc warning
  into an error.

```bash
cd rust
cargo fmt --all --check                                         # or `cargo fmt --all` to fix
cargo clippy --workspace --all-targets --locked -- -D warnings  # every warning fails
cargo doc --workspace --no-deps --locked                        # fails on any rustdoc warning
cargo +1.85 check --workspace --all-targets --locked            # the MSRV still builds
```

`rust/xtask` has the same `[lints]` policy as the product crate, less the allows that only the
JavaScript arithmetic needs.

Fix a lint rather than allowing it. If an allow is unavoidable, scope it to the item and give it
a one-line reason.

## Calibration examples

Two Rust examples call the real Jev and need `JEV_API_KEY` (from the environment or
`~/.jev-router.env`). They are developer tools and are not installed:

```bash
cargo run --manifest-path rust/Cargo.toml --release --example calibrate -- --runs 2
cargo run --manifest-path rust/Cargo.toml --release --example live_routing
```

`calibrate` asks Jev about the labelled prompts in `rust/examples/calibration-cases.json` and reports how
often it picked the intended tier and where each turn finally ran; `live_routing` asks about a few
sample prompts. The tuning they measure is in `rust/src/config.rs`, which is its source of truth:
run `calibrate` before and after a change to the guidance, costs, effort or thresholds there, and
keep the change only when the pick score holds. `/jev-calibrate retune` drives this on a branch.

## Conformance tests

The black-box conformance tests (SPEC 16.3) treat the proxy and the status line as black boxes.
They are Rust integration tests, `rust/tests/conformance_routing.rs`, `conformance_models.rs`,
`conformance_transport.rs` and `conformance_expectations.rs`, with their shared machinery in
`rust/tests/conformance/` (the fake Jev and Anthropic servers, the client, and the harness's own
JSON, SHA-256 and HTTP code, so it shares nothing with the crate it tests). They start the
`jev-proxy-host` and `jev-statusline` that Cargo builds for the test run
(`CARGO_BIN_EXE_jev-proxy-host`, `CARGO_BIN_EXE_jev-statusline`), so they always test the current
source and never a stale build; there is nothing to point them at. What each request should
become is precomputed in `conformance/fixtures/harness-expectations.json`, and
`conformance_expectations.rs` cross-checks every entry of that file against the golden cases.

They run as part of `cargo test`. To run just them, from `rust/`:

```bash
cargo test --locked --test 'conformance*'
cargo test --locked --test conformance_routing    # one file
```

They need only the Rust toolchain and no network access. Every test has a 60 s limit, and a
request the program never answers fails after 20 s, so a hang fails its test instead of the run.
[`conformance/README.md`](../conformance/README.md) describes the environment each program runs
in, and how to add or amend an expectation.

## Release tasks (cargo xtask)

The developer and release tasks live in the `xtask` crate (`rust/xtask`), a member of the
workspace that is never shipped. `rust/.cargo/config.toml` defines the alias, so run them from
`rust/`; relative paths are taken from there:

```bash
cargo xtask bump-version [<before>] [<after>] [--dry-run]
cargo xtask package --bin-dir target/release [--target <platform>] [--out ../dist] [--stage-only]
cargo xtask installer-smoke-test --bin-dir target/release
cargo xtask installer-smoke-test --archive ../dist/jev-claude-<platform>.tar.gz
cargo xtask installer-smoke-test --bin-dir target/release --installers <dir> --only links,profiles
```

- `bump-version` works out the next release version from every commit since the last `v*` release
  tag up to `<after>` (default `HEAD`), so commits from a push whose CI run failed still count; with
  no tag yet it uses `<before>..<after>`, or just `<after>` when `<before>` is missing or all zeros
  (a branch's first push). The bump is minor for `feat:`, major for a breaking change, patch
  otherwise (see the README's
  [Versions and commit messages](../README.md#versions-and-commit-messages)). It writes the version
  to the root `package.json` and prints only the version. `--dry-run` prints it without writing. CI's
  `release` job runs it.
- `package` turns built programs into `jev-claude-<platform>.tar.gz` (`windows-x64`,
  `macos-arm64`, `macos-x64`, `linux-x64`, `linux-arm64`; the host's by default) and a
  `SHA256SUMS`, in `--out` (default `dist/` at the repository root, which git ignores). It holds
  the seven programs, the skills (less `/jev-calibrate retune`), a `package.json` with the name
  and version, `.env.example` and `LICENSE`; a skill that would not work from an installed copy
  fails the build. `--stage-only` leaves the folder and skips the tar.
- `installer-smoke-test` packages a build (`--bin-dir`) or takes an archive (`--archive`), serves
  it from a local HTTP server, and runs the real `install.sh` (and, on Windows, `install.ps1` under
  Windows PowerShell and PowerShell 7 when present, the documented `irm | iex` lines from
  cmd.exe and Git Bash, and `install.sh` under Git Bash) in a throwaway home. Each run must
  succeed, verify the checksum, install every program and leave a folder whose programs find their
  root; around that it runs dozens of cases per installer: the options and settings, every kind of
  install folder, a release going wrong (an unreachable host, a 404, an interrupted download, a bad
  or missing checksum, a short archive, a new `jev-check` that fails, with the earlier install
  surviving), Claude Code (missing, installed with consent, refused, broken, with nobody to ask),
  prerequisites offered through stand-ins for the package managers, the platform and CPU mapping,
  and on Unix the links and profile edits, on Windows the user PATH. `--installers <dir>` uses the
  `install.sh` and `install.ps1` in another folder, and `--only <groups>` runs just those groups
  (they are listed in the doc comment of `rust/xtask/src/smoke.rs`). It never touches your PATH,
  profile or home, installs no real prerequisite, never fetches Anthropic's installer, and uses a
  stand-in Claude Code. Two settings exist only for it and are left unset on a real machine:
  `JEV_CLAUDE_INSTALL_BASE` (where Anthropic's installer is fetched from, so a stub can stand in
  for it) and `JEV_TEST_USER_ENV_KEY` (a scratch registry key in place of the Windows user PATH).

## The full gate

A change to `rust/` is ready when all of these pass. CI runs the same commands on Windows, the
gating platform (SPEC 16), on Linux and on macOS, in the `rust` job of `.github/workflows/ci.yml`.
That workflow has these jobs:

- `lint`: shellcheck on `install.sh` and `.github/scripts`, and actionlint on the
  workflow and `.github/actions`;
- `rust`: the commands below, plus the launcher tests that need `node` run twice, once with it
  (required, so removing `setup-node` fails the build) and once with every `node` taken off PATH
  (they must each skip with a note), and on Windows a check that the C runtime is linked in
  (`.github/scripts/assert-static-crt.ps1`);
- `release-targets`: each published target built for its triple, checked for static linking,
  packaged with `cargo xtask package` and installed by the real installer on a machine of its own
  architecture (Intel Mac on a `macos-15-intel` runner), before any release is tagged;
- `installers`: `cargo xtask installer-smoke-test --bin-dir ../built` on each system against the
  programs the `rust` job built (locally: `--bin-dir target/release`), plus an experimental
  Windows 11 on ARM leg that installs the x64 programs through emulation, and, on Windows, the
  skills' command lines run under Windows PowerShell 5.1 and PowerShell 7
  (`.github/scripts/skill-lines.ps1`);
- `installer-alpine`: `install.sh` on Alpine in a `rust:1-alpine` container (musl, BusyBox
  `wget`, `tar` and `sha256sum`, no `curl` or `git`);
- `release`, `binaries` and `publish`, which run on `master` only after all of the above passed.

Run the commands from inside `rust/` (not with `--manifest-path`), because
`rust/.cargo/config.toml`, which makes rustdoc warnings fatal and defines `cargo xtask`, is read
only there. A `RUSTDOCFLAGS` variable in the environment overrides it. `cargo +1.85` needs
rustup's `cargo` proxy (see [Troubleshooting](#troubleshooting)).

```bash
cd rust
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked          # includes the golden cases and the conformance tests
cargo doc --workspace --no-deps --locked
cargo +1.85 check --workspace --all-targets --locked
cargo build --release --locked
cargo xtask installer-smoke-test --bin-dir target/release
```

After a push to `master` passes, the `release` job bumps the version with
`cargo xtask bump-version`, and the `binaries` job builds each platform from the release tag
with the same steps as `release-targets` (a build for the target triple such as
`x86_64-unknown-linux-musl`, the static-linking check, `cargo xtask package --out ../dist`, and
`cargo xtask installer-smoke-test --archive ../dist/<archive>`), and `publish` creates the GitHub
release with `SHA256SUMS`.

## Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| `[jev] Claude Code is not installed, or claude is not on your PATH` | Install Claude Code (https://code.claude.com/docs/en/setup), then check with `claude --version` |
| `[jev] no JEV_API_KEY found - starting Claude Code without routing` | Put `JEV_API_KEY=...` in `~/.jev-router.env`. A project `.env` works too, but only in that directory |
| The status line shows nothing or a "not found" error | `jev-statusline` is not next to `jev-claude`. Keep the seven programs together, or reinstall them with `cargo install --path rust` |
| The installer stops with `FAIL` lines | Each one has a fix listed under "To fix it"; apply them and run the installer again. Nothing was installed. A missing Claude Code is one of them: say yes when it offers to install it, or set `JEV_ASSUME_YES=1` |
| The installer will not install a prerequisite | It has no terminal to ask on. Run it from a terminal, set `JEV_ASSUME_YES=1`, or run the command it printed yourself |
| The installer says `No prebuilt programs are published` (HTTP 404) | No release has this platform's archive yet. Build from source ([Install the toolchain](#install-the-toolchain)), or pin an older release that has one with `JEV_VERSION` |
| `irm` fails with `Could not create SSL/TLS secure channel` (Windows PowerShell 5.1, or the Command Prompt line, which runs 5.1) | Windows PowerShell 5.1 can default to TLS versions that GitHub refuses, and this happens before `install.ps1` runs. Run `[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12; irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 \| iex`, or use PowerShell 7 (`pwsh`). From cmd.exe: `powershell -NoProfile -ExecutionPolicy Bypass -Command "[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12; irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 \| iex"` |
| The installer says `Claude Code ... does not run` | A `claude` was found but `claude --version` fails. On Alpine it usually needs `apk add libgcc libstdc++ ripgrep` (the installer offers it); otherwise reinstall Claude Code with the command the installer printed, then run the installer again |
| `[jev] invalid upstream URL (ANTHROPIC_BASE_URL): <value>` | `ANTHROPIC_BASE_URL` is set to something that is not a full `http://` or `https://` address (for example `localhost:4000`). Unset it, or give it a complete URL. The launcher exits before Claude Code starts |
| The installer says `The checksums differ` | The download is not what the release published (a bad mirror or a damaged transfer). Nothing was installed; run it again, and do not use the file |
| `jev-claude` is not found after the installer ran | Open a new terminal. If it still is not, check `command -v jev-claude` (Bash), `Get-Command jev-claude -All` (PowerShell) or `where jev-claude` (Command Prompt): the install's folder may not be on that shell's PATH (a terminal opened before the install has the old PATH) |
| `jev-claude` runs but behaves like the old edition, or says `Cannot find module .../bin/jev-claude.mjs` | The retired Node edition's `jev-claude` comes first on PATH. Remove it: `pnpm remove --global jev-router`, then delete the old clone `~/jev-claude` (`rm -rf ~/jev-claude`; PowerShell `Remove-Item -Recurse -Force "$HOME\jev-claude"`; Command Prompt `rmdir /s /q "%USERPROFILE%\jev-claude"`). The installer printed both |
| An installed `/jev-explain`, `/jev-legend` or `/jev-calibrate` says `command not found` | These skills run the native programs by name, so the install's `bin` folder must be on the PATH of the shell Claude Code runs commands in. Open a new terminal after the installer, or add the folder to PATH. Verified with Claude Code on Windows (Git Bash): `claude -p "/jev-legend" --add-dir <install folder>` prints the key |
| `/jev-explain`, `/jev-legend` or `/jev-calibrate` is missing | The programs are outside the clone and `JEV_ROOT` is unset, so `--add-dir` was not passed. Set `JEV_ROOT` |
| `jev-check` says `Mode installed` although you run from a clone | Same cause: set `JEV_ROOT` to the clone |
| Build fails in `ring` with `failed to find tool "cl.exe"` (Windows) or `cc` (Unix) | Install the C compiler from [Install the toolchain](#install-the-toolchain) and open a new terminal |
| `link.exe not found` | The MSVC Build Tools are missing. Install them as above, or use a "Developer PowerShell" |
| `cargo +1.85 ...` says `no such command: +1.85` | The `cargo` on `PATH` is not rustup's proxy (for example, a standalone MSI install comes first on `PATH`). Use `rustup run 1.85 cargo check --all-targets --locked`, or put `~/.cargo/bin` first on `PATH` |
| A test fails with `git` not found | Put `git` on `PATH`; the tests make real repositories. A missing `node` only skips the few npm-stand-in launcher tests, with a note |
| You want to see routing decisions | Set `JEV_DEBUG=1`, then read `~/.jev-claude.log`. In a terminal, the launcher prints the log's path at start |

## Behaviour notes

These are where the Rust code differs in mechanism from the retired Node edition but matches its
behaviour.

- **Client disconnects (SPEC 7.2 step 3, 20.10).** For `/v1/messages`, body processing runs in
  its own `tokio::spawn` task: the Jev request, the status file and the decision log. It finishes
  even when Claude Code drops the connection while Jev is being asked. hyper drops the request
  future when the connection closes, so the upstream request is never sent. A disconnect after
  forwarding has started cancels the upstream request too (SPEC 7.2 step 7). A test in
  `tests/hardening.rs` covers this.
- **Status-file I/O runs on Tokio's blocking pool.** Writing a decision, marking a manual model,
  writing the calibration file, and `JEV_DUMP` all run through `spawn_blocking` and are awaited,
  so the write still happens before the request is forwarded, as in the Node edition, where these writes
  are synchronous. A slow disk or the status lock never stalls the runtime threads that relay
  other streams. `JEV_DEBUG` log lines are still written inline, which keeps them in order.
- **Accept errors back off.** If the listener's `accept` fails for a reason other than a peer
  that already left, for example because the process has run out of file descriptors, the proxy
  waits 5 ms, doubling up to 1 s, before it retries, as Go's `net/http` does. Without the wait,
  the accept loop would spin a CPU core.
- **Signals (SPEC 10 step 8).** On Unix, SIGHUP and SIGTERM are forwarded to Claude Code as the
  same signal. The launcher exits with Claude Code's exit code (1 if a signal killed it), or with
  1 if Claude Code has not exited within 5 s. On Windows, closing the console, logging off or
  shutting down terminates Claude Code, as Node's `child.kill()` does there. Ctrl+C is left to
  Claude Code.

## Differences from the retired Node edition

Each one is either required by SPEC.md (including revision 4's changes to sections 7.1, 8.1, 13
and 14) or limited to inputs that made the Node edition itself crash. This list is the only copy;
`rust/README.md` links here.

- `jev-check` prints its `as of` time as `YYYY-MM-DD HH:MM:SS UTC` (SPEC 13), while the Node
  edition printed `toLocaleString()`. SPEC 13 excludes this text from comparison, and local
  time-zone rules would need a dependency or OS bindings that this port does not take.
- `needsInstall` (SPEC 14) is also true when a changed path starts with `rust/`, because the
  clone's programs must then be rebuilt with `cargo build --release --locked`. The Node edition
  looked only for the root `pnpm-lock.yaml`, which still counts. When the build step fails,
  `applyUpdate` reports `building the update exited with <code>`; the Node edition reported
  `installing dependencies exited with <code>`.
- The default status directory on Unix is `<temp>/jev-claude-<uid>` (SPEC 8.1), with the real user
  id, where the Node edition used `<temp>/jev-claude`: on Linux and WSL `/tmp` is shared, so one
  name let the first user's owner-only folder lock every other user out. On upgrade a Unix user
  loses the saved-model memo once, and the old folder is left alone. Windows keeps
  `<temp>\jev-claude`.
- `jev-check` finds a source checkout by `rust/Cargo.toml` beside the root's `.git` (SPEC 13), where
  the Node edition looked for `node/scripts/calibrate.mjs`.
- An npm `.cmd` shim's script runs with `node` from `PATH`, while the Node edition used its own
  executable.
  When there is no `node`, the cmd.exe route is used (SPEC 10.2).
- The child environment drops `JEV_API_KEY` and `TYPESAFE_API_KEY`. On Windows, names compare
  ASCII case-insensitively, as Windows environment names do, so `jev_api_key` is dropped too.
  This applies to inherited variables and to those added from the settings files
  (`env::apply_child_env`). The Node edition deleted them case-sensitively from a copy of
  `process.env`.
- When the Node edition called `toUpperCase()` or `includes()` on a value that is not a string, it
  threw, for
  example on a numeric `current_model` in a status file or a numeric `reason`. This port
  converts the value with `String()` or treats it as no reason.
- The process environment is read as UTF-8, with invalid sequences replaced. A lone surrogate
  inside `metadata.user_id` therefore becomes U+FFFD in the session id. The conversation key is
  the same either way, because hashing replaces it anyway (SPEC 3.3).
- `claudeModels` breaks `releasedAt` ties by UTF-16 code units rather than with `localeCompare`
  (SPEC 7.5, 19.1).
- Request headers that a client sends more than once were joined or de-duplicated by Node's rules
  before forwarding. The response framing headers (`date`, `transfer-encoding`,
  `content-length`) are hyper's, which SPEC 7.7 allows.
- Temporary files are named `<file>.<pid>.<seq>.tmp` (SPEC 3.10) because requests are served
  concurrently; the Node edition wrote `<file>.<pid>.tmp`, which was safe only because it was
  single-threaded.
- The Jev request carries exactly the headers of SPEC 5.1 (`User-Agent: jev-router-rust/<version>`).
  The Node edition's SDK also sent `accept-encoding`, `accept-language`, `sec-fetch-mode`,
  `X-TypeSafe-SDK` and `X-TypeSafe-Runtime`, with the SDK's own `User-Agent` (SPEC 19.6).

## Source layout and dependencies

| Module | SPEC | Notes |
| --- | --- | --- |
| `jsjson` | 3.3 | JSON.parse / JSON.stringify semantics: JavaScript key order; f64 numbers printed as `Number#toString` prints them; WTF-8 strings, so lone surrogates round-trip; `undefined` |
| `jsstr` | 3.1, 3.2, 3.5-3.7 | String semantics: UTF-16 lengths and slices over WTF-8; `trim` with the JS whitespace set; `Math.round`, `toFixed(2)`, `ToNumber`; lookahead emulation |
| `osdirs` | 3.8 | The home and temp directory rules |
| `repo` | 2.3, 2.4 | The repository root and the release version |
| `envx` | 9.1 | The process environment, plus the values `loadEnv` adds; names compare case-insensitively on Windows |
| `config` | 4 | The tuning (source of truth) |
| `policy`, `reasons`, `model_names`, `icons`, `legend`, `usage`, `worktree` | 7.6, 12.4, 12.3, 12.1, 13, 12.5, 12.2 | The pure parts of routing and of the status line |
| `explain`, `env`, `settings`, `status`, `log`, `update`, `launch`, `firstrun`, `router`, `proxy` | 13, 9.1, 9.2, 8, 15, 14, 10, 11, 6, 7 | The module named in that SPEC section (`firstrun` is `first-run` there) |
| `statusline` | 12 | The rendering, shared by the `jev-statusline` binary and the tests |
| `http`, `process`, `fsx`, `timefmt` | | Shared plumbing: HTTP/1.1 connections and bodies, child processes with timeouts, file modes and temp names, ISO timestamps |

The modules keep the names that the section titles of SPEC.md use, which are the names of the
original Node modules (tag `v0.11.0`).

The crate uses only the dependencies that SPEC 2.2 lists, and no JSON library, because `jsjson`
does all the parsing and writing:

| Crate | Why |
| --- | --- |
| `tokio` | The async runtime: sockets, timers, signals, the launcher's child process, and the blocking pool for status files |
| `hyper` 1.x | The HTTP/1.1 server (the proxy) and client (upstream and Jev), with no decompression and no redirect following |
| `hyper-util` | `TokioIo`, the adapter between Tokio sockets and hyper |
| `http-body-util` | Collecting and building bodies |
| `rustls`, `tokio-rustls`, `webpki-roots` | TLS to `api.anthropic.com` and `api.typesafe.ai`. `rustls` is built with the `ring` provider rather than its default `aws-lc-rs`, which needs CMake and NASM to build on Windows |
| `sha1` | `conversationKey` |
| `regex` | Every pattern Node uses, as `regex::bytes` over WTF-8 (SPEC 3.1) |
