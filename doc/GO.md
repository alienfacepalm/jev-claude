# The Go implementation

`go/` is a complete port of jev-claude to Go: the launcher, the local routing proxy, the status
line, and the helper programs. It follows [`SPEC.md`](../SPEC.md) and behaves the same as the
Node implementation in [`node/`](../node), which stays the reference. Every port must pass the
same golden cases and black-box harness, so you can swap one for another.

The Go build is a set of native executables that use only the standard library: no
runtime to install and no third-party modules. The installers (`install.sh`, `install.ps1`), the
shim, and the plugin still set up the Node implementation, so you install this one by hand, as
described below.

- [Programs](#programs)
- [Install the toolchain](#install-the-toolchain)
- [Build](#build)
- [Install the programs and put them on PATH](#install-the-programs-and-put-them-on-path)
- [Run jev-claude](#run-jev-claude)
- [Configuration](#configuration)
- [Tests](#tests)
- [Lint and format](#lint-and-format)
- [Conformance harness](#conformance-harness)
- [What CI runs](#what-ci-runs)
- [Troubleshooting](#troubleshooting)
- [Behaviour notes](#behaviour-notes)
- [Known divergences from Node](#known-divergences-from-node)
- [Source layout](#source-layout)

## Programs

`go build -o bin/ ./cmd/...` produces seven executables, with `.exe` added on Windows:

| Program | Source | What it does | SPEC |
| --- | --- | --- | --- |
| `jev-claude` | `cmd/jev-claude` | The launcher. It loads your settings, starts the proxy, and runs the real `claude` through it | 10 |
| `jev-statusline` | `cmd/jev-statusline` | The status line command that Claude Code runs | 12 |
| `jev-explain <session id>` | `cmd/jev-explain` | The explanation panel for `/jev-explain` | 13 |
| `jev-legend` | `cmd/jev-legend` | The status line key for `/jev-legend` | 13 |
| `jev-check` | `cmd/jev-check` | The read-only setup report for `/jev-calibrate` | 13 |
| `jev-update-check` | `cmd/jev-update-check` | The background update check. It writes `~/.jev-router/update.json` | 14 |
| `jev-proxy-host` | `cmd/jev-proxy-host` | Starts the proxy on its own, prints `PORT=<port>`, and runs until killed. The conformance harness uses it | 16.3 |

The module is `github.com/alienfacepalm/jev-claude/go` and needs Go 1.23 or newer. It carries no
version of its own: the release version lives only in the root `package.json`, which the
programs read at run time (SPEC 2.3).

## Install the toolchain

You need Go 1.23 or newer and `git`. Node.js 22+ is needed too, with `pnpm install` run at the
repository root: the conformance harness is a Node program, and the `/jev-*` skills inside
Claude Code run the Node helper programs (see [Run jev-claude](#run-jev-claude)). No C compiler
is needed.

**Windows** (PowerShell):

```powershell
winget install --id GoLang.Go
# Open a new terminal, then:
go version
```

**macOS:**

```bash
brew install go
go version
```

**Linux** (any distribution; distribution packages are often older than 1.23):

```bash
curl -fsSLO https://go.dev/dl/go1.23.12.linux-amd64.tar.gz     # or the newest release on https://go.dev/dl/
sudo rm -rf /usr/local/go && sudo tar -C /usr/local -xzf go1.23.12.linux-amd64.tar.gz
echo 'export PATH="/usr/local/go/bin:$HOME/go/bin:$PATH"' >> ~/.bashrc && . ~/.bashrc
go version
```

`go version` must print 1.23 or later. The lint tools are installed separately; see
[Lint and format](#lint-and-format).

## Build

From the repository root. These commands work the same in Git Bash, PowerShell, and on macOS
and Linux:

```bash
cd go
go build ./...               # check that everything compiles
go build -o bin/ ./cmd/...   # the seven programs into go/bin/
```

`go/bin/` is ignored by git (`go/.gitignore`). The trailing slash in `-o bin/` matters: it tells
Go to write one executable per command into that directory.

## Install the programs and put them on PATH

Two rules apply, whichever way you install:

1. **Keep the seven programs together.** `jev-claude` points Claude Code's status line at the
   `jev-statusline` program in its own directory. If you copy `jev-claude` somewhere on its own,
   the status line command points at a file that does not exist.
2. **Tell programs outside the clone where the clone is.** The programs find the repository by
   walking up from their own location to the nearest directory holding
   `.claude/skills/jev-calibrate/SKILL.md` (SPEC 2.4). Inside `go/bin/` this always works.
   Anywhere else, set `JEV_ROOT` to the clone. Without it, the launcher cannot pass
   `--add-dir <clone>`, so the `/jev-*` skills are missing outside the clone; `jev-check` reports
   `Mode installed` instead of `repository`; `jev-update-check` has no clone to check and exits
   without writing; and the release version sent to Jev in the User-Agent falls back to `0.0.0`.

### Option A: use the build directory (recommended)

Put `go/bin` on `PATH`. Rebuilding updates it in place, and no `JEV_ROOT` is needed.

Git Bash, macOS, or Linux. Add the line to `~/.bashrc` or `~/.zshrc` to keep it:

```bash
export PATH="/path/to/jev-claude/go/bin:$PATH"
```

PowerShell for the current session:

```powershell
$env:Path = "C:\path\to\jev-claude\go\bin;$env:Path"
```

PowerShell for every new terminal (your user `Path`):

```powershell
$dir = "C:\path\to\jev-claude\go\bin"
[Environment]::SetEnvironmentVariable("Path", "$dir;" + [Environment]::GetEnvironmentVariable("Path", "User"), "User")
```

### Option B: `go install`

This copies all seven programs into one directory, `$(go env GOBIN)` or, when that is empty,
`$(go env GOPATH)/bin` (usually `~/go/bin`, which the Go installers put on `PATH`):

```bash
cd go
go install ./cmd/...
```

Then set `JEV_ROOT` to the clone permanently, because these copies live outside it:

```bash
echo 'export JEV_ROOT="/path/to/jev-claude"' >> ~/.bashrc          # Git Bash / Linux; ~/.zshrc on macOS
```

```powershell
[Environment]::SetEnvironmentVariable("JEV_ROOT", "C:\path\to\jev-claude", "User")
```

Run `go install ./cmd/...` again after each `git pull` to update the copies.

### Next to the Node and Rust installs

If you also ran the installer, Node's `jev-claude` is on `PATH` as well, and whichever directory
comes first on `PATH` wins. To see which one runs, use `command -v jev-claude` (Bash) or
`Get-Command jev-claude -All` (PowerShell). To keep Node as the default and still try Go, leave
`go/bin` off `PATH` and call it by path, or define a short name:

```bash
alias jev-claude-go='/path/to/jev-claude/go/bin/jev-claude'
```

```powershell
function jev-claude-go { & "C:\path\to\jev-claude\go\bin\jev-claude.exe" @args }
```

Every implementation shares the same files: settings, the status directory, the decision log,
and the update state. Use whichever you like, one session at a time or side by side.

## Run jev-claude

Give `jev-claude` the same arguments you would give `claude`:

```bash
jev-claude                         # interactive session
jev-claude -p "what is 2+2?"       # one-shot
jev-claude --resume                # any claude flag passes through
```

What it needs:

- **The real Claude Code CLI on `PATH`** as `claude`. On Windows the launcher finds npm's
  `claude.cmd` shim and runs the script behind it with `node` from `PATH`, so arguments are
  never re-parsed by `cmd.exe`; without `node` it falls back to `cmd.exe` with full quoting.
- **A Jev API key** in `~/.jev-router.env` (see [Configuration](#configuration)). Without one,
  Claude Code starts without routing and the launcher says how to turn it on.
- **Node.js and `pnpm install` at the repository root**, for the slash commands only.
  `/jev-calibrate`, `/jev-explain` and `/jev-legend` are skills in `.claude/skills/` that run
  `node node/bin/...`, whichever implementation launched the session. The status files they read
  have the same format in every port.

On the first interactive run, the launcher offers to check your setup with `/jev-calibrate`.
Then it starts the proxy on a loopback port and points Claude Code at it with
`ANTHROPIC_BASE_URL`. It adds **Jev Router** to `/model`, installs its status line (unless you
already have one or set `JEV_NO_STATUSLINE`), waits for Claude Code to exit, and exits with
Claude Code's exit code. Like the Node launcher, it does not print an update notice or accept
`--update` (SPEC 1.2); `jev-update-check` is there for when that wiring lands.

A first argument that is a Claude Code subcommand (`jev-claude mcp list`, `plugin`, `doctor`, `update`, `auth`, and the rest of `claude --help`'s commands) is passed straight through: no proxy, no `--add-dir`, no status line, since those commands manage Claude Code and reject the extra flags.

You can also run the helper programs directly, for example `jev-explain <session id>`,
`jev-legend` or `jev-check`.

## Configuration

The Go programs read the same files and variables as Node. A variable already set in the
environment always wins over the files:

| Source | What it may set |
| --- | --- |
| The process environment | Anything |
| `~/.jev-router.env` (and the older `~/.jev-claude.env`) | Anything; this is your own settings file |
| `.env` in the directory you start `jev-claude` from | Only `JEV_API_KEY`, `TYPESAFE_API_KEY`, `JEV_DEBUG`, `JEV_ALLOW_FABLE`, `JEV_NO_STATUSLINE`, `JEV_ICONS`, and the `JEV_*_EFFORT` / `JEV_*_FORCE_EFFORT` keys, so a cloned repository can never redirect your traffic |

Start from the commented template:

```bash
cp .env.example ~/.jev-router.env                      # Git Bash, macOS, Linux
```

```powershell
Copy-Item .env.example "$HOME\.jev-router.env"
```

Then paste your key after `JEV_API_KEY=`.

| Variable | Effect |
| --- | --- |
| `JEV_API_KEY` (or `TYPESAFE_API_KEY`) | Turns routing on. It is sent only to Jev and is removed from Claude Code's environment |
| `ANTHROPIC_API_KEY` | Runs Claude Code on an API key instead of your sign-in. Set it only in your own file or shell |
| `JEV_ALLOW_FABLE` | `0`, `false`, `no` or `off` stops routing to Fable |
| `JEV_SONNET_EFFORT`, `JEV_OPUS_EFFORT`, `JEV_FABLE_EFFORT` | The effort a tier gets when Claude Code sends none (`low`, `medium`, `high`, `xhigh`, `max`) |
| `JEV_FORCE_EFFORT`, `JEV_<TIER>_FORCE_EFFORT` | An effort that replaces the one Claude Code sends; the per-tier key wins |
| `JEV_DEBUG` | Logs each routing decision to `~/.jev-claude.log` |
| `JEV_NO_STATUSLINE` | Leaves Claude Code's status line alone |
| `JEV_ICONS` | `symbols` or `text` for the status line labels; the default is symbols, except in a plain Windows console |
| `ANTHROPIC_BASE_URL` | When set before launch, the proxy forwards to it instead of `https://api.anthropic.com` |
| `ANTHROPIC_MODEL` | Starts the session on that model instead of Jev Router |
| `TYPESAFE_BASE_URL`, `TYPESAFE_DEFAULT_MODEL` | Point the router at another Jev endpoint or model |
| `JEV_ROOT` | The repository clone, for programs installed outside it |
| `JEV_STATUS_DIR` | The status directory; the default is `<temp>/jev-claude` |
| `JEV_DUMP` | `1` saves each request body into the status directory; any other value is a path prefix. This is for debugging only, because bodies hold your conversation |

The root [README's Configuration section](../README.md#configuration) explains each setting in
more depth.

## Tests

```bash
cd go
go test ./...
```

The tests need `git` on `PATH`; the launcher-shim tests also run `node` and are skipped when it
is missing. They run offline: every server they talk to is a loopback fake. `go test ./...`
runs:

- the ported Node tests, one `_test.go` per file in `node/test/`, with each Node test title kept
  as the subtest name. They use real temporary files, real git repositories (a bare repository
  stands in for GitHub in the update tests), real child processes, and loopback HTTP servers (a
  fake Jev and a fake Anthropic upstream). `internal/update` also checks that a git remote that
  accepts the connection and never answers cannot hold `jev-update-check` past its timeout;
- `internal/conformance`, which checks every file in `conformance/cases/` (SPEC 16.2), including
  `jev-request` against a loopback fake Jev and `status-line` against a freshly built
  `jev-statusline`;
- unit tests for the helper packages (`jsjson`, `jsstr`, `osdirs`, `repo`, ...).

Everything a test writes goes to temporary directories; the status store is pointed at a
throwaway directory by `TestMain`.

To run one package or one test:

```bash
go test ./internal/proxy/
go test ./internal/proxy/ -run 'TestHardeningRouting/a_client_that_leaves' -v
```

`go test -race ./...` needs cgo and a C compiler (gcc or clang), so it runs on Linux and macOS
but not on a stock Windows Go install.

## Lint and format

The lint policy is checked in: [`go/.golangci.yml`](../go/.golangci.yml) for golangci-lint and
[`go/staticcheck.conf`](../go/staticcheck.conf) for standalone staticcheck. golangci-lint runs
errcheck, govet (with nilness, unusedwrite, sortslice and stringintconv), staticcheck (which in
golangci-lint v2 includes gosimple and stylecheck), unused, ineffassign, errorlint, bodyclose,
noctx, nilerr, copyloopvar, durationcheck, makezero, wastedassign, revive (exported identifiers
must be documented, no stutter, no shadowed builtins, early returns), gocritic (diagnostic,
style and performance tags), misspell, unconvert, usestdlibvars, predeclared and nolintlint, and
the gofmt and goimports formatters.

Install the pinned tools once. golangci-lint v2.1.6 is the newest release whose module builds
with Go 1.23, and staticcheck 2024.1.1 is the release for Go 1.23; `GOTOOLCHAIN=local` stops the
`go` command from downloading a newer toolchain to build them:

```bash
GOTOOLCHAIN=local go install github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.1.6
GOTOOLCHAIN=local go install honnef.co/go/tools/cmd/staticcheck@2024.1.1
```

```powershell
$env:GOTOOLCHAIN = "local"
go install github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.1.6
go install honnef.co/go/tools/cmd/staticcheck@2024.1.1
Remove-Item Env:GOTOOLCHAIN
```

Both land in `$(go env GOPATH)/bin`. Then, from `go/`:

```bash
gofmt -l .                       # prints nothing; `gofmt -w .` fixes
go vet ./...
staticcheck ./...
golangci-lint run ./...          # also reports gofmt/goimports problems
golangci-lint fmt --diff ./...   # shows the formatting fix; `golangci-lint fmt ./...` applies it
```

Each must exit 0. Newer golangci-lint releases add checks (for example, a stricter `noctx`), so
use the pinned version for the gate.

Fix a finding rather than suppressing it. The configuration turns off three checks, each with
its reason in the file: ST1000 (package comments, covered by revive), ST1003 (initialisms, which
would rename identifiers that mirror Node's), and ST1005 (lower-case error strings: the port's
error texts reproduce Node's thrown messages, such as `Command failed: git ...`, and they reach
`update.json` and the debug log). A rare unavoidable suppression is a
`//nolint:<linter> // <reason>` on the line; nolintlint rejects one without a linter name or a
reason. Errors that cannot be acted on (`Close`, `os.Remove`, writes to a client that has gone)
are discarded with `_ =` and a comment, never silently.

## Conformance harness

The harness in `conformance/harness/` treats the proxy and the status line as black boxes.
Install the Node dependencies once (`pnpm install` at the repository root), build the programs,
then run it from the repository root.

Git Bash (Windows):

```bash
(cd go && go build -o bin/ ./cmd/...)
JEV_IMPL_CMD_PROXY=go/bin/jev-proxy-host.exe \
JEV_IMPL_CMD_STATUSLINE=go/bin/jev-statusline.exe \
node --test conformance/harness
```

macOS and Linux: the same command without `.exe`.

PowerShell:

```powershell
Push-Location go; go build -o bin/ ./cmd/...; Pop-Location
$env:JEV_IMPL_CMD_PROXY = "go/bin/jev-proxy-host.exe"
$env:JEV_IMPL_CMD_STATUSLINE = "go/bin/jev-statusline.exe"
node --test conformance/harness
Remove-Item Env:JEV_IMPL_CMD_PROXY, Env:JEV_IMPL_CMD_STATUSLINE
```

All 17 harness tests must pass. Relative paths are resolved against the repository root, and on
Windows the `.exe` suffix is required, because the harness checks that the file exists. An
absolute path works too, but in Git Bash give Node a Windows path (`cygpath -w`), not `/tmp/...`.
The golden cases need no separate step, because `go test ./...` already runs them.

## What CI runs

The same commands as above, from the repository root, on Windows (the gating platform, SPEC 16)
and on Linux. The lint tools are installed as in [Lint and format](#lint-and-format).

```bash
cd go && test -z "$(gofmt -l .)"      # Bash; in PowerShell: if (gofmt -l .) { exit 1 }
cd go && go vet ./...
cd go && staticcheck ./...
cd go && golangci-lint run ./...
cd go && go test ./...
cd go && go build -o bin/ ./cmd/...
# then the conformance harness, as in the previous section
```

Linux CI can add `cd go && go test -race ./...`.

## Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| `[jev] Claude Code is not installed, or claude is not on your PATH` | Install Claude Code (https://code.claude.com/docs/en/setup), then check with `claude --version` |
| `[jev] no JEV_API_KEY found - starting Claude Code without routing` | Put `JEV_API_KEY=...` in `~/.jev-router.env`. A project `.env` works too, but only in that directory |
| `[jev] could not start the routing proxy: invalid upstream URL` | `ANTHROPIC_BASE_URL` is set to something that is not an `http(s)://` URL. Fix or unset it |
| The status line shows nothing or a "not found" error | `jev-statusline` is not next to `jev-claude`. Keep the seven programs together: rebuild with `go build -o bin/ ./cmd/...` or `go install ./cmd/...` |
| `/jev-explain`, `/jev-legend` or `/jev-calibrate` is missing | The programs are outside the clone and `JEV_ROOT` is unset, so `--add-dir` was not passed. Set `JEV_ROOT`, or use `go/bin` |
| A `/jev-*` skill fails with a Node error | The skills run Node: install Node.js 22+ and run `pnpm install` at the repository root |
| `jev-check` says `Mode installed` although you run from a clone | Same cause: set `JEV_ROOT` to the clone |
| `go: go.mod requires go >= 1.23` | Your Go is older. Install 1.23 or newer (see [Install the toolchain](#install-the-toolchain)) |
| `go install ...golangci-lint@v2.1.6` downloads another Go | `GOTOOLCHAIN` is `auto`. Set `GOTOOLCHAIN=local` as shown in [Lint and format](#lint-and-format) |
| `golangci-lint` reports `noctx` issues the docs do not mention | You are running a newer golangci-lint than the pinned v2.1.6 |
| `golangci-lint` fails to load packages under a Go newer than 1.23 | v2.1.6 predates that toolchain. CI lints with the `go.mod` toolchain, 1.23; do the same locally, for example `GOTOOLCHAIN=go1.23.12 golangci-lint run ./...` |
| `-race requires cgo` | `go test -race` needs a C compiler; run it on Linux or macOS, or install gcc and set `CGO_ENABLED=1` |
| A test fails with `git` not found | Put git on `PATH`; the tests run it as a real process |
| The harness says the command does not exist | On Windows add `.exe`, and give either a path relative to the repository root or a Windows absolute path |
| You want to see routing decisions | Set `JEV_DEBUG=1`, then read `~/.jev-claude.log`. In a terminal, the launcher prints the log's path at start |

## Behaviour notes

These are places where the Go code differs in mechanism but matches Node's behaviour.

- **Client disconnects (SPEC 7.2 step 3, 20.10).** The proxy reads the whole request body first
  (`io.ReadAll`), so `net/http` then watches the connection and cancels the request context if
  the client closes it. The Jev call runs under its own 3 s deadline (`router.AskJev`, with
  `context.Background()`), not tied to the client, as in Node, so the conversation state, the
  status file and the decision log are still updated. After processing, `ServeHTTP` checks
  `r.Context().Err()` and returns without contacting upstream if the client has gone. Once the
  upstream request has started, it shares the client's request context, so a disconnect
  cancels it (SPEC 7.2 step 7), and a failed write mid-stream ends the handler and closes the
  upstream body. Neither the Node tests nor the harness cover the "gone while Jev is asked"
  path.
- **One snapshot of the conversation state (SPEC 3.10).** The proxy holds one mutex for the
  conversation table and every conversation's tier and model, and releases it only while Jev is
  asked. The tier, the exact model and "routed before" are read together in the same critical
  section that finds the conversation, as Node reads them with no `await` in between, so a
  second request for the same conversation can never mix one state's tier with another's model.
- **git timeouts are hard limits (SPEC 14).** The update check runs `git` with a deadline and
  `WaitDelay`, so `Run` returns at most a second after the deadline even when a helper such as
  `git-remote-https` still holds the output pipe. Node's `execFile` gets the same effect by
  destroying the pipes before it kills git.
- **Signals (SPEC 10 step 8).** On Unix, SIGHUP and SIGTERM are forwarded to Claude Code as the
  same signal. The launcher exits with Claude Code's exit code (1 if a signal killed it), or
  with 1 if Claude Code has not exited within 5 s. On Windows, closing the console, logging off
  or shutting down terminates Claude Code, as Node's `child.kill()` does there. Ctrl+C is left
  to Claude Code.

## Known divergences from Node

Each is either required by SPEC.md or a place where Go's runtime cannot do what Node does; none
shows in the golden cases or the harness. This list is the only copy; `go/README.md` links here.

- **Temporary file names** are `<file>.<pid>.<seq>.tmp` (SPEC 3.10); Node uses
  `<file>.<pid>.tmp`, which is safe only because Node is single-threaded.
- **`releasedAt` ties** in `claudeModels` are broken by UTF-16 code-unit order, not
  `localeCompare` (SPEC 7.5, 19.1); the two agree on the ISO dates the API sends. A tie between
  two models whose `created_at` is not a string fails as Node's `localeCompare` call would; Node
  may fail or not depending on which side its sort compares (SPEC 20.8).
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
- **Signals on Windows**: forwarding SIGTERM/SIGHUP (console close, logoff, shutdown) to Claude
  Code terminates it, which is also what Node's `child.kill` does there. Ctrl+C at the first-run
  prompt arrives as a console interrupt rather than a raw-mode keystroke; both settle the prompt
  as `interrupt` and exit 130.
- **Terminal detection** on Windows uses `GetConsoleMode`, as Node does; elsewhere it tests for a
  character device, so `/dev/null` as stdout counts as a terminal where Node says it is not.
- **An invalid upstream URL** (`ANTHROPIC_BASE_URL` that is not an http(s) URL) fails when the
  proxy starts; Node fails on the first proxied request instead.
- **Cutting a surrogate pair** (a 48-unit label or a 28-unit branch that ends mid-pair) drops the
  lone half, where Node keeps it (SPEC 3.2).
- **A `.cmd` shim's script** runs with `node` from `PATH`, while Node uses its own executable.
  When there is no `node`, the `cmd.exe` route is used (SPEC 10.2).

## Source layout

| Path | What it holds |
| --- | --- |
| `go/cmd/<program>/main.go` | One `main` package per program, kept thin |
| `go/internal/<module>` | One package per Node module in `node/src/`: `config`, `env`, `explain`, `firstrun`, `icons`, `jev`, `launch`, `legend`, `logx`, `modelnames`, `policy`, `proxy`, `reasons`, `router`, `settings`, `status`, `statusline`, `update`, `worktree` |
| `go/internal/jsjson` | JSON with JavaScript semantics: key order, number formatting, lone surrogates, `undefined` |
| `go/internal/jsstr` | UTF-16 lengths and slicing, JavaScript `trim` and `\s`, the WHATWG UTF-8 decoder |
| `go/internal/osdirs`, `go/internal/repo` | Node's home and temp rules; the repository root and release version |
| `go/internal/conformance` | Loads and checks the golden cases |
| `go/.golangci.yml`, `go/staticcheck.conf` | The lint policy |

Where SPEC.md is silent, the Node source is the reference; comments name the SPEC section or the
Node function each piece ports.
