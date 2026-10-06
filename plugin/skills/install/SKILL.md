---
name: install
description: Install or update the jev-claude launcher on this machine. Downloads the repository to ~/jev-claude and runs its installer. Changes nothing until you approve each command.
disable-model-invocation: true
---

# Install jev-claude

jev-claude is a launcher that starts a local proxy and runs Claude Code through it, so each turn
goes to the model that gets the best result for the least cost. Claude Code's own authorization,
a subscription sign-in or an API key, is forwarded to Anthropic untouched; jev-claude never reads
or stores it.

This skill installs the launcher. It cannot make **this** session route through it: routing
only starts when Claude Code is launched by `jev-claude`, so the last step is for the user.

Work through the steps in order. Run each command on its own so the user sees and approves it.
Stop and say what is wrong at the first step that fails; do not work around a failure.

## 1. Check the prerequisites

Run `node --version` and `git --version`.

- Node.js 22 or later is required. If it is missing or older, stop and point the user to
  https://github.com/alienfacepalm/jev-claude#installing-nodejs. Do not install Node yourself.
- Git is required. If it is missing, stop and point the user to https://git-scm.com/downloads.

## 2. Get the code

The folder is `$JEV_CLAUDE_DIR` when that is set, otherwise `~/jev-claude`.

- **The folder does not exist:** `git clone --depth 1 https://github.com/alienfacepalm/jev-claude.git <folder>`
- **It exists and contains a `.git` folder:** this is an update. Run `git -C <folder> pull --ff-only`.
  If that fails (local changes, or the copy has diverged), stop and tell the user; do not reset or
  discard anything.
- **It exists but is not a jev-claude clone:** stop and ask the user which folder to use. Never
  delete or overwrite it.

## 3. Run the installer

The installer installs pnpm if it is missing, installs the dependencies with
`pnpm install --frozen-lockfile`, installs the `jev-claude` command, and creates
`~/.jev-router.env` (readable only by the user) for the keys.

- macOS, Linux, WSL, or Windows with Git Bash: `bash <folder>/install.sh`
- Windows without bash: `powershell -NoProfile -ExecutionPolicy Bypass -File <folder>\install.ps1`

This session has no terminal to answer the installer's key prompts, so it skips them and saves no
key. That is intended. **Never ask the user to paste a key into this conversation, and never read
`~/.jev-router.env`:** anything typed here is kept in the transcript.

## 4. Tell the user what to do next

Say this, in your own words, and then stop:

1. **Add the Jev key.** Create one at https://console.typesafe.ai/keys, then either run the
   installer again in a normal terminal, where it asks for the key without showing it, or open
   `~/.jev-router.env` in an editor and paste the key after `JEV_API_KEY=`. Without a key,
   `jev-claude` starts Claude Code without routing and says so.
2. **Open a new terminal** if the installer said to (it adds a folder to the PATH the first time).
3. **Exit this session and start `jev-claude`** from any project, instead of `claude`. Plain
   `claude` is unchanged and keeps working.
4. **Updates.** Each time `jev-claude` starts it looks for a newer version in the background and,
   when there is one, prints a line saying so. `jev-claude --update` applies it. Nothing updates
   by itself.
