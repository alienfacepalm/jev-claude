// Command jev-check is the read-only setup report behind /jev-calibrate (SPEC 13).
package main

import (
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
	"github.com/alienfacepalm/jev-claude/go/internal/repo"
	"github.com/alienfacepalm/jev-claude/go/internal/status"
)

func exists(p string) bool {
	_, err := os.Stat(p)
	return err == nil
}

// join is `list.join(", ")`.
func join(list []any) string {
	parts := make([]string, len(list))
	for i, v := range list {
		if !jsjson.IsNullish(v) {
			parts[i] = jsjson.JSString(v)
		}
	}
	return strings.Join(parts, ", ")
}

func row(label, text string) string { return jsstr.PadEnd16(label, 13) + text }

func main() {
	root := repo.Root()
	repository := root != "" && exists(filepath.Join(root, ".git")) && exists(filepath.Join(root, "node", "scripts", "calibrate.mjs"))
	routing := os.Getenv("ANTHROPIC_CUSTOM_MODEL_OPTION") == config.AutoModel
	cal := status.ReadCalibration(status.CalibrationFile())

	lines := []string{"Jev Router setup check (read-only: nothing was changed)", ""}
	model := os.Getenv("ANTHROPIC_MODEL")
	pinned := model != "" && model != config.AutoModel
	switch {
	case !routing:
		lines = append(lines, row("Routing", "off - no JEV_API_KEY found. Add JEV_API_KEY=... to ~/.jev-router.env and restart jev-claude."))
	case pinned:
		lines = append(lines, row("Routing", "available, but this session started on "+model+" because ANTHROPIC_MODEL is set. Choose Jev Router in /model to route."))
	default:
		lines = append(lines, row("Routing", "on - Jev Router picks a model for each turn"))
	}
	switch {
	case os.Getenv("ANTHROPIC_AUTH_TOKEN") != "":
		lines = append(lines, row("Claude", "auth token (ANTHROPIC_AUTH_TOKEN)"))
	case os.Getenv("ANTHROPIC_API_KEY") != "":
		lines = append(lines, row("Claude", "API key (ANTHROPIC_API_KEY), billed per token - if you approved it when Claude Code asked; "+
			"otherwise your sign-in. Change it with 'Use custom API key' in /config."))
	default:
		lines = append(lines, row("Claude", "your Claude Code sign-in"))
	}
	var tuned []string
	for _, t := range config.Tiers {
		tuned = append(tuned, t.Name+" "+t.ID)
	}
	lines = append(lines, row("Tuned for", strings.Join(tuned, ", ")))

	if cal.At == nil {
		lines = append(lines, row("Your account", "not read yet - Claude Code has not loaded the model list. Run /jev-calibrate again shortly."))
	} else {
		listed := join(cal.Models)
		if listed == "" {
			listed = "no Claude models listed"
		}
		// Node prints `new Date(at).toLocaleString()`; this is the en-US form in local time.
		when := time.UnixMilli(int64(cal.At.(float64))).Local().Format("1/2/2006, 3:04:05 PM")
		lines = append(lines, row("Your account", listed+" (as of "+when+")"))
		offered := map[string]bool{}
		for _, id := range cal.Models {
			offered[config.TierOf(id)] = true
		}
		var missing []string
		for _, t := range config.Tiers {
			if !offered[t.Name] {
				missing = append(missing, t.Name)
			}
		}
		if len(missing) > 0 {
			lines = append(lines, row("", "not offered to this account: "+strings.Join(missing, ", ")+" (routing steps around them)"))
		}
		if len(cal.Newer) > 0 {
			lines = append(lines, row("Newer models", join(cal.Newer)+" - routing already uses them, but the router was tuned on the versions before. Update jev-router to get tuning for them."))
		} else {
			lines = append(lines, row("Newer models", "none - the router is tuned for the newest models your account offers"))
		}
	}
	if config.FableAllowed(config.ProcessEnv) {
		lines = append(lines, row("Fable", "offered when the work calls for it (bills extra usage credits; JEV_ALLOW_FABLE=0 turns it off)"))
	} else {
		lines = append(lines, row("Fable", "off (JEV_ALLOW_FABLE is set to turn it off)"))
	}
	mode := "installed"
	if repository {
		mode = "repository"
	}
	lines = append(lines, row("Mode", mode))
	os.Stdout.WriteString(jsstr.ToUTF8(strings.Join(lines, "\n")) + "\n")
}
