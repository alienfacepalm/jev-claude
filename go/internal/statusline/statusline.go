// Package statusline renders the line Claude Code shows (SPEC 12), ported from
// node/bin/jev-statusline.mjs.
package statusline

import (
	"regexp"
	"runtime"
	"strings"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/icons"
	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
	"github.com/alienfacepalm/jev-claude/go/internal/modelnames"
	"github.com/alienfacepalm/jev-claude/go/internal/reasons"
	"github.com/alienfacepalm/jev-claude/go/internal/status"
	"github.com/alienfacepalm/jev-claude/go/internal/worktree"
)

const (
	dim       = "\x1b[2m"
	bold      = "\x1b[1m"
	reset     = "\x1b[0m"
	dot       = "\xc2\xb7"
	pause     = "\xe2\x8f\xb8"
	ellipsis  = "\xe2\x80\xa6"
	maxBranch = 28
)

var colors = map[string]string{"haiku": "\x1b[32m", "sonnet": "\x1b[36m", "opus": "\x1b[35m", "fable": "\x1b[33m"}

func colorOf(tier any) string {
	if s, ok := tier.(string); ok {
		return colors[s]
	}
	return ""
}

var familyOf = regexp.MustCompile(`claude-([a-z]+)-`)

// statusTierOf is the status line's own `/claude-([a-z]+)-/` capture, or Undefined.
func statusTierOf(model any) any {
	m := familyOf.FindStringSubmatch(jsjson.JSString(jsjson.Coalesce(model, "")))
	if m == nil {
		return jsjson.Undefined
	}
	return m[1]
}

func clip(v any, max int) string {
	s, ok := v.(string)
	if !ok {
		return jsjson.JSString(v)
	}
	if jsstr.Len16(s) > max {
		return jsstr.Slice16(s, 0, max-1) + ellipsis
	}
	return s
}

// trimTail strips JSWS from the end only.
func trimTail(s string) string {
	full := jsstr.Trim("x" + s)
	return full[1:]
}

func percent(v any) string {
	r := jsjson.MathRound(jsjson.ToNumber(v) * 100)
	if r == 0 {
		r = 0 // -0 prints as 0
	}
	return jsjson.NumberToString(r)
}

func shortOr(model any, fallbacks ...any) string {
	if s, ok := modelnames.ShortName(model); ok {
		return s
	}
	v := jsjson.Undefined
	for _, f := range fallbacks {
		if v = jsjson.Coalesce(v, f); !jsjson.IsNullish(v) {
			break
		}
	}
	return jsjson.JSString(v)
}

// Render builds the line (with its newline) for the given stdin bytes, reading the status
// directory, the real clock and git.
func Render(stdin []byte, env config.Getenv) string {
	set := icons.Icons(env, runtime.GOOS)
	ic := func(mark string) string { return bold + mark + reset }

	var input any = jsjson.NewObject()
	text := jsstr.DecodeBytes(stdin)
	if text == "" {
		text = "{}"
	}
	if v, err := jsjson.Parse(text); err == nil {
		input = v
	}

	st := status.ReadStatus(jsjson.Prop(input, "session_id"))
	dirPath := jsjson.JSString(jsjson.Coalesce(jsjson.Coalesce(jsjson.Path(input, "workspace", "current_dir"), jsjson.Prop(input, "cwd")), ""))
	segments := regexp.MustCompile(`[\\/]`).Split(dirPath, -1)
	dir := segments[len(segments)-1]
	pct := jsjson.MathRound(jsjson.ToNumber(jsjson.Coalesce(jsjson.Path(input, "context_window", "used_percentage"), 0.0)))
	if pct == 0 {
		pct = 0
	}
	view := status.AgentView(st, 90000, status.NowMs())

	mainLine := func(entry any) string {
		p := ""
		if c := jsjson.Prop(entry, "confidence"); !jsjson.IsNullish(c) {
			p = " " + dim + "(" + percent(c) + "%)" + reset
		}
		level := ""
		if e := jsjson.Prop(entry, "effort"); jsjson.Truthy(e) {
			level = " " + dim + dot + reset + " " + ic(set.Effort) + " " + jsjson.JSString(e)
		}
		why := ""
		if said, ok := reasons.Short(jsjson.Prop(entry, "reason")); ok {
			why = " " + dim + "(" + said + ")" + reset
		}
		name := shortOr(jsjson.Prop(entry, "model"), jsjson.Prop(entry, "model"), jsjson.Prop(entry, "tier"))
		return ic(set.Model) + " " + colorOf(jsjson.Prop(entry, "tier")) + name + reset + p + level + why
	}

	var mainEntry any = jsjson.Undefined
	if view.Main != nil {
		mainEntry = view.Main
	}
	routed := dim + "jev: waiting for first prompt" + reset
	switch {
	case jsjson.Truthy(jsjson.Prop(mainEntry, "manual")) || (view.Main == nil && jsjson.Truthy(jsjson.Prop(st, "manual"))):
		shown := jsjson.Coalesce(jsjson.Coalesce(jsjson.Path(input, "model", "display_name"), jsjson.Prop(mainEntry, "model")), "")
		routed = trimTail(dim + pause + " manual" + reset + " " + jsjson.JSString(shown))
	case view.Main != nil:
		routed = mainLine(view.Main)
	case st != nil:
		routed = mainLine(st)
	}

	agents := ""
	if n := len(view.Subagents); n > 0 {
		shown := view.Subagents
		if n > 3 {
			shown = shown[:3]
		}
		var names []string
		for _, a := range shown {
			tier := jsjson.Coalesce(a.Value("tier"), statusTierOf(a.Value("model")))
			mark := ""
			if jsjson.Truthy(a.Value("manual")) {
				mark = pause
			}
			name := shortOr(a.Value("model"), a.Value("tier"), a.Value("model"), "?")
			names = append(names, colorOf(tier)+mark+name+reset)
		}
		if n > len(shown) {
			names = append(names, dim+"+"+jsjson.NumberToString(float64(n-len(shown)))+reset)
		}
		agents = " " + dim + dot + reset + " " + ic(set.Agents) + " " + strings.Join(names, dim+","+reset)
	}

	cal := status.ReadCalibration(status.CalibrationFile())
	notice := ""
	if len(cal.Newer) > 0 {
		more := ""
		if len(cal.Newer) > 1 {
			more = " +" + jsjson.NumberToString(float64(len(cal.Newer)-1))
		}
		notice = " " + dim + dot + reset + " \x1b[33mnew " + jsjson.JSString(cal.Newer[0]) + more + ": /jev-calibrate" + reset
	}

	loc := worktree.LocationInfo(input, worktree.GitBranch)
	where := ""
	var locWorktree any = jsjson.Undefined
	if loc != nil {
		locWorktree = loc.Worktree
		if !jsjson.IsNullish(loc.Branch) {
			b := loc.Branch
			if !jsjson.Truthy(b) {
				b = "(detached)"
			}
			where += " " + dim + dot + reset + " " + ic(set.Branch) + " \x1b[34m" + clip(b, maxBranch) + reset
		}
		if jsjson.Truthy(loc.Worktree) {
			where += " " + dim + dot + reset + " " + ic(set.Worktree) + " \x1b[32m" + jsjson.JSString(loc.Worktree) + reset
		}
	}
	dirPart := ""
	if dir != "" && dir != locWorktree {
		dirPart = " " + dim + dot + reset + " " + ic(set.Dir) + " " + dir
	}
	return routed + agents + dirPart + where + " " + dim + dot + reset + " " + ic(set.Context) + " " + jsjson.NumberToString(pct) + "%" + notice + "\n"
}
