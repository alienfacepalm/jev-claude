# jev-claude

Per-turn model routing for Claude Code. For each new prompt, Jev picks the Claude model that gets
the best result for the least money: quality first, then cost, speed last. Claude Code itself is
unchanged: its interface, tools, sessions, permissions, and sign-in all work as before.

This is a fork of [jev-router](https://github.com/gargpratyush/jev-router), focused on Claude Code.
The original also routed OpenAI Codex; that code is still here but is not maintained or tested in
this fork (see [OpenAI Codex](#openai-codex)).

## Install

You need [Node.js](https://nodejs.org) 20.12 or later, [git](https://git-scm.com/downloads), and
[Claude Code](https://code.claude.com/docs/en/setup) signed in with your usual account (no
Anthropic API key is needed). You also need a Jev API key: sign in at
[console.typesafe.ai](https://console.typesafe.ai) and create one on the
[API keys page](https://console.typesafe.ai/keys). The installer asks for it.

Run the line for your system:

| System | Command |
| --- | --- |
| macOS, Linux, WSL | `curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh \| bash` |
| Windows, PowerShell | `irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 \| iex` |
| Windows, Command Prompt | `powershell -NoProfile -Command "irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 \| iex"` |
| Windows, Git Bash | the macOS / Linux line |

The installer:

1. checks for Node.js and Claude Code, and installs [pnpm](https://pnpm.io) if it is missing;
2. downloads jev-claude to `~/jev-claude` (or updates it there);
3. installs the `jev-claude` command;
4. asks for your Jev key and saves it to `~/.jev-router.env`, readable only by you.

Run it again at any time to update. To install somewhere else, set `JEV_CLAUDE_DIR` first; to skip
the key prompt, set `JEV_API_KEY`. From a clone, run `./install.sh`, or `.\install.ps1` in
PowerShell (`powershell -ExecutionPolicy Bypass -File .\install.ps1` if scripts are blocked).

Then start it from any project:

```bash
jev-claude
```

The `jev-router` package on npm is the original project and does not include this fork's changes.

### Installing by hand

```bash
git clone https://github.com/alienfacepalm/jev-claude.git
cd jev-claude
pnpm install
pnpm add --global "link:$(pwd)"          # Git Bash: "link:$(pwd -W)"; PowerShell: "link:$PWD"
cp .env.example ~/.jev-router.env        # PowerShell: Copy-Item .env.example "$HOME\.jev-router.env"
```

Then open `~/.jev-router.env` and paste your key after `JEV_API_KEY=`.
[`.env.example`](.env.example) explains each setting. To use a key for one project only, copy it
to `.env` in that project instead. (`pnpm link --global`, used by older guides, does not exist in
current pnpm.)

## Usage

The first time you start `jev-claude` with no arguments, it asks once whether to run a setup
check (see [Setup check](#setup-check)). Every Claude Code argument is passed through:

```bash
jev-claude --resume
jev-claude -p "fix the failing test"
```

## How a model is chosen

`jev-claude` adds **Jev Router** to Claude Code's `/model` picker and starts on it. Picking another
model pauses routing; picking **Jev Router** again resumes it.

For each new prompt, Jev scores the work and picks a tier:

| Tier | Model | Best for | Cost per completed task |
| --- | --- | --- | --- |
| Haiku | Claude Haiku 4.5 | Trivial, mechanical edits | Cheapest; does no reasoning |
| Sonnet | Claude Sonnet 5.5 | Well-scoped changes, bugs with a known cause, tests, documents and analysis | About 40-60% less than Opus |
| Opus | Claude Opus 5.5 | Open-ended or multi-step coding, unknown-cause bugs, security, migrations, judgement calls | About 1.6-2.6x Sonnet |
| Fable | Claude Fable 5.1 | Long-horizon autonomous work, problems a strong model has already failed at, adversarial reviews of plans | Most expensive ($10 / $50 per million tokens) |

The costs are per completed task, not per token: a model that costs twice as much per token can
use fewer tokens. They come from Anthropic's and Artificial Analysis's published measurements
(September 2026), noted with their sources in [`src/config.mjs`](src/config.mjs).

Each tier always uses the newest version your account offers, read from Claude Code's model list,
so a new release such as Opus 6 is used as soon as your account has it.

Then these rules apply ([`src/policy.mjs`](src/policy.mjs)):

- **You can ask.** A prompt such as `use opus` or `switch to the strong model` wins. A tier word in
  ordinary prose ("replace this with long polling") is not a request.
- **Unsure answers step down one tier.** When Jev is less than 60% sure, the turn runs one tier
  below its pick (an unsure Fable pick runs on Opus), never below Sonnet or the model already in
  use. Jev is told to pick the stronger model when it is unsure a cheaper one would succeed, since
  a failed turn is paid for twice.
- **New conversations start on Sonnet**, and stay on the current model if Jev cannot be reached.
  Routing never blocks Claude Code.
- **Large conversations keep their model** when switching down would cost more in prompt-cache
  rebuilding than it saves.
- **Unavailable tiers step up**, never silently down, and never up into Fable.
- **Fable is offered by default** but only reached by a confident pick or by asking. It bills extra
  usage credits; set `JEV_ALLOW_FABLE=0` to turn it off.

A turn keeps its model through all of its tool calls. The main conversation and each sub-agent are
routed separately.

**Effort.** Claude Code sends its own reasoning effort (`high` by default) with every request, and
the router keeps it. The per-tier efforts in [Configuration](#configuration) apply only to requests
that name none.

## Status line

`jev-claude` adds a status line showing the model the last turn ran on, Jev's confidence, and the
reason when it was not simply Jev's pick. Sub-agents follow `⤷`, with their model and version:

```text
claude-sonnet-5-5 (94%) · my-project · 8% context
claude-opus-5-5 (91%) (keeping the cache) · ⤷ Haiku 4.5,Sonnet 5.5 · my-project · 34% context
⏸ manual Opus 4.6 · my-project · 21% context
```

When your account offers a newer version of a model than the router was tuned for, the line ends
with a notice such as `new claude-opus-6: /jev-calibrate`. Routing already uses the new model; the
notice means the costs and guidance were measured on the previous one, and a jev-claude update
will bring tuning for it. Only new versions of known models are noticed, not new model names.

An existing custom `statusLine` in your Claude Code settings is kept. Set `JEV_NO_STATUSLINE=1` to
turn Jev's off.

> Choosing a model with `Enter` in `/model` can save it as Claude Code's default. `jev-claude`
> puts your previous default back when it exits, so `jev-router` never breaks plain `claude`. If a
> session is killed outright, the next `jev-claude` run restores it.

## Setup check

Run `/jev-calibrate` in a `jev-claude` session for a read-only report on your setup:

```text
Routing      on - Jev Router picks a model for each turn
Tuned for    haiku claude-haiku-4-5-20251001, sonnet claude-sonnet-5-5, opus claude-opus-5-5, fable claude-fable-5-1
Your account claude-haiku-4-5-20251001, claude-sonnet-5-5, claude-opus-5-5, claude-fable-5-1
Newer models none - the router is tuned for the newest models your account offers
Fable        offered when the work calls for it (bills extra usage credits; JEV_ALLOW_FABLE=0 turns it off)
```

It changes nothing. On the first plain launch, `jev-claude` offers to run it for you, once. The
answer is kept in `~/.jev-router/first-run.json`; delete that file to be asked again. The offer is
skipped in a project that defines its own `jev-calibrate` skill.

Run inside this repository itself, `/jev-calibrate` goes further and re-tunes the router; see
[Calibrating for new models](#calibrating-for-new-models).

## Why a model was chosen

Run `/jev-explain` to see the factors behind the last routing decision:

```text
┌─────────────────────────────────┐
│ Jev Router                      │
│                                 │
│ Jev request                     │
│ Prompt: explain the router      │
│ Current tier: HAIKU             │
│ Context tokens: 6200            │
│                                 │
│ Jev response                    │
│ Task complexity     0.82        │
│ Reasoning required  0.91        │
│ Tool complexity     0.64        │
│ Context size        0.31        │
│                                 │
│ Recommended tier: SONNET        │
│ Selected model: SONNET          │
│                                 │
│ Confidence: 94%                 │
│ Decision: Jev recommendation    │
└─────────────────────────────────┘
```

The report is built from the exact prompt, Jev request, and Jev response saved when the turn was
routed; it does not ask Jev again. Up to 20 recent decisions are kept per session in your
operating system's temporary directory:

| Platform | Location |
| --- | --- |
| Windows | `%TEMP%\jev-claude\<session-id>.json` |
| macOS | `$TMPDIR/jev-claude/<session-id>.json` |
| Linux | `${TMPDIR:-/tmp}/jev-claude/<session-id>.json` |

These files hold prompt text, so only you can read them (directory mode 700, files 600). Files
untouched for 7 days are deleted.

## Configuration

Settings go in `~/.jev-router.env` (see [`.env.example`](.env.example)), in a project's `.env`, or
in your shell's environment.

| Variable | Effect |
| --- | --- |
| `JEV_API_KEY` | Your Jev key; routing is off without it. `TYPESAFE_API_KEY` also works. |
| `JEV_ALLOW_FABLE` | Fable is offered by default; `0`, `false`, `no` or `off` turns it off. |
| `JEV_SONNET_EFFORT`, `JEV_OPUS_EFFORT`, `JEV_FABLE_EFFORT` | Effort (`low`, `medium`, `high`, `xhigh`, `max`) for requests that name none. Defaults: Sonnet `high`, Opus `medium`, Fable `high`. Claude Code normally sends its own. |
| `JEV_NO_STATUSLINE` | Turns off Jev's status line. |
| `JEV_DEBUG` | Logs routing decisions to `~/.jev-claude.log`. |
| `JEV_DUMP` | Saves whole request bodies for debugging. `1` writes them, owner-only, to the status directory; any other value is a path prefix. |

Your shell's environment wins, then a `.env` in the directory you start `jev-claude` from, then
`~/.jev-router.env`, then the older `~/.jev-claude.env`. A blank value is ignored, so a copied but
unfilled `.env.example` never hides your real key.

A project's `.env` may be someone else's file, so only the `JEV_*` settings in the table above are
read from it. Anything that could redirect traffic or run code, such as `ANTHROPIC_BASE_URL`,
`TYPESAFE_BASE_URL` or `NODE_OPTIONS`, must be set in your shell or in `~/.jev-router.env`. The
Jev key itself is removed from the environment Claude Code runs with.

## How it works

`jev-claude` starts a local proxy and launches the real Claude Code pointed at it. Claude Code's
own authorization is forwarded untouched; the proxy never reads or stores it.

```text
you -> Claude Code -> jev-claude proxy -> Anthropic
                         |
                         +-> Jev: choose a model
```

Only requests for **Jev Router** are routed; a model you pick yourself passes straight through.
Request fields the chosen model cannot accept, such as adaptive thinking on Haiku, are removed
before forwarding, and old MCP tool schemas that the API would reject are normalised.

## OpenAI Codex

The original project's `jev-codex` command and its Codex proxy are still in this repository, but
this fork is for Claude Code: Codex support is not maintained or tested here and may come back in
a later version. Its `JEV_CODEX_*_MODEL` settings are still read.

## Development

```bash
pnpm install
cp .env.example .env    # then paste your key

pnpm test
node scripts/live-routing.mjs
node bin/jev-claude.mjs -p "what is 2+2?"
```

`pnpm test` runs offline. `scripts/live-routing.mjs` asks the real Jev about a few sample
prompts.

### Calibrating for new models

`node scripts/calibrate.mjs --runs 2` asks the real Jev about the labelled prompts in
[`scripts/calibration-cases.mjs`](scripts/calibration-cases.mjs). It reports how often Jev picked
the intended tier, and where each turn finally ran (an unsure pick running one tier lower shows as
`step`). Run it before and after any change to the guidance, costs, effort, or thresholds in
`src/config.mjs`, and keep a change only when the pick score holds.

When a new Claude model ships, run `/jev-calibrate` in a `jev-claude` session started in this
repository. It checks Anthropic's model notes and published per-task measurements, updates
`src/config.mjs`, and measures each change with the script above, on a branch.

### Versions and commit messages

Every push to `master` runs [a GitHub Action](.github/workflows/version-bump.yml) that runs the
tests, bumps the version in `package.json`, commits it as `chore(release): vX.Y.Z`, and tags it.
Pull after pushing to pick up that commit. The size of the bump comes from the
[Conventional Commits](https://www.conventionalcommits.org) prefixes of the commits pushed:

| Commit subject | Bump |
| --- | --- |
| `feat: ...` | minor (0.5.0 to 0.6.0) |
| `fix: ...`, `docs: ...`, `chore: ...`, or no prefix | patch (0.5.0 to 0.5.1) |
| `feat!: ...`, or `BREAKING CHANGE:` in the message | major (0.5.0 to 1.0.0) |

## Limitations

- Your prompt text is sent to TypeSafe for the routing decision. Nothing else is.
- Jev adds a short delay to the first request of each turn; tool calls within a turn add none.
- Claude Code's request format is not a public contract. Use `JEV_DUMP` to diagnose changes.
- jev-claude and its installers are written for macOS, Linux (including WSL), and Windows. They
  have been tested on Windows 10 (PowerShell 7 and 5.1, Git Bash) with Claude Code 2.1.287;
  macOS and Linux run the same code but have not been tested yet.

## Contributing

Issues and pull requests are welcome. Use [Issues](https://github.com/alienfacepalm/jev-claude/issues)
to report bugs, request improvements, or ask questions. Include your Claude Code version,
reproduction steps, expected behaviour, and useful logs with secrets removed.

For a pull request:

1. Open an issue first; every pull request should link an approved issue that explains the
   problem and how the fix was validated.
2. Fork the repository and create a focused branch from `master`.
3. Make the smallest change that solves the problem, with commit subjects as described in
   [Versions and commit messages](#versions-and-commit-messages).
4. Run `pnpm test` and include tests for non-trivial behaviour changes.

Never commit API keys or other secrets: copy `.env.example` to `.env` (which git ignores) for local
settings. All contributions are reviewed, and only the repository owner merges pull requests.

## License

MIT. Originally created as [jev-router](https://github.com/gargpratyush/jev-router).
