// Package icons holds the status line's marks (SPEC 12.1), ported from node/src/icons.mjs.
package icons

import (
	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
)

// Set is one table of marks.
type Set struct {
	Model, Effort, Agents, Dir, Branch, Worktree, Context string
}

// Entries returns the marks as name, mark pairs in Node's key order.
func (s Set) Entries() [][2]string {
	return [][2]string{
		{"model", s.Model}, {"effort", s.Effort}, {"agents", s.Agents}, {"dir", s.Dir},
		{"branch", s.Branch}, {"worktree", s.Worktree}, {"context", s.Context},
	}
}

// Symbols are single-colour glyphs; the branch is the Powerline glyph U+E0A0.
var Symbols = Set{
	Model: "\xe2\x9c\xa7\xe2\x9c\xa6", Effort: "\xe2\x97\x94", Agents: "\xe2\x9c\xa6", Dir: "\xe2\x9d\x90",
	Branch: "\xee\x82\xa0", Worktree: "\xe2\x8c\x82", Context: "\xe2\x89\xa1",
}

// Text is the word for each mark, for a console that cannot draw the glyphs.
var Text = Set{Model: "model", Effort: "effort", Agents: "agents", Dir: "dir", Branch: "branch", Worktree: "worktree", Context: "ctx"}

// Icons picks the table: JEV_ICONS symbols or text/ascii, else words only on Windows when no
// modern terminal announces itself.
func Icons(env config.Getenv, goos string) Set {
	choice, _ := env("JEV_ICONS")
	switch jsstr.ASCIILower(choice) {
	case "symbols":
		return Symbols
	case "text", "ascii":
		return Text
	}
	for _, k := range []string{"WT_SESSION", "TERM_PROGRAM", "ConEmuPID"} {
		if v, _ := env(k); v != "" {
			return Symbols
		}
	}
	if goos == "windows" {
		return Text
	}
	return Symbols
}
