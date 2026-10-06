# Conformance data

Inputs shared by every implementation's tests, so that Node.js, Go, Rust, and Python are checked
against the same real data. Behaviour is defined in [`SPEC.md`](../SPEC.md).

| Path | What it is |
| --- | --- |
| `fixtures/claude-code-print-request.json` | A request captured from `claude -p`, used to test routing and request rewriting in the proxy |
| `fixtures/subagent-handback-prompt.txt` | A sub-agent's hand-back text, which must not be read as a user's model override |

Tests read these files by a path relative to their own location; never copy them into a language
directory, so a change here reaches every implementation at once.
