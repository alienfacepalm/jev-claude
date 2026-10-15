# jev-claude in Rust

jev-claude in Rust: the launcher, the local routing proxy, the status line, and the other
programs in [`SPEC.md`](../SPEC.md) section 1.1. This is the shipped edition. `SPEC.md` is
normative. Where it says nothing, the golden cases in [`conformance/cases`](../conformance/cases)
and this crate decide; the Node edition the spec was written from is retired (tag `v0.11.0`), and
this is the only implementation.

**The full guide is [`doc/RUST.md`](../doc/RUST.md).** It covers installing the toolchain on
Windows, macOS and Linux, installing the programs and putting them on PATH, configuration,
troubleshooting, and the [list of known divergences](../doc/RUST.md#differences-from-the-retired-node-edition) (written relative to the retired Node edition).

## Install a release (no Node.js, no Rust)

Every release carries ready-made programs, so you only need [Claude Code](https://code.claude.com/docs/en/setup)
(the installer offers to install it). The installer checks your machine, verifies the download's
SHA-256, installs to `~/jev-claude-rust`, and puts it on your PATH, the same way on every system
(macOS, Linux and WSL, Windows from PowerShell, Command Prompt or Git Bash):

```bash
curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh | bash
```

```powershell
irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex
```

From Command Prompt: `powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex"`.

[`doc/RUST.md`](../doc/RUST.md#install-a-release) lists the checks, the
settings, and how to update or remove it. The rest of this page is for building from source.

## Quick start

You need Rust 1.85+ from [rustup](https://rustup.rs) and a C compiler (the MSVC Build Tools on
Windows, Xcode's command-line tools on macOS, or `build-essential` on Linux), plus `git`. Building
from source is for contributors and for platforms without a release archive; everyone else should
use the installer above.

```bash
cd rust
cargo build --release --locked
./target/release/jev-claude        # .\target\release\jev-claude.exe in PowerShell; takes claude's arguments
```

Keep the seven programs in `target/release/` together, because `jev-claude` runs
`jev-statusline` from its own directory. Put that directory on `PATH`, or run
`cargo install --path . --locked` and set `JEV_ROOT` to the clone. Your key goes in
`~/.jev-router.env`. See the guide's
[install](../doc/RUST.md#install-the-programs-and-put-them-on-path) and
[configuration](../doc/RUST.md#configuration) sections.

## Check a change

Run these from this directory. They are the gate a change must pass; see the guide's
[full gate](../doc/RUST.md#the-full-gate):

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked              # --workspace adds the xtask tests
cargo doc --workspace --no-deps --locked
cargo +1.85 check --workspace --all-targets --locked     # MSRV
```

`cargo test` also runs the black-box conformance tests (`tests/conformance_*.rs`), which start the
built `jev-proxy-host` and `jev-statusline` against fake Jev and Anthropic servers and check them
against [`conformance/cases`](../conformance/cases). It needs `git` on `PATH`. A few launcher tests
start an npm-style stand-in `claude` through `node`, the way an npm-installed Claude Code starts;
without Node.js on `PATH` they print a note and skip, and nothing else uses Node.

Release and developer tasks are `cargo xtask` commands, run from this directory:

```bash
cargo xtask bump-version [<before>] [<after>] [--dry-run]    # writes the next version to package.json, prints it
cargo xtask package --bin-dir target/release --out ../dist   # archive + SHA256SUMS (--target <platform>)
cargo xtask installer-smoke-test --bin-dir target/release   # or --archive <file.tar.gz>; --installers <dir>, --only <groups>
```

Relative paths are taken from this directory; `package` writes to `dist/` at the repository root
by default, which git ignores. The guide's
[Release tasks](../doc/RUST.md#release-tasks-cargo-xtask) section says what each one does.

The router's tuning (tier ids, thresholds, the prose sent to Jev) lives in `src/config.rs`; it is
the source of truth, and the calibration examples in `examples/` read it.

The lint policy lives in `Cargo.toml` under `[lints]`. Unsafe code is denied, apart from three
FFI calls that are each justified in place; missing docs warn; and clippy's `all` and `pedantic`
groups apply, with a short list of allows whose reasons are given there. See
[Lint, format and docs](../doc/RUST.md#lint-format-and-docs).
