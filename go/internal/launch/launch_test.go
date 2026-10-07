package launch

import "testing"

func TestIsClaudeSubcommandOnlyTheFirstArgumentSpelledExactly(t *testing.T) {
	for _, name := range []string{"mcp", "plugin", "plugins", "doctor", "update", "upgrade", "auth", "agents", "kill", "stop"} {
		if !IsClaudeSubcommand([]string{name, "x"}) {
			t.Errorf("%q followed by an argument should be a claude subcommand", name)
		}
	}
	for _, name := range []string{
		"agents", "attach", "auth", "auto-mode", "doctor", "gateway", "import", "install", "kill", "logs", "mcp",
		"plugin", "plugins", "purge", "respawn", "rm", "setup-token", "stop", "ultrareview", "update", "upgrade",
	} {
		if !IsClaudeSubcommand([]string{name}) {
			t.Errorf("%q alone should be a claude subcommand", name)
		}
	}
	for _, args := range [][]string{
		nil,
		{},
		{"update the docs"}, // a prompt that starts with the word
		{"-p", "mcp"},       // the word later on
		{"MCP"},
		{"--model", "opus"},
		{"--model", "opus", "mcp", "list"},
		{""},
	} {
		if IsClaudeSubcommand(args) {
			t.Errorf("%q should not be a claude subcommand", args)
		}
	}
}
