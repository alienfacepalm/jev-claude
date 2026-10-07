# The Node.js implementation

`node/` is the reference implementation of jev-claude: the launcher, the local routing proxy,
the status line, and the helper programs, in plain ES modules on Node.js. [`SPEC.md`](../SPEC.md)
describes its behaviour for the ports, and where the spec is silent, this code is the answer.
The golden cases in `conformance/cases/` are generated from it, and the black-box harness in
`conformance/harness/` runs against it by default.

It is also the implementation the installers (`install.sh`, `install.ps1`), the `shim/`, the
Claude Code plugin and the `/jev-*` skills set up and call. Its one runtime dependency is
[`@typesafe-ai/sdk`](https://www.npmjs.com/package/@typesafe-ai/sdk), the Jev client.

- [Programs](#programs)
- [Install the toolchain](#install-the-toolchain)
- [Build](#build)
- [Install the programs and put them on PATH](#install-the-programs-and-put-them-on-path)
- [Run jev-claude](#run-jev-claude)
- [Configuration](#configuration)
- [Updating](#updating)
- [Tests](#tests)
- [Lint and format](#lint-and-format)
- [Golden cases](#golden-cases)
- [Conformance harness](#conformance-harness)
- [What CI runs](#what-ci-runs)
- [Troubleshooting](#troubleshooting)
- [Known divergences and gaps](#known-divergences-and-gaps)
- [Source layout](#source-layout)

## Programs

Every program is a `.mjs` file run by `node`. Nothing is compiled.

| Program | File | What it does | SPEC |
| --- | --- | --- | --- |
| `jev-claude` | `node/bin/jev-claude.mjs` | The launcher. It loads your settings, starts the proxy in its own process, and runs the real `claude` through it | 10 |
| `jev-statusline` | `node/bin/jev-statusline.mjs` | The status line command Claude Code runs. The launcher points Claude Code at it by absolute path | 12 |
| `jev-explain <session id>` | `node/bin/jev-explain.mjs` | The explanation panel behind `/jev-explain` | 13 |
| `jev-legend` | `node/bin/jev-legend.mjs` | The status line key behind `/jev-legend` | 13 |
| `jev-check` | `node/bin/jev-check.mjs` | The read-only setup report behind `/jev-calibrate` | 13 |
| `jev-update-check` | `node/bin/jev-update-check.mjs` | One update check. It writes `~/.jev-router/update.json` (see [Updating](#updating)) | 14 |
| proxy host | `node/scripts/proxy-host.mjs` | Starts the proxy on its own, prints `PORT=<port>`, and runs until killed. The conformance harness uses it | 16.3 |

The root `package.json` links `jev-claude`, `jev-explain` and `jev-legend` onto `PATH`. The
others are run by path: by the launcher (the status line), by the skills (`jev-check`), or by
the harness (the proxy host).

The developer scripts in `node/scripts/` (`calibrate.mjs`, `live-routing.mjs`,
`bump-version.mjs`) call the real Jev or cut releases. They stay Node-only (SPEC 1.2).

## Install the toolchain

You need Node.js **22.16 or later on the 22 line, or 24 or later** (24 LTS recommended), pnpm,
and git. Node 22.0 to 22.15 and every 23.x are not supported: their `util.parseEnv`, which
reads `~/.jev-router.env`, disagrees with the parser SPEC 9.1 defines, so they would load
different settings than the ports do. `package.json` says so in `engines`, and
`node/test/env-cases.test.mjs` fails on those versions.

Windows (PowerShell):

```powershell
winget install OpenJS.NodeJS.LTS      # Node 24 LTS
winget install Git.Git
# Open a new terminal, then:
corepack enable pnpm                  # or: iwr https://get.pnpm.io/install.ps1 -useb | iex
node --version; pnpm --version
```

macOS:

```bash
brew install node git                 # or: nvm install 24 && nvm alias default 24
corepack enable pnpm                  # or: curl -fsSL https://get.pnpm.io/install.sh | sh -
node --version && pnpm --version
```

Linux: distribution packages are often older than 22.16, so use a version manager:

```bash
# nvm: https://github.com/nvm-sh/nvm#installing-and-updating, then in a new terminal:
nvm install 24 && nvm alias default 24
corepack enable pnpm                  # or: curl -fsSL https://get.pnpm.io/install.sh | sh -
node --version && pnpm --version
```

nvm-windows, fnm and Volta work as well; the root
[README's Installing Node.js](../README.md#installing-nodejs) section lists the commands for
each. The repository pins its pnpm version in `packageManager`, and Corepack fetches that
version on first use. Use pnpm for everything here, never npm or npx.

## Build

There is nothing to compile. Install the dependencies once, from the repository root:

```bash
pnpm install --frozen-lockfile
```

The root is a pnpm workspace whose one package is `node/` (`@jev-router/node`). This installs the
Jev SDK into `node/node_modules` and the development tools (Biome) into the root
`node_modules`. `--frozen-lockfile` refuses to touch `pnpm-lock.yaml`, which is what CI and the
installers use; drop it only when you are changing dependencies on purpose.

## Install the programs and put them on PATH

### Option A: the installer (recommended)

The installer checks Node and Claude Code, clones to `~/jev-claude` (or uses the clone you run
it from), installs the dependencies, links the commands onto `PATH` and asks for your Jev key:

| System | Command |
| --- | --- |
| macOS, Linux, WSL, Git Bash | `curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh \| bash` |
| Windows, PowerShell | `irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 \| iex` |

From a clone, run `./install.sh`, or `.\install.ps1` in PowerShell
(`powershell -ExecutionPolicy Bypass -File .\install.ps1` if scripts are blocked).

### Option B: by hand, from a clone

```bash
git clone https://github.com/alienfacepalm/jev-claude.git
cd jev-claude
pnpm install --frozen-lockfile
pnpm add --global "link:$(pwd)"          # Git Bash: "link:$(pwd -W)"
cp .env.example ~/.jev-router.env        # then paste your key after JEV_API_KEY=
```

PowerShell:

```powershell
git clone https://github.com/alienfacepalm/jev-claude.git
Set-Location jev-claude
pnpm install --frozen-lockfile
pnpm add --global "link:$PWD"
Copy-Item .env.example "$HOME\.jev-router.env"
```

`pnpm add --global link:` puts `jev-claude`, `jev-explain` and `jev-legend` into pnpm's global
bin directory. If pnpm says it has no global bin directory, run `pnpm setup`, open a new
terminal, and run the `pnpm add --global` line again. Check with `command -v jev-claude` (Bash)
or `Get-Command jev-claude` (PowerShell). `pnpm link --global`, used by older guides, does not
exist in current pnpm.

The link points at the clone, so the clone must stay where it is: moving or deleting it breaks
the commands. An install made before the code moved into `node/` must be linked again (re-run
the installer or the `pnpm add --global` line): a plain `git pull` leaves the old command
pointing at `bin/jev-claude.mjs`, which no longer exists.

Without linking anything, run the launcher by path:

```bash
node node/bin/jev-claude.mjs -p "what is 2+2?"
```

### Next to the Go, Rust and Python builds

The ports build programs with the same names. Whichever directory comes first on `PATH` wins;
see which with `command -v jev-claude` (Bash) or `Get-Command jev-claude -All` (PowerShell). The
installer sets up this implementation, so it is the default; call a port by its path, or give it
a short name, as each port's guide (`doc/GO.md`, `doc/RUST.md`) describes. Every implementation
shares the same files: settings, the status directory, the decision log and the update state,
so you can switch between them freely.

## Run jev-claude

Give `jev-claude` the same arguments you would give `claude`:

```bash
jev-claude                         # interactive session
jev-claude -p "what is 2+2?"       # one-shot
jev-claude --resume                # any claude flag passes through
```

What it needs:

- **The real Claude Code CLI on `PATH`** as `claude`. On Windows the launcher finds npm's
  `claude.cmd` shim and runs the script behind it with its own `node`, so arguments are never
  re-parsed by `cmd.exe`.
- **A Jev API key** in `~/.jev-router.env` (see [Configuration](#configuration)). Without one,
  Claude Code starts without routing and the launcher says how to turn it on.

On the first plain interactive run it offers once to check your setup with `/jev-calibrate`.
Then it starts the proxy on a loopback port inside the launcher process and points Claude Code
at it with `ANTHROPIC_BASE_URL`, adds **Jev Router** to `/model`, installs its status line
(unless you have your own or set `JEV_NO_STATUSLINE`), passes `--add-dir <clone>` so the
`/jev-*` skills are available, and exits with Claude Code's exit code. On the way out it puts back
the default model Claude Code had saved before the session.

If `ANTHROPIC_BASE_URL` is already set, the proxy forwards to it instead of
`https://api.anthropic.com`. It must be an absolute `http://` or `https://` URL: anything else
stops the launch before Claude Code starts, with
`[jev] invalid upstream URL (ANTHROPIC_BASE_URL): <value>` and exit code 1.

## Configuration

Settings come from four places. A variable already set in the environment always wins:

| Source | What it may set |
| --- | --- |
| The process environment | Anything |
| `.env` in the directory you start `jev-claude` from | Only `JEV_API_KEY`, `TYPESAFE_API_KEY`, `JEV_DEBUG`, `JEV_ALLOW_FABLE`, `JEV_NO_STATUSLINE`, `JEV_ICONS`, and the `JEV_*_EFFORT` / `JEV_*_FORCE_EFFORT` keys, so a cloned repository can never redirect your traffic |
| `~/.jev-router.env` | Anything; this is your own settings file |
| `~/.jev-claude.env` | Anything; the older name, read last |

A blank value is skipped, so a copied but unfilled `.env.example` never hides a real key. The
files are parsed with Node's own `util.parseEnv` (SPEC 9.1).

| Variable | Effect |
| --- | --- |
| `JEV_API_KEY` (or `TYPESAFE_API_KEY`) | Turns routing on. It is sent only to Jev and removed from Claude Code's environment |
| `ANTHROPIC_API_KEY` | Runs Claude Code on an API key instead of your sign-in. Set it only in your own file or shell |
| `JEV_ALLOW_FABLE` | `0`, `false`, `no` or `off` stops routing to Fable |
| `JEV_SONNET_EFFORT`, `JEV_OPUS_EFFORT`, `JEV_FABLE_EFFORT` | The effort a tier gets when Claude Code sends none (`low`, `medium`, `high`, `xhigh`, `max`) |
| `JEV_FORCE_EFFORT`, `JEV_<TIER>_FORCE_EFFORT` | An effort that replaces the one Claude Code sends; the per-tier key wins |
| `JEV_DEBUG` | Logs each routing decision: to `~/.jev-claude.log` (owner-only) when stdout is a terminal, so Claude Code's screen is not overwritten, otherwise to stderr |
| `JEV_NO_STATUSLINE` | Leaves Claude Code's status line alone |
| `JEV_ICONS` | `symbols` or `text` for the status line labels; symbols by default, words in the legacy Windows console |
| `ANTHROPIC_BASE_URL` | When set before launch, the proxy forwards to it. Must be an `http(s)://` URL |
| `ANTHROPIC_MODEL` | Starts the session on that model instead of Jev Router |
| `TYPESAFE_BASE_URL`, `TYPESAFE_DEFAULT_MODEL` | Point the router at another Jev endpoint or model |
| `JEV_STATUS_DIR` | The status directory; the default is `<temp>/jev-claude` |
| `JEV_DUMP` | `1` saves each request body, owner-only, into the status directory; any other value is a path prefix. For debugging only: bodies hold your conversation |

The root [README's Configuration section](../README.md#configuration) explains each setting in
more depth. Status files are written to a temporary name and renamed into place; on Windows the
rename retries for up to about 200 ms while another process (antivirus, the search indexer) holds
the file, and the temporary file is removed if it still fails.

## Updating

Re-run the installer (Option A), or update the clone and install again:

```bash
git -C ~/jev-claude pull --ff-only
cd ~/jev-claude && pnpm install --frozen-lockfile
```

The update library (`node/src/update.mjs`) and `jev-update-check` exist and are tested, but the
launcher does not run the check, print an update notice, or handle `--update` yet (SPEC 1.2).
`jev-claude --update` today is passed through to Claude Code, which runs its own updater and
leaves jev-claude as it was. [UPDATES.md](UPDATES.md) describes the planned behaviour.

## Tests

From the repository root:

```bash
pnpm test                    # unit tests, then the conformance harness against Node
pnpm run test:unit           # node/test/**/*.test.mjs only
pnpm run test:conformance    # node --test conformance/harness only
```

One file, or tests whose name matches a pattern:

```bash
node --test node/test/proxy.test.mjs
node --test --test-name-pattern="sub-agent" node/test/proxy.test.mjs
```

The tests use `node:test` and `node:assert/strict`, no framework. They run offline and against
real things: loopback HTTP servers standing in for Jev and Anthropic, real files in temporary
directories, real `git` repositories, and real child processes. Every test that reaches
`node/src/status.mjs` imports `./isolate-status.mjs` first, so status files land in a throwaway
directory rather than among your live sessions. A few tests are Windows-only (the `.cmd` shim,
the file-locking retry) and are skipped elsewhere.

`node/test/env-cases.test.mjs` runs every `.env` golden case through `loadEnv` and a real
`~/.jev-router.env`; it is the test that fails on a Node with the wrong `parseEnv`.

To try routing against the real Jev (needs `JEV_API_KEY`):

```bash
node node/scripts/live-routing.mjs
```

## Lint and format

[Biome](https://biomejs.dev) lints and formats the Node and conformance code. It is a pinned
development dependency at the repository root, because one configuration (`biome.json`) covers
both `node/` and `conformance/`.

```bash
pnpm lint             # biome lint .   - the recommended rules
pnpm run format:check # biome format . - fails if a file is not formatted
pnpm run format       # biome format --write .
pnpm run check        # biome check . (lint + format), then pnpm test
```

What `biome.json` sets, and why:

- **Style** is the existing one: 2 spaces, double quotes, semicolons, trailing commas, LF line
  endings, and a 120-column line for `node/`.
- **Scope** is `node/` and `conformance/`, minus `conformance/cases`, `conformance/fixtures` and
  `conformance/reference`, which are generated, captured or vendored and must stay byte-exact.
  `.gitignore` is honoured.
- **The harness and `generate.mjs`** use a 200-column line: their tests and requests are written
  one per line on purpose.
- **`conformance/generator/` is linted but not formatted.** Its files are tables with one case
  per line, mirroring the case files; the formatter would explode them into thousands of lines.
- **Import sorting is off.** Every test imports `./isolate-status.mjs` first, before anything that
  loads `status.mjs`, and sorting would move it.

There is one suppression, with its reason inline: the ANSI-stripping regex in
`node/test/statusline.test.mjs` contains a control character on purpose. Where a rule's
preferred rewrite would change SPEC behaviour, the code is written to satisfy the rule instead;
for example, `body.tools?.forEach(...)` stays a `forEach` with a block body, because a `tools`
that is not an array must throw (SPEC 7.3 step 1) and a `for...of` loop would walk a string's
characters.

Install the editor extension ("Biome" in VS Code and JetBrains) to see the same diagnostics as
you type; it reads `biome.json` and the pinned binary from `node_modules`.

## Golden cases

`conformance/cases/*.json` are what the Node functions return for each input (SPEC 16.2). After
a change to Node's behaviour, regenerate them and run every port's tests:

```bash
node conformance/generate.mjs
git status conformance/cases          # only the cases you meant to change
```

The generator runs only on **Node 24.21.x**, the reference runtime SPEC 16.2 names: several
expected values come from Node itself (`util.parseEnv`, `toFixed`, `localeCompare`), so another
version could rewrite cases nobody changed. On any other version it exits 1 and says so.
`node conformance/generate.mjs --force` runs it anyway, for a deliberate move to a new reference
runtime; that commit must also update `REFERENCE_NODE` in the generator, SPEC.md (its header and
16.2) and `conformance/cases/README.md`.

The output is deterministic: a second run leaves `git status conformance/cases` unchanged. The
generator also checks that every status write it records really landed, and stops with
`status write did not land` rather than record a stale file.

## Conformance harness

The harness treats a proxy and a status line as black boxes. With nothing set, it runs Node's:
`pnpm test` already includes it. To run it alone, from the repository root:

Git Bash, macOS, Linux:

```bash
node --test conformance/harness
```

The same thing, with Node's programs named explicitly, the way you would name a port's:

```bash
JEV_IMPL_CMD_PROXY="node node/scripts/proxy-host.mjs" \
JEV_IMPL_CMD_STATUSLINE="node node/bin/jev-statusline.mjs" \
node --test conformance/harness
```

PowerShell:

```powershell
node --test conformance/harness

# or, named explicitly:
$env:JEV_IMPL_CMD_PROXY = "node node/scripts/proxy-host.mjs"
$env:JEV_IMPL_CMD_STATUSLINE = "node node/bin/jev-statusline.mjs"
node --test conformance/harness
Remove-Item Env:JEV_IMPL_CMD_PROXY, Env:JEV_IMPL_CMD_STATUSLINE
```

All 17 tests must pass. Each harness test has a 60 s limit, and a request the proxy never answers
fails after 20 s with `the proxy under test sent no response within 20s`, so a stuck
implementation fails its tests instead of hanging the run. Relative paths in `JEV_IMPL_CMD_*`
are resolved against the repository root; `conformance/README.md` has the details.

## What CI runs

The gates for this implementation, from the repository root, are the same on Windows (the
gating platform, SPEC 16), macOS and Linux:

```bash
pnpm install --frozen-lockfile
pnpm lint
pnpm run format:check
pnpm test                       # unit tests + conformance harness
```

On the Node 24.21 leg only, also check that the golden cases match the code:

```bash
node conformance/generate.mjs
git diff --exit-code conformance/cases
```

Today the only workflow, `.github/workflows/version-bump.yml`, runs `pnpm install
--frozen-lockfile` and `pnpm test` on Linux with Node 22 before it cuts a release. Since
`pnpm test` includes the harness, that release gate now covers it too; the lint, format,
Windows and golden-case checks above are the gates for the CI matrix that is still to be added.

## Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| `[jev] Claude Code is not installed, or claude is not on your PATH` | Install Claude Code (https://code.claude.com/docs/en/setup), then check with `claude --version` |
| `[jev] no JEV_API_KEY found - starting Claude Code without routing` | Put `JEV_API_KEY=...` in `~/.jev-router.env`. A project `.env` works too, but only in that directory |
| `[jev] invalid upstream URL (ANTHROPIC_BASE_URL): ...` | `ANTHROPIC_BASE_URL` is set to something that is not an `http(s)://` URL, such as `localhost:4000`. Fix it (`http://localhost:4000`) or unset it |
| `Cannot find module .../bin/jev-claude.mjs` | The global link predates the move into `node/`. Re-run the installer, or `pnpm add --global "link:<clone>"` |
| pnpm warns about `engines`, or `env-cases.test.mjs` fails | Your Node is older than 22.16, or a 23.x. Install 24 LTS |
| `jev-claude: command not found` after a by-hand install | pnpm's global bin directory is not on `PATH`. Run `pnpm setup`, open a new terminal, and link again |
| `jev-claude --update` updated Claude Code instead | Expected today; see [Updating](#updating) |
| `conformance/generate.mjs: the golden cases are generated by Node 24.21.x` | You are on another Node. Switch to 24.21 (`nvm install 24.21`), or see [Golden cases](#golden-cases) for `--force` |
| `pnpm lint` reports files under `conformance/cases` | They should be excluded; check that `biome.json` is the repository's and that you run from the root |
| A test fails with `git` not found | Put git on `PATH`; the update tests run it as a real process |
| You want to see routing decisions | Set `JEV_DEBUG=1`, then read `~/.jev-claude.log`. In a terminal, the launcher prints the log's path at start |

## Known divergences and gaps

Node is the reference, so the ports are measured against it rather than the other way round.
This is where Node itself departs from SPEC.md or from what other pages describe. This list is
the only copy.

- **No launcher update wiring** (SPEC 1.2): the update library and `jev-update-check` exist, but
  `jev-claude` neither checks nor prints a notice, and `--update` goes to Claude Code. The notice
  text in `update.mjs` ("Run `jev-claude --update`.") belongs to the planned feature and is
  pinned by the golden cases.
- **Temporary file names** are `<file>.<pid>.tmp`. That is safe only because a Node process
  writes one file at a time; multi-threaded ports add a sequence number (SPEC 3.10).
- **Jev request headers**: the SDK's `fetch` sends `accept-encoding`, `accept-language`,
  `sec-fetch-mode`, and `X-TypeSafe-SDK` / `X-TypeSafe-Runtime` beyond SPEC 5.1's set, with the
  SDK's own `User-Agent`. The ports send exactly SPEC 5.1's headers.
- **Duplicate client headers** are merged or dropped by Node's `http` module rules before they
  are forwarded; the ports forward them as repeated lines.
- **Invalid upstream URL**: Node refuses it when the proxy starts, with the message above. A
  port that still answers 502 per request, or words the message differently, differs here until
  SPEC 10 pins the message.
- **`util.parseEnv`** is the parser, so Node 22.0 to 22.15 and 23.x read `.env` files
  differently from SPEC 9.1; `engines` excludes them.

## Source layout

| Path | Contents |
| --- | --- |
| `node/bin/` | The six programs (`jev-claude`, `jev-statusline`, `jev-explain`, `jev-legend`, `jev-check`, `jev-update-check`) |
| `node/src/` | One module per concern: `config`, `env`, `router`, `policy`, `reasons`, `proxy`, `status`, `atomic-rename`, `settings`, `launch`, `first-run`, `explain`, `legend`, `icons`, `model-names`, `worktree`, `update`, `log` |
| `node/scripts/` | `proxy-host.mjs` (for the harness), and the developer tools `calibrate.mjs`, `calibration-cases.mjs`, `live-routing.mjs`, `bump-version.mjs` |
| `node/test/` | The unit tests (`*.test.mjs`) and `isolate-status.mjs` |
| `node/package.json` | The workspace package: the SDK dependency and the `test` script |
| `package.json` (root) | The installed package: the release version, the `bin` links, the root scripts, Biome |
| `biome.json` (root) | The lint and format policy |
| `conformance/` | Fixtures, golden cases and their generator, and the harness, shared by every implementation |
