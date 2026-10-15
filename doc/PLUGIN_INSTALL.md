# Installing from inside Claude Code

The repository is also a Claude Code plugin marketplace. It offers one plugin, `jev-installer`,
whose only job is to set up the `jev-claude` launcher on the machine.

```text
/plugin marketplace add alienfacepalm/jev-claude
/plugin install jev-installer@jev-claude
/jev-installer:install
```

## What the skill does

`/jev-installer:install` is user-invoked only (`disable-model-invocation`), so Claude never runs it
on its own. It runs the same one-line installer as the [README](../README.md#install), one command
at a time, each through Claude Code's normal permission prompt:

1. Picks the line for your system: `install.sh` (macOS, Linux, WSL, Git Bash) or `install.ps1`
   (Windows PowerShell). On Windows both put the install's `bin` folder on your user PATH, so
   either one covers Git Bash, PowerShell and Command Prompt.
2. Runs it. The installer checks the machine, downloads the latest release for your platform,
   verifies its SHA-256, installs the native programs to `~/jev-claude-rust`, and puts them on
   your PATH. It needs no Node.js, pnpm, Rust or clone. Running it again updates the install.
3. Tells you what to do next: add the Jev key, open a new terminal if asked, exit, and start
   `jev-claude`.

Claude Code is the one required prerequisite, and it is already installed because you are running
this inside it. If the installer cannot find `claude` on that shell's PATH anyway, the skill shows
you the failing check and the command the installer offers, and does not install a second copy
for you.

## Limits

- **Routing starts at the next launch.** A plugin can neither set `ANTHROPIC_BASE_URL` nor change
  the connection of the session it runs in, so the skill installs the launcher and you start
  `jev-claude` instead of `claude`.
- **No keys are saved, and nothing is asked.** The session has no terminal for the installer's
  hidden key prompt or its "install this prerequisite?" questions, so the installer skips the key
  prompt and installs no prerequisite on its own: where one is missing, it prints the exact command
  instead. The skill then shows you every such command and asks; only if you agree does it run the
  installer again with `JEV_ASSUME_YES=1`. If you decline, you run the commands in your own
  terminal. Keys pasted into a session are kept in its transcript, so the skill never
  asks for them. Add the Jev key by running the installer again in a normal terminal, or by
  editing `~/.jev-router.env`.
- **Skills and the status line stay with the launcher.** `jev-claude` already adds `/jev-calibrate`,
  `/jev-explain`, `/jev-legend` and its status line when it starts; the plugin does not duplicate
  them. The skills run the native programs by name, so open a new terminal after installing, so
  that the install's `bin` folder is on the PATH.

## Maintenance

- `.claude-plugin/marketplace.json` lists the plugin; the plugin lives in `plugin/`.
- The plugin has no `version`, so every commit is a new version for people who installed it. They
  receive it when they enable auto-update for the marketplace (**Marketplaces** in `/plugin`) or
  run `/plugin marketplace update jev-claude`; auto-update is off by default. It matters little:
  the skill only runs the installer, which fetches the current release itself.
- The plugin is named `jev-installer`, not `jev-claude`: `claude plugin validate` warns that names
  containing "claude" read as Anthropic's own.
- Check changes with `claude plugin validate .` and `claude plugin validate ./plugin`. The only
  warning left is the missing `version`, which is intended.
