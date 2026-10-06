# jev-claude documentation

jev-claude starts a local proxy and launches the real Claude Code through it, so each turn runs on
the model that gets the best result for the least cost. Claude Code's own sign-in or API key is
forwarded to Anthropic untouched.

| Page | What it covers |
| --- | --- |
| [../README.md](../README.md) | What it is, installing, configuration, how routing works, development |
| [plugin-install.md](plugin-install.md) | Installing from inside Claude Code with the `jev-installer` plugin |
| [updates.md](updates.md) | The launch-time update check and `jev-claude --update` |

Configuration lives in `~/.jev-router.env`; the settings are listed in the README's
[Configuration](../README.md#configuration) section.
