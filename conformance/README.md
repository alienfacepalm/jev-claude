# Conformance data

The test data that defines what the programs must do. Behaviour is defined in
[`SPEC.md`](../SPEC.md); these files pin it down with real inputs. Nothing here belongs to the
implementation: the golden test reads the cases, and the black-box harness runs the built programs
as child processes.

| Path | What it is |
| --- | --- |
| `fixtures/claude-code-print-request.json` | A request captured from `claude -p`, used to test routing and request rewriting in the proxy |
| `fixtures/subagent-handback-prompt.txt` | A sub-agent's hand-back text, which must not be read as a user's model override |
| `fixtures/harness-expectations.json` | What the black-box harness expects for the exact inputs it sends: the two Jev menus, the Jev questions for each, the prompt, conversation key and label of each request, and the body the proxy should forward. Precomputed data, so the harness holds no routing logic of its own; see "Harness expectations" below |
| `reference/node_dotenv_parse_content.cc` | Node's env-file parser, vendored; the implementation follows it line for line (SPEC 9.1) |
| `cases/*.json` | Golden cases (SPEC 16.2): frozen fixtures that [`SPEC.md`](../SPEC.md) owns. `cases/README.md` documents the encoding, how to call each, and how to add or amend a case by hand |

The golden cases and the harness expectations were generated once from the Node implementation,
which has been retired. The generator and the implementation are kept in history at tag `v0.11.0`
(commit `5b51f14`); no generator remains in the tree.

## The black-box harness

The harness (SPEC 16.3) is a set of Rust integration tests in `rust/tests/`. They start the
`jev-proxy-host` and `jev-statusline` that Cargo builds for the test run
(`CARGO_BIN_EXE_jev-proxy-host`, `CARGO_BIN_EXE_jev-statusline`), so they always test the current
source, never a stale binary. Each proxy runs in a fresh temporary directory with `HOME`,
`USERPROFILE`, `TEMP`, `TMP`, `TMPDIR` and `JEV_STATUS_DIR` pointed inside it, every other `JEV_*`,
`TYPESAFE_*`, `ANTHROPIC_*` and `CLAUDE_*` variable removed, a fake Jev on loopback as
`TYPESAFE_BASE_URL`, and a fake Anthropic API on loopback as `ANTHROPIC_BASE_URL`. Nothing reaches
the network or the real home directory.

| File | What it checks |
| --- | --- |
| `rust/tests/conformance_routing.rs` | Routed, user-picked, auxiliary, continuation, sub-agent, off-menu, Jev-down, token-count and malformed turns: the Jev request, the forwarded request and headers, the status file, and the status line |
| `rust/tests/conformance_models.rs` | `GET /v1/models`: relayed for 200, 401 and broken catalogs, the account catalog as the Jev menu, and `calibration.json` |
| `rust/tests/conformance_transport.rs` | Streaming chunk by chunk, client and upstream aborts, the 502 path, `HEAD`, redirects, compressed bodies, an upstream base path |
| `rust/tests/conformance_expectations.rs` | No program: `fixtures/harness-expectations.json` against the golden cases (below) |
| `rust/tests/conformance/` | Shared machinery: the fake servers, the client, the expectation lookups, and the harness's own JSON, SHA-256 and HTTP code, so it shares nothing with the crate it tests |

Run it from `rust/`, all of it or one file:

```sh
cargo test --locked --test 'conformance*'
cargo test --locked --test conformance_routing
```

The harness needs only the Rust toolchain, and no network access. Every harness test has a 60 s
limit, and a request the program under test never answers fails after 20 s (the proxy also has
20 s to print its port, and the status line 20 s to finish), so a program that accepts a request
and hangs fails its test instead of hanging the run. CI ([`.github/workflows/ci.yml`](../.github/workflows/ci.yml))
runs it with the rest of `cargo test` on Windows, Ubuntu and macOS.

## Harness expectations

The harness never computes what a request should become. `fixtures/harness-expectations.json`
holds it, and `rust/tests/conformance/mod.rs` looks entries up by a SHA-256 of the input, with the
session id taken out (each test uses its own session id so that its status file is its own). The
file has:

| Key | Holds |
| --- | --- |
| `menus` | `static`: the menu with no account catalog; `catalog`: the one the test catalog in `conformance_models.rs` yields |
| `questions` | the Jev `questions` object for each menu, looked up by the menu itself |
| `turns` | per request (`sha256` of its text with the session id replaced by `@@SESSION@@`): `prompt`, `label`, and `keys`, the conversation key under each session id the tests use (the key hashes the session id, so it differs by session) |
| `forwarded` | per request and tier: `sha256` of `JSON.stringify([<request text with the session id replaced by @@SESSION@@>, tier, model-or-null])`, and `body`, the exact text the proxy forwards, with `@@SESSION@@` where the session id goes |

`conformance_expectations.rs` checks every entry against the golden cases, which are
authoritative, so a hand edit that disagrees with them fails there rather than as a confusing
routing failure. Most entries equal a golden case outright: the static menu, both Jev question
sets, the captured request's prompt, key and label, and its Haiku, Sonnet and Opus rewrites. The
rest are derived from golden cases by a rule the goldens themselves show, and the test checks the
rule on the goldens first:

- the conversation keys under the harness's session ids, by SPEC 7.5's formula (checked against the
  `conversation-key` cases);
- the sub-agent turn's prompt and label, the captured turn's golden values with the task swapped
  and the `agent-label` cut (checked against `exactly 48`, `exactly 49` and `80 x`); the user-picked
  turn, the captured turn's entry (the model is not part of a turn's prompt, key or label);
- the catalog menu's Sonnet description (the golden account menu's, without the input-tokens
  clause that this catalog's Sonnet lacks);
- the continuation and sub-agent bodies (the golden rewrite with their own messages), the auxiliary
  and throwing bodies (the golden `sonnet floor named` rule), and the user-picked body (unchanged,
  as `unknown tier untouched`).

It also anchors the two formulas the harness computes itself, the context tokens and the decision
metrics, to the golden `jev-request` case, and it fails when the file holds an entry none of these
checks reaches.

### Adding or amending an expectation

A harness test that sends an input the file does not know fails with a message saying which. To add
or change an entry by hand:

1. Work out the expected value from SPEC.md and the golden cases (`apply-tier`, `sanitize-schema`,
   `conversation-key`, `agent-label`, `new-turn-prompt`, `jev-request`), or from the reference at
   tag `v0.11.0` (`git worktree add ../jev-v0.11.0 v0.11.0`, then call `applyTier`,
   `sanitizeSchema`, `conversationKey` and the rest of the Node functions that tree still has).
   Never paste in what the implementation produced without checking it against one of those.
2. Compute the lookup hash the way the table above says, from the exact text the test sends
   (`sessionless` and `forwarded_sha` in `rust/tests/conformance/mod.rs` are the definitions), and
   write the entry. Keep the file's layout: two-space indent, UTF-8, LF, one trailing newline.
3. Add the entry's cross-check to `conformance_expectations.rs`: compare it with a golden case, or
   derive it from one by a rule the test first checks on the goldens. The last test there,
   `every_entry_in_the_file_has_a_cross_check`, lists the inputs it knows and fails until the new
   one is listed.
4. Run `cargo test --locked --test 'conformance*'` from `rust/`.

Tests read the files here by path from the repository root; never copy them into `rust/`, so a
change here reaches every test at once. Editing the root `.env.example`, the handback fixture,
`rust/examples/calibration-cases.json` or the captured request means editing the matching golden cases
by hand; `golden_inputs_still_match_their_sources` in `conformance_expectations.rs` fails until you do.
