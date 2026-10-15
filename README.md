# jev-claude

> **jev-claude is a fork of [jev-router](https://github.com/gargpratyush/jev-router) by
> [@gargpratyush](https://github.com/gargpratyush).** The routing proxy, the Jev integration, the
> status line, and `/jev-explain` come from that project. This fork narrows it to Claude Code and
> re-tunes the routing; see [Changes from jev-router](#changes-from-jev-router).

Per-turn model routing for Claude Code. For each new prompt, Jev picks the Claude model that gets
the best result for the least money: quality first, then cost, speed last. Claude Code itself is
unchanged: its interface, tools, sessions, permissions, and sign-in all work as before.

## Install

You need a Jev API key: sign in at [console.typesafe.ai](https://console.typesafe.ai) and create one
on the [API keys page](https://console.typesafe.ai/keys). The installer asks for it. Everything
else is checked, and installed for you if it is missing, with your permission. Run the line for
your shell; it works the same on every operating system that has that shell:

| Shell | Command |
| --- | --- |
| macOS, Linux, WSL, Git Bash | `curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh \| bash` |
| Windows PowerShell (5.1 or 7) | `irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 \| iex` |
| Windows Command Prompt (cmd.exe) | `powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 \| iex"` |

The Command Prompt line starts Windows PowerShell for the one command and runs the same
`install.ps1`; the double quotes keep cmd.exe from reading the `|` itself. Each line installs the
prebuilt programs of the latest release, the same way on every system. Building from source is
only for contributors and for platforms without a release (see [Development](#development)). If
`irm` fails on Windows PowerShell 5.1 with `Could not create SSL/TLS secure channel`, see
[Troubleshooting](doc/RUST.md#troubleshooting) for the one-line fix.

Then start it from any project:

```bash
jev-claude
```

jev-claude is a set of native programs. It needs no Node.js, no pnpm, no Rust toolchain and no
clone of this repository, and git is optional. It does need [Claude Code](https://code.claude.com/docs/en/setup),
because jev-claude is a launcher for it; the installer installs it when it is missing. Claude Code
asks you to sign in the first time it starts, or runs on an Anthropic API key (see
[Using an Anthropic API key](#using-an-anthropic-api-key)).

### What the installer checks and installs

It runs every check and prints each result (`ok`, `info`, `warn` or `FAIL`) before it changes
anything, then lists all the failures together with the fix for each. Then it installs what it can:

| Item | Needed? | If it is missing |
| --- | --- | --- |
| A download tool: `curl` (or `wget`), or PowerShell's web commands on Windows | required | offers to install `curl` with your package manager (`apt-get`, `dnf`, `yum`, `zypper`, `apk`, `pacman` or `brew`) |
| `tar` | required | offers to install it with your package manager; Windows 10 version 1803 and later has one, and an older Windows stops with the fix |
| A checksum tool (`sha256sum`, `shasum` or `openssl`; built in on Windows) | required | offers to install one; the download is never used unchecked |
| A network route to GitHub | required | any answer from the release host counts; silence stops the installer with a note to check your connection, VPN or proxy |
| [Claude Code](https://code.claude.com/docs/en/setup) | required | installs it with Anthropic's own installer, after asking. If you decline, the installer stops before it changes anything and prints the command to run yourself; if Anthropic's installer fails, it stops there with the same command, before any of jev-claude is installed. It also reports whether Claude Code is the native build or the older npm package, which runs on Node.js. A `claude` that is found but does not run (`claude --version` fails) is a `FAIL` with the command to reinstall it. Under WSL a Claude Code on the Windows side does not count: WSL needs its own |
| Claude Code's libraries on Alpine | required there | Claude Code's musl build needs `libgcc`, `libstdc++` and `ripgrep`; on Alpine the installer offers `apk add --no-cache libgcc libstdc++ ripgrep`, and then offers to set `USE_BUILTIN_RIPGREP=0` under `env` in `~/.claude/settings.json` so Claude Code uses that `ripgrep` |
| git | optional | offers to install it; the status line leaves out the branch and worktree without it, and declining is fine |
| Rust | never needed | never installed; you install the ready-made programs |
| Node.js, pnpm | not used | never needed and never installed; Node.js is printed for information if it is there |
| The install folder | required | `~/jev-claude-rust` (or `JEV_INSTALL_DIR`) must be missing, empty, or an earlier jev-claude install. Anything else stops the installer in this first step, before anything is changed, and is never touched |
| An old Node edition of jev-claude | warning only | reported, with how to remove it and which one wins on your PATH; never deleted for you (see [Upgrading from the Node edition](#upgrading-from-the-node-edition)) |

Before it changes your system to install any of these, the installer shows the exact command and
asks `y/N`. It runs `sudo` only when you are not root and `sudo` exists, says so in the question,
and never runs it silently. It touches nothing outside the item it is installing, and a failed
install is reported with the manual fix. When there is nobody to ask (a CI job or a script with no
terminal, or a Windows PowerShell console whose input is redirected or that was started with
`-NonInteractive`), it installs nothing: it prints the command instead, and stops if the item is
required. The same holds inside a Claude Code session, where an agent, not you, would be the one
answering. Piping the script itself (`curl ... | bash`) or redirecting its output to a file still
asks, on your terminal. Set `JEV_ASSUME_YES=1` to answer yes to every question (see [Settings](#settings-for-the-installer)).

The installer prints six numbered steps, the same on every system: 1 checking this machine (the
table above), 2 downloading the archive for your platform from the latest
[release](https://github.com/alienfacepalm/jev-claude/releases), 3 verifying it (its SHA-256 must
match the release's `SHA256SUMS`; a mismatch installs nothing), 4 installing it into
`~/jev-claude-rust`, 5 putting it on your PATH, and 6 running the installed `jev-check` as a final
test. Then it asks for your Jev key, and optionally an Anthropic API key, and saves them to
`~/.jev-router.env`, readable only by you.

Step 4 unpacks the archive beside the install folder and swaps it in only when every program is
there; an earlier install is kept until the new one's `jev-check` has run in step 6, and is put back
if it fails. An install folder that is neither empty nor an earlier jev-claude install is refused
and never touched.

Step 5 is the only one that differs by system, and the result is the same: typing `jev-claude` in a
new terminal works. On macOS, Linux and WSL the programs are linked into `~/.local/bin` (the
installer adds that folder to your shell profile only if it is not already on your PATH; if the
folder cannot be written, it puts the install's own `bin` on your PATH instead). On Windows, from
PowerShell or Git Bash, `~/jev-claude-rust\bin` goes first on your Windows user PATH. When Claude
Code lives in `%USERPROFILE%\.local\bin` and that folder is not on your user PATH (Anthropic's
Windows installer leaves it off), both installers add it too, last, so `jev-claude` finds `claude`
in new windows. The `/jev-calibrate`, `/jev-explain` and `/jev-legend` skills in the
install run the native programs by name, so the install's `bin` folder must be on the PATH of the
shell Claude Code uses; open a new terminal after installing.

### Settings for the installer

Set these in the environment before the command, or pass an option to the script:

| Setting | Effect |
| --- | --- |
| `JEV_ASSUME_YES=1` (or `-y`, `--yes`; `-Yes` when running `install.ps1` as a file) | answer yes to every "install this?" question, for an unattended install |
| `JEV_INSTALL_CLAUDE=1` or `0` | install Claude Code when it is missing without asking, or never offer (the installer then stops, since it is required) |
| `JEV_INSTALL_DIR` | where to install; the default is `~/jev-claude-rust`. It must be empty, missing, or an earlier jev-claude install: any other folder is refused and left as it is |
| `JEV_BIN_DIR` | where macOS, Linux and WSL link the programs; the default is `~/.local/bin` |
| `JEV_VERSION` | a release tag such as `v0.11.0`, instead of the latest |
| `JEV_RELEASE_URL` | download from a mirror with the same layout (`latest/download/<archive>`, `download/<tag>/<archive>`): an `https://` address or a `file://` folder on disk |
| `JEV_NO_MODIFY_PATH=1` | leave your shell profile and PATH alone; the installer prints what to add |
| `JEV_API_KEY`, `ANTHROPIC_API_KEY` | save these keys without asking for them |
| `-h`, `--help` (`-Help` for `install.ps1` as a file) | print the settings and exit. Piped through `\| bash` or `\| iex`, it points to the script's header instead, where the settings are listed |

The yes/no settings (`JEV_ASSUME_YES`, `JEV_INSTALL_CLAUDE`, `JEV_NO_MODIFY_PATH`) take `1`, `true`
or `yes` for on and `0`, `false` or `no` for off, in any letter case. `JEV_INSTALL_DIR` and
`JEV_BIN_DIR` may be relative (to the folder you run the installer from) or, in Bash, start with
`~`; they are made absolute before anything is written.

The variable has to reach the installer, not `curl`. In Bash, put it after the pipe, or pass the
flag; in PowerShell and Command Prompt, set it first:

```bash
curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh | JEV_ASSUME_YES=1 bash
curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh | bash -s -- -y
```

```powershell
$env:JEV_ASSUME_YES = '1'; irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex
```

```bat
set JEV_ASSUME_YES=1
powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex"
```

Through `iex` the script takes no options, so on Windows use the variables. `--rust`, `--node` and
`JEV_IMPL` from older instructions are still accepted and ignored, with a note: there is one edition.

### Platforms

| System | Release archive | Notes |
| --- | --- | --- |
| Windows 10 or 11, x64 | `windows-x64` | Windows PowerShell 5.1, PowerShell 7, Command Prompt (through `powershell`) and Git Bash |
| Windows 11 on ARM64 | `windows-x64` | runs through Windows' x64 emulation; there is no native ARM64 build. Windows 10 on ARM cannot run x64 programs, so both installers stop there |
| macOS, Apple silicon | `macos-arm64` | also from a terminal running under Rosetta |
| macOS, Intel | `macos-x64` | |
| Linux, x86-64 | `linux-x64` | statically linked (musl), so it runs on glibc and musl systems alike, Alpine included |
| Linux, ARM64 | `linux-arm64` | statically linked, as above |
| WSL | as Linux | |

**Windows.** PowerShell and Command Prompt run `install.ps1`; Git Bash runs `install.sh`. Both
install the same programs into the same folder and put its `bin` first on your Windows user PATH,
the setting every Windows shell starts from, so one run covers PowerShell, Command Prompt and Git
Bash alike; open a new terminal to pick it up (Windows Terminal and VS Code give new tabs the PATH
they started with, so restart the app if a new tab cannot find `jev-claude`). Git Bash installs as
Windows does, not as Linux: the programs are not linked, and no Git Bash profile line is written.
`tar.exe` ships with Windows 10 version 1803 and later.

**WSL** is Linux: run the Bash line inside your WSL distribution. It installs a Linux copy for that
distribution, separate from a Windows install, and Claude Code must be installed inside WSL too
(the installer offers it).

**macOS.** The stock `bash` 3.2 runs the installer. The PATH line, if one is needed, goes in
`~/.zshrc` (or `$ZDOTDIR/.zshrc`) for zsh, the default shell, and for bash in the first of
`~/.bash_profile`, `~/.bash_login` and `~/.profile` that exists, since a macOS terminal starts a
login shell. `tar` and `shasum` come with the system.

**Linux.** The programs are static, so there is nothing to match against your C library. The
PATH line goes in `~/.zshrc`, `~/.bashrc`, fish's `config.fish` or `~/.profile`, for your login
shell. The installer itself is a Bash script: a minimal Alpine image has no `bash`, so start with
`apk add bash`. It has BusyBox `wget` but no `curl`, which the installer is happy with; fetch it
with `wget -qO- https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh | bash`.
It offers the rest, including the libraries Claude Code needs on Alpine, through `apk`.

On any other platform (32-bit systems, FreeBSD, other CPU architectures) the installer stops and
says why. There is no automatic source build: build the programs yourself, as
[doc/RUST.md](doc/RUST.md#install-the-toolchain) describes, and put them on your PATH.

### Updating

Run the same line again. It downloads the latest release, checks it, replaces the install, and
says which version it replaced (`updated 0.10.0 -> 0.11.0`, or `reinstalled version 0.11.0`). Your
settings in `~/.jev-router.env` stay.

### Removing

There is no uninstaller; removing it takes a few deletions. Delete `~/jev-claude-rust` (or your
`JEV_INSTALL_DIR`) and the seven `jev-*` links in `~/.local/bin` (or your `JEV_BIN_DIR`). Delete
the line the installer added to your shell profile, if there is one: it follows a
`# jev-claude (Rust edition)` comment. On Windows, remove the install's `bin` entry from your user
PATH (Settings > System > About > Advanced system settings > Environment Variables). The programs
also leave a few files you can delete: the `~/.jev-router` folder (`first-run.json`, `update.json`),
`~/.jev-claude.log` (written only with `JEV_DEBUG`), and the status folder, `jev-claude-<uid>` (on
Windows `jev-claude`) in your temp directory, or your `JEV_STATUS_DIR`. `~/.jev-router.env` is your
own file and stays, as does Claude Code. [doc/RUST.md](doc/RUST.md#install-a-release) has the
details.

### Upgrading from the Node edition

Up to v0.11.0 the default install was a Node.js edition, a git clone in `~/jev-claude` linked with
pnpm; `--rust` (or `JEV_IMPL=rust`) installed these native programs instead. The Node edition is
retired; the program is now Rust, and nothing about running it needs Node.js. Run the line above to
install the new one. It goes to `~/jev-claude-rust`, a different folder, so it does not collide with
the old clone, and the installer never deletes the old one. Both read the same settings file, so
your keys carry over.

If you used `--rust` before, there is nothing to remove: the same install line updates
`~/jev-claude-rust` in place, and your shell profile line is kept. `--rust` and `JEV_IMPL` are
still accepted and ignored, with an info row in the report (`-Rust` on Windows).

Otherwise, remove the old one, so that it cannot be the one that runs:

```bash
pnpm remove --global jev-router     # unlinks the old jev-claude command
rm -rf ~/jev-claude                 # the old clone; PowerShell: Remove-Item -Recurse -Force "$HOME\jev-claude"
```

If you linked the old clone some other way, delete that link instead. If you installed pnpm and
Node.js only for jev-claude, you can remove them too. Until the old
command is gone, whichever `jev-claude` comes first on your PATH runs: check with `type -a
jev-claude` (Bash) or `Get-Command jev-claude -All` (PowerShell). When it finds an old Node edition
(a `jev-claude` on your PATH that is a JavaScript file, a script or pnpm shim, or a clone at
`~/jev-claude`), the installer reports it as a `warn` row, says how to remove it, and in step 5
which `jev-claude` wins in that shell. A `jev-claude` that reports `Cannot find module
.../bin/jev-claude.mjs` is the old one.

### Settings file and API key

The installer writes your keys to `~/.jev-router.env`. To change them later, edit that file; the
install includes a commented template, `~/jev-claude-rust/.env.example` (also
[`.env.example`](.env.example) in this repository), and each setting is explained in
[Configuration](#configuration). To use a key for one project only, put it in a `.env` in that
project instead.

The `jev-router` package on npm is the original project and does not include this fork's changes.

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
| Haiku | Claude Haiku 5.5 | Trivial, mechanical edits | Cheapest; does no reasoning |
| Sonnet | Claude Sonnet 5.5 | Well-scoped changes, bugs with a known cause, tests, documents and analysis | About 40-60% less than Opus |
| Opus | Claude Opus 5.5 | Open-ended or multi-step coding, unknown-cause bugs, security, migrations, judgement calls | About 1.6-2.6x Sonnet |
| Fable | Claude Fable 5.1 | Long-horizon autonomous work, problems a strong model has already failed at, adversarial reviews of plans | Most expensive ($10 / $50 per million tokens) |

The costs are per completed task, not per token: a model that costs twice as much per token can
use fewer tokens. They come from Anthropic's and Artificial Analysis's published measurements
(September 2026), noted with their sources in [`rust/src/config.rs`](rust/src/config.rs).

Each tier always uses the newest version your account offers, read from Claude Code's model list,
so a new release such as Opus 6 is used as soon as your account has it.

Then these rules apply ([`rust/src/policy.rs`](rust/src/policy.rs)):

- **You can ask.** A prompt such as `use opus` or `switch to the strong model` wins. A tier word in
  ordinary prose ("replace this with long polling") is not a request, and neither is one you rule
  out ("do not use fable", "never use haiku for this") or ask about ("why does the planner use
  opus?"): a negated instruction is ignored, so "don't use haiku, use opus" means opus, and a
  sentence ending in `?` is a question, not an order.
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
that name none. To run a tier at one effort whatever Claude Code asks for (including `/effort`), set
`JEV_FORCE_EFFORT`, or `JEV_<TIER>_FORCE_EFFORT` for a single tier.

## Status line

`jev-claude` adds a status line showing the model the last turn ran on, Jev's confidence, the
reasoning effort it ran at, and the reason when it was not simply Jev's pick. Haiku takes no
effort, so none is shown for it. Sub-agents follow `✦`, with their model and version. Every item is
a bold symbol in the terminal's own text colour, so it contrasts with any background, and its value (`✧✦` model, `◔` effort, `✦` sub-agents, `❐` directory, `⌂` worktree,
`≡` context):

```text
✧✦ Sonnet 5.5 (94%) · ◔ high · ❐ my-project · ≡ 8% · ~/work/my-project
✧✦ Opus 5.5 (91%) · ◔ medium (keeping the cache) · ✦ Haiku 5.5,Sonnet 5.5 · ❐ my-project · ≡ 34% · ~/work/my-project
☞ manual Opus 4.6 · ❐ my-project · ≡ 21% · ~/work/my-project
```

The last item, dimmed, is the full path of the current directory, for quick reference. It is the path
Claude Code reports (a Windows path keeps its backslashes) except that your home directory is written
`~`, so a deep path under it leaves more room. Only a whole leading directory is shortened. It sits
last because that is
the rightmost place a status line can put it: Claude Code does not say how wide the terminal is, so
the line cannot be right-aligned, and a line that is too long loses its end first, which is this path
and not the model. It is left out when Claude Code reports no directory.

`☞` marks a model you picked by hand with `/model`, which Jev leaves alone: in front of `manual` when it
is the main model, and in front of a sub-agent's name when that sub-agent is pinned to its own model.

Right after the context share come tiny versions of the `/usage` meters, one for the 5-hour limit and
one for the 7-day limit: the label, a bar of four cells (each a quarter of the limit, and any use that
shows as at least 1% fills the first), and the percentage used. The value is dimmed while there is
plenty left and turns bold from 70%:

```text
✧✦ Sonnet 5.5 (94%) · ◔ high · ❐ my-project · ≡ 8% · 5h ▰▰▱▱ 41% · 7d ▰▱▱▱ 12% · ~/work/my-project
```

Claude Code reports these limits only to claude.ai Pro and Max accounts, and only after the first
response of a session, so on an API key, or before that first reply, the meters are simply absent. A
window that has passed its reset time is dropped by Claude Code, and then so is its meter. Only the
5-hour and 7-day limits are available to a status line: the per-model weekly rows of `/usage` are
not. Where the line falls back to words (below) there are no bars, just `5h 41% · 7d 12%`.

In a git checkout the line also shows the branch after the directory, and in a worktree (a Claude
Code `--worktree` session, or any directory in a linked worktree from `git worktree add`) the
worktree's name too. `(detached)` stands in for the branch when none is checked out, and a
directory that is not a git checkout shows neither. To keep the line short, a branch longer than 28
characters is cut with `…`, and the directory is left out when the worktree has the same name. The
effort and directory symbols are plain Unicode that stock macOS, Windows Terminal and Linux fonts
draw. The branch uses the Powerline glyph (U+E0A0) that zsh themes such as agnoster and powerlevel10k
draw, so it needs a Powerline or Nerd Font in your terminal:

```text
✧✦ Sonnet 5.5 (94%) · ◔ high · ❐ my-project ·  fix/login · ⌂ login-fix · ≡ 8% · ~/work/my-project
```

Run `/jev-legend` for a key to what each symbol means. It is drawn from the same symbols the line
prints, so it also reads correctly when the line has fallen back to words. A status line only
prints what it is given, and Claude Code has no keybinding that runs a slash command, so a typed
`/jev-legend` (Tab completes it) is the way to ask; outside a session, `jev-legend` prints it too.

When your account offers a newer version of a model than the router was tuned for, the line gets
a notice after the context share, such as `new claude-opus-6: /jev-calibrate`. Routing already uses the new model; the
notice means the costs and guidance were measured on the previous one, and a jev-claude update
will bring tuning for it. Only new versions of known models are noticed, not new model names.

The legacy Windows console (the old cmd.exe and PowerShell window) cannot draw the glyphs, so there
the line falls back to words (`model Sonnet 5.5 (94%) · effort high · dir my-project · …`).
Set `JEV_ICONS=symbols` or `text` to choose yourself.

An existing custom `statusLine` in your Claude Code settings is kept. Set `JEV_NO_STATUSLINE=1` to
turn Jev's off.

> Choosing a model with `Enter` in `/model` can save it as Claude Code's default. `jev-claude`
> puts your previous default back when it exits, so `jev-router` never breaks plain `claude`. If a
> session is killed outright, the next `jev-claude` run restores it.

## Setup check

Run `/jev-calibrate` in a `jev-claude` session for a read-only report on your setup:

```text
Jev Router setup check (read-only: nothing was changed)

Routing      on - Jev Router picks a model for each turn
Claude       your Claude Code sign-in
Tuned for    haiku claude-haiku-5-5, sonnet claude-sonnet-5-5, opus claude-opus-5-5, fable claude-fable-5-1
Your account claude-haiku-5-5, claude-sonnet-5-5, claude-opus-5-5, claude-fable-5-1 (as of 2026-10-14 12:00:00 UTC)
Newer models none - the router is tuned for the newest models your account offers
Fable        offered when the work calls for it (bills extra usage credits; JEV_ALLOW_FABLE=0 turns it off)
Mode         installed
```

`Mode` is `installed` for a release install and `repository` in a source checkout (a clone with
`.git` and `rust/Cargo.toml`), where `/jev-calibrate retune` is available. If your account offers a
model newer than the router was tuned for, `Newer models` names it and says to run the jev-claude
installer again, which brings the tuning of the newest release. The report changes nothing. On the first plain launch, `jev-claude` offers to run it for you, once. The
answer is kept in `~/.jev-router/first-run.json`; delete that file to be asked again. The offer is
skipped in a project that defines its own `jev-calibrate` skill.

`/jev-calibrate` only ever reports, so it is safe in an installed copy. To re-tune the router, run
`/jev-calibrate retune` in a clone you develop in, started with `JEV_ROOT` set to the clone (it
needs the Rust toolchain); see [Calibrating for new models](#calibrating-for-new-models).

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
| macOS | `$TMPDIR/jev-claude-<uid>/<session-id>.json` |
| Linux | `${TMPDIR:-/tmp}/jev-claude-<uid>/<session-id>.json` |

`<uid>` is your numeric user id, so users who share `/tmp` each get a folder of their own;
`JEV_STATUS_DIR` chooses another folder. These files hold prompt text, so only you can read them
(directory mode 700, files 600). Files untouched for 7 days are deleted.

## Configuration

Settings go in `~/.jev-router.env` (see [`.env.example`](.env.example)), in a project's `.env`, or
in your shell's environment.

| Variable | Effect |
| --- | --- |
| `JEV_API_KEY` | Your Jev key; routing is off without it. `TYPESAFE_API_KEY` also works. |
| `ANTHROPIC_API_KEY` | Optional: run Claude Code on this Anthropic API key instead of your sign-in (see [Using an Anthropic API key](#using-an-anthropic-api-key)). Not read from a project's `.env`. |
| `JEV_ALLOW_FABLE` | Fable is offered by default; `0`, `false`, `no` or `off` turns it off. |
| `JEV_SONNET_EFFORT`, `JEV_OPUS_EFFORT`, `JEV_FABLE_EFFORT` | Effort (`low`, `medium`, `high`, `xhigh`, `max`) for requests that name none. Defaults: Sonnet `high`, Opus `medium`, Fable `high`. Claude Code normally sends its own. |
| `JEV_FORCE_EFFORT`, `JEV_SONNET_FORCE_EFFORT`, `JEV_OPUS_FORCE_EFFORT`, `JEV_FABLE_FORCE_EFFORT` | Effort (`low`, `medium`, `high`, `xhigh`, `max`) that replaces the one Claude Code sends. `JEV_FORCE_EFFORT` applies to every tier that takes an effort; a tier's own setting wins over it. Unset by default. Haiku takes no effort, so it is never forced. |
| `JEV_NO_STATUSLINE` | Turns off Jev's status line. |
| `JEV_ICONS` | `symbols` or `text`: the status line's labels. Symbols by default; on Windows, words in the legacy console, which cannot draw them. |
| `JEV_DEBUG` | Logs routing decisions to `~/.jev-claude.log`. |
| `JEV_DUMP` | Saves whole request bodies for debugging. `1` writes them, owner-only, to the status directory; any other value is a path prefix. Shell or `~/.jev-router.env` only; not read from a project's `.env`. |

Your shell's environment wins, then a `.env` in the directory you start `jev-claude` from, then
`~/.jev-router.env`, then the older `~/.jev-claude.env`. A blank value is ignored, so a copied but
unfilled `.env.example` never hides your real key.

A project's `.env` may be someone else's file, so only these settings are read from it:
`JEV_API_KEY`, `TYPESAFE_API_KEY`, `JEV_DEBUG`, `JEV_ALLOW_FABLE`, `JEV_NO_STATUSLINE`, `JEV_ICONS`
and the `JEV_*EFFORT` settings. `JEV_DUMP`, which names a path, and anything that could redirect
traffic or run code, such as `ANTHROPIC_BASE_URL`, `TYPESAFE_BASE_URL` or `NODE_OPTIONS`, must be
set in your shell or in `~/.jev-router.env`. The
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

The program is Rust, in `rust/`, and it follows [`SPEC.md`](SPEC.md). It is the only
implementation: the repository holds no Python and no JavaScript. The previous Node.js edition is
retired and available at the git tag `v0.11.0`. This section is for contributors and for anyone on
a platform without a release archive; everyone else should use the [installer](#install).

You need the Rust toolchain (1.85 or later, through rustup) and a C compiler, plus `git`;
[doc/RUST.md](doc/RUST.md#install-the-toolchain) has the commands for Windows, macOS and Linux.
Node.js is optional: a few launcher tests start an npm-style stand-in `claude` through `node`, the
way an npm-installed Claude Code starts, and without `node` on PATH they print a note and skip.

Run these from `rust/` (its `.cargo/config.toml`, which defines `cargo xtask` and makes rustdoc
warnings fatal, is read only there):

```bash
cd rust
cargo build --release --locked                           # the seven programs, in rust/target/release
cargo fmt --all --check                                  # or `cargo fmt --all` to fix
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked                          # unit, integration, golden, conformance and xtask tests
cargo doc --workspace --no-deps --locked
cargo +1.85 check --workspace --all-targets --locked     # the minimum supported Rust version
```

`cargo test` runs offline: every server the tests talk to is a loopback fake. It includes the
black-box conformance tests (`rust/tests/conformance_*.rs`), which start the built
`jev-proxy-host` and `jev-statusline` against a fake Jev and a fake Anthropic API; run just those
with `cargo test --locked --test 'conformance*'` ([conformance/README.md](conformance/README.md)).

The release and developer tasks are `cargo xtask` commands (the `rust/xtask` crate, never shipped),
also run from `rust/`; relative paths are taken from there:

```bash
cargo xtask bump-version [<before>] [<after>] [--dry-run]   # the next version from the commits since the last release tag; writes package.json (--dry-run: only prints it)
cargo xtask package --bin-dir target/release --out ../dist   # jev-claude-<platform>.tar.gz and SHA256SUMS
cargo xtask installer-smoke-test --bin-dir target/release    # or --archive ../dist/jev-claude-<platform>.tar.gz
```

The smoke test packages the build, serves it from a local server and runs the real `install.sh`
(and on Windows `install.ps1`, under Windows PowerShell and PowerShell 7) in a throwaway home,
with a stand-in for Claude Code and do-nothing package managers; it never touches your own home,
PATH, profile or user PATH, never reaches Anthropic's installer or a real package manager, and
runs dozens of cases per installer (a bad checksum, an interrupted download, a refused Claude Code
install, an odd install folder, the PATH and profile edits). `--installers <dir>` runs the
`install.sh` and `install.ps1` in another folder, and `--only <groups>` runs just some groups of
cases; the group names are in the doc comment of `rust/xtask/src/smoke.rs`.

To try your build against the real Jev and Claude Code, from the repository root:

```bash
cp .env.example .env                                     # then paste your key (git ignores .env)
cargo run --manifest-path rust/Cargo.toml --release --bin jev-claude -- -p "what is 2+2?"
cargo run --manifest-path rust/Cargo.toml --release --example live_routing   # asks the real Jev about a few sample prompts
```

The root keeps a `package.json` only as the version file the programs read at run time (SPEC.md
2.3); it has no scripts and nothing installs it. [doc/LAYOUT.md](doc/LAYOUT.md) says what lives
where, and [doc/RUST.md](doc/RUST.md) is the full guide: building, putting your build on PATH
beside an installed copy, tests, lint and format, the conformance tests, troubleshooting, and
where the program knowingly differs from the retired Node edition. [rust/README.md](rust/README.md)
is its quick start.

The `/jev-calibrate`, `/jev-explain` and `/jev-legend` skills run the native programs
(`jev-check`, `jev-explain`, `jev-legend`) by name, so those must be on `PATH` in the shell Claude
Code uses; the installer's `bin` folder is. A build finds the repository root (and with it the
skills) by walking up from its own location, so build it inside the clone; doc/RUST.md says what a
copy installed elsewhere loses and how `JEV_ROOT` fixes it.

### Continuous integration

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs on every push and pull request:

- `lint`: shellcheck on `install.sh` and the CI scripts, and actionlint on the
  workflow.
- `rust`, on Windows (the gating platform, SPEC.md 16), Ubuntu and macOS: format, clippy,
  `cargo test --workspace` (the golden cases, the black-box conformance tests and the xtask tests),
  the launcher tests that need `node` once with it and once without, rustdoc, the minimum Rust
  version, a release build, and on Windows a check that the C runtime is linked in.
- `release-targets`: every published target (Windows x64, Linux x64 and arm64 on musl, macOS Apple
  silicon and Intel) built, checked for static linking, packaged, and installed by the real
  installer on a machine of its own architecture.
- `installers`: `cargo xtask installer-smoke-test` against the freshly built programs on all three
  systems, plus Windows 11 on ARM (experimental), and the skills' command lines under Windows
  PowerShell 5.1 and PowerShell 7.
- `installer-alpine`: `install.sh` on Alpine (musl, BusyBox `wget`, `tar` and `sha256sum`, no `git`).

The commands are the ones above, and doc/RUST.md's [full gate](doc/RUST.md#the-full-gate).

### Calibrating for new models

`cargo run --manifest-path rust/Cargo.toml --release --example calibrate -- --runs 2` asks the
real Jev about the labelled prompts in
[`rust/examples/calibration-cases.json`](rust/examples/calibration-cases.json) (it needs
`JEV_API_KEY`). It reports how often Jev picked the intended tier, and where each turn finally ran
(an unsure pick running one tier lower shows as `step`). Run it before and after any change to the
guidance, costs, effort, or thresholds in `rust/src/config.rs`, the tuning's source of truth, and
keep a change only when the pick score holds.

When a new Claude model ships, build the clone (`cargo build --manifest-path rust/Cargo.toml
--release`), then start `jev-claude` from that clone with `JEV_ROOT` set to it (`JEV_ROOT=$PWD
jev-claude` in Bash, or `$env:JEV_ROOT = $PWD` first in PowerShell), or with the clone's
`rust/target/release` first on PATH, and run `/jev-calibrate retune`. The skill reads the
`jev-check` on PATH: an installed copy reports `Mode installed` and declines to retune. It checks
Anthropic's model notes and published per-task measurements, updates `rust/src/config.rs`, and
measures each change with the example above, on a branch.

### Versions and commit messages

Every push to `master` runs [CI](.github/workflows/ci.yml). Only when every job passes, on every
platform, does its `release` job bump the version in the root `package.json`
(`cargo xtask bump-version`), commit it as `chore(release): vX.Y.Z`, and tag it. Pull after
pushing to pick up that commit. The same run then builds the programs for Windows, macOS and Linux
from the tagged commit (statically linked: musl on Linux, the C runtime on Windows), installs each
archive with the real installer, and publishes them, with `SHA256SUMS`, as a GitHub release: that
release is what `install.sh` and `install.ps1` download. If one platform's build fails after the
tag exists, use **Re-run failed jobs** on that run: it rebuilds that platform and then publishes.
The size of the bump comes from the
[Conventional Commits](https://www.conventionalcommits.org) prefixes of every commit since the last
release tag, so commits from a push whose run failed are released by the next successful one:

| Commit subject | Bump |
| --- | --- |
| `feat: ...` | minor (0.5.0 to 0.6.0) |
| `fix: ...`, `docs: ...`, `chore: ...`, or no prefix | patch (0.5.0 to 0.5.1) |
| `feat!: ...`, or `BREAKING CHANGE:` in the message | major (0.5.0 to 1.0.0) |

## Limitations

- Your prompt text is sent to TypeSafe for the routing decision. Nothing else is.
- Jev adds a short delay to the first request of each turn; tool calls within a turn add none.
- Claude Code's request format is not a public contract. Use `JEV_DUMP` (set in your shell or
  `~/.jev-router.env`) to diagnose changes.
- jev-claude and its installers are written for macOS, Linux (including WSL), and Windows, and CI
  builds and tests the programs and runs the installers on all three. Hand testing against a real
  Claude Code has been on Windows 10 (PowerShell 7 and 5.1, Git Bash) with Claude Code 2.1.287;
  macOS and Linux have had less of it.

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
  calibration example and cases for re-tuning when new models ship.
- **Easier setup.** One-line installers for macOS, Linux, and Windows, `.env.example`, and
  documented support for running on an Anthropic API key.
- **Hardening.** A project's `.env` can no longer redirect traffic or run code, private files are
  owner-only, prompt overrides need an explicit instruction, Esc stops generation upstream, and
  the saved Claude Code model is restored even when a session is killed.
- **Native programs.** The program is Rust, installed from a ready-made release with no Node.js,
  pnpm or clone, and the installers install what is missing (Claude Code included).
- **Tooling.** Cargo throughout (`cargo xtask` for the release tasks, Rust integration tests for
  the conformance suite), and automatic version bumps and releases on `master`.

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
4. Run the gate in [Development](#development) (`cargo test --workspace --locked` and the rest,
   from `rust/`) and include tests for non-trivial behaviour changes. A change to behaviour
   changes [`SPEC.md`](SPEC.md), the golden cases in `conformance/cases/` (frozen fixtures, edited
   by hand) and the Rust code together.

Never commit API keys or other secrets: copy `.env.example` to `.env` (which git ignores) for local
settings. All contributions are reviewed, and only the repository owner merges pull requests.

## License

MIT, see [LICENSE](LICENSE). jev-claude is a fork of
[jev-router](https://github.com/gargpratyush/jev-router) by
[@gargpratyush](https://github.com/gargpratyush), also MIT-licensed; its copyright notice is kept
in LICENSE.
