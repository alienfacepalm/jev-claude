# jev-claude documentation

jev-claude starts a local proxy and launches the real Claude Code through it, so each turn runs on
the model that gets the best result for the least cost. Claude Code's own sign-in or API key is
forwarded to Anthropic untouched.

## Using jev-claude

| Page | What it covers |
| --- | --- |
| [../README.md](../README.md) | What it is, installing, configuration, how routing works, development |
| [PLUGIN_INSTALL.md](PLUGIN_INSTALL.md) | Installing from inside Claude Code with the `jev-installer` plugin |
| [UPDATES.md](UPDATES.md) | How to update today; the planned launch-time update check and `jev-claude --update` |

## Implementations

Each guide covers installing the toolchain on Windows, macOS and Linux, building, installing the
programs, running `jev-claude`, configuration, tests, lint and format, the conformance harness,
what CI runs, troubleshooting, and known divergences from Node.

| Page | What it covers |
| --- | --- |
| [NODE.md](NODE.md) | The Node.js reference implementation in `node/` (what the installers set up); Biome |
| [GO.md](GO.md) | The Go port in `go/`; gofmt, go vet, staticcheck, golangci-lint |
| [RUST.md](RUST.md) | The Rust port in `rust/`; rustfmt, clippy, the MSRV check |
| [PYTHON.md](PYTHON.md) | The Python port in `python/`; ruff, mypy, the virtual environment |

The short quick starts in [node/](../node/README.md), [go/](../go/README.md),
[rust/](../rust/README.md) and [python/](../python/README.md) link back to these guides.

## Contributing and the shared contract

| Page | What it covers |
| --- | --- |
| [LAYOUT.md](LAYOUT.md) | Where each implementation and the shared files live, each one's gates, and CI |
| [../SPEC.md](../SPEC.md) | The normative behaviour every port must match |
| [../conformance/README.md](../conformance/README.md) | The shared fixtures, golden-case generator and black-box harness |
| [../conformance/cases/README.md](../conformance/cases/README.md) | The golden-case encoding and how to call each case file |

Configuration lives in `~/.jev-router.env`; the settings are listed in the README's
[Configuration](../README.md#configuration) section.
