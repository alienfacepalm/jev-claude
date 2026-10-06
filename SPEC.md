# jev-claude port specification

Status: draft for review. Reference implementation: `node/` at commit `2f9967f`.

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
outside it except as section 16 allows.

```
go/
  go.mod                      module github.com/alienfacepalm/jev-claude/go, go 1.23
  cmd/<program>/main.go       one per program in 1.1
  internal/<package>/         config, env, jev, router, policy, reasons, proxy, status,
                              settings, launch, firstrun, statusline, explain, legend, icons,
                              modelnames, worktree, update, logx, jsonjs, repo
  README.md
rust/
  Cargo.toml                  package jev-router, edition 2024, rust-version 1.85
  src/lib.rs, src/<module>.rs
  src/bin/<program>.rs        one per program in 1.1
  tests/                      integration tests
  README.md
python/
  pyproject.toml              project jev-router, requires-python >= 3.12, [project.scripts]
  src/jev_router/<module>.py  one module per Node module
  src/jev_router/cli/         one entry function per program in 1.1
  tests/test_<module>.py
  README.md
```

Program names on disk are exactly the names in 1.1 (`jev-claude`, `jev-statusline`, ...), with
the platform's executable suffix where it has one.

### 2.2 Toolchains and dependencies

Keep dependencies to what the job needs; each one is a supply-chain and install cost for a tool
that holds an API key.

| Port | Toolchain | Dependencies | Build | Test |
| --- | --- | --- | --- | --- |
| Go | Go 1.23+ | standard library only | `go build ./...` | `go vet ./... && go test ./...` |
| Rust | stable, MSRV 1.85 | `tokio`, `hyper` 1.x, `hyper-util`, `http-body-util`, `hyper-rustls` (or `rustls` + `tokio-rustls`), `serde`, `serde_json` with `preserve_order`, `sha1`, `regex`; nothing else without a note in `rust/README.md` | `cargo build --release` | `cargo test` and `cargo clippy -- -D warnings` |
| Python | CPython 3.12+ | standard library only at runtime; tests use `unittest` (no pytest, which is not installed here) | `python -m pip install -e python` optional | `python -m unittest discover -s python/tests -t python` |

No port may depend on a TypeSafe SDK. The Jev client is written in each port against the wire
contract in section 5, so behaviour is identical and pinned.

### 2.3 Version

The release version lives only in the root `package.json` (`"version"`). Port manifests carry the
fixed version `0.0.0` and are never bumped. Code that needs the release version (section 14)
reads it from the root `package.json` at run time.

### 2.4 Repository root

Several behaviours need the repository root (the directory that holds `.git`, `package.json`, and
`.claude/skills`). The Node code derives it from its own file location (`node/bin/..` / `..`).
Ports resolve it as:

1. `JEV_ROOT`, when set and non-empty;
2. otherwise the nearest ancestor of the running executable's real path (symlinks resolved) that
   contains `.claude/skills/jev-calibrate/SKILL.md`;
3. otherwise none: the launcher then omits `--add-dir` and `shadowsSkill` is false.

`JEV_ROOT` is honoured by the ports only; it is not a Node setting and is not documented to users.

### 2.5 Shared state

All implementations read and write the same files with the same JSON shapes (section 8), so a
session started by one implementation can be displayed by another's status line. Field names,
types, and the meaning of absent fields must match the Node implementation. Byte-identical files
are not required except where this document says so.

---

## 3. JavaScript-compatibility rules

These rules apply everywhere below; a port that gets them wrong fails conformance in ways that are
hard to see.

### 3.1 Regular expressions

- JavaScript regexes without the `u` flag treat `\w`, `\d`, `\s`, `\b` as ASCII-only (except
  `\s`, which includes Unicode whitespace) and `i` as ASCII case folding for the patterns here.
  Python must compile with `re.ASCII` for `\w`/`\d`/`\b` patterns. Rust must disable Unicode
  classes (`(?-u:\w)`, or `RegexBuilder::unicode(false)` on byte input) where the pattern uses
  them. Go's RE2 classes are already ASCII.
- Go's `regexp` and Rust's `regex` have no lookaround. Three Node patterns use negative lookahead:
  the override patterns `(?![-\w])` (section 4.5), `versionOf` `(?!\d)` (7.5), and `shortName`
  `(?!\d)` (12.3). Ports must implement them by matching without the lookahead and then checking
  the character after the match explicitly, iterating to the next candidate match when the check
  fails, so results equal the JavaScript regex on every input. Python may use the lookahead
  directly.
- `/<system-reminder>[\s\S]*?<\/system-reminder>/g` and the other tag-stripping patterns are
  non-greedy across newlines.

### 3.2 String length and truncation

JavaScript measures `length`, `slice`, and `padEnd` in UTF-16 code units. Wherever Node truncates
or pads (`agentLabel`, the status line's branch clip, the explanation panel's rows, the debug
log's prompt excerpt), ports must count UTF-16 code units. When a cut would split a surrogate
pair, ports drop the lone high surrogate (Node would keep it and emit invalid text); this is the
one permitted divergence. `[...mark].length` in `formatLegend` counts code points.

### 3.3 JSON

- Object key order must be preserved through parse and re-serialise of request bodies and status
  files. Go needs an order-preserving JSON representation (a small ordered-object type in
  `internal/jsonjs`); `map[string]any` is not acceptable for bodies the proxy rewrites. Rust uses
  `serde_json` with `preserve_order`. Python `dict` already preserves order.
- `JSON.stringify` output, where its exact text matters (the context-size estimate, 7.4), is:
  no whitespace; strings escape only `"`, `\`, U+0000-U+001F (as `\b \f \n \r \t` or `\u00XX`
  lowercase hex), and lone surrogates; non-ASCII, `/`, `<`, `>`, `&` are not escaped; numbers in
  JavaScript's shortest round-trip form (`1e+21`, `5e-7`, integers without `.0`). Go must not use
  HTML escaping; Python must not use `ensure_ascii=True`. Each port provides one
  `stringify`-equivalent function and uses it for every JSON it writes.
- `JSON.stringify(v, null, 2)` (settings file restore, `JEV_DUMP`) is two-space indented with
  `": "` after keys.
- Status-file numbers that Node writes as integers (`at`, timestamps in ms) must be written as
  integers.

### 3.4 Time

Timestamps are milliseconds since the Unix epoch as integers (`Date.now()`), and ISO strings are
`Date.prototype.toISOString()` form: `YYYY-MM-DDTHH:mm:ss.sssZ`, UTC, three fractional digits.

---

## 4. Configuration (`node/src/config.mjs`)

Ports must reproduce every exported value and function. All prose strings sent to Jev (the score
questions, `COMPLEXITY_SCALE`, `GUIDANCE`, `COST`, and the five instruction sentences in
`questionForModels`) must be copied byte-for-byte from `node/src/config.mjs`; conformance case
`jev-request` (16.2) checks the assembled request.

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
- `AUTO_MODEL = "jev-router"`; `isAuto(m)` is exact equality.
- `fableAllowed(env)`: false only when `JEV_ALLOW_FABLE`, trimmed, matches `^(0|false|no|off)$`
  case-insensitively. `availableTiers(env)` is `TIER_NAMES` minus fable when not allowed.
- `effortFloor(name, env)`: null when the tier has no floor; else `JEV_<NAME>_EFFORT` trimmed and
  lower-cased if it is one of `EFFORTS`, else the floor.
- `forcedEffort(name, env)`: null when the tier takes no effort; else the first of
  `JEV_<NAME>_FORCE_EFFORT`, `JEV_FORCE_EFFORT` that, trimmed and lower-cased, is in `EFFORTS`.
- `shouldUseExactModel(reason, chosenTier, finalTier)`: `reason` is `jev` or `jev/no-change` and
  `chosenTier === finalTier`.

### 4.3 Thresholds and constants

`minConfidence 0.6`, `uncertainDefault "sonnet"`, `downgradeMaxContextTokens 20000`,
`jevTimeoutMs 1500`, `jevDeadlineMs 3000`, `jevMaxRetries 1`, `CONTEXT_WINDOW_TOKENS 200000`,
`COMPLEXITY_MAX_SCORE 9` (length of `COMPLEXITY_SCALE` minus one).

### 4.4 Questions

`QUESTIONS` has three score questions, `task_complexity`, `reasoning_required`,
`tool_complexity`, in that order: `{ "type": "score", "instructions": <text>, "criteria":
COMPLEXITY_SCALE }`.

`questionForModels(models)` builds `{ "type": "choice", "instructions": [5 sentences],
"criteria": { <model id>: { "model": description ?? id, "cost": COST[tier], "what", "signals",
"not_for" } } }` with keys in the order the models are given and, inside each entry, in the order
`model, cost, what, signals, not_for`.

### 4.5 Override patterns

For each tier, in tier order, a case-insensitive pattern:

```
\b(?:use|switch to|switch over to|route to)\s+(?:the\s+)?(?:claude[-\s])?(?:(?:NAME)|GENERIC\s+(?:model|tier))(?![-\w])
```

with NAME/GENERIC: haiku/fast, sonnet/balanced, opus/strong, fable/long. See 3.1 for the
lookahead.

---

## 5. Jev wire contract

Ports replace `@typesafe-ai/sdk` 0.6.0 with their own client for exactly what Node uses.

### 5.1 Request

- `POST {base}/v1/systemone`, where `base` is `TYPESAFE_BASE_URL` (trimmed; blank means unset)
  with trailing slashes removed, defaulting to `https://api.typesafe.ai`.
- Headers: `Authorization: Bearer <key>`, `Accept: application/json`,
  `Content-Type: application/json`, `User-Agent: jev-router-<lang>/<release version or 0.0.0>`,
  and on retries `X-TypeSafe-Retry-Count: <attempt>`. The key is `JEV_API_KEY`, else
  `TYPESAFE_API_KEY`. The SDK's `X-TypeSafe-SDK`/`X-TypeSafe-Runtime` headers are not sent.
- Body: the router's request object (6.1) plus `"model": "jev-latest"` (or
  `TYPESAFE_DEFAULT_MODEL` when set), serialised per 3.3.
- The key must never be logged. No request or response body is ever logged.

### 5.2 Response

Success is any 2xx whose body parses as JSON. Shape used:

```json
{ "model": "jev-...", "usage": { "input_tokens": 0, "output_tokens": 0 },
  "answers": {
    "model": { "type": "choice", "choice": "<label>", "confidence": 0.0, "probabilities": { "<label>": 0.0 } },
    "task_complexity": { "type": "score", "score": 0.0, "confidence": 0.0, "legend": {}, "probabilities": {} },
    "reasoning_required": { "...": "as above" },
    "tool_complexity": { "...": "as above" } } }
```

The whole parsed response is kept verbatim (it is recorded in status files, 8.2).

### 5.3 Timeouts and retries

- Per attempt: 1500 ms covering connect, send, and reading the full body.
- Retries: at most 1. Retry on HTTP 408, 429, 500-599, connection failure, or attempt timeout.
  Do not retry other statuses.
- Delay before retry *n* (zero-based): `Retry-After-Ms` header if a finite number >= 0, else
  `Retry-After` as seconds or HTTP date, used only if <= 60000 ms; otherwise
  `round(min(150 * 2^n, 400) * (1 - random() * 0.25))`.
- An overall deadline of 3000 ms from the start of the call aborts whatever is in flight,
  including a backoff wait.
- Any failure (missing key, network, non-2xx, timeout, unparseable body, missing `answers.model`
  or score answers) makes the router return "no answer" (6.2). Nothing propagates to the request
  being proxied.

### 5.4 Prewarm

When the proxy starts with the real router, it sends one fire-and-forget `HEAD` to the origin of
the Jev base URL, ignoring every outcome, so the TLS handshake is not paid inside the first turn.
Ports should reuse that connection for the first real call (Go: shared `http.Client`; Rust: shared
client; Python: best effort, may skip if the HTTP stack cannot pool).

---

## 6. Router (`node/src/router.mjs`)

### 6.1 `askJev({prompt, current, contextTokens, models})`

Returns null immediately when `models` is empty. Otherwise sends:

```json
{ "state": { "request": "<prompt>",
             "session": { "current_model": "<current>", "context_tokens": 0 },
             "environment": { "available_models": ["<id>", "..."] } },
  "questions": { "task_complexity": {}, "reasoning_required": {}, "tool_complexity": {},
                 "model": "<questionForModels(models)>" } }
```

### 6.2 Result

On success: `{ choice, confidence, probabilities, type }` copied from `answers.model`, plus
`request` (the object above, without `model`), `response` (5.2 verbatim), `metrics` with
`taskComplexity`, `reasoningRequired`, `toolComplexity` = each score / 9, and
`contextSize = min(contextTokens / 200000, 1)`, and `ms`, the call's wall time. On failure: log
`routing failed, keeping <current>: <message>` (section 15) and return null.

---

## 7. Proxy (`node/src/proxy.mjs`)

### 7.1 Server

- Listens on `127.0.0.1`, port 0 (OS-assigned); `startProxy` returns the port and a close
  function.
- Options: `upstreamURL` (default `https://api.anthropic.com`), `route` (default the real
  router; tests inject a fake), `calibrationFile` (default 8.3).
- `HEAD` to any path: `200`, empty body, no upstream call.
- Concurrency: requests are independent; routing for one must not block others. Shared state
  (conversation map, catalog, mains map) must be safe under concurrent requests.

### 7.2 Per request

1. Read the whole request body.
2. If the path starts with `/v1/messages`, process the body (7.3). Any exception while processing
   is logged under debug as `could not process body: <message>`; if the parsed body's model is the
   sentinel, apply the conversation's tier (or `uncertainDefault`) with 7.5 and forward that;
   otherwise forward the original bytes.
3. If the client has gone away, stop without contacting upstream.
4. Forward to `upstream origin + upstream path (trailing "/" removed) + request path and query`,
   same method, headers per 7.7.
5. For `GET /v1/models` (path exactly `/v1/models`, optionally with a query): buffer the response,
   record every model whose id has a tier into the catalog (keyed by id, latest wins), write the
   calibration file (8.3) with `newer = newerThanCalibrated(catalog)` and `models = ids of
   newestPerTier(claudeModels(catalog))`, then send the original status and headers without
   `content-length`, and the buffered body. A body that fails to parse is logged under debug and
   still relayed.
6. Otherwise stream the response through: status and headers as received, body piped
   chunk-by-chunk without buffering (server-sent events must reach the client as they arrive).
   Under `JEV_DEBUG`, log `<status> served by <model>` for the first `"model":"..."` seen in the
   stream.
7. When the client disconnects before the response finishes, abort the upstream request (stops
   generation and billing). When upstream fails mid-stream, terminate the client connection. When
   upstream fails before headers, reply `502` with
   `{"type":"error","error":{"message":"<message>"}}`, `content-type: application/json`.

### 7.3 Body processing

1. Parse JSON. Call `dumpBody` (8.4). For each `tools[i].input_schema`, apply `sanitizeSchema`.
2. **Not the sentinel** (user picked a model): if `tools` is non-empty, compute `agentOf`; if
   `newTurnPrompt(body)` is non-null, `markManual(sessionOf(body), body.model, agent)` (8.2).
   Debug lines: `passthrough, user selected <model>` and, with tools,
   `<key> passthrough <main|sub> <model>`.
3. **Sentinel**:
   1. `agent = agentOf(body, mains)`; `state = stateFor(agent.key, fallback)` where `fallback` is
      `conversationKey` of the body without `metadata` when the body has a session id, else none.
   2. `current = state.tier ?? "sonnet"`; `prompt = newTurnPrompt(body)`;
      `explaining = prompt contains "<jev-explain>"`.
   3. If `prompt` and not `explaining`:
      - `models = newestPerTier(claudeModels(catalog) filtered to availableTiers())`;
        `available` = distinct tiers of `models`.
      - `currentModel = state.model ?? modelForTier(models, current)` where `modelForTier` is the
        first model of that tier, else `idOf(tier)`.
      - `contextTokens = round(len_utf16(stringify(body.messages)) / 4)` (3.2, 3.3);
        `Math.round` semantics (half rounds up).
      - `jev = route({prompt, current: currentModel, contextTokens, models})`.
      - `chosen` = the model in `models` whose id equals `jev.choice`; when none, the answer is
        treated as no answer.
      - `decision = decide({prompt, jev: chosen ? {...jev, choice: chosen.tier} : null, current,
        available, contextTokens: state.tier ? contextTokens : 0})`.
      - `model = chosen.id` if `shouldUseExactModel(reason, chosen.tier, tier)`, else
        `currentModel` if `tier === current`, else `modelForTier(models, tier)`.
      - `state.tier = tier; state.model = model`; build `fresh = { prompt, model, confidence:
        jev.confidence ?? null, metrics: jev.metrics ?? null, reason, jev: jev ? {request,
        response} : null }`.
      - Debug line:
        `<key> <main|sub[label]> <Nms p=0.00|no-jev> <current> -> <tier> (<reason>) ctx~<n> | <prompt first 60 units>`.
   4. `tier = state.tier ?? current`; `model = state.model ?? idOf(tier)`; debug
      `<key> rewrite <old model> -> <model>`; `applyTier(body, tier, model)`.
   5. If `fresh` and not explaining: `effort = body.output_config.effort ?? null`;
      `writeDecision(sessionOf(body) || agent.key, { tier, ...fresh, effort, at: now }, agent)`.
      Key order of the decision object: `tier, prompt, model, confidence, metrics, reason, jev,
      effort, at`.
4. Re-serialise the body (3.3) as the bytes to forward.

### 7.4 Conversation state

- `convos`: up to 50 entries `{tier, model}` keyed by conversation key, least recently used first.
  `stateFor(key, fallback)`: if `key` is absent and `fallback` is present, move the fallback's
  state to `key`. On access, move to most recent. When inserting at capacity, evict the least
  recent key that is not a main thread's key (values of `mains`), or the least recent of all if
  every key is a main.
- `mains`: session id -> the first conversation key that made a request with tools in that
  session; at most 51 entries, evicting the oldest insertion when more than 50 exist before
  inserting.
- `catalog`: model id -> model object from `/v1/models`, ids with a tier only.

### 7.5 Pure functions

Each must match Node on every conformance case.

- `sanitizeSchema(node)`: recursive in place. For `exclusiveMinimum`/`exclusiveMaximum` holding a
  boolean: if true and the paired `minimum`/`maximum` is a number, move that number into the
  exclusive key and delete the plain one; otherwise delete the exclusive key. Recurse into every
  value (arrays and objects), including values just written.
- `newTurnPrompt(body)`: null when `tools` is missing or empty. Last message whose role is not
  `system`; null unless its role is `user`. String content is used as is; array content is null
  when any block is `tool_result`, else the `text` of `text` blocks joined with `\n`; other
  content null. Remove `<system-reminder>...</system-reminder>` spans, trim, empty is null.
- `applyTier(body, tierName, model = idOf(tierName), env)`: unknown tier returns the body
  unchanged. Set `model`. Tier without thinking: delete `thinking`; filter
  `context_management.edits` to entries whose `type` does not contain `thinking`
  (case-insensitive), deleting `context_management` if the list empties. Tier without effort and
  an `output_config`: delete `output_config.effort`, deleting `output_config` if empty. Tier with
  effort: `effort = forcedEffort(tier) ?? (body.output_config?.effort ? null :
  effortFloor(tier))`; when non-null set `output_config = {...output_config, effort}` (existing
  keys first, `effort` replacing in place if present, else appended).
- `versionOf({id, tier})`: `[major, minor]` from
  `^(?:[\w-]+\.)?claude-<family>-(\d+)(?:-(\d{1,2})(?!\d))?`; `[0, 0]` when no family or no
  match.
- `claudeModels(catalog)`: models with a tier, mapped to `{id, tier, releasedAt: created_at ?? "",
  description}` where `description` joins the truthy parts `display_name`,
  `released <created_at first 10 chars>`, `<max_input_tokens> input tokens` with `"; "`; sorted
  by version descending, then `releasedAt` descending (string compare, `localeCompare` on ASCII
  ISO strings; ports use plain code-unit comparison). Empty result falls back to the four
  `TIERS` as `{id, tier: name, releasedAt: "", description: id}`. The sort must be stable.
- `newestPerTier(models)`: first model of each tier, in first-seen order.
- `newerThanCalibrated(catalog)`: ids from `newestPerTier(claudeModels(catalog))` whose version is
  greater than the version of that tier's configured `id`.
- `sessionOf(body)`: `JSON.parse(body.metadata.user_id ?? "{}").session_id ?? ""`; "" on any
  failure.
- `conversationKey(body)`: first 12 hex chars of SHA-1 of `<session>|<text>` (UTF-8), where text
  is the first message's string content or its `text` blocks concatenated with no separator.
- `agentLabel(body, max = 48)`: first message's string content or `text` blocks joined with a
  space; strip system reminders, collapse whitespace runs (`\s+`, JS definition) to one space,
  trim; longer than `max` units becomes the first `max - 1` units plus `…`.
- `agentOf(body, mains)`: `key = conversationKey(body)`, `session = sessionOf(body)`,
  `real = tools non-empty`; when `session && real && !mains.has(session)`, record it (7.4).
  `main = !session || mains.get(session) === key`. Returns
  `{key, label: agentLabel(body) || (main ? "main" : key), main}`.

### 7.6 Policy (`node/src/policy.mjs`)

- `ownWords(prompt)`: remove, in order, `<agent-message...</agent-message>`,
  `<system-reminder>...</system-reminder>`, fenced code ```` ```...``` ````, inline code
  `` `...` `` (no newline inside), and double-quoted spans (no newline inside), each replaced by
  one space.
- `detectOverride(prompt)`: the first override pattern (4.5) that matches `ownWords(prompt)`.
- `clampToAvailable(tier, available)`: the tier itself if available; else the first available tier
  above it, never `fable` unless `tier` is `fable`; else the highest available tier below it;
  else null.
- `decide({prompt, jev, current, available, contextTokens = 0})` returns
  `{tier, reason, changed}`:
  - `settle(t, r)`: `final = clampToAvailable(t, available) ?? current`;
    `why = final === t ? r : r + "+unavailable"`;
    `reason = final === current ? why + "/no-change" : why`; `changed = final !== current`.
  - Override present: `settle(override, "override")`.
  - No `jev`, or `jev.choice` not a tier name: `settle(current, "jev-unavailable")`.
  - `jev.confidence` not a number >= 0.6 (missing, NaN, strings all count as unsure):
    `settle(TIER_NAMES[max(rank(choice) - 1, rank("sonnet"), rank(current))],
    "low-confidence-default")`.
  - Choice ranks below current and `contextTokens > 20000`:
    `settle(current, "downgrade-not-worth-cache-rebuild")`.
  - Else `settle(choice, "jev")`.

### 7.7 Upstream headers

Copy the client's headers; set `host` to the upstream host (with port when non-default); remove
`content-length`, `transfer-encoding`, `connection`, `keep-alive`; set `content-length` to the
forwarded body length when it is non-zero. For `GET /v1/models`, and for every request when
`JEV_DEBUG` is set, remove `accept-encoding`. Header names are forwarded lower-cased. Response
headers are relayed as received (minus `content-length` for the models response).

---

## 8. Status store (`node/src/status.mjs`)

### 8.1 Directory and files

- Directory: `JEV_STATUS_DIR`, else `<os temp dir>/jev-claude`. Created with mode 0700 and
  chmod'ed to 0700 on every `ensureDir` (failures propagate to the caller, which swallows them);
  on Windows modes are no-ops.
- Session file: `<dir>/<sessionId with every char not in [A-Za-z0-9_-] removed>.json`, mode 0600.
- Writes go to `<file>.<pid>.tmp` then rename over the target, then chmod 0600.
- `settings.json` in the directory is the launcher's `--settings` file and is never pruned.
- `pruneStale(maxAge = 7 days)`: delete `*.json` other than `settings.json` whose mtime is older
  than `maxAge`; runs once per process, after the first successful `writeStatus`.
- Every status write failure is swallowed: status is cosmetic and must never affect a request.

### 8.2 Session status shape

```json
{ "tier": "opus", "prompt": "...", "model": "claude-opus-5-5", "confidence": 0.87,
  "metrics": { "taskComplexity": 0.5, "reasoningRequired": 0.4, "toolComplexity": 0.2, "contextSize": 0.01 },
  "reason": "jev", "jev": { "request": {}, "response": {} }, "effort": "high", "at": 1760000000000,
  "agents": { "<key>": { "label": "...", "main": true, "tier": "opus", "model": "...", "confidence": 0.87,
                         "effort": "high", "reason": "jev", "at": 1760000000000 } },
  "history": [ { "...decision...": "", "agent": { "key": "...", "label": "...", "main": true } } ],
  "manual": false }
```

- `writeDecision(sessionId, decision, agent)`: `history` = previous history plus the decision (with
  `agent: {key, label, main}` when given), last 20 kept. `agents` = previous agents with this
  agent merged (see below) when `agent` is given, else previous agents. Written object:
  `{...decision, agents (if any), history}`.
- `markManual(sessionId, model, agent)`: merge `{label, main, model, manual: true, at: now}` into
  agents; `manual` = true when no agent or the agent is main, else the previous `manual ?? false`;
  written object `{...previous, agents (if any), manual, at: now}`.
- Merge: `agents[key] = {...agents[key], ...entry}`; when more than 12 agents, keep the main and
  the 11 most recent non-main entries by `at`.
- `agentView(status, {freshMs = 90000, now})`: `main` = the agent with `main: true`, else null;
  `subagents` = non-main agents with `now - (at ?? 0) <= freshMs`, newest first.
- `mainDecision(status)`: the newest history entry whose `agent.main` is true, else `status`.
- `readStatus(id)`: parsed file or null.

### 8.3 Calibration file

`<dir>/calibration.json`: `{ "newer": [ids], "models": [ids], "at": ms }`. `readCalibration`
returns `{newer: [], models: [], at: null}` when missing or unreadable; `models` and `at` are only
trusted when `models` is an array and `at` a number (else `[]` and null); `newer` is `[]` unless
an array.

### 8.4 Dump

`dumpBody(body, setting = JEV_DUMP)`: unset does nothing. `1`, `true`, `yes` (case-insensitive)
mean prefix `<dir>/dump`; any other value is the prefix itself. File
`<prefix>.<ms>-<counter>.json`, counter per process from 0, mode 0600, `stringify(body, 2)`.

---

## 9. Environment and settings

### 9.1 `loadEnv({cwd, home, env})` (`node/src/env.mjs`)

Sources in priority order; earlier wins and an existing environment variable beats all files:

1. `<cwd>/.env`, only keys `JEV_API_KEY`, `TYPESAFE_API_KEY`, `JEV_DEBUG`, `JEV_ALLOW_FABLE`,
   `JEV_NO_STATUSLINE`, `JEV_ICONS`, and keys matching `^JEV_(?:[A-Z]+_)?(?:FORCE_)?EFFORT$`;
2. `<home>/.jev-router.env`, all keys;
3. `<home>/.jev-claude.env`, all keys.

A key is set only when not already present and its value is non-empty. Missing or unreadable files
are skipped. Parsing must match Node's `util.parseEnv` (dotenv syntax: `KEY=value`, optional
`export `, `#` comments, blank lines, single/double/backtick quotes, `\n` expansion inside double
quotes, inline `#` comments after unquoted values, multi-line quoted values). Conformance case
`parse-env` (16.2) pins the cases that matter.

`childEnv(env)`: a copy without `JEV_API_KEY` and `TYPESAFE_API_KEY`.

### 9.2 Saved model (`node/src/settings.mjs`)

- `USER_SETTINGS = <home>/.claude/settings.json`; memo `<status dir>/saved-model.json`.
- `readSavedModel(file, memo)`: read `model` from the settings file (undefined when the file is
  unreadable). If it is the sentinel, return the memo's `model`. Otherwise write the memo
  `{"model": <model or null>}` (mode 0600, ensuring the status dir when the memo is the default)
  and return the model.
- `restoreSavedModel(previous, file)`: only when the file's `model` is exactly the sentinel: set it
  to `previous`, or delete `model` when `previous` is null/undefined, and write
  `stringify(settings, 2) + "\n"`. Returns whether it wrote.

---

## 10. Launcher (`jev-claude`)

In order, matching `node/bin/jev-claude.mjs`:

1. `savedModelBefore = readSavedModel()`.
2. `loadEnv()`.
3. `args = argv[1:] + ["--add-dir", <repo root>]` (omit the pair when there is no root, 2.4).
   `env = childEnv()`.
4. Resolve `claude` on PATH (10.2). If absent, print the three-line `[jev]` message from Node to
   stderr and exit 1.
5. First-run offer (11) when `shouldOffer({args: original argv, interactive: stdin and stdout are
   TTYs, offered: wasOffered(), shadowed: shadowsSkill(cwd, root)})`. Ask the exact two-line
   question; `interrupt` exits 130; a non-null answer is recorded; `true` prepends
   `"/jev-calibrate check"` to `args`.
6. With `JEV_API_KEY` or `TYPESAFE_API_KEY`: start the proxy with `upstreamURL` =
   `ANTHROPIC_BASE_URL` when set (else default), set in the child env
   `ANTHROPIC_BASE_URL=http://127.0.0.1:<port>`, `CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1`,
   `ANTHROPIC_CUSTOM_MODEL_OPTION=jev-router`, `ANTHROPIC_CUSTOM_MODEL_OPTION_NAME=Jev Router`,
   `ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION=Route each turn to the cheapest model that can do it`,
   `ANTHROPIC_CUSTOM_MODEL_OPTION_SUPPORTED_CAPABILITIES=thinking,adaptive_thinking,interleaved_thinking,effort,max_effort`,
   `CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT=1`, and `ANTHROPIC_MODEL=jev-router` only
   when the parent has no `ANTHROPIC_MODEL`. On exit: close the proxy and
   `restoreSavedModel(savedModelBefore)`. Append the status-line args (10.1). Under `JEV_DEBUG`
   with a TTY stdout, print `[jev] routing decisions -> <log file>`; under `JEV_DEBUG` with an
   inherited base URL print `[jev] upstream <url>`.
   Without a key: print the two-line "no JEV_API_KEY found" message and launch without the proxy.
7. Spawn `claude` (10.2) with inherited stdio and `env`. Exit with its exit code (1 when killed by
   a signal or when spawning fails, after printing `[jev] could not start Claude Code: <msg>`).
8. Signals: ignore SIGINT (Claude Code decides); on SIGHUP/SIGTERM forward the signal to the child
   and exit 1 after 5 s if the child has not exited. Cleanup (proxy close, settings restore) must
   run on every exit path including these. On Windows, console control events map to the same
   intent.

### 10.1 Status line args

None when `JEV_NO_STATUSLINE` is set, or when `<cwd>/.claude/settings.json` or
`<home>/.claude/settings.json` parses and has a truthy `statusLine`. Otherwise write
`{"statusLine":{"type":"command","command":"<quoted path to this port's jev-statusline>"}}` to the
status dir's `settings.json` via the private writer and pass `--settings <that file>`; a write
failure means no args. For Node the command is `"<node>" "<script>"`; compiled ports use
`"<jev-statusline executable>"`; Python uses `"<sys.executable>" -m jev_router.cli.statusline` or
its installed script.

### 10.2 Command resolution and launch (`node/src/launch.mjs`)

- `resolveCommand(name, {exts, path, win})`: on Windows try each PATH entry (surrounding quotes
  stripped, empty entries skipped) with each `PATHEXT` suffix (default `.COM;.EXE;.BAT;.CMD`) and
  accept an existing file; elsewhere accept an executable file named exactly `name`.
- `launchSpec(file)`: `.ps1` runs `powershell.exe -NoProfile -ExecutionPolicy Bypass -File
  <file>`; `.cmd`/`.bat` runs the npm shim's target script with Node when `shimScript` finds one
  (regex `"%~?dp0%?\\([^"]+?\.[cm]?js)"`, case-insensitive), else runs through `cmd.exe /d /s /c`
  with the verbatim line built by `quoteForCmd`; anything else runs directly. Never through an
  implicit shell. The Node interpreter for a shim is `node` resolved on PATH (Node itself uses its
  own executable).
- `quoteForCmd(arg)`: MSVCRT quoting (double backslashes before a quote and at the end, escape the
  quote), wrap in quotes, then caret-escape `( ) [ ] % ! ^ " ` < > & | ; , space * ?` twice.

---

## 11. First run (`node/src/first-run.mjs`)

- Marker `<home>/.jev-router/first-run.json`, `{ "offeredAt": ISO, "accepted": bool }`;
  `wasOffered` is whether the file is readable.
- `shouldOffer` = interactive and not offered and not shadowed and no arguments.
- `shadowsSkill(cwd, root)` = `cwd` and `root` differ (resolved) and
  `<cwd>/.claude/skills/jev-calibrate` exists.
- `askYesNo(question)`: prompt on stderr, read one line from stdin. Empty or anything but
  `n`/`no` (case-insensitive, trimmed) is yes. Ctrl+C is `interrupt`. Closed or failing input is
  null. It must always settle.

---

## 12. Status line (`jev-statusline`)

Reads all of stdin as JSON (malformed or empty input is `{}`) and prints one line. The exact
composition, colours, ANSI codes, separators, and fallbacks are those of
`node/bin/jev-statusline.mjs`; ports must produce byte-identical output for the conformance
cases. Summary:

- Routed part: `⏸ manual <display_name or main model>` (dim marker) when the main agent or the
  session is manual; else the main agent's line, else the flat status's line (pre-agent sessions),
  else `jev: waiting for first prompt` (dim).
- Main line: model icon, tier colour, `shortName(model) ?? model ?? tier`, ` (NN%)` dim when
  confidence is set, ` · <effort icon> <effort>` when effort is set, ` (<shortReason>)` dim when
  non-null.
- Sub-agents: up to three fresh sub-agents' short names in tier colours (manual ones prefixed
  `⏸`), `+N` dim for the rest, joined with a dim comma, after the agents icon.
- Then directory (last path segment of `workspace.current_dir ?? cwd`, omitted when equal to the
  worktree name), branch (clipped at 28 units with `…`, `(detached)` for ""), worktree, context
  `NN%` (`round(context_window.used_percentage ?? 0)`), and the calibration notice
  `new <first>[ +N]: /jev-calibrate` in yellow when `newer` is non-empty.

### 12.1 Icons (`node/src/icons.mjs`)

`JEV_ICONS=symbols` forces glyphs; `text` or `ascii` forces words; otherwise words only on Windows
when none of `WT_SESSION`, `TERM_PROGRAM`, `ConEmuPID` is set. The glyph and word tables are
copied from Node exactly.

### 12.2 Location (`node/src/worktree.mjs`)

`gitBranch(dir)`: `git branch --show-current` in `dir`, 1 s timeout, stderr discarded; trimmed
stdout, or null on failure or when `dir` is empty. `locationInfo(input, branchOf)`:
`worktree = input.worktree.name ?? input.workspace.git_worktree ?? null`;
`dir = workspace.current_dir ?? cwd ?? worktree.path`;
`branch = worktree.branch ?? branchOf(dir)`; null when both are null.

### 12.3 Short names (`node/src/model-names.mjs`)

`shortName(model)`: first match of `claude-([a-z]+)-(\d+)(?:-(\d{1,2})(?!\d))?` anywhere in the
string; `<Family capitalised> <major>[.<minor>]`; null when no match.

### 12.4 Reasons (`node/src/reasons.mjs`)

The table of `match`, `short`, `long` is copied exactly, checked in order with substring matching.
`shortReason` returns the short text or null; `longReason` the long text or
`"the router's recommendation"`; `isNoChange` tests for `no-change`.

---

## 13. Explain, legend, check

- `jev-explain <sessionId>`: `status = readStatus(id)`, `main = mainDecision(status)`; prints
  `formatExplanation(main && status.manual ? {...main, manual: true} : main)`, a newline, then
  `formatAgents(status)` and a newline when non-empty. Box drawing, widths (33 and 52), wrapping,
  and wording exactly as `node/src/explain.mjs`. `recommendationOf` reads
  `jev.response.answers.model.choice` then `answers.model_tier.choice`.
- `jev-legend`: `Status line key\n\n<formatLegend(icons())>\n`, alignment by code points.
- `jev-check`: the read-only report of `node/bin/jev-check.mjs`, same rows and wording. `Mode` is
  `repository` when the repository root has `.git` and the port's own calibration tooling exists;
  ports have none, so they report `installed` unless the root has `.git` and
  `node/scripts/calibrate.mjs` exists. The `as of` timestamp uses the platform's local date-time
  format and is excluded from byte comparison.

---

## 14. Update library (`node/src/update.mjs`)

Ports implement, with the same results: `UPDATE_FILE = <home>/.jev-router/update.json`,
`CHECK_EVERY_MS = 6 h`, `readState`, `writeState` (temp file + rename), `isCheckDue`,
`compareVersions`, `updateNotice` (exact wording), `installedVersion(root)` (root
`package.json`), `inspectClone`, `checkForUpdate`, `needsInstall`, `applyUpdate(root, {install})`.
git is run as `git -C <root> ...` with `GIT_TERMINAL_PROMPT=0`, 5 s timeout locally and 20 s for
fetch and merge, no window on Windows. `jev-update-check` runs
`writeState(checkForUpdate(<repo root>))`.

---

## 15. Logging (`node/src/log.mjs`)

`LOG_FILE = <home>/.jev-claude.log`. `log(line)` writes `[jev] <line>\n` to stderr when stdout is
not a TTY; otherwise appends `<ISO time> [jev] <line>\n` to the log file, created 0600 and chmod'ed
to 0600 once per process; failures are swallowed. `debug(line)` logs only when `JEV_DEBUG` is set.
Note: in the launcher, the proxy runs in the launcher's process, whose stdout is the terminal.

---

## 16. Conformance

A port is done when all three layers pass on Windows (the development machine). macOS and Linux
are expected to work but are not gating.

### 16.1 Ported unit tests

Each port re-implements the behavioural cases of every file in `node/test/` in its
own test framework, one test file per Node test file, keeping each test's name as a comment or
the test name. Tests use real inputs and check real results: real files in temp directories, real
git repositories for the update tests, real HTTP servers on loopback for the proxy tests (a fake
Jev and a fake Anthropic upstream). Tests that only exercise a mock, or assert what they just set,
do not count. `bump-version.test.mjs` is out of scope (1.2). Tests that Node skips on Windows may
be skipped on Windows.

### 16.2 Golden cases

`conformance/cases/<name>.json` files, generated by `node conformance/generate.mjs` from the Node
functions themselves over real inputs (the fixtures, the calibration prompts, the inputs in
`node/test/`), each an array of `{ "name", "input", "expected" }`. Each port has one test that
loads every case file and checks its function against every case. The case set covers at least:
`detect-override`, `decide`, `new-turn-prompt`, `apply-tier`, `sanitize-schema`, `version-of`,
`claude-models`, `newest-per-tier`, `newer-than-calibrated`, `session-of`, `conversation-key`,
`agent-label`, `agent-of` (sequences sharing one `mains`), `effort-floor`, `forced-effort`,
`fable-allowed`, `short-name`, `reasons`, `format-explanation`, `format-agents`, `format-legend`,
`icons`, `location-info`, `compare-versions`, `update-notice`, `is-check-due`, `quote-for-cmd`,
`parse-env`, `stringify` (3.3, including the length used for context tokens), and `jev-request`
(the full Jev request body for given models, prompt, current model and context size). These files
are written by the lead before the ports start; ports never edit them.

### 16.3 Black-box harness

`conformance/harness/*.test.mjs` (Node, `node:test`) starts an implementation's `jev-proxy-host`
and status line as child processes, selected by `JEV_IMPL_CMD_PROXY` and
`JEV_IMPL_CMD_STATUSLINE` (command lines), and drives them with a fake Jev server
(`TYPESAFE_BASE_URL`) and a fake Anthropic upstream (`ANTHROPIC_BASE_URL`), with `JEV_STATUS_DIR`
and `HOME`/`USERPROFILE` pointed at temp directories. `jev-proxy-host` must: call `loadEnv`, start
the proxy with `upstreamURL` = `ANTHROPIC_BASE_URL` when set, print `PORT=<port>` and a newline on
stdout, flush, and run until killed. The harness checks forwarded bodies and headers, the Jev
request, streaming pass-through, the `/v1/models` handling and calibration file, Esc (client
abort) reaching upstream, the 502 path, status files, and status-line output for the same session.
It runs against Node first; it is the shared acceptance test for all ports.

### 16.4 Run commands

From the repository root:

```
pnpm test                                               # Node unit tests
node --test conformance/harness                         # harness against Node (default commands)
cd go && go vet ./... && go test ./...
cd rust && cargo clippy -- -D warnings && cargo test
python -m unittest discover -s python/tests -t python
```

and the harness against each port, with the commands documented in that port's README.

---

## 17. Plan

1. **Lead (before the ports):** write `conformance/generate.mjs` and the case files (16.2),
   `conformance/harness` (16.3), and make `node/scripts/proxy-host.mjs` honour
   `ANTHROPIC_BASE_URL`. All pass against Node. Commit.
2. **Ports, in parallel:** one agent per language, each in its own git worktree, writing only
   inside its language directory. Each implements sections 3-15, passes 16.1 and 16.2, then 16.3
   with its own commands, and writes `<lang>/README.md` (build, test, harness commands, any
   divergence with its reason). An agent that finds a defect in this document stops that part and
   reports it rather than choosing a behaviour.
3. **Lead (after):** review each port, run every layer for every implementation, merge, update the
   root README and `doc/` (`doc/LAYOUT.md`, a per-language status table), and record in this
   document any defect found.

---

## 18. Known Node behaviours to preserve

These look odd but are deliberate or load-bearing; ports keep them.

- A sub-agent hand-back that quotes `"use strong"` is not an override (quotes are stripped).
- Requests with no tools (Claude Code's title and summary calls) are never routed and never
  claim the main-thread slot.
- Hook output arrives as a trailing `system` message and is skipped when finding the user's turn.
- An answer naming a model not on the menu counts as no answer.
- A conversation the proxy has not routed yet passes `contextTokens = 0` to policy.
- `claude -p` may omit metadata on its first request; the fallback key carries state over.
- The decision is filed under the conversation key when there is no session id.
- `<jev-explain>` turns are never routed and never recorded.
