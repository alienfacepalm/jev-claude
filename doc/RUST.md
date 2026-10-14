# The Rust implementation

`rust/` is a complete port of jev-claude to Rust: the launcher, the local routing proxy, the
status line, and the helper programs. It follows [`SPEC.md`](../SPEC.md) and behaves the same as
the Node implementation in [`node/`](../node), which stays the reference. Every port must pass
the same golden cases and black-box harness, so you can swap one for another.

The Rust build is a set of native executables with no runtime to install. It starts in a few
milliseconds, and the proxy runs on Tokio and hyper. Most people should not build it: every
release carries ready-made programs, and `install.sh --rust` (or `install.ps1` with
`JEV_IMPL=rust`) installs them, with no Node.js and no Rust toolchain; see
[Install a release](#install-a-release-no-toolchain-no-nodejs). Build from source to work on the
code, or on a platform that has no release archive. The shim and the Claude Code plugin still
set up the Node implementation.

- [Programs](#programs)
- [Install a release (no toolchain, no Node.js)](#install-a-release-no-toolchain-no-nodejs)
- [Install the toolchain](#install-the-toolchain)
- [Build](#build)
- [Install the programs and put them on PATH](#install-the-programs-and-put-them-on-path)
- [Run jev-claude](#run-jev-claude)
- [Configuration](#configuration)
- [Tests](#tests)
- [Lint, format and docs](#lint-format-and-docs)
- [Conformance harness](#conformance-harness)
- [The full gate](#the-full-gate)
- [Troubleshooting](#troubleshooting)
- [Behaviour notes](#behaviour-notes)
- [Known divergences from Node](#known-divergences-from-node)
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
| `jev-update-check` | The update check. It writes `~/.jev-router/update.json`. As in Node, the launcher does not start it yet (SPEC 1.2) | 14 |
| `jev-proxy-host` | Starts the proxy on its own, prints `PORT=<port>`, and runs until killed. The conformance harness uses it | 16.3 |

The crate is `jev-router`: edition 2024, minimum supported Rust 1.85 (MSRV), and version
`0.0.0`. The release version lives only in the root `package.json`, which the programs read at
run time (SPEC 2.3).

## Install a release (no toolchain, no Node.js)

The installers fetch a release archive, check it, and set it up. Pick your system:

```bash
curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh | bash -s -- --rust   # macOS, Linux, WSL, Git Bash
```

```powershell
$env:JEV_IMPL = 'rust'; irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex   # Windows
```

From a clone, run `./install.sh --rust`, or `$env:JEV_IMPL = 'rust'; .\install.ps1`.

**What it checks first.** Every check is printed with `ok`, `info`, `warn` or `FAIL`. All the
required ones run before anything is changed, and all the failures are listed together with how
to fix each:

| Check | Kind | Notes |
| --- | --- | --- |
| Platform | required | `windows-x64` (an ARM64 PC runs it through emulation), `macos-arm64`, `macos-x64`, `linux-x64`, `linux-arm64`. Anything else: [build from source](#install-the-toolchain) |
| Download tool | required | `curl` for `install.sh`; PowerShell's own web cmdlets for `install.ps1`, with TLS 1.2 forced so Windows PowerShell 5.1 works |
| `tar` | required | Windows 10 version 1803 and later ships `tar.exe`; the installer prefers `System32\tar.exe` |
| Checksum tool | required | `sha256sum`, `shasum` or `openssl`; PowerShell uses .NET |
| Network | required | any answer from the release host counts; silence is reported as a network problem |
| Claude Code | warns | not found: offers Anthropic's installer (`curl -fsSL https://claude.ai/install.sh \| bash`, or `irm https://claude.ai/install.ps1 \| iex`), asking first unless `JEV_INSTALL_CLAUDE` is `1` or `0`. Also reports whether it is the native build or the npm package, which runs on Node.js |
| git | optional | only the status line's branch and worktree names use it |
| Node.js | not used | printed for information |

**What it does.** Six numbered steps, each printed: the checks; the download (the URL is shown);
the SHA-256 check (expected and actual are both shown, and a mismatch installs nothing); the
install; the PATH; and a run of the installed `jev-check`. The archive is unpacked next to the
destination and swapped in, so a failed unpack never leaves half an install, and a running
`jev-claude` that blocks the swap is reported, with a request to close it and run the installer again. Keys are then handled as in
the Node installer, into `~/.jev-router.env`.

**Where things go.** `~/jev-claude-rust` (`JEV_INSTALL_DIR` changes it) holds:

```text
bin/                          the seven programs
.claude/skills/               /jev-calibrate, /jev-explain, /jev-legend
package.json                  the release version, nothing else (SPEC 2.3)
.env.example  LICENSE
```

This is the same shape the programs look for in a clone (SPEC 2.4): they walk up from `bin/` to
the folder holding `.claude/skills/jev-calibrate/SKILL.md`, so `--add-dir` hands Claude Code the
skills and the programs report their version, with no `JEV_ROOT`. The skills in the archive are
generated from the repository's own by `node/scripts/package-rust-release.mjs`, which swaps the
call to `node .../node/bin/<program>.mjs` for the native program (found on PATH, so the same line
runs under Bash and PowerShell), and drops `/jev-calibrate retune`, which edits the repository's
Node scripts. That is why this edition needs no Node.js.

**PATH.** `install.sh` adds one `export PATH` line, marked `# jev-claude (Rust edition)`, to
`~/.zshrc`, `~/.bashrc` (macOS: `~/.bash_profile` when that is the file you have) or
`~/.profile`, once. `install.ps1` puts the folder first on your user PATH in the registry,
keeping `%VAR%` entries unexpanded, and tells running programs. `JEV_NO_MODIFY_PATH=1` touches
neither and prints what to add. Git Bash and PowerShell have separate PATHs on Windows, so run
both installers if you use both.

**Settings.** `JEV_VERSION` (a tag such as `v0.10.0`; the default is the latest release),
`JEV_INSTALL_DIR`, `JEV_NO_MODIFY_PATH`, `JEV_INSTALL_CLAUDE`, and `JEV_RELEASE_URL` (a mirror
with the same layout: `latest/download/<archive>` and `download/<tag>/<archive>`).

**Updating and removing.** Run the installer again: it reports `updated 0.9.2 -> 0.10.0` or
`reinstalled version 0.10.0`. To remove it, delete the install folder, the marked line in your
shell profile, and the folder's entry in your user PATH. Your settings in `~/.jev-router.env` are
yours and stay.

**The archives.** Each release has `jev-claude-<platform>.tar.gz` for the five platforms above and
a `SHA256SUMS` file. The names carry no version, so `releases/latest/download/<name>` always
works. To install by hand: download the archive and `SHA256SUMS`, compare with `sha256sum -c`,
unpack anywhere, and put its `bin` on PATH. CI builds them from the release tag with the
programs linked statically (musl on Linux, the C runtime on Windows), so nothing else has to be
installed, then installs each one with the real installer before publishing.

## Install the toolchain

You need Rust 1.85 or newer, installed through [rustup](https://rustup.rs), plus a C compiler,
because the `ring` TLS backend compiles a little C. You also need `git` and Node.js 22+ on
`PATH`: the tests use both, and the `/jev-*` skills inside Claude Code run the Node helper
programs (see [Run jev-claude](#run-jev-claude)).

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

### Next to the Node install

If you also ran the installer, Node's `jev-claude` is on `PATH` as well, and whichever directory
comes first on `PATH` wins. To see which one runs, use `command -v jev-claude` (Bash) or
`Get-Command jev-claude -All` (PowerShell). To keep Node as the default and still try Rust,
leave the Rust directory off `PATH` and call it by path, or define a short name:

```bash
alias jev-claude-rs='/path/to/jev-claude/rust/target/release/jev-claude'
```

```powershell
function jev-claude-rs { & "C:\path\to\jev-claude\rust\target\release\jev-claude.exe" @args }
```

The two implementations share the same files: settings, the status directory, the decision log,
and the update state. Use whichever you like, one session at a time or side by side.

## Run jev-claude

Give `jev-claude` the same arguments you would give `claude`:

```bash
jev-claude                         # interactive session
jev-claude -p "what is 2+2?"       # one-shot
jev-claude --resume                # any claude flag passes through
```

What it needs:

- **The real Claude Code CLI on `PATH`** as `claude`. On Windows the launcher finds npm's
  `claude.cmd` shim and runs the script behind it with `node`.
- **A Jev API key** in `~/.jev-router.env` (see [Configuration](#configuration)). Without one,
  Claude Code starts without routing and the launcher says how to turn it on.
- **Node.js and `pnpm install` at the repository root, in a clone only**, for the slash commands.
  In a clone, `/jev-calibrate`, `/jev-explain` and `/jev-legend` are skills in `.claude/skills/`
  that run `node node/bin/...`, whichever implementation launched the session. The status files
  they read have the same format in every port. An [installed release](#install-a-release-no-toolchain-no-nodejs)
  has its own copies of the skills that run the native programs, so it needs no Node.js.

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
| `JEV_STATUS_DIR` | The status directory; the default is `<temp>/jev-claude` |
| `JEV_DUMP` | `1` (or `true`, `yes`) saves each request body into the status directory; any other value is a path prefix. This is for debugging only, because bodies hold your conversation |

The root [README's Configuration section](../README.md#configuration) explains each setting in
more depth. The Rust programs read the same files and variables as Node.

## Tests

```bash
cd rust
cargo test --locked
```

The tests need `git` and `node` on `PATH`. They run offline: every server they talk to is a
loopback fake. `cargo test` runs:

- one test file per Node test file in `node/test/` (all except `bump-version.test.mjs`), with
  each Node test title kept verbatim as the Rust test's doc comment: `explain`, `first_run`,
  `hardening`, `icons`, `legend`, `model_names`, `policy`, `proxy`, `proxy_routing`, `settings`,
  `statusline`, `update`, `worktree`. They use real temporary files, real git repositories, the
  real `jev-statusline` binary, real child processes, and loopback HTTP servers (a fake Jev and a
  fake Anthropic upstream). `hardening` also covers Rust-only paths: the launcher's child
  environment, run as a real child process, and a client that disconnects while Jev is being
  asked (SPEC 20.10);
- `tests/bundle.rs`, which packages the programs cargo just built with
  `node/scripts/package-rust-release.mjs` and checks the result as a user gets it: all seven
  programs, the root found from `bin/` with no `JEV_ROOT`, the release version, every skill calling
  a native program that is in the bundle, and the launcher passing the bundle as `--add-dir`;
- `tests/golden.rs`, which checks every file in `conformance/cases/` (SPEC 16.2) against this
  implementation and decodes the tagged encoding in `conformance/cases/README.md`.
  `every_case_file_is_checked` fails when a new case file appears without a test;
- the unit tests inside `src/` (JSON, string and number semantics).

Every temporary directory is removed when its test ends. The status directory is one fixed
directory per test binary, `jev-status-test-rust-<binary>` under the system temp directory, and
each run empties it and reuses it.

To run one file or one test:

```bash
cargo test --test proxy
cargo test --test hardening a_client_that_leaves
```

## Lint, format and docs

The lint policy is in `rust/Cargo.toml` under `[lints]`, so plain `cargo clippy` and
`cargo build` apply it without extra flags:

- `unsafe_code = "deny"`. The only `unsafe` is three foreign-function calls that the standard
  library does not offer: the Windows profile directory and the Unix user id (for Node's
  `os.homedir` rules), and `kill(2)` to forward a signal to Claude Code. Each sits in one small
  function with `#[allow(unsafe_code)]` and a `SAFETY:` comment, which
  `clippy::undocumented_unsafe_blocks` enforces.
- `missing_docs = "warn"`: every public item of the library is documented.
- `clippy::all` and `clippy::pedantic` as warnings, with a short list of allows, each justified
  in `Cargo.toml`. The `as` casts are allowed because the port reproduces JavaScript's number
  and UTF-16 arithmetic on purpose. `must_use_candidate` is allowed because the crate is
  internal. `missing_errors_doc` and `missing_panics_doc` are allowed because SPEC.md defines the
  errors. `too_many_lines`, `similar_names` and `many_single_char_names` are allowed because the
  code follows the Node source line by line.
- `rustfmt.toml` sets a 120-column width, and `.cargo/config.toml` turns every rustdoc warning
  into an error.

```bash
cd rust
cargo fmt --check                                   # or `cargo fmt` to fix
cargo clippy --all-targets --locked -- -D warnings  # every warning fails
cargo doc --no-deps --locked                        # fails on any rustdoc warning
cargo +1.85 check --all-targets --locked            # the MSRV still builds
```

Fix a lint rather than allowing it. If an allow is unavoidable, scope it to the item and give it
a one-line reason.

## Conformance harness

The harness in `conformance/harness/` treats the proxy and the status line as black boxes. Build
the release programs first, then run it from the repository root.

Git Bash (Windows):

```bash
(cd rust && cargo build --release --locked)
JEV_IMPL_CMD_PROXY=rust/target/release/jev-proxy-host.exe \
JEV_IMPL_CMD_STATUSLINE=rust/target/release/jev-statusline.exe \
node --test conformance/harness
```

macOS and Linux: the same command without `.exe`.

PowerShell:

```powershell
Push-Location rust; cargo build --release --locked; Pop-Location
$env:JEV_IMPL_CMD_PROXY = "rust/target/release/jev-proxy-host.exe"
$env:JEV_IMPL_CMD_STATUSLINE = "rust/target/release/jev-statusline.exe"
node --test conformance/harness
Remove-Item Env:JEV_IMPL_CMD_PROXY, Env:JEV_IMPL_CMD_STATUSLINE
```

All 17 harness tests must pass. Relative paths are resolved against the repository root, and on
Windows the `.exe` suffix is required, because the harness checks that the file exists. The
golden cases need no separate step, because `cargo test` already runs them.

## The full gate

A change to `rust/` is ready when all of these pass. CI runs the same commands on Windows, the
gating platform (SPEC 16), and on Linux, in the `rust` job of `.github/workflows/ci.yml`, followed by
`node node/scripts/installer-smoke-test.mjs --bin-dir rust/target/release`, which packages the build
and installs it with the real installers. Run them from inside
`rust/` (not with `--manifest-path`), because `rust/.cargo/config.toml`, which makes rustdoc
warnings fatal, is read only there. A `RUSTDOCFLAGS` variable in the environment overrides it.
`cargo +1.85` needs rustup's `cargo` proxy (see [Troubleshooting](#troubleshooting)).
Run from the repository root:

```bash
cd rust && cargo fmt --check
cd rust && cargo clippy --all-targets --locked -- -D warnings
cd rust && cargo test --locked
cd rust && cargo doc --no-deps --locked
cd rust && cargo +1.85 check --all-targets --locked
cd rust && cargo build --release --locked
# then the conformance harness, as in the previous section
```

## Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| `[jev] Claude Code is not installed, or claude is not on your PATH` | Install Claude Code (https://code.claude.com/docs/en/setup), then check with `claude --version` |
| `[jev] no JEV_API_KEY found - starting Claude Code without routing` | Put `JEV_API_KEY=...` in `~/.jev-router.env`. A project `.env` works too, but only in that directory |
| The status line shows nothing or a "not found" error | `jev-statusline` is not next to `jev-claude`. Keep the seven programs together, or reinstall them with `cargo install --path rust` |
| The installer stops with `FAIL` lines | Each one has a fix listed under "To fix it"; apply them and run the installer again. Nothing was installed |
| The installer says `No prebuilt programs are published` (HTTP 404) | No release has this platform's archive yet. Build from source ([Install the toolchain](#install-the-toolchain)), or pin an older release that has one with `JEV_VERSION` |
| The installer says `The checksums differ` | The download is not what the release published (a bad mirror or a damaged transfer). Nothing was installed; run it again, and do not use the file |
| `jev-claude` is not found after the installer ran | Open a new terminal. If it still is not, another `jev-claude` (the Node edition's) may come first: the installer printed both folders |
| An installed `/jev-explain`, `/jev-legend` or `/jev-calibrate` says `command not found` | These skills run the native programs by name, so the install's `bin` folder must be on the PATH of the shell Claude Code runs commands in. Open a new terminal after the installer, or add the folder to PATH. Verified with Claude Code on Windows (Git Bash): `claude -p "/jev-legend" --add-dir <install folder>` prints the key |
| `/jev-explain`, `/jev-legend` or `/jev-calibrate` is missing | The programs are outside the clone and `JEV_ROOT` is unset, so `--add-dir` was not passed. Set `JEV_ROOT` |
| A `/jev-*` skill fails with a Node error | The skills run Node: install Node.js 22+ and run `pnpm install` at the repository root |
| `jev-check` says `Mode installed` although you run from a clone | Same cause: set `JEV_ROOT` to the clone |
| Build fails in `ring` with `failed to find tool "cl.exe"` (Windows) or `cc` (Unix) | Install the C compiler from [Install the toolchain](#install-the-toolchain) and open a new terminal |
| `link.exe not found` | The MSVC Build Tools are missing. Install them as above, or use a "Developer PowerShell" |
| `cargo +1.85 ...` says `no such command: +1.85` | The `cargo` on `PATH` is not rustup's proxy (for example, a standalone MSI install comes first on `PATH`). Use `rustup run 1.85 cargo check --all-targets --locked`, or put `~/.cargo/bin` first on `PATH` |
| A test fails with `node` or `git` not found | Put both on `PATH`; the tests start them as real processes |
| You want to see routing decisions | Set `JEV_DEBUG=1`, then read `~/.jev-claude.log`. In a terminal, the launcher prints the log's path at start |

## Behaviour notes

These are where the Rust code differs in mechanism but matches Node's behaviour.

- **Client disconnects (SPEC 7.2 step 3, 20.10).** For `/v1/messages`, body processing runs in
  its own `tokio::spawn` task: the Jev request, the status file and the decision log. It finishes
  even when Claude Code drops the connection while Jev is being asked. hyper drops the request
  future when the connection closes, so the upstream request is never sent. A disconnect after
  forwarding has started cancels the upstream request too (SPEC 7.2 step 7). A test in
  `tests/hardening.rs` covers this.
- **Status-file I/O runs on Tokio's blocking pool.** Writing a decision, marking a manual model,
  writing the calibration file, and `JEV_DUMP` all run through `spawn_blocking` and are awaited,
  so the write still happens before the request is forwarded, as in Node, where these writes
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

## Known divergences from Node

Each one is either required by SPEC.md or limited to inputs that make Node itself crash. This
list is the only copy; `rust/README.md` links here.

- `jev-check` prints its `as of` time as `YYYY-MM-DD HH:MM:SS UTC`, while Node prints
  `toLocaleString()`. SPEC 13 excludes this text from comparison, and local time-zone rules would
  need a dependency or OS bindings that this port does not take.
- An npm `.cmd` shim's script runs with `node` from `PATH`, while Node uses its own executable.
  When there is no `node`, the cmd.exe route is used (SPEC 10.2).
- The child environment drops `JEV_API_KEY` and `TYPESAFE_API_KEY`. On Windows, names compare
  ASCII case-insensitively, as Windows environment names do, so `jev_api_key` is dropped too.
  This applies to inherited variables and to those added from the settings files
  (`env::apply_child_env`). Node deletes them case-sensitively from a copy of `process.env`.
- When Node calls `toUpperCase()` or `includes()` on a value that is not a string, it throws, for
  example on a numeric `current_model` in a status file or a numeric `reason`. This port
  converts the value with `String()` or treats it as no reason.
- The process environment is read as UTF-8, with invalid sequences replaced. A lone surrogate
  inside `metadata.user_id` therefore becomes U+FFFD in the session id. The conversation key is
  the same either way, because hashing replaces it anyway (SPEC 3.3).
- `claudeModels` breaks `releasedAt` ties by UTF-16 code units rather than with `localeCompare`
  (SPEC 7.5, 19.1).
- Request headers that a client sends more than once are joined or de-duplicated by Node's rules
  before forwarding. The response framing headers (`date`, `transfer-encoding`,
  `content-length`) are hyper's, which SPEC 7.7 allows.

## Source layout and dependencies

| Module | Node source | Notes |
| --- | --- | --- |
| `jsjson` | (JSON.parse / JSON.stringify) | JavaScript key order; f64 numbers printed as `Number#toString` prints them; WTF-8 strings, so lone surrogates round-trip; `undefined` (SPEC 3.3) |
| `jsstr` | (String semantics) | UTF-16 lengths and slices over WTF-8; `trim` with the JS whitespace set; `Math.round`, `toFixed(2)`, `ToNumber`; lookahead emulation (SPEC 3.1, 3.2, 3.5-3.7) |
| `osdirs` | `os.homedir`, `os.tmpdir` | Node's rules (SPEC 3.8) |
| `repo` | (file location) | The repository root and the release version (SPEC 2.3, 2.4) |
| `envx` | `process.env` | The process environment, plus the values `loadEnv` adds; names compare case-insensitively on Windows |
| `config`, `policy`, `reasons`, `model_names`, `icons`, `legend`, `usage`, `worktree`, `explain`, `env`, `settings`, `status`, `log`, `update`, `launch`, `firstrun`, `router`, `proxy` | the module of the same name in `node/src` | |
| `statusline` | `node/bin/jev-statusline.mjs` | The rendering, shared by the binary and the tests |
| `http`, `process`, `fsx`, `timefmt` | | Shared plumbing: HTTP/1.1 connections and bodies, child processes with timeouts, file modes and temp names, ISO timestamps |

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
