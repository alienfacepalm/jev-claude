# Repository layout

jev-claude has one behaviour, specified in [`SPEC.md`](../SPEC.md), and one implementation of it,
in Rust, in `rust/`. Everything that is not Rust code (the specification, the conformance data, the
installers, the skills and the plugin) stays at the root. The repository holds no Python and no
JavaScript: the developer and release tasks are a Cargo `xtask` crate, and the conformance suite is
Rust integration tests. The earlier Node.js implementation is retired and available at the git tag
`v0.11.0`.

```
jev-claude/
├─ SPEC.md               # the behaviour the implementation must match
├─ README.md  doc/       # user and developer documentation (doc/README.md is the index)
│                        #   doc/images/ holds the README's how-it-works picture (a light and a dark SVG)
├─ package.json          # the release version only (see below); no scripts, no dependencies
├─ .env.example  LICENSE  # the settings template (copied to ~/.jev-router.env by the installers)
├─ .github/workflows/    # ci.yml (every job, then the release)
├─ .github/actions/  .github/scripts/   # the release-target composite action; CI check scripts (static linking, skill lines)
├─ .claude/skills/       # /jev-calibrate, /jev-explain, /jev-legend
├─ plugin/  .claude-plugin/   # the jev-installer plugin and its marketplace entry
├─ install.sh  install.ps1    # the installers (macOS, Linux, WSL, Git Bash / Windows PowerShell)
├─ conformance/          # frozen golden cases, fixtures and the vendored env-file parser (test data)
├─ dist/                 # release archives from `cargo xtask package` (git-ignored)
└─ rust/                 # the implementation
   ├─ Cargo.toml  Cargo.lock      # package jev-router ([lints] policy) and the workspace (., xtask)
   ├─ rustfmt.toml  .cargo/config.toml   # 120 columns; rustdoc warnings are errors; the `cargo xtask` alias
   ├─ src/  src/bin/     # the library, and one file per program
   ├─ tests/             # unit, golden (golden.rs) and black-box conformance (conformance_*.rs,
   │                     #   conformance/) tests, and the release bundle check (bundle.rs)
   ├─ examples/          # calibrate.rs and live_routing.rs call the real Jev (developer tools);
   │                     #   calibration-cases.json holds the labelled prompts calibrate scores
   ├─ xtask/             # `cargo xtask bump-version | package | installer-smoke-test` (never shipped)
   └─ target/            # build output (git-ignored)
```

## The implementation

| | Rust |
| --- | --- |
| Status | complete; the only implementation, installed from a release by the installers |
| Guide | [RUST.md](RUST.md) (quick start: [rust/README.md](../rust/README.md)) |
| Toolchain | Rust stable, MSRV 1.85 |
| Runtime dependencies | hyper, tokio, rustls (`ring`), sha1, regex |
| Dev tools | rustfmt, clippy; `cargo xtask` for the release tasks |
| Tests (as SPEC 16.4) | `cd rust && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked` |
| Lint and format | `cargo fmt --all --check`, `cargo doc --workspace --no-deps --locked`, `cargo +1.85 check --workspace --all-targets --locked` |

The "Tests" row is the minimum SPEC.md 16.4 requires; the "Lint and format" row is what CI adds on
top. The exact commands are in RUST.md's [full gate](RUST.md#the-full-gate). Run them from `rust/`:
its `.cargo/config.toml` is read only there.

The implementation provides the seven programs in [`SPEC.md`](../SPEC.md) 1.1 (`jev-claude`,
`jev-statusline`, `jev-explain`, `jev-legend`, `jev-check`, `jev-update-check`, `jev-proxy-host`)
and passes three layers of tests, all run by `cargo test --workspace`: its own unit and integration
tests (whose titles come from the retired Node edition's tests), every golden case in
`conformance/cases/` (frozen fixtures that SPEC.md owns), and the black-box conformance tests in
`rust/tests/conformance_*.rs`, which drive the built proxy and status line as child processes. RUST.md
lists where it knowingly differs from the retired Node edition.

No part of the product needs Node.js. The installers install the programs from a release archive
(below), and the repository's skills run the native programs by name (`jev-check`, `jev-explain`,
`jev-legend`), which must be on `PATH` in the shell Claude Code uses. The only use of Node.js left
is optional and in the tests: a few launcher tests start an npm-style stand-in `claude` through
`node`, and skip with a note when it is absent.

## Continuous integration

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) runs on every push and pull request.
Windows is the gating platform (SPEC.md 16), so each job that builds or runs a program covers it:

| Job | Runs on | Runs |
| --- | --- | --- |
| `lint` | Ubuntu | shellcheck on `install.sh` and `.github/scripts`, and actionlint (with shellcheck on every `run:` block) on `ci.yml` and `.github/actions` |
| `rust` | Windows, Ubuntu, macOS | `cargo fmt --all --check`, `cargo clippy --workspace --all-targets` with `-D warnings`, `cargo test --workspace` (unit, integration, golden, black-box conformance and xtask tests; Node.js is set up and required, so the npm-stand-in launcher tests run rather than skip), those launcher tests again with `node` taken off `PATH` (Ubuntu and Windows; each must skip with its note), `cargo doc --workspace`, `cargo +1.85 check --workspace` (MSRV), `cargo build --release`, and on Windows a check that the C runtime is statically linked; it also uploads the seven built programs for the `installers` job |
| `release-targets` | the target's own machine: Windows x64, Ubuntu x64 and arm64, macOS Apple silicon, macOS Intel (`macos-15-intel`) | each published target built for its triple (musl on Linux), checked for static linking, packaged with `cargo xtask package` and installed by the real installer; the same steps as `binaries`, so a target that cannot build, link, package or install is found before a release is tagged |
| `installers` | Windows, Ubuntu, macOS, and Windows 11 on ARM (experimental, cannot block a release) | `cargo xtask installer-smoke-test --bin-dir ../built` (from `rust/`): packages the `rust` job's build as the release archive and installs it with the real `install.sh` (and `install.ps1`, under both Windows PowerShell and PowerShell 7, on Windows) from a local server, in a throwaway home. The Linux leg installs the glibc-linked test build; the published musl archives are installed by `release-targets`. The ARM leg installs the x64 programs through emulation. On Windows it then runs `.github/scripts/skill-lines.ps1`, which runs the skills' command lines under both PowerShells |
| `installer-alpine` | Ubuntu, in a `rust:1-alpine` container | `install.sh` on Alpine: musl libc, BusyBox `wget`, `tar` and `sha256sum` (no `curl`), and no `git` |

The `release` job `needs` `lint`, `rust`, `release-targets`, `installers` and `installer-alpine`, so a
version is only cut from a push that passed everything. It
runs on pushes to `master` (not pull requests, and not the `chore(release):` commit it makes
itself), bumps the version in the root `package.json` from every commit since the last release tag
with `cargo xtask bump-version`, commits it and tags it. A newer push supersedes a branch's run in
progress, except on `master`, where a running run is never cancelled; a `master` run that is
still waiting can be replaced by a newer one, which is harmless, because the bump counts every
commit since the last release tag, so the newer run releases those commits too. After it,
`binaries` (one job per platform) and `publish` run in the same workflow, because a push made
with the workflow's token starts no further run. `binaries` builds the five archives (musl for
Linux) from the release tag with the same steps as `release-targets`, and installs each one with
`cargo xtask installer-smoke-test --archive`; `publish` adds `SHA256SUMS` and creates
the GitHub release that the installers download. If a `binaries` leg fails after the tag exists,
**Re-run failed jobs** on that run rebuilds it and then publishes.

## Why the root keeps a package.json

The programs read the release version from the `package.json` at the top of the repository (or of
an installed folder), at run time (SPEC.md 2.3), and the release job rewrites that file with
`cargo xtask bump-version`. So the root `package.json` stays, as a version file only: `name`,
`version`, `private`, `description`, `license` and `repository`. It has no scripts and no
dependencies, nothing runs `npm` or `pnpm` on it, and it is not an installable package.

## The release archive

`cargo xtask package` (in `rust/xtask/src/package.rs`) turns a build into
`jev-claude-<platform>.tar.gz`, which unpacks to:

```
bin/                 the seven programs
.claude/skills/      the repository's three skills
package.json         name (jev-router) and release version only
.env.example  LICENSE
```

This is the shape SPEC.md 2.4 walks up to, so the programs find their root and version with no
`JEV_ROOT`. The skills are copied as they are in the repository (they already run the native
programs by name); only the `/jev-calibrate retune` section, which edits source files, is dropped,
and a skill that would not work from an installed copy fails the build. `rust/tests/bundle.rs` and
the xtask tests check the archive, and `cargo xtask installer-smoke-test` installs it with the
real `install.sh` / `install.ps1` from a local server (a wrong checksum must install nothing).
The `binaries` and `publish` jobs of `ci.yml` build the five platform archives from the release
tag, run that smoke test on each one (every runner is of its own architecture, the Intel Mac one
included), and publish them with `SHA256SUMS` as a GitHub release. The Linux archives are static
(musl), so they run on any distribution, and each is checked for static linking; the Windows ones
link the C runtime in.

## The repository root

Several behaviours need the **repository root**: the directory holding `.git`, the release version
in `package.json`, and the `.claude/skills` directory handed to Claude Code with `--add-dir`. The
programs find it by walking up from their own executable to the nearest directory containing
`.claude/skills/jev-calibrate/SKILL.md` (SPEC.md 2.4). That is a clone, or an installed release
folder. So build inside the clone, or set `JEV_ROOT` for a copy installed elsewhere.

## Conformance

`conformance/` is test data: captured fixtures, the vendored env-file parser the implementation
follows, the golden cases, and the expectations the black-box conformance tests look up. The
golden cases are frozen fixtures that SPEC.md owns: they were generated once from the Node
functions, no generator remains, and a change to the behaviour edits SPEC.md, the affected cases
and the Rust code together. The tests that use them are in `rust/tests/` (`golden.rs`,
`conformance_*.rs`); run the conformance tests alone with `cargo test --locked --test
'conformance*'` from `rust/`. See [`conformance/README.md`](../conformance/README.md).
