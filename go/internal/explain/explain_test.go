package explain

import (
	"os"
	"path/filepath"
	"regexp"
	"testing"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/reasons"
)

// Ported from node/test/explain.test.mjs.

func parse(t *testing.T, s string) any {
	v, err := jsjson.Parse(s)
	if err != nil {
		t.Fatal(err)
	}
	return v
}

func matches(t *testing.T, text, pattern string) {
	t.Helper()
	if !regexp.MustCompile(pattern).MatchString(text) {
		t.Errorf("%q does not match %s", text, pattern)
	}
}

func short(r string) any {
	s, ok := reasons.Short(r)
	if !ok {
		return nil
	}
	return s
}

func TestExplain(t *testing.T) {
	t.Run("formats the last routing decision", func(t *testing.T) {
		out := FormatExplanation(parse(t, `{
			"prompt": "Explain the router architecture", "tier": "sonnet", "confidence": 0.94, "reason": "jev",
			"jev": {"request": {"state": {"session": {"current_model": "haiku", "context_tokens": 6200}}},
			        "response": {"answers": {"model": {"choice": "claude-sonnet-5-5", "confidence": 0.94}}}},
			"metrics": {"taskComplexity": 0.82, "reasoningRequired": 0.91, "toolComplexity": 0.64, "contextSize": 0.31}}`))
		matches(t, out, `Task complexity     0\.82`)
		matches(t, out, `Prompt: Explain the router`)
		matches(t, out, `Current model: HAIKU`)
		matches(t, out, `Context tokens: 6200`)
		matches(t, out, `Recommended tier: SONNET`)
		matches(t, out, `Selected model: SONNET`)
		matches(t, out, `Confidence: 94%`)
		// The sentence wraps inside the box rather than being cut at its edge.
		matches(t, out, `Decision: the router's\s*\x{2502}\n\x{2502} recommendation`)
	})

	t.Run("shows Jev's own pick when policy overruled it", func(t *testing.T) {
		out := FormatExplanation(parse(t, `{"tier": "opus", "model": "claude-opus-5-5", "confidence": 0.97,
			"reason": "downgrade-not-worth-cache-rebuild/no-change",
			"jev": {"response": {"answers": {"model": {"choice": "claude-haiku-4-5-20251001"}}}}}`))
		matches(t, out, `Recommended tier: HAIKU`)
		matches(t, out, `Selected model: CLAUDE-OPUS-5-5`)
	})

	t.Run("reads the recommendation from sessions recorded before the rename", func(t *testing.T) {
		matches(t, FormatExplanation(parse(t, `{"tier": "opus", "jev": {"response": {"answers": {"model_tier": {"choice": "sonnet"}}}}}`)), `Recommended tier: SONNET`)
	})

	t.Run("Claude skill pre-approves its read-only explanation command", func(t *testing.T) {
		data, err := os.ReadFile(filepath.Join("..", "..", "..", ".claude", "skills", "jev-explain", "SKILL.md"))
		if err != nil {
			t.Fatal(err)
		}
		matches(t, string(data), `(?m)^allowed-tools: Bash\(node \*\)\r?$`)
	})

	t.Run("says a held decision in words a person reads, not the reason code", func(t *testing.T) {
		if short("downgrade-not-worth-cache-rebuild/no-change") != "keeping the cache" ||
			short("jev-unavailable") != "router offline" || short("jev+unavailable") != "nearest available" {
			t.Error("short reasons")
		}
		matches(t, reasons.Long("downgrade-not-worth-cache-rebuild"), `re-read the whole conversation`)
	})

	t.Run("the status line stays quiet where the reason is obvious or not actionable", func(t *testing.T) {
		if short("low-confidence-default") != nil || short("override") != nil {
			t.Error("these say nothing on the status line")
		}
		matches(t, reasons.Long("low-confidence-default"), `unsure`)
		matches(t, reasons.Long("override"), `named this model`)
	})

	t.Run("an ordinary recommendation adds nothing to the status line", func(t *testing.T) {
		if short("jev") != nil || short("jev/no-change") != nil || reasons.Long("jev") != "the router's recommendation" {
			t.Error("ordinary recommendation")
		}
	})
}

func TestFormatAgentsListsEveryAgentOfTheSession(t *testing.T) {
	out := FormatAgents(parse(t, `{"agents": {
		"m": {"label": "main", "main": true, "model": "claude-opus-5-5", "confidence": 0.94, "at": 1000},
		"s": {"label": "grep for callers", "main": false, "model": "claude-haiku-4-5-20251001", "manual": true, "at": 0}}}`), 61000)
	matches(t, out, `main  CLAUDE-OPUS-5-5\s+94%\s+1m`)
	matches(t, out, `sub   CLAUDE-HAIKU-4-5-20251001 manual\s+1m`)
	matches(t, out, `\x{2502}       grep for callers`)
	if FormatAgents(parse(t, `{"tier": "opus"}`), 0) != "" {
		t.Error("a session from before agent tracking has no table")
	}
}
