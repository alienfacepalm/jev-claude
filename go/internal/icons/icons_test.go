package icons

import (
	"testing"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
)

// Ported from node/test/icons.test.mjs.

func env(m map[string]string) config.Getenv { return config.MapEnv(m) }

func isText(s Set) bool { return s.Dir == "dir" }

func TestIcons(t *testing.T) {
	t.Run("symbols everywhere but the legacy Windows console", func(t *testing.T) {
		if isText(Icons(env(nil), "darwin")) || isText(Icons(env(nil), "linux")) {
			t.Error("macOS and Linux get symbols")
		}
		if !isText(Icons(env(nil), "windows")) {
			t.Error("conhost fonts lack the glyphs")
		}
	})
	t.Run("a Windows terminal that announces itself gets symbols", func(t *testing.T) {
		for _, e := range []map[string]string{{"WT_SESSION": "1"}, {"TERM_PROGRAM": "vscode"}, {"TERM_PROGRAM": "mintty"}, {"ConEmuPID": "42"}} {
			if isText(Icons(env(e), "windows")) {
				t.Errorf("%v", e)
			}
		}
	})
	t.Run("JEV_ICONS overrides the guess in both directions", func(t *testing.T) {
		if !isText(Icons(env(map[string]string{"JEV_ICONS": "text"}), "darwin")) ||
			!isText(Icons(env(map[string]string{"JEV_ICONS": "ASCII"}), "darwin")) ||
			isText(Icons(env(map[string]string{"JEV_ICONS": "symbols"}), "windows")) {
			t.Error("override ignored")
		}
	})
	t.Run("every item has a symbol and a word", func(t *testing.T) {
		text, symbols := Icons(env(map[string]string{"JEV_ICONS": "text"}), "darwin"), Icons(env(map[string]string{"JEV_ICONS": "symbols"}), "darwin")
		for i, e := range text.Entries() {
			s := symbols.Entries()[i]
			if s[0] != e[0] || e[1] == "" || s[1] == "" {
				t.Errorf("%s / %s", e[0], s[0])
			}
		}
	})
}
