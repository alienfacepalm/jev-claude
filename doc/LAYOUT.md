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

## Shared fixtures

`conformance/fixtures/` holds captured inputs (a real Claude Code request, a sub-agent hand-back
prompt) that every implementation's tests read, so the ports are tested against the same data
as the Node version. See [`conformance/README.md`](../conformance/README.md).
