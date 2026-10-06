// Package explain draws the explanation panel (SPEC 13), ported from node/src/explain.mjs.
package explain

import (
	"math"
	"regexp"
	"strings"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
	"github.com/alienfacepalm/jev-claude/go/internal/reasons"
	"github.com/alienfacepalm/jev-claude/go/internal/status"
)

const (
	width      = 33
	agentWidth = 52
	vbar       = "\xe2\x94\x82"
	hbar       = "\xe2\x94\x80"
	topLeft    = "\xe2\x94\x8c"
	topRight   = "\xe2\x94\x90"
	botLeft    = "\xe2\x94\x94"
	botRight   = "\xe2\x94\x98"
)

var wsRun = regexp.MustCompile(jsstr.JSWSClass + `+`)

func boxRow(text string, w int) string {
	return vbar + " " + jsstr.PadEnd16(jsstr.Slice16(text, 0, w-2), w-2) + " " + vbar
}

func row(text string) string { return boxRow(text, width) }

func metric(v any) string {
	if f, ok := v.(float64); ok && !math.IsNaN(f) && !math.IsInf(f, 0) {
		return jsjson.ToFixed2(f)
	}
	return "n/a"
}

func wrapped(label string, value any) []string {
	text := jsstr.Trim(wsRun.ReplaceAllLiteralString(label+jsjson.JSString(value), " "))
	var lines []string
	for _, word := range strings.Split(text, " ") {
		if len(lines) == 0 || jsstr.Len16(lines[len(lines)-1]+" "+word) > width-2 {
			lines = append(lines, word)
		} else {
			lines[len(lines)-1] += " " + word
		}
	}
	out := make([]string, len(lines))
	for i, l := range lines {
		out[i] = row(l)
	}
	return out
}

func recommendationOf(s any) string {
	answers := jsjson.Path(s, "jev", "response", "answers")
	choice := jsjson.Coalesce(jsjson.Path(answers, "model", "choice"), jsjson.Path(answers, "model_tier", "choice"))
	if !jsjson.Truthy(choice) {
		return jsjson.JSString(jsjson.Coalesce(jsjson.Prop(s, "tier"), "unknown"))
	}
	if t := config.TierOf(choice); t != "" {
		return t
	}
	return jsjson.JSString(choice)
}

func decisionText(reason any) string {
	if reason == jsjson.Undefined {
		reason = ""
	}
	prefix := ""
	if reasons.IsNoChange(reason) {
		prefix = "kept this model - "
	}
	return prefix + reasons.Long(reason)
}

func percent(confidence any) string {
	return jsjson.NumberToString(jsjson.MathRound(jsjson.ToNumber(confidence)*100)) + "%"
}

func upper(v any) string { return jsstr.ToUpper(jsjson.JSString(v)) }

// FormatExplanation is the panel for one decision.
func FormatExplanation(s any) string {
	if !jsjson.Truthy(s) {
		return "Jev Router: no routing decision has been recorded for this session."
	}
	if jsjson.Truthy(jsjson.Prop(s, "manual")) {
		return "Jev Router: routing is paused because you selected a model manually."
	}
	m := jsjson.Coalesce(jsjson.Prop(s, "metrics"), jsjson.NewObject())
	session := jsjson.Path(s, "jev", "request", "state", "session")
	confidence := "n/a"
	if c := jsjson.Prop(s, "confidence"); !jsjson.IsNullish(c) {
		confidence = percent(c)
	}
	lines := []string{
		topLeft + strings.Repeat(hbar, width) + topRight,
		row("Jev Router"),
		row(""),
		row("Jev request"),
	}
	lines = append(lines, wrapped("Prompt: ", jsjson.Coalesce(jsjson.Prop(s, "prompt"), "not recorded"))...)
	lines = append(lines,
		row("Current model: "+upper(jsjson.Coalesce(jsjson.Prop(session, "current_model"), "unknown"))),
		row("Context tokens: "+jsjson.JSString(jsjson.Coalesce(jsjson.Prop(session, "context_tokens"), "unknown"))),
		row(""),
		row("Jev response"),
		row("Task complexity     "+metric(jsjson.Prop(m, "taskComplexity"))),
		row("Reasoning required  "+metric(jsjson.Prop(m, "reasoningRequired"))),
		row("Tool complexity     "+metric(jsjson.Prop(m, "toolComplexity"))),
		row("Context size        "+metric(jsjson.Prop(m, "contextSize"))),
		row(""),
		row("Recommended tier: "+jsstr.ToUpper(recommendationOf(s))),
		row("Selected model: "+upper(jsjson.Coalesce(jsjson.Coalesce(jsjson.Prop(s, "model"), jsjson.Prop(s, "tier")), "unknown"))),
		row(""),
		row("Confidence: "+confidence),
	)
	lines = append(lines, wrapped("Decision: ", decisionText(jsjson.Prop(s, "reason")))...)
	lines = append(lines, botLeft+strings.Repeat(hbar, width)+botRight)
	return strings.Join(lines, "\n")
}

func age(at any, now float64) string {
	ms := now
	if !jsjson.IsNullish(at) {
		ms = jsjson.ToNumber(at)
	}
	s := math.Max(0, jsjson.MathRound((now-ms)/1000))
	switch {
	case s < 60:
		return jsjson.NumberToString(s) + "s"
	case s < 3600:
		return jsjson.NumberToString(jsjson.MathRound(s/60)) + "m"
	}
	return jsjson.NumberToString(jsjson.MathRound(s/3600)) + "h"
}

// FormatAgents lists every agent routed in the session, "" when there is no per-agent record.
func FormatAgents(s any, now float64) string {
	view := status.AgentView(s, math.Inf(1), now)
	var all []*jsjson.Object
	if view.Main != nil {
		all = append(all, view.Main)
	}
	all = append(all, view.Subagents...)
	if len(all) == 0 {
		return ""
	}
	lines := []string{
		topLeft + strings.Repeat(hbar, agentWidth) + topRight,
		boxRow("Jev Router \xc2\xb7 agents this session", agentWidth),
		boxRow("", agentWidth),
	}
	for _, a := range all {
		role := "sub"
		if jsjson.Truthy(a.Value("main")) {
			role = "main"
		}
		model := jsstr.PadEnd16(upper(jsjson.Coalesce(jsjson.Coalesce(a.Value("model"), a.Value("tier")), "unknown")), 22)
		p := ""
		if jsjson.Truthy(a.Value("manual")) {
			p = "manual"
		} else if c := a.Value("confidence"); !jsjson.IsNullish(c) {
			p = percent(c)
		}
		lines = append(lines, boxRow(jsstr.PadEnd16(role, 5)+" "+model+" "+jsstr.PadEnd16(p, 7)+" "+age(a.Value("at"), now), agentWidth))
		if label := a.Value("label"); jsjson.Truthy(label) && label != "main" {
			lines = append(lines, boxRow("      "+jsjson.JSString(label), agentWidth))
		}
	}
	lines = append(lines, botLeft+strings.Repeat(hbar, agentWidth)+botRight)
	return strings.Join(lines, "\n")
}
