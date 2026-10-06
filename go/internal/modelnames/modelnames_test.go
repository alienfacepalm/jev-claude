package modelnames

import (
	"testing"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
)

// Ported from node/test/model-names.test.mjs.

func expect(t *testing.T, model any, want any, why string) {
	t.Helper()
	got, ok := ShortName(model)
	var g any = got
	if !ok {
		g = nil
	}
	if g != want {
		t.Errorf("ShortName(%v) = %v, want %v %s", model, g, want, why)
	}
}

func TestModelNames(t *testing.T) {
	t.Run("a model id reads as its family and version", func(t *testing.T) {
		expect(t, "claude-opus-5-5", "Opus 5.5", "")
		expect(t, "claude-sonnet-5-5", "Sonnet 5.5", "")
		expect(t, "claude-fable-5-1", "Fable 5.1", "")
		expect(t, "claude-opus-6", "Opus 6", "a whole-number release")
		expect(t, "claude-sonnet-5-10", "Sonnet 5.10", "")
	})
	t.Run("a date suffix or a context tag is not part of the version", func(t *testing.T) {
		expect(t, "claude-haiku-4-5-20251001", "Haiku 4.5", "")
		expect(t, "claude-opus-4-6[1m]", "Opus 4.6", "")
	})
	t.Run("anything that is not a Claude model id has no short name", func(t *testing.T) {
		expect(t, "mystery-9", nil, "")
		expect(t, "jev-router", nil, "")
		expect(t, jsjson.Undefined, nil, "")
	})
}
