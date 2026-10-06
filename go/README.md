# jev-claude in Go

A port of the Node.js implementation in [`../node`](../node) to Go, standard library only.
[`../SPEC.md`](../SPEC.md) is the specification; where it is silent, the Node source is.

Module `github.com/alienfacepalm/jev-claude/go`, Go 1.23 or newer. The manifest carries no
version: the release version is read from the root `package.json` at run time (SPEC 2.3).

## Programs

| Program | Source | Purpose |
| --- | --- | --- |
| `jev-claude` | `cmd/jev-claude` | The launcher: starts the proxy and runs the real `claude` |
| `jev-statusline` | `cmd/jev-statusline` | The status line Claude Code runs |
| `jev-explain` | `cmd/jev-explain` | The `/jev-explain` panel for a session id |
| `jev-legend` | `cmd/jev-legend` | The status line key |
| `jev-check` | `cmd/jev-check` | The read-only setup report behind `/jev-calibrate` |
| `jev-update-check` | `cmd/jev-update-check` | The background update check |
| `jev-proxy-host` | `cmd/jev-proxy-host` | The proxy alone, printing `PORT=<port>` (used by the harness) |

The library is in `internal/`, one package per Node module plus `jsjson` (JSON with JavaScript
key order, numbers and lone surrogates), `jsstr` (UTF-16 lengths, JavaScript `trim`, the `\s`
set, the WHATWG UTF-8 decoder), `osdirs` (Node's home and temp rules), and `repo` (the
repository root and release version). `internal/conformance` loads the golden cases.

The launcher passes `--add-dir <root>` and points `--settings` at the `jev-statusline`
executable in its own directory, so install the programs side by side. The repository root is
`JEV_ROOT` when set, else the nearest ancestor of the executable holding
`.claude/skills/jev-calibrate/SKILL.md` (SPEC 2.4).

## Build

```sh
cd go
go build ./...                       # check everything compiles
go build -o ../bin/ ./cmd/...        # the seven programs into ../bin (any directory works)
```

## Test

```sh
cd go
gofmt -l .                           # prints nothing
go vet ./...
go test ./...
```

`go test ./...` runs the ported Node tests (one `_test.go` per file in `node/test`, each Node
test title kept as the subtest name), tests for the helper packages, and the golden cases:
`internal/conformance` checks every case in `conformance/cases/*.json`, including
`jev-request` against a loopback fake Jev and `status-line` against the built
`jev-statusline`. The update tests drive real git; the proxy tests use real loopback servers;
the shim test runs `node` when it is on `PATH`. Everything a test writes goes to temporary
directories.

## Conformance harness

Build the programs, then run the Node harness against them from the repository root. On
Windows:

```powershell
cd go; go build -o $env:TEMP\jev-go\ ./cmd/...; cd ..
$env:JEV_IMPL_CMD_PROXY = "$env:TEMP\jev-go\jev-proxy-host.exe"
$env:JEV_IMPL_CMD_STATUSLINE = "$env:TEMP\jev-go\jev-statusline.exe"
node --test conformance/harness
```

Elsewhere:

```sh
(cd go && go build -o /tmp/jev-go/ ./cmd/...)
JEV_IMPL_CMD_PROXY=/tmp/jev-go/jev-proxy-host JEV_IMPL_CMD_STATUSLINE=/tmp/jev-go/jev-statusline \
  node --test conformance/harness
```

## Divergences from Node

Each is either required by SPEC.md or a place where Go's runtime cannot do what Node does; none
shows in the golden cases or the harness.

- **Temporary file names** are `<file>.<pid>.<seq>.tmp` (SPEC 3.10); Node uses
  `<file>.<pid>.tmp`, which is safe only because Node is single-threaded.
- **`releasedAt` ties** in `claudeModels` are broken by UTF-16 code-unit order, not
  `localeCompare` (SPEC 7.5, 19.1); the two agree on the ISO dates the API sends.
- **Duplicate request headers** from the client are forwarded as repeated header lines. Node
  merges most duplicates into one comma-joined line and drops duplicates of a few headers
  (`host`, `authorization`, `user-agent`, and others) by its own table.
- **Jev request headers** are exactly SPEC 5.1's. Node's `fetch` also sends `accept-encoding`,
  `accept-language` and `sec-fetch-mode`, and the SDK its `X-TypeSafe-SDK`/`-Runtime` headers.
- **Outbound proxies**: neither HTTP client honours `HTTP_PROXY`/`HTTPS_PROXY`, matching Node's
  `http.request` and `fetch` defaults (Go's default transport would honour them).
- **`jev-check`'s "as of" time** is printed as `1/2/2006, 3:04:05 PM` in local time, the en-US
  shape of Node's `toLocaleString()`; other locales differ (excluded from comparison, SPEC 13).
- **`toUpperCase`** in the explanation panel applies Unicode simple upper-casing plus the
  common unconditional expansions (`ß` to `SS`, ligatures), not the whole SpecialCasing table.
- **Signals on Windows**: forwarding SIGTERM/SIGHUP (console close, logoff, shutdown) to
  Claude Code terminates it, which is also what Node's `child.kill` does there. Ctrl+C at the
  first-run prompt arrives as a console interrupt rather than a raw-mode keystroke; both settle
  the prompt as `interrupt` and exit 130.
- **Terminal detection** on Windows uses `GetConsoleMode`, as Node does; elsewhere it tests for
  a character device, so `/dev/null` as stdout counts as a terminal where Node says it is not.
- **An invalid upstream URL** (`ANTHROPIC_BASE_URL` that is not an http(s) URL) fails when the
  proxy starts; Node fails on the first proxied request instead.
- **Cutting a surrogate pair** (a 48-unit label or a 28-unit branch that ends mid-pair) drops
  the lone half, where Node keeps it (SPEC 3.2).
