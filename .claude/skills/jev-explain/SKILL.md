---
name: jev-explain
description: Show why Jev Router selected the model used for the last prompt.
disable-model-invocation: true
allowed-tools: Bash(jev-explain *) PowerShell(jev-explain *)
---

<jev-explain>
Return the report below verbatim in a plain text code block. Do not add analysis or use tools.

If instead it says `jev-explain` was not found or is not recognized, jev-claude is not installed,
or it was installed after this terminal was opened and a new terminal will find it. Say so, give
the install command for the user's system, and stop:

- macOS, Linux, WSL, Git Bash: `curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh | bash`
- Windows PowerShell: `irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex`

!`jev-explain "${CLAUDE_SESSION_ID}" ; echo ""`
</jev-explain>
