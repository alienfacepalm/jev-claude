# Repository layout

jev-claude has one behaviour and several implementations of it. Each implementation lives in its
own top-level directory and follows its language's conventions inside; everything that is not
specific to a language stays at the root and is shared.

```
jev-claude/
├─ SPEC.md               # the behaviour every implementation must match
├─ README.md  doc/       # user and developer documentation
├─ package.json          # the installed launcher and the release version (see below)
├─ pnpm-workspace.yaml
├─ .claude/skills/       # /jev-calibrate, /jev-explain, /jev-legend
├─ plugin/  .claude-plugin/
├─ install.sh  install.ps1  shim/
├─ conformance/          # fixtures and cases shared by every implementation's tests
├─ node/                 # Node.js: the reference implementation
│  ├─ package.json       # workspace package @jev-router/node (dependencies, test script)
│  ├─ bin/  src/  test/  scripts/
├─ go/                   # Go port: go.mod, cmd/, internal/
├─ rust/                 # Rust port: Cargo.toml, src/, tests/
└─ python/               # Python port: pyproject.toml, src/jev_router/, tests/
```

## Implementations

| | Node.js | Go | Rust | Python |
| --- | --- | --- | --- | --- |
| Status | reference; what the installers set up | complete | complete | complete |
| Toolchain | Node 22+, pnpm | Go 1.23+ | Rust 1.85+ | CPython 3.12+ |
| Dependencies | `@typesafe-ai/sdk` | standard library | hyper, tokio, rustls (`ring`), sha1, regex | standard library |
| Unit tests | `pnpm test` | `cd go && go vet ./... && go test ./...` | `cd rust && cargo clippy --all-targets -- -D warnings && cargo test` | `python -m unittest discover -s python/tests -t python` |
| Docs | this repository's README | [`go/README.md`](../go/README.md) | [`rust/README.md`](../rust/README.md) | [`python/README.md`](../python/README.md) |

Every port implements the seven programs in [`SPEC.md`](../SPEC.md) 1.1 (`jev-claude`,
`jev-statusline`, `jev-explain`, `jev-legend`, `jev-check`, `jev-update-check`, `jev-proxy-host`)
and passes three layers of tests: its own port of `node/test/`, every golden case in
`conformance/cases/` (generated from the Node functions), and the black-box harness in
`conformance/harness/`, which drives any implementation's proxy and status line. Each port's
README gives the commands for running the harness against it and lists where it knowingly
differs from Node. The installers, the shim, the skills, and the plugin still use the Node
implementation; choosing another one at install time is not built yet.

## Why the root keeps a package.json

Installs are git clones that the installers link onto the PATH with
`pnpm add --global link:<clone>`, and the update check reads the release version from
`package.json` at the top of the clone. The root `package.json` therefore stays the installed
package: it carries the version, the pinned pnpm, and `bin` entries that point into `node/bin/`.
It is a pnpm workspace root, so `pnpm install` at the root installs `node/`'s dependencies and
`pnpm test` at the root runs `node/`'s tests.

## Two roots in the Node code

Code in `node/` distinguishes:

- the **package root**, `node/`: its own `scripts/` and `src/`;
- the **repository root**, the parent of `node/`: `.git`, the release version, and the
  `.claude/skills` directory handed to Claude Code with `--add-dir`.

## Conformance

`conformance/` is shared by every implementation: captured fixtures, the vendored env-file
parser the ports copy, the golden cases (`node conformance/generate.mjs` regenerates them from
the Node code, deterministically), and the harness (`node --test conformance/harness`, against
Node by default). A change to Node's behaviour means regenerating the cases and re-running every
port's tests. See [`conformance/README.md`](../conformance/README.md).
