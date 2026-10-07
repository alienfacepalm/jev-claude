// Package legend is the status line key (SPEC 13), ported from node/src/legend.mjs.
package legend

import (
	"strings"

	"github.com/alienfacepalm/jev-claude/go/internal/icons"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
)

const ellipsis = "\xe2\x80\xa6"

// Format draws the key with the marks in set, aligned by code points.
func Format(set icons.Set) string {
	rows := [][2]string{
		{set.Model, "the model the last turn ran on, then Jev's confidence in that pick"},
		{set.Effort, "the reasoning effort it ran at (Haiku takes none, so none is shown)"},
		{set.Agents, "the models sub-agents are running on, with +N for any more"},
		{set.Dir, "the directory; left out when the worktree has the same name"},
		{set.Branch, "the git branch, cut with " + ellipsis + " when long; (detached) when none is checked out"},
		{set.Worktree, "the linked git worktree you are working in"},
		{set.Context, "how much of the context window is used"},
		{"\xe2\x98\x9e", "you picked the model with /model, so Jev leaves it alone"},
		{"(why)", "a reason in brackets after the effort, only when the pick is not the obvious one"},
		{"new " + ellipsis, "a newer model than the router was tuned for: run /jev-calibrate"},
	}
	width := 0
	for _, r := range rows {
		if n := jsstr.CodePoints(r[0]); n > width {
			width = n
		}
	}
	lines := make([]string, len(rows))
	for i, r := range rows {
		lines[i] = r[0] + strings.Repeat(" ", width-jsstr.CodePoints(r[0])) + "  " + r[1]
	}
	return strings.Join(lines, "\n")
}
