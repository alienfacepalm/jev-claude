# jev-claude documentation

jev-claude starts a local proxy and launches the real Claude Code through it, so each turn runs on
the model that gets the best result for the least cost. Claude Code's own sign-in or API key is
forwarded to Anthropic untouched.

## Using jev-claude

| Page | What it covers |
| --- | --- |
| [../README.md](../README.md) | What it is, installing the prebuilt programs (one command per shell: Bash, PowerShell, Command Prompt), what the installer checks, each platform, updating, removing, upgrading from the Node edition, the setup check, configuration, how routing works, development |
| [PLUGIN_INSTALL.md](PLUGIN_INSTALL.md) | Installing from inside Claude Code with the `jev-installer` plugin |
| [UPDATES.md](UPDATES.md) | How to update (run the installer again); the planned launch-time update check and `jev-claude --update` |

## The implementation

jev-claude is written in Rust, in `rust/`, and that is the only implementation. The Node.js
edition no longer exists in the tree; it is available at the git tag `v0.11.0`.

| Page | What it covers |
| --- | --- |
| [RUST.md](RUST.md) | The full guide: installing a release (what the installer checks and installs), installing the toolchain on Windows, macOS and Linux and building from source (for contributors and unsupported platforms), putting a build on PATH, tests, lint and format, the calibration examples, the conformance tests, the `cargo xtask` release tasks, the full gate CI runs, troubleshooting, and the differences from the retired Node edition |
| [../rust/README.md](../rust/README.md) | The short quick start, linking back to RUST.md |

## Contributing and the shared contract

| Page | What it covers |
| --- | --- |
| [LAYOUT.md](LAYOUT.md) | What lives where, the gates, CI and the release, and why the root keeps a `package.json` |
| [../SPEC.md](../SPEC.md) | The normative behaviour the implementation must match |
| [../conformance/README.md](../conformance/README.md) | The shared fixtures, the frozen golden cases and the black-box conformance tests |
| [../conformance/cases/README.md](../conformance/cases/README.md) | The golden-case encoding and how to call each case file |

Configuration lives in `~/.jev-router.env`; the settings are listed in the README's
[Configuration](../README.md#configuration) section.
