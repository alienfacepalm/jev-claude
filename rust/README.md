# jev-claude in Rust

The Rust port of jev-claude: the launcher, the local routing proxy, the status line, and the
other programs in [`SPEC.md`](../SPEC.md) section 1.1. `SPEC.md` is normative; where it is
silent, the Node implementation in [`node/`](../node) is the reference.

The package is `jev-router` (edition 2024, MSRV 1.85), version `0.0.0`: the release version lives
only in the root `package.json` and is read from there at run time (SPEC 2.3).

## Programs

| Binary | What it does |
| --- | --- |
| `jev-claude` | Launcher: loads settings, starts the proxy, runs the real `claude` (SPEC 10) |
| `jev-statusline` | Status line command Claude Code runs (SPEC 12) |
| `jev-explain <session id>` | Explanation panel for `/jev-explain` (SPEC 13) |
| `jev-legend` | Status line key for `/jev-legend` |
| `jev-check` | Read-only setup report for `/jev-calibrate` |
| `jev-update-check` | Background update check, writes `~/.jev-router/update.json` (SPEC 14) |
| `jev-proxy-host` | Starts the proxy alone, prints `PORT=<port>`, runs until killed (SPEC 16.3) |

The binaries find the repository root from their own location (the nearest ancestor holding
`.claude/skills/jev-calibrate/SKILL.md`), or from `JEV_ROOT` when it is set (SPEC 2.4).

## Build and test

From this directory:

```
cargo build --release
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

`cargo test` runs:

- one test file per Node test file in `node/test/` (except `bump-version.test.mjs`), each Node
  test title kept as the doc comment of its Rust test: `explain`, `first_run`, `hardening`,
  `icons`, `legend`, `model_names`, `policy`, `proxy`, `proxy_routing`, `settings`,
  `statusline`, `update`, `worktree`. They use real temp files, real git repositories, the real
  `jev-statusline` binary, and loopback HTTP servers (a fake Jev and a fake Anthropic upstream);
- `tests/golden.rs`, which checks every file in `conformance/cases` (SPEC 16.2) against this
  implementation, decoding the tagged encoding of `conformance/cases/README.md`. The status-line
  cases run the real `jev-statusline` binary, the `jev-request` cases go through a loopback fake Jev, and
  `every_case_file_is_checked` fails when a new case file appears without a test.

The tests need `git` and `node` on `PATH` (one launcher test runs an npm shim's script with
Node, as the launcher does).

## Conformance harness

Build the release binaries, then from the repository root:

```
cd rust && cargo build --release && cd ..
JEV_IMPL_CMD_PROXY=rust/target/release/jev-proxy-host.exe \
JEV_IMPL_CMD_STATUSLINE=rust/target/release/jev-statusline.exe \
node --test conformance/harness
```

On macOS and Linux drop the `.exe` suffix. In PowerShell set the two variables with
`$env:JEV_IMPL_CMD_PROXY = "rust/target/release/jev-proxy-host.exe"` and so on.

## Layout

| Module | Node source | Notes |
| --- | --- | --- |
| `jsjson` | (JSON.parse / JSON.stringify) | JavaScript key order, f64 numbers printed as `Number#toString`, WTF-8 strings so lone surrogates round-trip, `undefined` (SPEC 3.3) |
| `jsstr` | (String semantics) | UTF-16 lengths and slices over WTF-8, `trim` with the JS whitespace set, `Math.round`, `toFixed(2)`, `ToNumber`, lookahead emulation (SPEC 3.1, 3.2, 3.5-3.7) |
| `osdirs` | `os.homedir`, `os.tmpdir` | Node's rules (SPEC 3.8) |
| `repo` | (file location) | Repository root and release version (SPEC 2.3, 2.4) |
| `envx` | `process.env` | The process environment plus the values `loadEnv` adds; Windows names compare case-insensitively |
| `config`, `policy`, `reasons`, `model_names`, `icons`, `legend`, `worktree`, `explain`, `env`, `settings`, `status`, `log`, `update`, `launch`, `firstrun`, `router`, `proxy` | the module of the same name in `node/src` | |
| `statusline` | `node/bin/jev-statusline.mjs` | The rendering, shared by the binary and tests |
| `http`, `process`, `fsx`, `timefmt` | | Shared plumbing: HTTP/1.1 connections and bodies, child processes with timeouts, file modes and temp names, ISO timestamps |

## Dependencies

Only those SPEC 2.2 lists:

| Crate | Why |
| --- | --- |
| `tokio` | Async runtime, sockets, timers, signals, child process for the launcher |
| `hyper` 1.x | HTTP/1.1 server (the proxy) and client (upstream and Jev), with no decompression and no redirect following |
| `hyper-util` | `TokioIo`, the adapter between tokio sockets and hyper |
| `http-body-util` | Collecting and building bodies |
| `rustls`, `tokio-rustls`, `webpki-roots` | TLS to `api.anthropic.com` and `api.typesafe.ai`. `rustls` is built with the `ring` provider rather than its default `aws-lc-rs`, which needs CMake and NASM to build on Windows |
| `sha1` | `conversationKey` |
| `regex` | Every pattern Node uses, as `regex::bytes` over WTF-8 (SPEC 3.1) |

No JSON library is used: `jsjson` parses and writes everything.

## Divergences from Node

Each is either required by SPEC.md or limited to inputs Node itself handles by crashing.

- `jev-check` prints its `as of` time as `YYYY-MM-DD HH:MM:SS UTC`; Node prints
  `toLocaleString()`. SPEC 13 excludes this text from comparison, and local time-zone rules need
  a dependency or OS bindings this port does not take.
- An npm `.cmd` shim's script is run with `node` from `PATH` (Node uses its own executable), and
  the cmd.exe route is used when there is no `node` (SPEC 10.2).
- The child environment drops `JEV_API_KEY` and `TYPESAFE_API_KEY` with `Command::env_remove`,
  which matches names case-insensitively on Windows; Node deletes them case-sensitively from a
  copy of `process.env`.
- Signals: on Windows a console close, logoff, or shutdown terminates Claude Code, as Node's
  `child.kill()` does there. On Unix, SIGHUP and SIGTERM end Claude Code with `kill` (SIGKILL)
  rather than forwarding the same signal, since forwarding a signal needs `libc`. Unix is not
  gating (SPEC 16).
- `jev-statusline` given the JSON value `null` on stdin renders as for `{}`; Node throws reading
  `session_id` of `null` and prints nothing.
- Where Node calls `toUpperCase()` or `includes()` on a value that is not a string (a numeric
  `current_model` in a status file, a numeric `reason`) and so throws, this port converts the
  value with `String()` or treats it as no reason.
- The process environment is read as UTF-8 with invalid sequences replaced; a lone surrogate
  inside `metadata.user_id` becomes U+FFFD in the session id (the conversation key is the same,
  since hashing replaces it anyway, SPEC 3.3).
- `claudeModels` breaks `releasedAt` ties by UTF-16 code units rather than `localeCompare`
  (SPEC 7.5, 19.1).
- Request headers that a client sends more than once are joined or de-duplicated with Node's
  rules before forwarding; response framing headers (`date`, `transfer-encoding`,
  `content-length`) are hyper's (SPEC 7.7 allows them).

## Notes on the golden cases

The first `write-decision` case expects the session file to be gone after its first step. That
comes from the generator: it pins `Date.now` to 1.8e12 (January 2027) while running these steps,
and Node's first status write in a process prunes files older than a week by that clock, which
deletes the file it has just written. The Rust test reproduces it by pruning with the same clock
after the first step; see the report on SPEC issues for why the case itself should change.
