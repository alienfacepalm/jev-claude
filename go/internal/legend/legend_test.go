package legend

import (
	"regexp"
	"strings"
	"testing"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/icons"
)

// Ported from node/test/legend.test.mjs.

func TestLegend(t *testing.T) {
	t.Run("the key explains every mark the status line draws, in the set it is drawing", func(t *testing.T) {
		for _, choice := range []string{"symbols", "text"} {
			set := icons.Icons(config.MapEnv(map[string]string{"JEV_ICONS": choice}), "darwin")
			legend := Format(set)
			for _, e := range set.Entries() {
				if !strings.Contains(legend, e[1]) {
					t.Errorf("%s: %s", choice, e[0])
				}
			}
		}
	})

	t.Run("the symbols are the same ones the status line prints", func(t *testing.T) {
		legend := Format(icons.Icons(config.MapEnv(map[string]string{"JEV_ICONS": "symbols"}), "darwin"))
		if !regexp.MustCompile(`^\x{25C6} +the model`).MatchString(legend) {
			t.Errorf("first row: %q", strings.SplitN(legend, "\n", 2)[0])
		}
		if !regexp.MustCompile(`(?m)\x{E0A0} +the git branch`).MatchString(legend) {
			t.Error("the branch row uses the Powerline glyph")
		}
	})
}
