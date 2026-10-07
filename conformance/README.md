# Conformance data

Inputs shared by every implementation's tests, so that Node.js, Go, Rust, and Python are checked
against the same real data. Behaviour is defined in [`SPEC.md`](../SPEC.md).

| Path | What it is |
| --- | --- |
| `fixtures/claude-code-print-request.json` | A request captured from `claude -p`, used to test routing and request rewriting in the proxy |
| `fixtures/subagent-handback-prompt.txt` | A sub-agent's hand-back text, which must not be read as a user's model override |
| `reference/node_dotenv_parse_content.cc` | Node's env-file parser, vendored; ports implement it line for line (SPEC 9.1) |
| `cases/*.json` | Golden cases (SPEC 16.2), generated from the Node functions; `cases/README.md` documents the encoding and how to call each |
| `generate.mjs`, `generator/` | The case generator: `node conformance/generate.mjs` rewrites `cases/`, deterministically. It runs only on Node 24.21.x, the reference runtime (SPEC 16.2), and exits with a message on any other version; `--force` re-baselines on another version on purpose. It stops instead of recording a status write that did not land |
| `harness/` | The black-box harness (SPEC 16.3): `node --test conformance/harness` (or `pnpm test` / `pnpm run test:conformance` at the root) runs it against Node; set `JEV_IMPL_CMD_PROXY` and `JEV_IMPL_CMD_STATUSLINE` to run it against a port |

Run `pnpm install` at the repository root first: the generator and the harness import the Node
implementation, which needs `node/node_modules`.

The harness runs each program with its working directory in a fresh temporary directory, so give
`JEV_IMPL_CMD_*` absolute paths (a word that names an existing path relative to the repository root
is made absolute for you). The command lines split on spaces; quote a word with `"` or `'`.

Every harness test has a 60 s limit, and a request the program under test never answers fails
after 20 s, so an implementation that accepts a request and hangs fails its tests instead of
hanging the run. The Node code here is linted and formatted with Biome (`pnpm lint`,
`pnpm run format:check`); `cases/`, `fixtures/` and `reference/` are excluded and stay
byte-exact. See [`doc/NODE.md`](../doc/NODE.md).

Tests read these files by a path relative to their own location; never copy them into a language
directory, so a change here reaches every implementation at once. The generator also reads the
root `.env.example` into a `parse-env` case, so editing that file means regenerating the cases.

CI ([`.github/workflows/ci.yml`](../.github/workflows/ci.yml)) regenerates the cases on Node 24.21
and fails if `conformance/cases` changes, and runs the harness against every implementation on
Windows and Ubuntu.
