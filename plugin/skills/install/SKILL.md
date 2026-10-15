---
name: install
description: Install or update the jev-claude launcher on this machine with its one-line installer. Changes nothing until you approve each command.
disable-model-invocation: true
---

# Install jev-claude

jev-claude is a launcher that starts a local proxy and runs Claude Code through it, so each turn
goes to the model that gets the best result for the least cost. Claude Code's own authorization,
a subscription sign-in or an API key, is forwarded to Anthropic untouched; jev-claude never reads
or stores it.

This skill installs the launcher. It cannot make **this** session route through it: routing
only starts when Claude Code is launched by `jev-claude`, so the last step is for the user.

The installer downloads ready-made programs from the latest release into `~/jev-claude-rust` and
puts them on the PATH. It needs no git clone, no Node.js, no pnpm and no Rust toolchain. It checks
everything it needs before it changes anything, and installs what is missing itself (Claude Code,
curl, tar, a checksum tool, and optionally git), each time after showing the exact command.

Work through the steps in order. Run each command on its own so the user sees and approves it.
Stop and say what is wrong at the first step that fails; do not work around a failure.

## 1. Pick the installer line

The only prerequisites are what fetches the installer: `curl` on macOS, Linux and WSL, Windows
PowerShell on Windows (built in). Use the line for the shell the user will start `jev-claude` from:

- macOS, Linux, WSL, or Windows from Git Bash:
  `curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh | bash`
- Windows, run from this session's Bash or PowerShell tool:
  `powershell -NoProfile -Command 'irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex'`
  (the installer's own one-liner, `irm ... | iex`, in a fresh Windows PowerShell, so the same line
  works from both tools; the single quotes do not protect `|` in Command Prompt, so a user typing
  it in cmd.exe should use the README's Command Prompt line instead:
  `powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex"`
  where the double quotes keep cmd.exe from reading the `|`)

On Windows both lines put the install's `bin` folder first on the user PATH, so one run covers
Git Bash, PowerShell and Command Prompt; there is no need to run both.
If `curl` itself is missing, stop and tell the user to install it with their package manager; do
not fetch the installer another way.

## 2. Run the installer

Run the line as it is, without any yes option. Running it again on an installed machine is an
update: it replaces the programs with the latest release and keeps the user's settings.

This session has no terminal, so the installer cannot ask its questions here. When something the
installer would install is missing, it does not install it: it lists each missing prerequisite with
the exact command it would run, and stops before changing anything if one of them is required.
Only git is optional (the status line uses it for branch names): when it alone was skipped, the
install finishes, and offering git the same way is up to the user. Claude Code is required, and
this session is running in it, so its row should say `ok`. If it says anything else anyway
(`FAIL` or `warn`; this shell's PATH may not list Claude Code's folder), show the user that row
and the command it offers; do not agree to a second copy on the user's behalf. For any prerequisite it skipped:

1. Show the user that list, every command exactly as the installer printed it, and ask whether
   they agree to all of them. Say which ones use `sudo` or need an administrator's approval.
2. Only after the user explicitly agrees, run the installer again with `JEV_ASSUME_YES=1`, which
   answers yes to every one of those questions:
   - `curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh | JEV_ASSUME_YES=1 bash`
   - `powershell -NoProfile -Command '$env:JEV_ASSUME_YES = "1"; irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex'`
3. If the user declines, or wants only some of them, do not pass `JEV_ASSUME_YES`: give them the
   commands to run in their own terminal, then run the installer again here. Do the same when a
   command needs a password, such as `sudo` asking for one, since this session cannot type it.

Never set `JEV_ASSUME_YES` on your own judgement, never edit the commands the installer printed,
and never run an install command of your own instead of the installer's.

The installer saves no key in this session: it skips its key prompts inside Claude Code and only
creates `~/.jev-router.env` (readable only by the user) for the keys. That is intended. **Never ask
the user to paste a key into this conversation, never pass one to the installer, and never read
`~/.jev-router.env`:** anything typed here is kept in the transcript.

If the installer says it could not replace the install folder (a running `jev-claude` or Claude
Code session that uses it holds the programs open, which on Windows includes this session when it
was started by `jev-claude`), do not retry: tell the user to exit this session and run the
installer line in their own terminal.

If the installer reports an old Node edition (an earlier version of this skill installed one as a
git clone in `~/jev-claude`), pass on how it says to remove it and which one wins on the PATH. Do
not remove anything yourself.

The installer ends by running `jev-check`; return its report to the user.

## 3. Tell the user what to do next

Say this, in your own words, and then stop:

1. **Add the Jev key.** Create one at https://console.typesafe.ai/keys, then either run the
   installer line again in a normal terminal, where it asks for the key without showing it, or
   open `~/.jev-router.env` in an editor and paste the key after `JEV_API_KEY=`. Without a key,
   `jev-claude` starts Claude Code without routing and says so.
2. **Open a new terminal** if the installer said to (it adds a folder to the PATH the first time).
3. **Exit this session and start `jev-claude`** from any project, instead of `claude`. Plain
   `claude` is unchanged and keeps working.
4. **Updates.** Run the installer line again to update; it installs the latest release and keeps
   the settings. Nothing updates by itself.
