# jev-claude in Rust

The Rust port of jev-claude: the launcher, the local routing proxy, the status line, and the
other programs in [`SPEC.md`](../SPEC.md) section 1.1. `SPEC.md` is normative. Where it says
nothing, the Node implementation in [`node/`](../node) is the reference.

**The full guide is [`doc/RUST.md`](../doc/RUST.md).** It covers installing the toolchain on
Windows, macOS and Linux, installing the programs and putting them on PATH, configuration,
troubleshooting, and the list of known divergences from Node.

## Quick start

You need Rust 1.85+ from [rustup](https://rustup.rs), a C compiler (the MSVC Build Tools on
Windows, Xcode's command-line tools on macOS, or `build-essential` on Linux), plus `git` and
Node.js on `PATH`.

```bash
cd rust
cargo build --release --locked
./target/release/jev-claude        # .\target\release\jev-claude.exe in PowerShell; takes claude's arguments
```

Keep the seven programs in `target/release/` together, because `jev-claude` runs
`jev-statusline` from its own directory. Put that directory on `PATH`, or run
`cargo install --path . --locked` and set `JEV_ROOT` to the clone. Your key goes in
`~/.jev-router.env`, as for the Node version. See the guide's
[install](../doc/RUST.md#install-the-programs-and-put-them-on-path) and
[configuration](../doc/RUST.md#configuration) sections.

## Check a change

Run these from this directory. They are the gate a change must pass; see the guide's
[full gate](../doc/RUST.md#the-full-gate):

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo doc --no-deps --locked
cargo +1.85 check --all-targets --locked     # MSRV
```

Then run the conformance harness against the release programs, from the repository root. These
are the Git Bash commands; on macOS and Linux, drop `.exe`. The guide gives the PowerShell form:

```bash
JEV_IMPL_CMD_PROXY=rust/target/release/jev-proxy-host.exe \
JEV_IMPL_CMD_STATUSLINE=rust/target/release/jev-statusline.exe \
node --test conformance/harness
```

The lint policy lives in `Cargo.toml` under `[lints]`. Unsafe code is denied, apart from three
FFI calls that are each justified in place; missing docs warn; and clippy's `all` and `pedantic`
groups apply, with a short list of allows whose reasons are given there. See
[Lint, format and docs](../doc/RUST.md#lint-format-and-docs).
