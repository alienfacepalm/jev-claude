# jev-claude port specification

Status: revision 3. Sections 1-18 passed Fable's second review (its notes applied); sections 19-20
were added afterwards from building the conformance suite and the ports, and were checked in a later
two-model review (Fable and Opus), whose corrections to 19.1 and 20.8 are applied. Added since
those reviews and not reviewed by either: section 3.11 (replacing a file), the trailing working
directory in section 12, the negated-verb and question strips in 7.6, and section 10.3 (Claude
subcommands). Reference implementation: `node/` at commit `e5a09fb`, run on Node.js
24.21.0.

This document specifies how the Node.js implementation of jev-claude is ported to Go, Rust, and
Python. It is normative for the ports. Where this document is silent, the Node.js source is the
specification; where the two disagree, the disagreement is a defect in this document and must be
raised, not resolved silently by either port.

Requirement words: **must** is required for conformance, **should** is the expected choice unless
the port's notes justify another, **may** is optional.

---

## 1. What is being ported

jev-claude is a launcher for Claude Code. It starts a local HTTP proxy on `127.0.0.1`, points
Claude Code at it with `ANTHROPIC_BASE_URL`, and launches the real `claude` CLI. For each new user
turn whose request names the sentinel model `jev-router`, the proxy asks TypeSafe's Jev model
which Claude tier fits, applies a local policy to the answer, rewrites the request to that model,
and forwards it to Anthropic (or to an upstream proxy that was already configured). It publishes
each decision to per-session status files, which a status-line command, an explanation command,
and a setup check read.

### 1.1 In scope (every port must provide)

| Program | Node entry | Purpose |
| --- | --- | --- |
| `jev-claude` | `node/bin/jev-claude.mjs` | The launcher (section 10) |
| `jev-statusline` | `node/bin/jev-statusline.mjs` | Status line command Claude Code runs (section 12) |
| `jev-explain` | `node/bin/jev-explain.mjs` | Explanation panel for `/jev-explain` (section 13) |
| `jev-legend` | `node/bin/jev-legend.mjs` | Status line key for `/jev-legend` (section 13) |
| `jev-check` | `node/bin/jev-check.mjs` | Read-only setup report for `/jev-calibrate` (section 13) |
| `jev-update-check` | `node/bin/jev-update-check.mjs` | Background update check (section 14) |
| `jev-proxy-host` | `node/scripts/proxy-host.mjs` | Starts the proxy alone and prints its port; used by the conformance harness (section 16.3) |

Plus the library behaviour those programs use, sections 3-15.

### 1.2 Out of scope

- `node/scripts/calibrate.mjs`, `calibration-cases.mjs`, `live-routing.mjs`: developer tools that
  call the real Jev. They stay Node-only.
- `node/scripts/bump-version.mjs` and the release workflow: the release version stays in the root
  `package.json` (section 2.3).
- Installers, the `shim/claude` script, the Claude Code plugin, and the skills: they keep invoking
  the Node implementation. Choosing an implementation at install time is future work.
- Auto-update wiring in the launcher. `node/src/update.mjs` exists but the Node launcher does not
  yet print the update notice or accept `--update` (`doc/UPDATES.md` describes the intended
  behaviour). Ports implement the update library and `jev-update-check` (section 14) and must not
  add launcher wiring the Node version lacks.

---

## 2. Repository integration

### 2.1 Layout

Each port lives in its own top-level directory, idiomatic for its language, and writes nothing
outside it.

```
go/
  go.mod                      module github.com/alienfacepalm/jev-claude/go, go 1.23
  cmd/<program>/main.go       one per program in 1.1
  internal/<package>/         config, env, jev, router, policy, reasons, proxy, status,
                              settings, launch, firstrun, statusline, explain, legend, icons,
                              modelnames, worktree, update, logx, jsjson, jsstr, osdirs, repo
  README.md
rust/
  Cargo.toml                  package jev-router, edition 2024, rust-version 1.85
  src/lib.rs, src/<module>.rs
  src/bin/<program>.rs        one per program in 1.1
  tests/                      integration tests
  README.md
python/
  pyproject.toml              project jev-router, requires-python >= 3.12, [project.scripts]
  src/jev_router/<module>.py  one module per Node module, plus jsjson, jsstr, osdirs, repo
  src/jev_router/cli/         one module per program in 1.1, each with main()
  tests/__init__.py           puts ../src on sys.path, so tests run without installing
  tests/test_<module>.py
  README.md
```

Program names on disk are exactly the names in 1.1, with the platform's executable suffix where it
has one. The helper modules `jsjson` (3.3), `jsstr` (3.1, 3.2, 3.5), `osdirs` (3.8) and `repo`
(2.4) hold the JavaScript-compatibility code so it exists once per port.

### 2.2 Toolchains and dependencies

Keep dependencies to what the job needs; each one is a supply-chain and install cost for a tool
that holds an API key.

| Port | Toolchain | Runtime dependencies | Build | Test |
| --- | --- | --- | --- | --- |
| Go | Go 1.23+ | standard library only | `go build ./...` | `go vet ./... && go test ./...` |
| Rust | stable, MSRV 1.85 | `tokio`, `hyper` 1.x, `hyper-util`, `http-body-util`, `hyper-rustls` (or `rustls` + `tokio-rustls` + `webpki-roots`), `sha1`, `regex`; anything else needs a line of justification in `rust/README.md` | `cargo build --release` | `cargo test && cargo clippy --all-targets -- -D warnings` |
| Python | CPython 3.12+ | standard library only; tests use `unittest` | none needed | `python -m unittest discover -s python/tests -t python` |

No port may depend on a TypeSafe SDK; the Jev client is written against section 5. No port may use
`serde_json` (or any library JSON encoder) to produce bytes the proxy forwards or files it writes;
`jsjson` does that (3.3). A library parser may be used for reading only where it meets 3.3.

### 2.3 Version

The release version lives only in the root `package.json` (`"version"`). Port manifests carry the
fixed version `0.0.0` and are never bumped. Code that needs the release version (5.1, 14) reads it
from the root `package.json` at run time, or uses `0.0.0` when there is no root.

### 2.4 Repository root

Several behaviours need the repository root (the directory holding `.git`, `package.json`, and
`.claude/skills`). Node derives it from its own file location. Ports resolve it, once at start:

1. `JEV_ROOT`, when set and non-empty in the process environment (ports only; Node ignores it, so
   it is documented in the Go, Rust and Python guides as the override for programs installed
   outside the clone, and not in the root README's Configuration table or `.env.example`; it is
   never a project `.env` key);
2. otherwise the nearest ancestor that contains `.claude/skills/jev-calibrate/SKILL.md`, walking up
   from: Go `os.Executable()` and Rust `std::env::current_exe()`, each after
   `filepath.EvalSymlinks` / `canonicalize`; Python the `jev_router` package's `__file__`;
3. otherwise none. Then: the launcher omits the `--add-dir <root>` pair; `shadowsSkill` is false
   (the first-run offer and its `/jev-calibrate check` prompt still work); `jev-check` reports mode
   `installed`; `jev-update-check` exits 0 without writing anything; the Jev `User-Agent` uses
   `0.0.0`.

### 2.5 Shared state

All implementations read and write the same files with the same JSON shapes (section 8, 9.2, 11,
14), in the same directories (3.8), so a session started by one implementation can be displayed by
another's status line. Field names, types, key order, and the meaning of absent fields must match
the Node implementation.

---

## 3. JavaScript-compatibility rules

These rules apply everywhere below. Getting them wrong fails conformance in ways that are hard to
see, so each port puts them in its helper modules (2.1) and tests them directly.

### 3.1 Regular expressions

- **Character classes.** Node's patterns have no `u` flag. Their `\w` is `[A-Za-z0-9_]`, `\d` is
  `[0-9]`, `\b` is the boundary between `\w` and non-`\w` in that ASCII sense. Their `\s` is
  **not** ASCII: it is the JavaScript whitespace set
  `[\t\n\x0B\f\r \xA0\u1680\u2000-\u200A\u2028\u2029\u202F\u205F\u3000\uFEFF]` (call it `JSWS`).
  Ports must never use their engine's own `\s`; every `\s` in a ported pattern is written out as
  `JSWS`. `\w`/`\d`/`\b` are written ASCII-only: Python compiles with `re.ASCII` (safe, since `\s`
  is spelled out), Rust uses `(?-u:\b)` and explicit ASCII classes, Go's RE2 classes are already
  ASCII.
- **Case-insensitivity.** JavaScript's non-`u` `i` flag folds ASCII letters only for these
  patterns (`ſ` does not match `s`, U+212A does not match `k`). Python with `re.ASCII` behaves the
  same. Rust's `(?i)` folds those in Unicode mode, and the pattern must stay in Unicode mode for the
  `JSWS` class, so Rust scopes case-insensitivity to the literal words inside ASCII groups,
  `(?i-u:use|switch to|...)`, or uses the lowering approach below. Go's `(?i)` folds Unicode, so Go
  must not use it: it matches against a copy of the text with ASCII `A-Z` lowered (same length,
  same offsets) using lower-case patterns.
- **Lookahead.** Go's `regexp` and Rust's `regex` have none. Python may use the pattern as written.
  Go and Rust implement the three Node patterns that use it by these rules, which give the same
  result as the JavaScript engine on every input:
  - *Override patterns* (4.5), trailing `(?![-\w])`: search with the lookahead removed. If the
    character after the match is end of text or not in `[-A-Za-z0-9_]`, it matches. Otherwise
    search again starting at the failed match's **start + 1** (not its end), and repeat until a
    match passes or none remains. This is complete: the tail has no variable-length part that
    could backtrack into a shorter passing match (`NAME` alternatives and `model`/`tier` are fixed
    words), and start + 1 always falls inside the verb (`use`, `switch`, `route`), where no
    alternative can begin. The restarted search must still see the real preceding character for
    `\b`: Rust uses `Regex::find_at(text, start)`, never a search over a sub-slice; Go searches the
    whole text from the start offset with its own `\b` check against the real preceding character.
  - *`versionOf`* (7.5) and *`shortName`* (12.3), `-(\d{1,2})(?!\d)` inside an optional group:
    after `-(\d+)` (the major), look at the text that follows. If it is `-` followed by a maximal
    digit run `R` with `1 <= len(R) <= 2`, the minor is `R`; in every other case (no `-`, no
    digits, or 3+ digits such as `-20250514`) there is **no minor and the match still succeeds**.
    So `claude-opus-4-20250514` is `[4, 0]` / `Opus 4`, `claude-haiku-4-5-20251001` is
    `[4, 5]` / `Haiku 4.5`.
- **Non-greedy spans.** `/<system-reminder>[\s\S]*?<\/system-reminder>/g` and the other
  tag-stripping patterns are non-greedy and cross newlines (`[\s\S]` means any UTF-16 unit; ports
  use "any character including newline").

### 3.2 String length and truncation

JavaScript measures `length`, `slice`, `padEnd`, and `includes` offsets in UTF-16 code units.
Wherever Node measures, truncates, or pads (`agentLabel`, the status line's branch clip, the
explanation panel's rows and wrapping, the debug log's 60-unit prompt excerpt, the context-size
estimate), ports count UTF-16 code units. If a cut would split a surrogate pair, ports drop the
lone high surrogate (Node keeps it); the golden cases contain no input that would split a pair,
so this never shows in a byte comparison. `[...mark].length` in `formatLegend` counts code points.

### 3.3 JSON (`jsjson`)

Each port has one JSON module with JavaScript semantics, used for every request body the proxy
parses and forwards, the Jev request and response, and every file written.

**Parse** (`JSON.parse` semantics):

- Input bytes decode as UTF-8 with each invalid sequence replaced by U+FFFD (Node's
  `Buffer.toString()`), then parse.
- Every number becomes an IEEE-754 double (`12345678901234567890` becomes
  `12345678901234567000`; `1.0` becomes `1`; `-0` stays negative zero).
- Strings may contain lone surrogates from `\uD800`-style escapes; these parse and must survive a
  round trip as the same escape. Go and Rust therefore hold strings in a representation that can
  carry them (WTF-8 bytes, or UTF-16 units); Python `str` already can. Regex processing, trimming,
  slicing, and concatenation operate on that surrogate-carrying representation (Rust
  `regex::bytes` over WTF-8, or UTF-16; Go `regexp` over WTF-8 bytes; Python `str`), so a lone
  surrogate in a prompt or label is still a lone surrogate in the Jev request and status file.
  Only hashing and printing to a terminal convert to UTF-8, replacing each lone surrogate with
  U+FFFD, so `conversationKey` hashes a lone surrogate as `EF BF BD`.
- **Key order is JavaScript's object order**: keys that are canonical array indices (a decimal
  integer string with no leading zeros except `"0"` itself, value below 2^32 - 1) come first in
  ascending numeric order, then every other key in insertion order. So
  `{"b":1,"2":2,"a":3,"1":4,"01":5}` re-serialises as `{"1":4,"2":2,"b":1,"a":3,"01":5}`. This
  order governs stringify, every iteration over an object's keys or entries (`sanitizeSchema`,
  `merge`, `agentView`), and object spread. A repeated key keeps its first position and takes its
  last value.
- The literals `NaN`, `Infinity`, `-Infinity` are not JSON and fail to parse.
- Go: no `map[string]any`; an ordered object type implementing the order above. Rust: no
  `serde_json::Value`. Python: `json.loads` is acceptable on the already U+FFFD-decoded `str`
  (never on bytes, whose path sniffs UTF-16/32 BOMs), with `parse_int=float`, a `parse_constant`
  that **raises** (so a body Node rejects takes the error path), and an `object_pairs_hook` that
  builds the ordered object above.

**Stringify** (`JSON.stringify` semantics, compact): no whitespace; object keys in order; strings
escape `"` and `\`, `\b \f \n \r \t` by their short forms, other U+0000-U+001F as `\u00xx`
(lowercase hex), lone surrogates as `\udxxx` (lowercase hex), and nothing else (non-ASCII, `/`,
`<`, `>`, `&`, U+2028 stay literal); numbers in JavaScript `Number.prototype.toString` form:
shortest round-trip digits, fixed notation when the decimal exponent `n` (as in `d.ddd × 10^n`)
satisfies `-7 < n < 21` (`0.000001`, `123456789012345680000`), otherwise exponent notation with an
explicit sign (`1e-7`, `1e+21`, `5e-324`, `1.7976931348623157e+308`), `-0` written as `0`; `NaN`
and `±Infinity` written as `null`; `undefined` object members omitted and `undefined` array
elements written as `null`.
**Indented** form (`JSON.stringify(v, null, 2)`, used for `restoreSavedModel` and `JEV_DUMP`): two
spaces per level, `": "` after keys, `[]` and `{}` for empty containers.

### 3.4 Time

Timestamps are integer milliseconds since the Unix epoch (`Date.now()`). ISO strings are
`Date.prototype.toISOString()`: `YYYY-MM-DDTHH:mm:ss.sssZ`, UTC, three fractional digits.

### 3.5 Trimming

`String.prototype.trim()` strips exactly the `JSWS` characters from both ends. Python `str.strip()`, Go `strings.TrimSpace`, and
Rust `str::trim` strip different sets (U+001C-U+001F, U+0085, missing U+FEFF) and must not be
used where Node calls `trim()`: `newTurnPrompt`, `agentLabel`, `fableAllowed`, `effortFloor`,
`forcedEffort`, `askYesNo`, `gitBranch`, `inspectClone` output, the SDK's environment reads (5.1).
The env-file parser's own `trim_spaces` is different again (3.8 / 9.1).

### 3.6 Number conversion

Where Node compares or formats a value of unknown type, ports apply ECMAScript `ToNumber`:
numbers as is; `null` -> 0; `true`/`false` -> 1/0; strings by `StringToNumber` (trim `JSWS`; empty
-> 0; decimal literals with optional sign, fraction and exponent; `0x`/`0o`/`0b` integer literals
without sign; `Infinity`/`+Infinity`/`-Infinity`; anything else NaN); `undefined` and plain objects -> NaN;
arrays convert through their string form `Array.prototype.join(",")`, in which `null` and
`undefined` elements are empty and nested arrays join recursively, then `StringToNumber`: `[]` ->
0, `[null]` -> 0, `[0.9]` and `["0.9"]` and `[[0.7]]` -> the number, `[1,2]` -> NaN. Strings with a
sign before `0x` (`"-0x1"`) are NaN. This applies to `jev.confidence` in policy
(7.6), `Number(jev.confidence).toFixed(2)` in the debug line, and
`Math.round(context_window.used_percentage ?? 0)` in the status line.

### 3.7 Rounding and fixed-point formatting

- `Math.round(x)`: `r = floor(x)`; result `r + 1` when `x - r >= 0.5` (that subtraction is exact
  for doubles), else `r`; `-0` for `-0.5 <= x < 0` and for `-0`; NaN and infinities pass through.
  So `2.5` -> 3, `-2.5` -> -2, `0.49999999999999994` -> 0. The naive `floor(x + 0.5)` is wrong
  (it gives 1 for that last input).
  Python `round()` and Go `math.Round` differ and must not be used.
- `x.toFixed(2)`: as ECMAScript specifies: for negative `x`, `"-"` plus the result for `-x`
  (`-0` prints `0.00`); otherwise pick the integer `n` minimising `|n/100 - x|` over the exact
  value of the double, the larger `n` on a tie, and print it with exactly two decimals; NaN prints
  `NaN`. Example: `0.125` ->
  `0.13`, `0.375` -> `0.38`, `1.005` -> `1.00` (its double is below 1.005). Python `f"{x:.2f}"`
  and Go `FormatFloat(x, 'f', 2, 64)` round half-to-even and are wrong on ties.
- `Math.round(confidence * 100)` for percentages uses the `Math.round` rule above.

### 3.8 Directories (`osdirs`)

Node's rules, which all implementations must share (platform helpers such as Go `os.TempDir`,
Python `tempfile.gettempdir`, Rust `env::home_dir`/`temp_dir` differ and must not be used):

- **Home** (`os.homedir()`): Windows: `USERPROFILE` when non-empty, else the profile directory
  from the OS. Elsewhere: `HOME` when non-empty, else the passwd entry of the current user.
- **Temp** (`os.tmpdir()`): Windows: `TEMP`, else `TMP`, else `%SystemRoot%\temp`, else
  `%windir%\temp`, else the literal `undefined\temp` (Node concatenates the undefined value); then
  remove one trailing `\` unless the path is a drive root such as `C:\`.
  Elsewhere: `TMPDIR`, else `TMP`, else `TEMP`, else `/tmp`; then remove one trailing `/` when the
  path is longer than one character. Empty variables count as unset.

### 3.9 Values fixed at start

Node computes these when its modules load, which in the launcher is before `loadEnv` runs, so env
files cannot change them; ports must read them from the process environment at program start:
the status directory (`JEV_STATUS_DIR`, 8.1), `LOG_FILE` (home at start), and whether stdout is a
TTY (15). Every other setting is read when used, after `loadEnv`.

### 3.10 Concurrency

Node is single-threaded; Go, Rust, and Python ports serve requests concurrently and must keep
Node's results:

- One process-wide lock around each status-file read-modify-write (`writeDecision`, `markManual`)
  and around `writeCalibration`, so concurrent sub-agent decisions never lose an `agents` entry.
- Temporary files are named `<file>.<pid>.<seq>.tmp` with a per-process counter, so concurrent
  writers never share one.
- The proxy's `convos`, `mains`, and `catalog` sit under one lock. Hold it for `agentOf`,
  `stateFor`, and reading `state.tier`/`state.model`; release it while awaiting the router; take it
  again to write `tier`/`model` **into the same state object obtained before** (no second lookup,
  even if the entry was evicted meanwhile), exactly as Node mutates the object it holds.

### 3.11 Replacing a file

The files other processes read while Jev rewrites them (the session status files and
`calibration.json`, both through `writePrivate` (8.1), and the update state (14)) are written to a
temporary file (3.10) and renamed over the target. Windows refuses that rename for a moment while
another process, such as antivirus, the search indexer or a reader that opened the file without
delete sharing, has the target open; Node measured about a dozen refusals in 20,000 back-to-back
renames. So the rename is `renameOver(temp, file)`:

- Try the rename. On success, stop.
- If it fails with a permission or sharing error (Windows `ERROR_ACCESS_DENIED` 5,
  `ERROR_SHARING_VIOLATION` 32 or `ERROR_LOCK_VIOLATION` 33; Node's `EPERM`, `EACCES`, `EBUSY`),
  wait 20 ms and try again, for at most 10 attempts (about 180 ms of waiting).
- Any other error, or one still present on the 10th attempt: remove `temp` (it can hold prompt
  text and nothing else ever cleans it up), then return the original error. Callers swallow it as
  8.1 and 14 already say, so a write that never gets the file is dropped and leaves nothing behind.

Node retries on every platform; the ports retry on Windows only (on other platforms a permission
error is permanent, and retrying only costs 180 ms). Nothing but that delay differs.

---

## 4. Configuration (`node/src/config.mjs`)

Ports reproduce every exported value and function. Every prose string sent to Jev (the score
questions, `COMPLEXITY_SCALE`, `GUIDANCE`, `COST`, and the five instruction sentences in
`questionForModels`) is copied byte-for-byte from `node/src/config.mjs`; golden case `jev-request`
checks the assembled request.

### 4.1 Tiers

| name | id | family | thinking | effort | floor |
| --- | --- | --- | --- | --- | --- |
| haiku | `claude-haiku-4-5-20251001` | haiku | false | false | none |
| sonnet | `claude-sonnet-5-5` | sonnet | true | true | high |
| opus | `claude-opus-5-5` | opus | true | true | medium |
| fable | `claude-fable-5-1` | fable | true | true | high |

Order is cheapest first and defines `rankOf`. `EFFORTS = low, medium, high, xhigh, max`.

### 4.2 Functions

- `tierOf(model)`: the first tier whose `family` is a substring of `model`; null for non-strings.
- `idOf(name)`, `tierSpec(name)`, `rankOf(name)` (-1 when unknown), `TIER_NAMES`.
- `AUTO_MODEL = "jev-router"`; `isAuto(m)` is strict equality with that string.
- `fableAllowed(env)`: false only when `JEV_ALLOW_FABLE`, trimmed (3.5), matches
  `^(0|false|no|off)$` case-insensitively. `availableTiers(env)` is `TIER_NAMES` minus fable when
  not allowed.
- `effortFloor(name, env)`: null when the tier has no floor; else `JEV_<NAME>_EFFORT` trimmed and
  lower-cased if it is one of `EFFORTS`, else the floor. `<NAME>` is the tier name upper-cased.
- `forcedEffort(name, env)`: null when the tier takes no effort; else the first of
  `JEV_<NAME>_FORCE_EFFORT`, `JEV_FORCE_EFFORT` that, trimmed and lower-cased, is in `EFFORTS`;
  else null.
- `shouldUseExactModel(reason, chosenTier, finalTier)`: `reason` is `jev` or `jev/no-change` and
  `chosenTier === finalTier`.

### 4.3 Thresholds and constants

`minConfidence 0.6`, `uncertainDefault "sonnet"`, `downgradeMaxContextTokens 20000`,
`jevTimeoutMs 1500`, `jevDeadlineMs 3000`, `jevMaxRetries 1`, `CONTEXT_WINDOW_TOKENS 200000`,
`COMPLEXITY_MAX_SCORE 9`.

### 4.4 Questions

`QUESTIONS` holds three score questions in this order: `task_complexity`, `reasoning_required`,
`tool_complexity`, each `{ "type": "score", "instructions": <text>, "criteria":
COMPLEXITY_SCALE }`.

`questionForModels(models)` is `{ "type": "choice", "instructions": [the 5 sentences],
"criteria": { <model id>: { "model": description ?? id, "cost": COST[tier], "what", "signals",
"not_for" } } }`, keys in the order the models are given and, inside each entry, in the order
`model, cost, what, signals, not_for`.

### 4.5 Override patterns

For each tier, in tier order, a case-insensitive pattern (3.1 for classes, case, and lookahead):

```
\b(?:use|switch to|switch over to|route to)\s+(?:the\s+)?(?:claude[-\s])?(?:(?:NAME)|GENERIC\s+(?:model|tier))(?![-\w])
```

with NAME/GENERIC: haiku/fast, sonnet/balanced, opus/strong, fable/long. The spaces inside
`switch to`, `switch over to`, `route to` are literal single spaces.

---

## 5. Jev wire contract

Ports replace `@typesafe-ai/sdk` 0.6.0 with their own client for exactly what Node uses.

### 5.1 Request

- `POST {base}/v1/systemone`. `base` is `TYPESAFE_BASE_URL` trimmed (3.5), blank counting as
  unset, trailing slashes removed; default `https://api.typesafe.ai`.
- The key: `JEV_API_KEY` if defined (even empty: Node uses `??`), else `TYPESAFE_API_KEY` if
  defined (even empty, untrimmed). Both undefined is a failure (5.3). Note the launcher's gate
  (10, step 6) uses truthiness instead, so an empty `JEV_API_KEY` from the real environment with a
  real `TYPESAFE_API_KEY` routes but sends `Bearer ` with nothing; preserve that.
- Headers: `Authorization: Bearer <key>`, `Accept: application/json`,
  `Content-Type: application/json`, `User-Agent: jev-router-<go|rust|python>/<release version>`,
  and on retry *n* (1-based) `X-TypeSafe-Retry-Count: <n>`. The SDK's `X-TypeSafe-SDK` and
  `X-TypeSafe-Runtime` headers are not sent.
- Body: `jsjson.stringify` of the router request (6.1) with `"model"` appended:
  `TYPESAFE_DEFAULT_MODEL` trimmed when non-blank, else `jev-latest`.
- The key is never logged. No request or response body is ever logged.

### 5.2 Response

Expected shape (only what is used):

```json
{ "model": "jev-...", "usage": { "input_tokens": 0, "output_tokens": 0 },
  "answers": {
    "model": { "type": "choice", "choice": "<label>", "confidence": 0.0, "probabilities": { "<label>": 0.0 } },
    "task_complexity": { "type": "score", "score": 0.0, "confidence": 0.0, "legend": {}, "probabilities": {} },
    "reasoning_required": { "...": "as above" },
    "tool_complexity": { "...": "as above" } } }
```

The whole parsed response is kept verbatim and recorded in status files (8.2).

### 5.3 Failure, timeouts, retries

- **Failure** (the router logs and returns null, 6.2; an empty `models` list is not a failure but
  returns null before any request or log line, 6.1): no key; non-2xx after
  retries; connection error or timeout after retries; the 3000 ms deadline; a body that is not a
  JSON object; `answers` missing or not an object; any of `task_complexity`,
  `reasoning_required`, `tool_complexity` missing or null. Everything else is **success**, including
  a missing `answers.model` or one without `choice`/`confidence` (6.2 says what follows).
- Per attempt: 1500 ms covering connect, send, and reading the whole body.
- At most 1 retry. Retry on HTTP 408, 429, 500-599, a connection error, or an attempt timeout;
  never on other statuses.
- Delay before retry *n* (zero-based): only for an HTTP-status retry, use `retry-after-ms` if it
  is a finite number >= 0, else `retry-after` as a number of seconds (only if >= 0) or an HTTP
  date (`max(0, date - now)`); use that value only if <= 60000 ms. Otherwise, and always for
  connection/timeout retries, `round(min(150 * 2^n, 400) * (1 - random() * 0.25))`.
- The 3000 ms deadline, from the start of the call, aborts whatever is in flight including a
  backoff wait.
- Nothing from a failure propagates to the request being proxied.

### 5.4 Prewarm

When the proxy starts with the real router, it sends one fire-and-forget `HEAD` to the origin of
the Jev base URL (`TYPESAFE_BASE_URL` untrimmed, else the default), ignoring every outcome. Ports
should reuse that connection for the first real call; Python may skip the reuse.

---

## 6. Router (`node/src/router.mjs`)

### 6.1 `askJev({prompt, current, contextTokens, models})`

Returns null at once when `models` is empty. Otherwise sends:

```json
{ "state": { "request": "<prompt>",
             "session": { "current_model": "<current>", "context_tokens": 0 },
             "environment": { "available_models": ["<id>", "..."] } },
  "questions": { "task_complexity": {}, "reasoning_required": {}, "tool_complexity": {},
                 "model": "<questionForModels(models)>" } }
```

### 6.2 Result

On success: every field of `answers.model` (when it is an object), then `request` (the object
above, without `model`), `response` (5.2 verbatim), `metrics` with `taskComplexity`,
`reasoningRequired`, `toolComplexity` = `ToNumber(score) / 9` (NaN when `score` is absent, which
`stringify` writes as `null`), `contextSize = min(contextTokens / 200000, 1)`, and `ms`, the
call's wall time in integer milliseconds. When `answers.model` is missing, `choice` and
`confidence` are absent: the proxy then finds no `chosen` model, policy says `jev-unavailable`,
the decision is still recorded with `confidence: null` and `jev: {request, response}`, and the
debug line prints `p=NaN`.

On failure: `log("routing failed, keeping <current>: <message>")` (section 15; the message text is
free-form) and return null.

---

## 7. Proxy (`node/src/proxy.mjs`)

### 7.1 Server

- Listens on `127.0.0.1`, port 0; `startProxy` returns the port and a close function.
- Options: `upstreamURL` (default `https://api.anthropic.com`), `route` (default the real
  router; tests inject a fake), `calibrationFile` (default 8.3).
- `HEAD` to any path: `200`, empty body, no upstream call.
- Requests are independent; routing one never blocks another (3.10).
- HTTP/1.1 only, both towards the client and upstream.

### 7.2 Per request

1. Read the whole request body.
2. If the raw request target starts with `/v1/messages` (this includes
   `/v1/messages/count_tokens`, which is processed and can be routed and recorded like a turn),
   process the body (7.3). If processing throws, debug-log `could not process body: <message>`
   and then: when a body was parsed and is an object whose `model` is the sentinel, apply
   `state.tier ?? "sonnet"` with `state.model ?? idOf(tier)` (7.5) using the `state` local to this
   request, which is null unless `stateFor` already ran, and forward `stringify(body)` (the body as
   mutated so far, e.g. after `sanitizeSchema`); otherwise forward the original bytes. A body that
   parses to `null` throws on `.model` and forwards the original bytes; a body that parses to
   another non-object (`42`, `"x"`, `true`) is processed, not routed, and re-serialised.
3. If the client has disconnected, stop without contacting upstream.
4. Forward to `upstream origin + upstream path with one trailing "/" removed + raw request target`,
   same method, headers per 7.7.
5. `GET` whose raw target matches `^/v1/models(?:\?|$)`: buffer the response; **whatever its
   status**, iterate `.data ?? []` as JavaScript's `for...of` does (absent or `null` means none; an
   array its elements; a string its characters, none of which has a tier; any other value such as
   an object, number, or boolean throws, which is debug-logged and the calibration file is **not**
   written) and for each entry whose `id` has a tier set `catalog[id] = entry`; then write the calibration file (8.3) with
   `newer = newerThanCalibrated(catalog)` and `models = ids of newestPerTier(claudeModels(catalog))`
   (so an error body with an empty catalog records the four static ids). A body that does not
   parse is debug-logged and the calibration file is not written. Then reply with the upstream
   status and headers minus `content-length`, and the buffered body.
6. Otherwise stream the response through: status and headers as received, body forwarded
   chunk-by-chunk as it arrives, never buffered whole (server-sent events must reach the client
   live). Under `JEV_DEBUG`, log `<status> served by <model>` for the first chunk matching
   `"model"\s*:\s*"([^"]+)"`.
7. Client disconnect before the response finishes aborts the upstream request (stops generation
   and billing). An upstream error after headers were sent terminates the client connection. An
   upstream error before headers replies `502`, `content-type: application/json`, body
   `{"type":"error","error":{"message":"<message>"}}` (message free-form).

### 7.3 Body processing

1. Parse (3.3). Call `dumpBody` (8.4). If `tools` is present, call `sanitizeSchema` on each
   element's `input_schema`; a non-array `tools`, or a `null` element, throws (error path, 7.2).
2. **Not the sentinel** (user picked a model): debug `passthrough, user selected <model>`. If
   `tools` is a non-empty array: `agent = agentOf(body)`, debug
   `<key> passthrough <main|sub> <model>`, and if `newTurnPrompt(body)` is non-null,
   `markManual(sessionOf(body), body.model, agent)` (8.2).
3. **Sentinel**:
   1. `agent = agentOf(body, mains)`; `state = stateFor(agent.key, fallback)`, where `fallback`
      is `conversationKey` of the body with `metadata` removed when `sessionOf(body)` is truthy,
      else none.
   2. `current = state.tier ?? "sonnet"`; `prompt = newTurnPrompt(body)`;
      `explaining = prompt` contains `<jev-explain>`.
   3. If `prompt` is non-null and not `explaining`:
      - `models = newestPerTier(claudeModels(catalog values in insertion order) filtered to
        availableTiers())`; `available` = distinct tiers of `models`, first-seen order.
      - `currentModel = state.model ?? modelForTier(models, current)`; `modelForTier(models, t)`
        is the id of the first model of tier `t`, else `idOf(t)`.
      - `contextTokens = MathRound(utf16Length(stringify(body.messages)) / 4)` (3.2, 3.3, 3.7).
        (`messages` is necessarily an array here: `newTurnPrompt` returned a prompt, and it throws,
        taking the error path, when `messages` is a non-iterable non-array value such as an object
        or a number.)
      - `jev = await route({prompt, current: currentModel, contextTokens, models})`.
      - `chosen` = the model in `models` whose id equals `jev?.choice`, else none.
      - `decision = decide({prompt, jev: chosen ? {...jev, choice: chosen.tier} : null, current,
        available, contextTokens: state.tier ? contextTokens : 0})`.
      - `model = chosen.id` if `shouldUseExactModel(reason, chosen?.tier, tier)`, else
        `currentModel` if `tier === current`, else `modelForTier(models, tier)`.
      - `state.tier = tier; state.model = model` (3.10);
        `fresh = { prompt, model, confidence: jev?.confidence ?? null, metrics: jev?.metrics ??
        null, reason, jev: jev ? {request: jev.request, response: jev.response} : null }`.
      - Debug: `<key> <main|sub[<label>]> <jev ? "<ms>ms p=<toFixed2(ToNumber(confidence))>" :
        "no-jev"> <current> -> <tier> (<reason>) ctx~<contextTokens> | <first 60 units of prompt>`.
   4. `tier = state.tier ?? current`; `model = state.model ?? idOf(tier)`; debug
      `<key> rewrite <body.model> -> <model>`; `applyTier(body, tier, model)`.
   5. If `fresh` and not `explaining`: `effort = body.output_config?.effort ?? null`;
      `writeDecision(sessionOf(body) || agent.key, decision, agent)` with `decision` keys in this
      order: `tier, prompt, model, confidence, metrics, reason, jev, effort, at` (`at` = now).
4. `stringify(body)` is what is forwarded.

`newTurnPrompt` itself throws on a `null` content block (Node reads `b.type`), taking the error
path; a non-string `text` in a `text` block joins as `""` for null/undefined and as `String(x)`
otherwise. Ports reproduce both rather than guarding.

### 7.4 Conversation state

- `convos`: at most 50 entries `{tier, model}` keyed by conversation key, least recently used
  first. `stateFor(key, fallback)`: if `key` is absent and `fallback` is present, move the
  fallback's state object to `key`. Accessing an entry moves it to most recent. Inserting at
  capacity evicts the least recent key that is not a value in `mains`, or the least recent of all
  if every key is a main.
- `mains`: session id -> the first conversation key seen with tools in that session. Before
  inserting, when it already holds more than 50 entries, delete the oldest insertion.
- `catalog`: model id -> model object, ids with a tier only, insertion order kept (a re-set id
  keeps its first position, like a JavaScript `Map`).

### 7.5 Pure functions

Each must match Node on every golden case.

- `sanitizeSchema(node)`: in place, recursive. For each of (`exclusiveMinimum`, `minimum`),
  (`exclusiveMaximum`, `maximum`) where the exclusive key holds a boolean: if it is `true` and the
  bound is a number, set the exclusive key to the bound's value and delete the bound; otherwise
  delete the exclusive key. Then recurse into every value of the object (or element of an array),
  in key order, after these edits.
- `newTurnPrompt(body)`: null when `tools` is not a non-empty array. Take the last message whose
  `role` is not `system`; null unless it exists and its role is `user`. String content is used as
  is; array content is null when any block has `type` `tool_result`, else the `text` of the blocks
  with `type` `text`, joined with `\n`; other content null. Remove every
  `<system-reminder>...</system-reminder>` span, trim (3.5), empty is null.
- `applyTier(body, tierName, model = idOf(tierName), env)`: unknown tier returns the body
  unchanged. Set `body.model`. Tier without thinking: delete `thinking`; if
  `context_management.edits` is an array, keep entries whose `type` (or `""`) does not match
  `/thinking/i`, deleting `context_management` when none remain. Then: tier without effort and a
  truthy `output_config`: delete `output_config.effort`, deleting `output_config` when it has no
  keys left. Tier with effort: `effort = forcedEffort(tier) ?? (truthy(output_config?.effort) ?
  null : effortFloor(tier))`, where truthiness is JavaScript's (`""`, `0`, `false`, `null` are
  falsy); when non-null set `output_config = {...output_config, effort}` (existing keys keep their
  order, `effort` replaced in place or appended).
- `versionOf({id, tier})`: `[major, minor]` from
  `^(?:[\w-]+\.)?claude-<family>-(\d+)(?:-(\d{1,2})(?!\d))?` (3.1); `[0, 0]` when the tier has no
  family or nothing matches.
- `claudeModels(catalog)`: models whose `id` has a tier, mapped to
  `{id, tier, releasedAt: created_at ?? "", description}`, `description` being the truthy items
  of [`display_name`, `"released " + created_at.slice(0, 10)`, `max_input_tokens + " input
  tokens"`] joined with `"; "`; sorted, stably, by version descending, then `releasedAt`
  descending by plain UTF-16 code-unit comparison. An empty result falls back to the four tiers as
  `{id, tier: name, releasedAt: "", description: id}`.
- `newestPerTier(models)`: the first model of each tier, in first-seen order.
- `newerThanCalibrated(catalog)`: ids from `newestPerTier(claudeModels(catalog))` whose version is
  greater than the version of that tier's configured `id`.
- `sessionOf(body)`: `JSON.parse(body.metadata.user_id ?? "{}").session_id ?? ""`, "" on any
  exception. A non-string `session_id` is returned as is (a number prints into the conversation
  key as its JavaScript string form; `fileFor` then fails and the status write is swallowed).
- `conversationKey(body)`: the first 12 hex digits of SHA-1 over the UTF-8 of
  `<String(session)>|<text>` (3.3 for lone surrogates), `text` being the first message's string
  content or its `text` blocks' text concatenated with no separator.
- `agentLabel(body, max = 48)`: the first message's string content, or its `text` blocks' text
  joined with one space; remove system reminders; replace each run of `JSWS` with one space; trim;
  if longer than `max` units, the first `max - 1` units plus `…`.
- `agentOf(body, mains)`: `key = conversationKey(body)`, `session = sessionOf(body)`,
  `real` = `tools` is a non-empty array; when `session` is truthy and `real` and `mains` lacks the
  session, record it (7.4). `main = !session || mains.get(session) === key`. Returns
  `{key, label: agentLabel(body) || (main ? "main" : key), main}`.

### 7.6 Policy (`node/src/policy.mjs`)

- `ownWords(prompt)`: `String(prompt ?? "")`, then replace with one space, in this order, every
  match of `<agent-message[\s\S]*?<\/agent-message>`,
  `<system-reminder>[\s\S]*?<\/system-reminder>`, ```` ```[\s\S]*?``` ````, `` `[^`\n]*` ``,
  `"[^"\n]*"`, then these two, both case-insensitive (3.1):
  - the **negated verb**, so a tier that follows it has no verb left:
    `(?:\bnot|\bcannot|n['’]t|\bnever|\bno|\bavoid|\bwithout|\bdont)\s+(?:(?:ever|really|actually|just|simply)\s+)?(?:use|switch to|switch over to|route to)`
    (`’` is U+2019; the spaces inside `switch to` and `switch over to` are single literal spaces;
    `\s` is `JSWS`). "do not use fable", "don't ever switch to haiku" and "never use fable for
    this" no longer name a tier, and "don't use haiku, use opus" is left as "do haiku, use opus".
    `\bno` needs whitespace right after it, so "nobody use opus" and "know use opus" are not
    negated, and a word between the negation and the verb ("do not just now use opus") is not
    skipped;
  - the **question**: `[^.!?\n]*\?`, every run of characters that contains no `.`, `!`, `?` or
    line break and ends in a `?`, so "why does the planner use opus?" asks about a model rather
    than asking for one. A `.` or `!` inside the question ends the run early, so "use opus for the
    v1.2 migration?" is still an override (a pinned limitation, not a goal); a `?` inside a URL
    only drops the text before it back to the previous `.`.

  Both are plain patterns with no lookahead and no restart, and both are applied with the same
  every-match, leftmost-first, non-overlapping rules as the other replacements.
- `detectOverride(prompt)`: the tier of the first override pattern (4.5), in tier order, that
  matches `ownWords(prompt)`; else null.
- `clampToAvailable(tier, available)`: the tier if available; else the first available tier above
  it, skipping `fable` unless `tier` is `fable`; else the highest available tier below it; else
  null.
- `decide({prompt, jev, current, available, contextTokens = 0})` returns
  `{tier, reason, changed}`:
  - `settle(t, r)`: `final = clampToAvailable(t, available) ?? current`;
    `why = final === t ? r : r + "+unavailable"`;
    `reason = final === current ? why + "/no-change" : why`; `changed = final !== current`.
  - An override: `settle(override, "override")`.
  - No `jev`, or `jev.choice` not a tier name: `settle(current, "jev-unavailable")`.
  - Not `ToNumber(jev.confidence) >= 0.6` (3.6; NaN is unsure, numeric strings count):
    `settle(TIER_NAMES[max(rank(choice) - 1, rank("sonnet"), rank(current))],
    "low-confidence-default")`.
  - Choice ranks below current and `contextTokens > 20000`:
    `settle(current, "downgrade-not-worth-cache-rebuild")`.
  - Else `settle(choice, "jev")`.

### 7.7 Headers

Header names compare case-insensitively everywhere; values pass through unchanged.

- **To upstream**: the client's headers; `host` set to the upstream URL's host (with port only
  when the URL has one); `content-length`, `transfer-encoding`, `connection`, `keep-alive`
  removed; `content-length` set to the forwarded body's byte length when it is non-zero. For
  `GET /v1/models`, and for every request while `JEV_DEBUG` is set, `accept-encoding` is removed.
- Ports must not let their HTTP client add or change anything else: Go sets
  `Transport.DisableCompression = true`, `ForceAttemptHTTP2 = false`, a `CheckRedirect` that
  returns `http.ErrUseLastResponse`, and sends `User-Agent` only when the client sent one (setting
  it to `""` otherwise suppresses Go's default); Python uses `http.client` with
  `skip_host=True, skip_accept_encoding=True` and sets `host` itself; Rust uses hyper's HTTP/1.1
  client without decompression layers. Redirects are relayed, never followed. Response bodies are
  relayed byte for byte, never decompressed.
- **To the client**: upstream status and headers as received (minus `content-length` for the
  models reply). A port's HTTP server may add `date`, `connection`, `transfer-encoding`,
  `content-length` framing headers; nothing else.
- The harness (16.3) compares, upstream side: method, path and query, body bytes, `host`,
  `content-length`, and every client-sent header except `connection`, `keep-alive`,
  `transfer-encoding`, `accept-encoding`; client side: status, body bytes, and the headers
  `content-type`, `content-encoding`, `request-id`, and any `anthropic-*` or `x-*` header. It
  never compares `date`, `connection`, `transfer-encoding`, `content-length` on the client side,
  or `server`.

---

## 8. Status store (`node/src/status.mjs`)

### 8.1 Directory and files

- Directory: `JEV_STATUS_DIR` from the environment at start (3.9) when non-empty, else
  `<temp>/jev-claude` (3.8). `ensureDir` creates it (mode 0700, recursive) and chmods it to 0700;
  its errors propagate to the caller, which swallows them. Modes are no-ops on Windows.
- Session file: `<dir>/<sessionId with every character outside [A-Za-z0-9_-] removed>.json`.
- `writePrivate(file, text)`: `ensureDir`, write a unique temp file (3.10) with mode 0600, rename
  over `file` (`renameOver`, 3.11), chmod 0600.
- `writeStatus(id, status)`: **does nothing when `id` is empty** (falsy). Otherwise
  `writePrivate`, then, once per process after the first successful write, `pruneStale`. Errors
  are swallowed. `writeDecision` and `markManual` call `readStatus(id)` first and end in
  `writeStatus`, so an empty id writes nothing.
- `pruneStale(maxAge = 7 days)`: delete every `*.json` in the directory except `settings.json`
  whose mtime is older than `maxAge` (this includes old session files, `calibration.json`,
  `saved-model.json`, `dump.*.json`); errors are ignored.
- `settings.json` is the launcher's `--settings` file (10.1).

### 8.2 Session status

Shape:

```json
{ "tier": "opus", "prompt": "...", "model": "claude-opus-5-5", "confidence": 0.87,
  "metrics": { "taskComplexity": 0.5, "reasoningRequired": 0.4, "toolComplexity": 0.2, "contextSize": 0.01 },
  "reason": "jev", "jev": { "request": {}, "response": {} }, "effort": "high", "at": 1760000000000,
  "agents": { "<key>": { "label": "...", "main": true, "tier": "opus", "model": "...", "confidence": 0.87,
                         "effort": "high", "reason": "jev", "at": 1760000000000 } },
  "history": [ { "tier": "...", "...": "...", "agent": { "key": "...", "label": "...", "main": true } } ],
  "manual": false }
```

- `writeDecision(id, decision, agent)`: `entry` = `{...decision, agent: {key, label, main}}` when
  `agent` is given, else `decision`. `history` = previous `history` (or `[]`) plus `entry`, last 20
  kept. `agents` = `merge(previous.agents, agent, {label, main, tier, model, confidence, effort,
  reason, at: decision.at ?? now})` when `agent` is given, else previous `agents`. Writes
  `{...decision, agents (only when defined), history}`.
- `markManual(id, model, agent)`: `agents` = `merge(previous.agents, agent, {label, main, model,
  manual: true, at: now})` when `agent` is given, else previous `agents`; `manual` = `true` when
  no agent or the agent is main, else `previous.manual ?? false`. Writes
  `{...previous, agents (only when defined), manual, at: now}`.
- `merge(existing, agent, entry)`: `agents[agent.key] = {...agents[agent.key], ...entry}` (so a
  `manual: true` set earlier survives a later routed merge, which does not mention `manual`). When
  more than 12 keys result, keep every `main: true` entry and the 11 most recent non-main entries
  by `at ?? 0` (stable sort, newest first; ties keep insertion order); delete the rest.
- `agentView(status, {freshMs = 90000, now})`: `main` = the first agent with truthy `main`, else
  null; `subagents` = non-main agents with `now - (at ?? 0) <= freshMs`, sorted stably newest
  first.
- `mainDecision(status)`: the newest `history` entry whose `agent.main` is truthy, else `status`;
  null for null.
- `readStatus(id)`: the parsed file, or null on any error.

### 8.3 Calibration file

`<dir>/calibration.json`: `{"newer": [ids], "models": [ids], "at": ms}`, written with
`writePrivate`, errors swallowed. `readCalibration` returns `{newer: [], models: [], at: null}`
when missing or unreadable; `models` and `at` are trusted only when `models` is an array and `at` a
number (else `[]` and null); `newer` is `[]` unless it is an array.

### 8.4 Dump

`dumpBody(body, setting = JEV_DUMP)`: unset or empty does nothing. `1`, `true`, `yes`
(case-insensitive) mean prefix `<dir>/dump`; any other value is the prefix itself. `ensureDir`
runs whenever the prefix starts with the status directory's path (a string prefix test). File `<prefix>.<ms>-<counter>.json`, counter per process from 0, mode 0600, indented
`stringify` (3.3). Errors are swallowed.

---

## 9. Environment and settings

### 9.1 `loadEnv({cwd, home, env})` (`node/src/env.mjs`)

Sources in priority order; earlier wins and an existing environment variable beats every file:

1. `<cwd>/.env`, only keys `JEV_API_KEY`, `TYPESAFE_API_KEY`, `JEV_DEBUG`, `JEV_ALLOW_FABLE`,
   `JEV_NO_STATUSLINE`, `JEV_ICONS`, and keys matching `^JEV_(?:[A-Z]+_)?(?:FORCE_)?EFFORT$`;
2. `<home>/.jev-router.env`, every key;
3. `<home>/.jev-claude.env`, every key.

A key is set only when absent from `env` and its value is non-empty. Missing or unreadable files
are skipped. Each file is parsed by Node's `util.parseEnv`, whose algorithm is
`Dotenv::ParseContent` in Node 24.21.0, vendored at
`conformance/reference/node_dotenv_parse_content.cc`. Ports implement that function line for line
over the file's bytes (its `trim_spaces` strips only space, tab, and `\n`; `\r` is deleted
everywhere first; a repeated key keeps its last value). Golden case `parse-env` pins it.

`childEnv(env)`: a copy without `JEV_API_KEY` and `TYPESAFE_API_KEY`.

### 9.2 Saved model (`node/src/settings.mjs`)

- `USER_SETTINGS = <home>/.claude/settings.json`; memo `<status dir>/saved-model.json`.
- `readSavedModel(file, memo)`: the settings file's `model` (undefined when the file is unreadable
  or not JSON). If it is the sentinel, return the memo's `model` (undefined if unreadable).
  Otherwise write the memo `{"model": <model, or null when absent>}` with mode 0600 (ensuring the
  status directory when the memo is the default path; errors swallowed) and return the model.
- `restoreSavedModel(previous, file)`: only when the file's `model` is exactly the sentinel: set it
  to `previous`, or delete `model` when `previous` is null or undefined, and write the indented
  `stringify` plus `\n`. Returns whether it wrote; every error returns false.

---

## 10. Launcher (`jev-claude`)

In order, as `node/bin/jev-claude.mjs`:

1. `savedModelBefore = readSavedModel()`.
2. `loadEnv()`.
3. `args = argv[1:]`. `passthrough = isClaudeSubcommand(args)` (10.3). Unless `passthrough`, append
   `--add-dir <root>` (also omitted with no root, 2.4). `env = childEnv()`.
4. Resolve `claude` (10.2). If absent, print to stderr exactly:
   `[jev] Claude Code is not installed, or \`claude\` is not on your PATH.`,
   `[jev] jev-claude runs the real Claude Code CLI; install it first:`,
   `[jev]   https://code.claude.com/docs/en/setup` (one line each) and exit 1.
5. First-run offer (11) when `shouldOffer({args: argv[1:], interactive: stdin and stdout are
   TTYs, offered: wasOffered(), shadowed: shadowsSkill(cwd, root)})`. The question is the two
   lines in `node/bin/jev-claude.mjs` verbatim. `interrupt` exits 130; a non-null answer is
   recorded with `markOffered`; `true` inserts `/jev-calibrate check` as the first element of
   `args`.
6. If `passthrough`, do nothing here (no proxy, no model variables, no status-line args, no
   "no JEV_API_KEY" notice): the subcommand runs on `env` as `childEnv()` left it. Otherwise, if
   `JEV_API_KEY || TYPESAFE_API_KEY` is truthy: start the proxy with `upstreamURL` =
   `ANTHROPIC_BASE_URL` when truthy (else the default) and set in `env`:
   `ANTHROPIC_BASE_URL=http://127.0.0.1:<port>`, `CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1`,
   `ANTHROPIC_CUSTOM_MODEL_OPTION=jev-router`, `ANTHROPIC_CUSTOM_MODEL_OPTION_NAME=Jev Router`,
   `ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION=Route each turn to the cheapest model that can do it`,
   `ANTHROPIC_CUSTOM_MODEL_OPTION_SUPPORTED_CAPABILITIES=thinking,adaptive_thinking,interleaved_thinking,effort,max_effort`,
   `CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT=1`, and `ANTHROPIC_MODEL=jev-router` when
   the parent's `ANTHROPIC_MODEL` is unset **or empty**. Register exit cleanup: close the proxy,
   `restoreSavedModel(savedModelBefore)`. Append the status-line args (10.1). Under `JEV_DEBUG` and
   an inherited base URL, print `[jev] upstream <url>`; under `JEV_DEBUG` with a TTY stdout, print
   `[jev] routing decisions -> <LOG_FILE>`.
   Otherwise print `[jev] no JEV_API_KEY found - starting Claude Code without routing` and
   `[jev] set it in <home>/.jev-router.env to enable routing` (home joined with the platform
   separator) and launch without the proxy.
7. Spawn `claude` (10.2) with inherited stdio and `env`. Exit with its exit code, or 1 when it was
   killed by a signal. If spawning fails, print `[jev] could not start Claude Code: <message>`
   and exit 1.
8. Signals: ignore SIGINT (Claude Code decides). On SIGHUP or SIGTERM, forward the signal to the
   child, then exit 1 if it has not exited within 5 s. Cleanup runs on every exit path, including
   these. On Windows, console close/logoff/shutdown events count as SIGHUP/SIGTERM and Ctrl+C /
   Ctrl+Break as SIGINT.

### 10.1 Status line args

None when `JEV_NO_STATUSLINE` is truthy, or when `<cwd>/.claude/settings.json` or
`<home>/.claude/settings.json` parses and has a truthy `statusLine`. Otherwise `writePrivate` the
status directory's `settings.json` with `{"statusLine":{"type":"command","command":"<cmd>"}}` and
pass `--settings <that file>`; a write failure means no args. `<cmd>` is how to run this port's
status line: Node `"<node executable>" "<script>"`; Go and Rust `"<jev-statusline executable next
to the running one>"`; Python `"<sys.executable>" -m jev_router.cli.statusline`.

### 10.2 Command resolution and launch (`node/src/launch.mjs`)

- `resolveCommand(name, {exts, path, win})`: on Windows, for each `;`-separated PATH entry
  (empty entries skipped, a leading `"` and a trailing `"` each removed independently) and each suffix of `exts`, else of
  `PATHEXT` split on `;`, else `.COM;.EXE;.BAT;.CMD`, accept the first path that **exists** (any
  kind, directories included, as Node's `F_OK`). Elsewhere, for each `:`-separated entry, accept
  `name` if it is executable (`X_OK`).
- `shimScript(file)`: case-insensitive `"%~?dp0%?\\([^"]+?\.[cm]?js)"` over the shim's text; the
  captured path, split on `\` and joined to the shim's directory, if it exists; else null.
- `launchSpec(file)`: `.ps1` (case-insensitive) runs `powershell.exe -NoProfile -ExecutionPolicy
  Bypass -File <file>`; `.cmd`/`.bat` runs the shim script with Node when `shimScript` finds one
  (Node uses its own executable; ports use `node` resolved with `resolveCommand`, falling back to
  the cmd route when there is none); else `ComSpec` or `cmd.exe` with the verbatim command line
  `/d /s /c "<caret-escaped shim> <quoteForCmd(arg)>..."`; anything else runs directly. Never
  through an implicit shell. The verbatim line needs raw argument passing: Go
  `SysProcAttr.CmdLine`, Rust `CommandExt::raw_arg`, Python a string command with `shell=False`.
- `quoteForCmd(arg)`: double each run of backslashes that precedes a `"` and escape the `"`;
  double a trailing run of backslashes; wrap in `"`; then caret-escape every character of
  `()[]%!^"`<>&|;, *?` and do that a second time on the result.

### 10.3 Claude subcommands (`isClaudeSubcommand`, `node/src/launch.mjs`)

`isClaudeSubcommand(args)` is true when `args[0]` is exactly (case-sensitive, the whole argument)
one of the names of the `claude` CLI's own subcommands as of Claude Code 2.1.292:
`agents`, `attach`, `auth`, `auto-mode`, `doctor`, `gateway`, `import`, `install`, `kill`, `logs`,
`mcp`, `plugin`, `plugins`, `purge`, `respawn`, `rm`, `setup-token`, `stop`, `ultrareview`,
`update`, `upgrade`. False for an empty `args`. Only the first argument counts: `-p mcp` and
`--model opus mcp list` are sessions, as is a prompt such as `"update the docs"`. These commands
manage Claude Code and reject `--add-dir` (`claude mcp list --add-dir x` fails with "unknown
option"), so a launch that names one runs it untouched (10, steps 3 and 6). A subcommand added in a
later Claude Code is not in the list and is launched as a session, as before.

---

## 11. First run (`node/src/first-run.mjs`)

- Marker `<home>/.jev-router/first-run.json`, `{"offeredAt": ISO, "accepted": bool}`;
  `wasOffered` is whether the file can be read. `markOffered` creates the directory; errors are
  swallowed.
- `shouldOffer` = `interactive && !offered && !shadowed && args.length === 0`.
- `shadowsSkill(cwd, root)` = the lexically resolved paths differ (no symlink resolution;
  case-sensitive string comparison, as Node's `path.resolve`) and
  `<cwd>/.claude/skills/jev-calibrate` exists.
- `askYesNo(question)`: write the question to stderr and read one line from stdin. A line that is
  not `n`/`no` (case-insensitive, trimmed) is `true`, so an empty line is `true`; `n`/`no` is
  `false`; Ctrl+C is `interrupt`; end of input or an input error is null. It always settles.

---

## 12. Status line (`jev-statusline`)

Reads all of stdin as JSON (malformed or empty input is `{}`) and writes one line plus `\n`. The
composition, ANSI codes, separators, and fallbacks are those of `node/bin/jev-statusline.mjs`; the
output must be byte-identical on the golden and harness cases.

- **Routed part**: when `main?.manual || (!main && status?.manual)`:
  `<DIM>☞ manual<RESET> <input.model.display_name ?? main.model ?? "">`, trailing spaces trimmed
  (a non-manual main entry wins over a manual flat flag). Else the main agent's line; else, when a
  status exists, the flat status's line (sessions before agent tracking); else
  `<DIM>jev: waiting for first prompt<RESET>`.
- **Main line**: `<model icon> <tier colour><shortName(model) ?? model ?? tier><RESET>`, then
  ` <DIM>(<MathRound(confidence*100)>%)<RESET>` when confidence is not null/undefined, then
  ` <DIM>·<RESET> <effort icon> <effort>` when effort is truthy, then ` <DIM>(<short>)<RESET>`
  when `shortReason(reason)` is non-null.
- **Sub-agents**: from `agentView(status)` (90 s freshness), the first three: the colour of
  `tier ?? statusTierOf(model)` (`statusTierOf` is the status line's own
  `/claude-([a-z]+)-/` capture, not config's `tierOf`), `☞ ` (with its trailing space) when manual,
  `shortName(model) ?? tier ?? model ?? "?"`, reset; `<DIM>+N<RESET>` for the rest; joined with
  `<DIM>,<RESET>`, after ` <DIM>·<RESET> <agents icon> `.
- Then the directory (last segment of `workspace.current_dir ?? cwd ?? ""` split on `/` or `\`,
  omitted when empty or equal to the worktree name), the branch (`(detached)` for "", clipped to 28
  units with `…`, blue), the worktree (green), ` <DIM>·<RESET> <context icon> <pct>%` with
  `pct = MathRound(ToNumber(context_window.used_percentage ?? 0))`, the calibration notice
  ` <DIM>·<RESET> <yellow>new <newer[0]>[ +<n-1>]: /jev-calibrate<RESET>` when `newer` is
  non-empty, and last the whole working directory, ` <DIM>· <path><RESET>`, where `path` is
  `workspace.current_dir ?? cwd ?? ""` as Claude Code sent it (no clipping, no separator
  conversion) except that the home directory is written `~`: with `home` = `os.homedir()` (3.8)
  minus any trailing `/` or `\` (a failed lookup counts as empty), `path` becomes `~` + the rest
  when it starts with `home` and what follows is empty, `/`, or `\`; a `home` that is empty (so
  also the filesystem root) or a path that does not start with a whole `home` directory leaves it
  unchanged (the comparison is case-sensitive and does not resolve symlinks). The directory item
  and the branch lookup keep using the path as sent. The part is omitted when `path` is empty. The
  line cannot be right-aligned, since Claude Code does not say how wide the terminal is; last is
  the rightmost item, and the first one cut when the line is too long.
- Icons are bold: `<BOLD><mark><RESET>`. Colours: haiku green `\x1b[32m`, sonnet cyan `\x1b[36m`,
  opus magenta `\x1b[35m`, fable yellow `\x1b[33m`.

### 12.1 Icons (`node/src/icons.mjs`)

`JEV_ICONS` lower-cased: `symbols` forces glyphs; `text` or `ascii` forces words; otherwise words
only on Windows when none of `WT_SESSION`, `TERM_PROGRAM`, `ConEmuPID` is truthy. Both tables are
copied from Node exactly.

### 12.2 Location (`node/src/worktree.mjs`)

`gitBranch(dir)`: null for an empty `dir`; else `git branch --show-current` in `dir`, 1 s
timeout, stderr discarded, stdout trimmed (3.5); null on any failure. `locationInfo(input,
branchOf)`: `worktree = input.worktree.name ?? input.workspace.git_worktree ?? null`;
`dir = input.workspace.current_dir ?? input.cwd ?? input.worktree.path`;
`branch = input.worktree.branch ?? branchOf(dir)`; null when `worktree` and `branch` are both null.

### 12.3 Short names (`node/src/model-names.mjs`)

`shortName(model)`: the first match anywhere of `claude-([a-z]+)-(\d+)(?:-(\d{1,2})(?!\d))?`
(3.1); `<family with first letter upper-cased> <major>` plus `.<minor>` when there is a minor;
null when nothing matches (or `model` is null/undefined).

### 12.4 Reasons (`node/src/reasons.mjs`)

The table of `match`, `short`, `long` is copied exactly and checked in order by substring.
`shortReason` returns the short text or null; `longReason` the long text or
`"the router's recommendation"`; `isNoChange` tests for the substring `no-change`.

---

## 13. Explain, legend, check

- `jev-explain <sessionId>`: `status = readStatus(id)`, `main = mainDecision(status)`; writes
  `formatExplanation(main && status.manual ? {...main, manual: true} : main)` and `\n`, then
  `formatAgents(status)` and `\n` when non-empty. Box drawing, widths (33 and 52), wrapping,
  `metric` (`toFixed(2)` for finite numbers, else `n/a`), ages, and wording exactly as
  `node/src/explain.mjs`. `recommendationOf` reads `jev.response.answers.model.choice`, then
  `answers.model_tier.choice`.
- `jev-legend`: `Status line key\n\n<formatLegend(icons())>\n`, aligned by code points.
- `jev-check`: the read-only report of `node/bin/jev-check.mjs`, same rows and wording. `Mode` is
  `repository` exactly when the repository root exists, contains `.git`, and contains
  `node/scripts/calibrate.mjs` (the same condition in every implementation); else `installed`.
  The `as of <date>` text uses the platform's local date-time format and is excluded from byte
  comparison.

---

## 14. Update library (`node/src/update.mjs`)

Ports implement with the same results: `UPDATE_FILE = <home>/.jev-router/update.json`,
`CHECK_EVERY_MS = 6 h`, `readState`, `writeState` (unique temp file + `renameOver` (3.11), errors swallowed),
`isCheckDue` (the last check is due when `checkedAt` is missing, does not parse, is in the future,
or is at least `everyMs` old; ports parse the `toISOString` form only and treat anything else as
unparseable), `compareVersions`, `updateNotice` (exact wording), `installedVersion(root)` (root
`package.json`), `inspectClone`, `checkForUpdate`, `needsInstall`, `applyUpdate(root, {install})`.

- git runs as `git -C <root> ...`, `GIT_TERMINAL_PROMPT=0`, timeouts 5 s locally and 20 s for
  fetch and merge, no console window on Windows, stdout trimmed (3.5).
- `inspectClone` compares the OS-canonical real path of `git rev-parse --show-toplevel` with that
  of `root`, case-insensitively on Windows, after normalising separators.
- The `reason` strings have fixed prefixes as in Node (`this folder is not a git clone`, `could
  not reach origin (`, ...); text after a prefix that quotes a git or process error is free-form.
- `jev-update-check`: `writeState(await checkForUpdate(<repo root>))`; with no root, exit 0
  without writing (2.4).

---

## 15. Logging (`node/src/log.mjs`)

`LOG_FILE = <home at start>/.jev-claude.log`. `log(line)`: when stdout was not a TTY at start
(3.9), write `[jev] <line>\n` to stderr; otherwise append `<ISO time> [jev] <line>\n` to the log
file, created with mode 0600 and chmod'ed to 0600 once per process; errors are swallowed.
`debug(line)` logs only when `JEV_DEBUG` is truthy at the time of the call. In the launcher the
proxy runs in the launcher's process, whose stdout is the user's terminal.

---

## 16. Conformance

A port is done when all three layers pass on Windows (the development machine). macOS and Linux
are expected to work but are not gating.

### 16.1 Ported unit tests

Each port re-implements the behavioural cases of every file in `node/test/` except
`bump-version.test.mjs` (out of scope, 1.2), one test file per Node test file, keeping each Node
test's title as the test name or a comment. Tests use real inputs and check real results: real
files in temp directories, real git repositories for the update tests, real HTTP servers on
loopback for the proxy tests (a fake Jev and a fake Anthropic upstream). A test that only checks a
mock, or asserts what it just set, does not count. Tests Node skips on Windows may be skipped on
Windows.

### 16.2 Golden cases

`conformance/cases/<name>.json`, generated by `node conformance/generate.mjs` (Node 24.21) from the
Node functions themselves, each an array of `{"name", "input", "expected"}`. Inputs are real: the
fixtures, the calibration prompts, the inputs in `node/test/`, captured model ids, plus the edge
inputs this document names. Each port has one test that loads every case file and checks its
implementation against every case. Case files are written by the lead before the ports start and
ports never edit them; a port that believes a case is wrong reports it.

Required case files and what each must include beyond ordinary inputs:

| Case file | Must include |
| --- | --- |
| `detect-override` | the prose negatives in `node/test/policy.test.mjs`; `use\u00a0opus`, `use\u3000opus`, `use\ufeffopus`, `use\u001fopus`; `ſ`/U+212A variants; restart-at-start+1 cases (`use opus-x use opus`); negated instructions (`do not use fable`, `don't use haiku, use opus`, curly apostrophe, `cannot`, `dont`) and questions (`why does the planner use opus?`, a `?` in a URL, a `.` inside a question) |
| `decide` | `confidence` as `"0.9"`, `[0.9]`, `" 0x1 "`, `null`, `""`, `"abc"`, `true`, missing |
| `new-turn-prompt`, `agent-label` | JSWS edge characters; non-string `text`; system messages after the user turn |
| `apply-tier` | `output_config.effort` as `""`, `0`, `false`; force and floor env combinations |
| `sanitize-schema` | nested arrays and objects; boolean true without a numeric bound |
| `version-of`, `short-name`, `claude-models`, `newest-per-tier`, `newer-than-calibrated` | `claude-opus-4-20250514`, `claude-sonnet-4-20250514`, `claude-3-7-sonnet-20250219`, `anthropic.claude-opus-6`, ties on version |
| `session-of`, `conversation-key` | numeric `session_id`; lone-surrogate text; non-JSON `user_id` |
| `agent-of` | sequences sharing one `mains`, including more than 51 sessions |
| `write-decision` | a sequence of `writeDecision`/`markManual` calls on one session (history cap 20, the 12-agent trim with ties, manual surviving a routed merge), compared as parsed JSON after each step; every decision carries an explicit `at`, and `at` values written by `markManual` (which uses the clock) are replaced by a placeholder on both sides before comparing |
| `agent-view`, `main-decision` | freshness at the 90 s boundary, missing `at`, pre-agent statuses |
| `effort-floor`, `forced-effort`, `fable-allowed`, `icons`, `reasons` | whitespace and case variants |
| `format-explanation`, `format-agents`, `format-legend` | `contextSize` exactly `0.125`; NaN and missing metrics; manual; long prompts that wrap; fixed `now` |
| `location-info` | each source of worktree and branch, with an injected branch lookup |
| `compare-versions`, `update-notice`, `is-check-due`, `needs-install` | pre-release tags; future `checkedAt` |
| `quote-for-cmd` | backslashes before quotes and at the end, every metacharacter, spaces |
| `parse-env` | every branch of the vendored parser: `A=a#b`, unclosed quotes with and without a following line, text after a closing quote, `\n` in each quote style, backticks, `export `, spaces around `=`, CRLF, empty key (`=a=b`), line without `=`, repeated keys, trailing key without newline |
| `stringify`, `parse` | number formats, escapes, lone surrogates, invalid UTF-8 bytes (hex-encoded input), duplicate keys, the UTF-16 length used for context tokens |
| `math` | `MathRound`, `toFixed2`, `ToNumber` over the values in 3.6 and 3.7 |
| `jev-request` | the full Jev request body for given models, prompt, current model and context size |
| `status-line` | rendered output for given stdin JSON, status file, calibration file, and `JEV_ICONS`; the status line uses the real clock, so freshness is fixed through the data (`at` far in the future for fresh sub-agents, absent or 0 for stale ones) rather than an injected `now` |
| `parse`, `stringify`, `sanitize-schema` (key order) | integer-like keys (`"0"`, `"2"`, `"01"`, `"4294967294"`, `"4294967295"`) mixed with ordinary keys |
| `new-turn-prompt`, `agent-label` (surrogates) | a lone surrogate in the user text, which must survive into the result |

The generator excludes inputs that would split a surrogate pair (3.2).

### 16.3 Black-box harness

`conformance/harness/*.test.mjs` (Node, `node:test`) starts an implementation's `jev-proxy-host`
and status line as child processes, chosen by `JEV_IMPL_CMD_PROXY` and `JEV_IMPL_CMD_STATUSLINE`
(command lines; default the Node ones). Each child runs with its working directory in a fresh temp
directory (so no developer `.env` is read), `JEV_STATUS_DIR`, `HOME`, `USERPROFILE`, `TEMP`, `TMP`,
`TMPDIR` pointed at temp directories, `JEV_ICONS=symbols`, `JEV_DEBUG` unset, a fake Jev server as
`TYPESAFE_BASE_URL` with `JEV_API_KEY` set, and a fake Anthropic upstream as `ANTHROPIC_BASE_URL`.
`jev-proxy-host` must `loadEnv`, start the proxy with `upstreamURL` = `ANTHROPIC_BASE_URL` when set,
write `PORT=<port>\n` to stdout and flush, and run until killed.

The harness checks, using the comparison rules of 7.7: forwarded bodies and headers for routed,
passthrough, auxiliary, tool-continuation, and malformed requests; the Jev request; streaming
(chunks arrive before the upstream finishes); `/v1/models` handling for 200 and 401 and the
calibration file; client abort reaching upstream; the 502 path; status files after each case; the
status line's output for the same session. It passes against Node first, and is the shared
acceptance test for every port.

### 16.4 Run commands

From the repository root:

```
pnpm test                                          # Node unit tests
node --test conformance/harness                    # harness against Node
(cd go && go vet ./... && go test ./...)
(cd rust && cargo clippy --all-targets -- -D warnings && cargo test)
python -m unittest discover -s python/tests -t python
```

Each port's README gives the `JEV_IMPL_CMD_PROXY` / `JEV_IMPL_CMD_STATUSLINE` values for running
the harness against it.

---

## 17. Plan

1. **Lead (before the ports):** `node/scripts/proxy-host.mjs` already forwards to
   `ANTHROPIC_BASE_URL` (done in this revision). Write `conformance/generate.mjs` and the case
   files (16.2) and `conformance/harness` (16.3); all pass against Node. Commit.
2. **Ports, in parallel:** one agent per language, each in its own git worktree, writing only
   inside its language directory. Each implements sections 3-15 (helper modules first, tested
   against the `stringify`, `parse`, `math`, `parse-env` cases), passes 16.1 and 16.2, then 16.3
   with its own commands, and writes `<lang>/README.md` (build, test, harness commands, any
   divergence with its reason). An agent that finds a defect in this document stops that part and
   reports it rather than choosing a behaviour.
3. **Lead (after):** review each port, run every layer for every implementation, merge, update the
   root README and `doc/` (`doc/LAYOUT.md`, a per-language status table), and record here any
   defect found.

---

## 18. Known Node behaviours to preserve

These look odd but are deliberate or load-bearing; ports keep them.

- A sub-agent hand-back that quotes `"use strong"` is not an override (quotes are stripped).
- Requests with no tools (Claude Code's title and summary calls) are never routed and never claim
  the main-thread slot.
- Hook output arrives as a trailing `system` message and is skipped when finding the user's turn.
- An answer naming a model not on the menu counts as no answer.
- A conversation the proxy has not routed yet passes `contextTokens = 0` to policy.
- `claude -p` may omit metadata on its first request; the fallback key carries state over.
- The decision is filed under the conversation key when there is no session id.
- `<jev-explain>` turns are never routed and never recorded.
- A main agent once marked manual keeps `agents[key].manual = true` after routing resumes, so the
  status line keeps showing `☞ manual` for it.
- `/v1/messages/count_tokens` requests are processed like turns.
- A `/v1/models` error response still rewrites `calibration.json` from the catalog it has.
- `JEV_STATUS_DIR` set only in an env file is ignored by the launcher's proxy but inherited by
  Claude Code and so by the status line.

---

## 19. Clarifications from building the conformance suite

Found while generating the golden cases; each is Node's behaviour, pinned by a case, and normative.

1. `claudeModels` breaks `releasedAt` ties with `localeCompare`; ports use code-unit order, and the
   cases only contain ISO dates where the two agree. (This item's former sentence on non-string
   `created_at` is superseded by 20.8.)
2. `isCheckDue` cases use only the `toISOString` form or strings no date parser accepts.
3. The harness runs with `node --test conformance/harness` through `conformance/harness/index.js`,
   which imports every `*.test.mjs`; `node --test "conformance/harness/*.test.mjs"` also works.
4. A string or array `answers.model` spreads into index keys (`{"0":"a","1":"b",...}`) in the
   router result (6.2).
5. `toFixed(2)` of `|x| >= 1e21` is `String(x)` (e.g. `1e+21`); of `±Infinity` is `Infinity` /
   `-Infinity`.
6. Node's SDK also sends `X-TypeSafe-SDK`, `X-TypeSafe-Runtime`, and `User-Agent:
   typesafe-sdk/0.6.0`; ports send only the headers in 5.1, so the harness checks `User-Agent` by
   pattern.
7. The status line prints a `-0` percentage (from a small negative `used_percentage`) as `0%`.
8. In `agentView`, an agent entry's own `key` field overrides the map key (`{key, ...a}`).
9. `compareVersions` uses `parseInt` semantics on each dotted part: trim `JSWS`, optional sign,
   leading decimal digits only (`"0x10"` -> 0, `"7abc"` -> 7), NaN -> 0.
10. `status-line` cases fix freshness with `at = 4102444800000` (fresh) or `0`/absent (stale), and
    need no real git repository (see `conformance/cases/README.md`).
11. `jev-request` cases use the default base URL and `jev-latest`; ports testing through a loopback
    fake compare method, the `/v1/systemone` path, and the body.

---

## 20. Clarifications from the ports

Raised by the Go, Rust, and Python agents; each is Node's behaviour and normative.

1. **Values fixed at start (3.9)** also include every path Node binds when its modules load:
   `USER_SETTINGS`, `SAVED_MODEL_MEMO`, `FIRST_RUN_FILE`, `UPDATE_FILE`, and `SETTINGS_FILE`
   (home and status directory as they were at start).
2. **Temporary file names (3.10):** Node writes `<file>.<pid>.tmp`; the ports' `<file>.<pid>.<seq>.tmp`
   is a deliberate, permitted divergence.
3. **Jev settings (5.1)** — the key, base URL, and default model — are read once per process, when
   the client is first built, and do not change afterwards.
4. **Upstream connection (7.7):** `host` omits the port when the URL's port is the scheme's default
   (Node's `URL` drops `:80` for `http` and `:443` for `https`). Ports never use an outbound proxy
   from `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY` (Node does not); Go sets `Transport.Proxy = nil`, and
   may call `Transport.RoundTrip` directly instead of using a `CheckRedirect`.
5. **A throw after the rewrite (7.2):** if processing throws after `applyTier` (for example
   `writeDecision` on a status file whose `history` is not iterable), `body.model` is no longer the
   sentinel, so the original bytes, sentinel included, are forwarded.
6. **`applyTier` with a truthy non-object `output_config` (7.5):** for a tier without effort,
   whether it is deleted follows `Object.keys(output_config).length` (a string's characters count;
   numbers and booleans have none, so they are deleted).
7. **Status line on `null` (12):** stdin holding the JSON literal `null` makes Node throw reading
   `null.session_id`: nothing is written to stdout and the exit code is 1. (Malformed or empty input
   is still `{}`.)
8. **`created_at` (7.5, 19.1):** `null` or absent becomes `releasedAt: ""`. A falsy value (`""`,
   `0`, `false`) adds no `released` item to `description` and is kept as-is in `releasedAt`. A
   truthy value that is neither a string nor an array throws `model.created_at.slice is not a
   function` whatever the catalog order (no calibration write; later routed turns take the error
   path). Strings and arrays slice (`["2025-01-02"]` gives `released 2025-01-02`). A non-string
   `releasedAt` (`0`, `false`, an array) throws `b.releasedAt.localeCompare is not a function` only
   when a version tie compares it as `b`; as `a` it is coerced to a string. Which operand Node sees
   as `b` follows V8's sort, so ports may throw whenever either operand of a tie comparison is a
   non-string `releasedAt`. No golden case contains a non-string `created_at`.
9. **TLS in Rust (2.2):** `rustls` is built with the `ring` provider, because the default
   `aws-lc-rs` needs CMake and NASM on Windows.
10. **Untested path (7.2 step 3):** a client that disconnects while Jev is being asked, before the
    upstream request starts, is covered by neither the Node tests nor the harness; each port
    implements it and documents how.
