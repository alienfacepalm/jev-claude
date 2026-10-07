# jev-claude in Go

A port of the Node.js implementation in [`../node`](../node) to Go 1.23+, standard library
only. [`../SPEC.md`](../SPEC.md) is the specification; where it is silent, the Node source is.

**The full guide is [`../doc/GO.md`](../doc/GO.md)**: installing Go on Windows, macOS and
Linux, installing the programs side by side and on `PATH`, configuration, tests, lint, the
conformance harness, troubleshooting, and the known divergences from Node.

## Quick start

Needs Go 1.23+, git, Claude Code (`claude` on `PATH`), and Node.js 22+ with `pnpm install` run
at the repository root (the `/jev-*` skills and the harness are Node programs).

```bash
cd go
go build -o bin/ ./cmd/...          # the seven programs into go/bin/ (git-ignored)
export PATH="$PWD/bin:$PATH"         # PowerShell: $env:Path = "$PWD\bin;$env:Path"
cp ../.env.example ~/.jev-router.env # then paste your key after JEV_API_KEY=
jev-claude                           # any claude arguments pass through
```

Keep the seven programs together (`jev-claude` points the status line at the `jev-statusline`
beside it). A build outside the clone needs `JEV_ROOT=<clone>`; see
[Install the programs and put them on PATH](../doc/GO.md#install-the-programs-and-put-them-on-path).

## Check a change

From `go/` (lint tools: see [Lint and format](../doc/GO.md#lint-and-format)):

```bash
gofmt -l .               # prints nothing
go vet ./...
staticcheck ./...
golangci-lint run ./...
go test ./...            # ported Node tests and every golden case
```

Then run the conformance harness against `go/bin/` as in
[Conformance harness](../doc/GO.md#conformance-harness).

## Layout

| Path | Contents |
| --- | --- |
| `cmd/` | The seven programs: `jev-claude`, `jev-statusline`, `jev-explain`, `jev-legend`, `jev-check`, `jev-update-check`, `jev-proxy-host` |
| `internal/` | One package per Node module, plus `jsjson`, `jsstr`, `osdirs`, `repo` and `conformance` |
| `.golangci.yml`, `staticcheck.conf` | The lint policy |
