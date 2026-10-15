---
name: jev-legend
description: Show what each symbol on the Jev status line means.
disable-model-invocation: true
allowed-tools: Bash(jev-legend *) PowerShell(jev-legend *)
---

<jev-legend>
Return the key below verbatim in a plain text code block. Do not add analysis or use tools.

If instead it says `jev-legend` was not found or is not recognized, jev-claude is not installed,
or it was installed after this terminal was opened and a new terminal will find it. Say so, give
the install command for the user's system, and stop:

- macOS, Linux, WSL, Git Bash: `curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh | bash`
- Windows PowerShell: `irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex`

!`jev-legend ; echo ""`
</jev-legend>
