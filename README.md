# jev-claude

> **jev-claude is a fork of [jev-router](https://github.com/gargpratyush/jev-router) by
> [@gargpratyush](https://github.com/gargpratyush).** The routing proxy, the Jev integration, the
> status line, and `/jev-explain` come from that project. This fork narrows it to Claude Code and
> re-tunes the routing; see [Changes from jev-router](#changes-from-jev-router).

Per-turn model routing for Claude Code. For each new prompt, Jev picks the Claude model that gets
the best result for the least money: quality first, then cost, speed last. Claude Code itself is
unchanged: its interface, tools, sessions, permissions, and sign-in all work as before.

## Install

You need [Node.js](https://nodejs.org) 22 or later (24 LTS recommended; see
[Installing Node.js](#installing-nodejs)), [git](https://git-scm.com/downloads), and
[Claude Code](https://code.claude.com/docs/en/setup), signed in with your Claude account or run
on an Anthropic API key (see [Using an Anthropic API key](#using-an-anthropic-api-key)). You also
need a Jev API key: sign in at
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
4. asks for your Jev key, and optionally an Anthropic API key, and saves them to
   `~/.jev-router.env`, readable only by you.

Run it again at any time to update. To install somewhere else, set `JEV_CLAUDE_DIR` first; to save
the keys without being asked, set `JEV_API_KEY` and `ANTHROPIC_API_KEY`. From a clone, run `./install.sh`, or `.\install.ps1` in
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

### Installing Node.js

jev-claude needs Node.js 22 or later: 22 is the oldest release line Node.js still supports
(until April 2027), and 24 is the current LTS. Check what you have with `node --version`.

A version manager is the easiest way to get it and switch later. Pick one:

**nvm** (macOS, Linux, WSL). Install it with the
[command in its README](https://github.com/nvm-sh/nvm#installing-and-updating), open a new
terminal, then:

```bash
nvm install 24
nvm alias default 24
```

**nvm-windows** (Windows; a separate project with similar commands). Run the installer from
[nvm-windows](https://github.com/coreybutler/nvm-windows/releases), open a new terminal, then:

```powershell
nvm install 24
nvm use 24
```

**fnm** (macOS, Linux, Windows). Install it with `brew install fnm`, `winget install Schniz.fnm`,
or the [script in its README](https://github.com/Schniz/fnm#installation), add its
[shell setup line](https://github.com/Schniz/fnm#shell-setup), then:

```bash
fnm install 24
fnm default 24
```

**Volta** (macOS, Linux, Windows). Install it with `curl https://get.volta.sh | bash`, or
`winget install Volta.Volta` on Windows, then `volta install node@24`.

Or install Node.js directly:

| System | Command |
| --- | --- |
| Windows | `winget install OpenJS.NodeJS.LTS`, or the installer from [nodejs.org](https://nodejs.org/en/download) |
| macOS | `brew install node`, or the installer from [nodejs.org](https://nodejs.org/en/download) |
| Linux | A version manager above. Distribution packages are often older than 22; check with `node --version` |

Open a new terminal after installing, so it finds the new `node`.

## Usage

The first time you start `jev-claude` with no arguments, it asks once whether to run a setup
check (see [Setup check](#setup-check)). Every Claude Code argument is passed through:

```bash
jev-claude --resume
jev-claude -p "fix the failing test"
```

### Using an Anthropic API key

By default Claude Code runs on your Claude subscription sign-in. To run it on an Anthropic API key
instead, billed per token to that key's account, create a key in the
[Claude Console](https://platform.claude.com) and add it to `~/.jev-router.env`:

```bash
ANTHROPIC_API_KEY=sk-ant-...
```

The installer offers to do this for you, and setting `ANTHROPIC_API_KEY` in your shell works too.
`jev-claude` passes the key to Claude Code and the proxy forwards it to Anthropic untouched, so
routing works the same either way; on an API key, it is what keeps the per-token bill down.

In an interactive session, Claude Code asks once whether to use the key and remembers the answer;
change it later with **Use custom API key** in `/config`. In print mode (`-p`) the key is always
used. `ANTHROPIC_AUTH_TOKEN`, as LLM gateways use, is forwarded the same way and takes precedence
over the key ([Claude Code authentication](https://code.claude.com/docs/en/authentication)).

A project's `.env` cannot set `ANTHROPIC_API_KEY`, so a cloned repository can never send your work
to someone else's account. `/jev-calibrate` shows which credentials a session runs on.

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

`jev-claude` adds a status line showing the model the last turn ran on, Jev's confidence, the
reasoning effort it ran at, and the reason when it was not simply Jev's pick. Haiku takes no
effort, so none is shown for it. Sub-agents follow `⤷ agents`, with their model and version. Every item is an icon, a dimmed
label and its value:

```text
🤖 model Sonnet 5.5 · 🎯 confidence 94% · 🧠 effort high · 📁 dir my-project · 📊 context 8%
🤖 model Opus 5.5 · 🎯 confidence 91% · 🧠 effort medium (keeping the cache) · ⤷ agents Haiku 4.5,Sonnet 5.5 · 📁 dir my-project · 📊 context 34%
⏸ manual Opus 4.6 · 📁 dir my-project · 📊 context 21%
```

In a git checkout the line also shows the branch after the directory, and in a worktree (a Claude
Code `--worktree` session, or any directory in a linked worktree from `git worktree add`) the
worktree's name too, the branch (🌿) and the worktree (🌳). `(detached)` stands in for the branch when
none is checked out, and a directory that is not a git checkout shows neither:

```text
🤖 model Sonnet 5.5 · 🎯 confidence 94% · 🧠 effort high · 📁 dir my-project · 🌿 branch fix/login · 🌳 worktree login-fix · 📊 context 8%
```

When your account offers a newer version of a model than the router was tuned for, the line ends
with a notice such as `new claude-opus-6: /jev-calibrate`. Routing already uses the new model; the
notice means the costs and guidance were measured on the previous one, and a jev-claude update
will bring tuning for it. Only new versions of known models are noticed, not new model names.

The icons are emoji, which Windows Terminal, the VS Code terminal and Git Bash draw. The legacy
Windows console (the old cmd.exe and PowerShell window) cannot, so there the line falls back to
the same line without the icons (`model Sonnet 5.5 · confidence 94% · effort high · dir my-project · …`). Set
`JEV_ICONS=emoji` or `text` to choose yourself.

An existing custom `statusLine` in your Claude Code settings is kept. Set `JEV_NO_STATUSLINE=1` to
turn Jev's off.

> Choosing a model with `Enter` in `/model` can save it as Claude Code's default. `jev-claude`
> puts your previous default back when it exits, so `jev-router` never breaks plain `claude`. If a
> session is killed outright, the next `jev-claude` run restores it.

## Setup check

Run `/jev-calibrate` in a `jev-claude` session for a read-only report on your setup:

```text
Routing      on - Jev Router picks a model for each turn
Claude       your Claude Code sign-in
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
| `ANTHROPIC_API_KEY` | Optional: run Claude Code on this Anthropic API key instead of your sign-in (see [Using an Anthropic API key](#using-an-anthropic-api-key)). Not read from a project's `.env`. |
| `JEV_ALLOW_FABLE` | Fable is offered by default; `0`, `false`, `no` or `off` turns it off. |
| `JEV_SONNET_EFFORT`, `JEV_OPUS_EFFORT`, `JEV_FABLE_EFFORT` | Effort (`low`, `medium`, `high`, `xhigh`, `max`) for requests that name none. Defaults: Sonnet `high`, Opus `medium`, Fable `high`. Claude Code normally sends its own. |
| `JEV_NO_STATUSLINE` | Turns off Jev's status line. |
| `JEV_ICONS` | `emoji` or `text`: the status line's icons. Emoji by default; on Windows, plain text in the legacy console, which cannot draw them. |
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
own authorization, a subscription sign-in or an API key, is forwarded untouched; the proxy never
reads or stores it.

```text
you -> Claude Code -> jev-claude proxy -> Anthropic
                         |
                         +-> Jev: choose a model
```

Only requests for **Jev Router** are routed; a model you pick yourself passes straight through.
Request fields the chosen model cannot accept, such as adaptive thinking on Haiku, are removed
before forwarding, and old MCP tool schemas that the API would reject are normalised.

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

## Changes from jev-router

jev-claude started as an import of [jev-router](https://github.com/gargpratyush/jev-router) by
[@gargpratyush](https://github.com/gargpratyush). What this fork changes:

- **Claude Code only.** The original's support for other coding CLIs is removed.
- **Routing re-tuned for value.** Jev is told each model's cost per completed task, from published
  benchmarks, instead of its per-token price, and to prefer the stronger model when unsure a
  cheaper one would succeed. Unsure answers run one tier below Jev's pick, new conversations
  start on Sonnet, and Fable is offered by default for long-horizon work and adversarial reviews.
- **Newest models automatically.** Each tier uses the newest version in your account, the status
  line flags models newer than the router's tuning, and sub-agents show their model's version.
- **Setup check and calibration.** `/jev-calibrate`, offered once on first launch, and the
  calibration script and cases for re-tuning when new models ship.
- **Easier setup.** One-line installers for macOS, Linux, and Windows, `.env.example`, and
  documented support for running on an Anthropic API key.
- **Hardening.** A project's `.env` can no longer redirect traffic or run code, private files are
  owner-only, prompt overrides need an explicit instruction, Esc stops generation upstream, and
  the saved Claude Code model is restored even when a session is killed.
- **Tooling.** pnpm instead of npm, and automatic version bumps on `master`.

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

MIT, see [LICENSE](LICENSE). jev-claude is a fork of
[jev-router](https://github.com/gargpratyush/jev-router) by
[@gargpratyush](https://github.com/gargpratyush), also MIT-licensed; its copyright notice is kept
in LICENSE.
