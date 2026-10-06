# Conformance data

Inputs shared by every implementation's tests, so that Node.js, Go, Rust, and Python are checked
against the same real data. Behaviour is defined in [`SPEC.md`](../SPEC.md).

| Path | What it is |
| --- | --- |
| `fixtures/claude-code-print-request.json` | A request captured from `claude -p`, used to test routing and request rewriting in the proxy |
| `fixtures/subagent-handback-prompt.txt` | A sub-agent's hand-back text, which must not be read as a user's model override |
| `reference/node_dotenv_parse_content.cc` | Node's env-file parser, vendored; ports implement it line for line (SPEC 9.1) |
| `cases/*.json` | Golden cases (SPEC 16.2), generated from the Node functions; `cases/README.md` documents the encoding and how to call each |
| `generate.mjs`, `generator/` | The case generator: `node conformance/generate.mjs` rewrites `cases/`, deterministically |
| `harness/` | The black-box harness (SPEC 16.3): `node --test conformance/harness` runs it against Node; set `JEV_IMPL_CMD_PROXY` and `JEV_IMPL_CMD_STATUSLINE` to run it against a port |

Run `pnpm install` at the repository root first: the generator and the harness import the Node
implementation, which needs `node/node_modules`.

The harness runs each program with its working directory in a fresh temporary directory, so give
`JEV_IMPL_CMD_*` absolute paths (a word that names an existing path relative to the repository root
is made absolute for you). The command lines split on spaces; quote a word with `"` or `'`.

Tests read these files by a path relative to their own location; never copy them into a language
directory, so a change here reaches every implementation at once.
