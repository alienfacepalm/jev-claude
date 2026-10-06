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
on its own. It does the following, one command at a time, each through Claude Code's normal
permission prompt:

1. Checks `node --version` (22 or later) and `git --version`, and stops with a pointer if either
   is missing.
2. Clones `https://github.com/alienfacepalm/jev-claude.git` to `$JEV_CLAUDE_DIR`, or `~/jev-claude`.
   If that folder is already a clone it runs `git pull --ff-only`; if it exists and is something
   else it stops and asks.
3. Runs `install.sh` (macOS, Linux, WSL, Git Bash) or `install.ps1` (Windows without bash).
4. Tells you what to do next: add the Jev key, open a new terminal if asked, exit, and start
   `jev-claude`.

## Limits

- **Routing starts at the next launch.** A plugin can neither set `ANTHROPIC_BASE_URL` nor change
  the connection of the session it runs in, so the skill installs the launcher and you start
  `jev-claude` instead of `claude`.
- **No keys are saved.** The session has no terminal for the installer's hidden key prompt, so the
  installer skips it. Keys pasted into a session are kept in its transcript, so the skill never
  asks for them. Add the Jev key by running the installer again in a normal terminal, or by
  editing `~/.jev-router.env`.
- **Skills and the status line stay with the launcher.** `jev-claude` already adds `/jev-explain`
  and `/jev-calibrate` and its status line when it starts; the plugin does not duplicate them.

## Maintenance

- `.claude-plugin/marketplace.json` lists the plugin; the plugin lives in `plugin/`.
- The plugin has no `version`, so every commit is a new version for people who installed it. They
  receive it when they enable auto-update for the marketplace (**Marketplaces** in `/plugin`) or
  run `/plugin marketplace update jev-claude`; auto-update is off by default. It matters little:
  the skill only runs the installer, which fetches the current code itself.
- The plugin is named `jev-installer`, not `jev-claude`: `claude plugin validate` warns that names
  containing "claude" read as Anthropic's own.
- Check changes with `claude plugin validate .` and `claude plugin validate ./plugin`. The only
  warning left is the missing `version`, which is intended.
