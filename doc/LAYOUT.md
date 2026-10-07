# Repository layout

jev-claude has one behaviour and several implementations of it. Each implementation lives in its
own top-level directory and follows its language's conventions inside; everything that is not
specific to a language stays at the root and is shared.

```
jev-claude/
├─ SPEC.md               # the behaviour every implementation must match
├─ README.md  doc/       # user and developer documentation (doc/README.md is the index)
├─ package.json          # the installed launcher and the release version (see below)
├─ pnpm-workspace.yaml  pnpm-lock.yaml
├─ biome.json            # Biome lint and format policy for node/ and conformance/
├─ .github/workflows/    # ci.yml (every implementation), version-bump.yml (releases)
├─ .claude/skills/       # /jev-calibrate, /jev-explain, /jev-legend
├─ plugin/  .claude-plugin/
├─ install.sh  install.ps1  shim/
├─ conformance/          # fixtures, golden cases and the harness shared by every implementation
├─ node/                 # Node.js: the reference implementation
│  ├─ package.json       # workspace package @jev-router/node (dependencies, test script)
│  ├─ bin/  src/  test/  scripts/
├─ go/                   # Go port: go.mod, cmd/, internal/
│  ├─ .golangci.yml  staticcheck.conf    # lint policy
│  └─ bin/               # local build output (git-ignored)
├─ rust/                 # Rust port: Cargo.toml (with the [lints] policy), src/, tests/
│  ├─ rustfmt.toml  .cargo/config.toml   # 120 columns; rustdoc warnings are errors
│  └─ target/            # build output (git-ignored)
└─ python/               # Python port: pyproject.toml (ruff, mypy, the dev extra), src/jev_router/, tests/
   └─ .venv/             # the development virtual environment (git-ignored)
```

## Implementations

| | Node.js | Go | Rust | Python |
| --- | --- | --- | --- | --- |
| Status | reference; what the installers set up | complete | complete | complete |
| Guide | [NODE.md](NODE.md) | [GO.md](GO.md) | [RUST.md](RUST.md) | [PYTHON.md](PYTHON.md) |
| Toolchain | Node 22.16+ to run, Node 24.21.x to regenerate golden cases; pnpm | Go 1.23+ | Rust stable, MSRV 1.85 | CPython 3.12+ |
| Runtime dependencies | `@typesafe-ai/sdk` | standard library | hyper, tokio, rustls (`ring`), sha1, regex | standard library |
| Dev tools | Biome (root devDependency) | staticcheck 2024.1.1, golangci-lint v2.1.6 | rustfmt, clippy | ruff, mypy (the `dev` extra, in `python/.venv`) |
| Tests (as SPEC 16.4) | `pnpm test` | `cd go && go vet ./... && go test ./...` | `cd rust && cargo clippy --all-targets -- -D warnings && cargo test` | `python -m unittest discover -s python/tests -t python` |
| Lint and format | `pnpm lint && pnpm run format:check` | `gofmt -l .` (must print nothing), `staticcheck ./...`, `golangci-lint run ./...` | `cargo fmt --check`, `cargo doc --no-deps`, `cargo +1.85 check --all-targets` | `ruff format --check python`, `ruff check python`, `mypy --config-file python/pyproject.toml` |

The "Tests" row is the minimum SPEC.md 16.4 requires; the "Lint and format" row is what each
implementation adds on top, and CI runs both. Each guide has the exact commands, for Bash and
PowerShell, in its "What CI runs" section (for Rust, "The full gate").

Every port implements the seven programs in [`SPEC.md`](../SPEC.md) 1.1 (`jev-claude`,
`jev-statusline`, `jev-explain`, `jev-legend`, `jev-check`, `jev-update-check`, `jev-proxy-host`)
and passes three layers of tests: its own port of `node/test/`, every golden case in
`conformance/cases/` (generated from the Node functions), and the black-box harness in
`conformance/harness/`, which drives any implementation's proxy and status line. Each guide
gives the commands for running the harness against it and lists where it knowingly differs from
Node. The installers, the shim, the skills, and the plugin still use the Node implementation;
choosing another one at install time is not built yet. Because the skills run `node/bin/*.mjs`,
Node.js must be on `PATH` whichever launcher you use.

## Continuous integration

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) runs on every push and pull request.
Each implementation is one job, run on `windows-latest` (the gating platform, SPEC.md 16) and
`ubuntu-latest`:

| Job | Runs |
| --- | --- |
| `node` | `pnpm install --frozen-lockfile`, `pnpm lint`, `pnpm run format:check`, `pnpm test` (unit tests and the harness against Node), then `node conformance/generate.mjs` and `git diff --exit-code conformance/cases`. Node is pinned to 24.21.x in every job, because the generator runs on nothing else |
| `go` | gofmt, `go vet`, staticcheck, golangci-lint, `go test` (plus `go test -race` on Ubuntu), `go build -o bin/ ./cmd/...`, then the harness against `go/bin` |
| `rust` | `cargo fmt --check`, clippy with `-D warnings`, `cargo test`, `cargo doc`, `cargo +1.85 check` (MSRV), `cargo build --release`, then the harness against `rust/target/release` |
| `python` | a virtual environment with the `dev` extra, `ruff format --check`, `ruff check`, mypy for `win32` and `linux`, `compileall`, `unittest`, then the harness against the venv interpreter |

[`.github/workflows/version-bump.yml`](../.github/workflows/version-bump.yml) is separate: on a
push to `master` it runs `pnpm test` on Ubuntu, then bumps and tags the release. It does not wait
for `ci.yml`, so check that CI is green before relying on a release.

## Why the root keeps a package.json

Installs are git clones that the installers link onto the PATH with
`pnpm add --global link:<clone>`, and the update check reads the release version from
`package.json` at the top of the clone. The root `package.json` therefore stays the installed
package: it carries the version, the pinned pnpm, and `bin` entries that point into `node/bin/`.
It is a pnpm workspace root, so `pnpm install` at the root installs `node/`'s dependencies (and
Biome), and `pnpm test` at the root runs `node/`'s tests and then the conformance harness.

## Two roots in the Node code

Code in `node/` distinguishes:

- the **package root**, `node/`: its own `scripts/` and `src/`;
- the **repository root**, the parent of `node/`: `.git`, the release version, and the
  `.claude/skills` directory handed to Claude Code with `--add-dir`.

The ports find the repository root by walking up from their own executable or package
(SPEC.md 2.4), so build them inside the clone, or set `JEV_ROOT` for a copy installed elsewhere.

## Conformance

`conformance/` is shared by every implementation: captured fixtures, the vendored env-file
parser the ports copy, the golden cases (`node conformance/generate.mjs` regenerates them from
the Node code, deterministically; it refuses to run on any Node but 24.21.x), and the harness
(`node --test conformance/harness`, against Node by default). A change to Node's behaviour means
regenerating the cases and re-running every port's tests; CI fails if the committed cases do not
match a fresh generation. See [`conformance/README.md`](../conformance/README.md).
