# jev-claude documentation

jev-claude starts a local proxy and launches the real Claude Code through it, so each turn runs on
the model that gets the best result for the least cost. Claude Code's own sign-in or API key is
forwarded to Anthropic untouched.

| Page | What it covers |
| --- | --- |
| [../README.md](../README.md) | What it is, installing, configuration, how routing works, development |
| [PLUGIN_INSTALL.md](PLUGIN_INSTALL.md) | Installing from inside Claude Code with the `jev-installer` plugin |
| [UPDATES.md](UPDATES.md) | The launch-time update check and `jev-claude --update` |
| [LAYOUT.md](LAYOUT.md) | Where each language's implementation and the shared files live |

Configuration lives in `~/.jev-router.env`; the settings are listed in the README's
[Configuration](../README.md#configuration) section.
