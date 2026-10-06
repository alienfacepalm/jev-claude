// Package config holds every routing knob (SPEC 4), ported from node/src/config.mjs. Prose
// sent to Jev is copied byte for byte.
package config

import (
	"os"
	"regexp"
	"strings"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
)

// Getenv reads one environment variable; os.LookupEnv is the process environment.
type Getenv = func(string) (string, bool)

// ProcessEnv is the live process environment.
var ProcessEnv Getenv = os.LookupEnv

// MapEnv is an environment held in a map, for tests and callers that pass one.
func MapEnv(m map[string]string) Getenv {
	return func(k string) (string, bool) {
		v, ok := m[k]
		return v, ok
	}
}

// Tier is one model tier.
type Tier struct {
	Name, ID, Family string
	Thinking, Effort bool
	Floor            string // "" when the tier has no effort floor
}

// Tiers, cheapest first.
var Tiers = []Tier{
	{Name: "haiku", ID: "claude-haiku-4-5-20251001", Family: "haiku"},
	{Name: "sonnet", ID: "claude-sonnet-5-5", Family: "sonnet", Thinking: true, Effort: true, Floor: "high"},
	{Name: "opus", ID: "claude-opus-5-5", Family: "opus", Thinking: true, Effort: true, Floor: "medium"},
	{Name: "fable", ID: "claude-fable-5-1", Family: "fable", Thinking: true, Effort: true, Floor: "high"},
}

// Efforts are the effort levels the API accepts.
var Efforts = []string{"low", "medium", "high", "xhigh", "max"}

// TierNames lists the tier names in order.
var TierNames = []string{"haiku", "sonnet", "opus", "fable"}

func isEffort(s string) bool {
	for _, e := range Efforts {
		if e == s {
			return true
		}
	}
	return false
}

// TierSpec returns the named tier.
func TierSpec(name string) (Tier, bool) {
	for _, t := range Tiers {
		if t.Name == name {
			return t, true
		}
	}
	return Tier{}, false
}

// RankOf is the tier's position, -1 when unknown.
func RankOf(name string) int {
	for i, n := range TierNames {
		if n == name {
			return i
		}
	}
	return -1
}

// IDOf is the tier's model id, "" when unknown.
func IDOf(name string) string {
	t, _ := TierSpec(name)
	return t.ID
}

// AutoModel is the sentinel model id offered in Claude Code's /model picker.
const AutoModel = "jev-router"

// IsAuto reports strict equality with the sentinel.
func IsAuto(model any) bool { return model == AutoModel }

// TierOf is the first tier whose family is a substring of model, "" when none (or not a string).
func TierOf(model any) string {
	s, ok := model.(string)
	if !ok {
		return ""
	}
	for _, t := range Tiers {
		if strings.Contains(s, t.Family) {
			return t.Name
		}
	}
	return ""
}

// lowerEnv is `env[key]?.trim().toLowerCase()`; ok is false when the key is undefined.
// Lower-casing is ASCII-only, which gives the same membership answers for these words.
func lowerEnv(env Getenv, key string) (string, bool) {
	v, ok := env(key)
	if !ok {
		return "", false
	}
	return jsstr.ASCIILower(jsstr.Trim(v)), true
}

// EffortFloor is the effort a tier gets when the request names none, "" when the tier has
// no floor.
func EffortFloor(name string, env Getenv) string {
	t, ok := TierSpec(name)
	if !ok || t.Floor == "" {
		return ""
	}
	if chosen, ok := lowerEnv(env, "JEV_"+strings.ToUpper(name)+"_EFFORT"); ok && isEffort(chosen) {
		return chosen
	}
	return t.Floor
}

// ForcedEffort is an effort that replaces the request's own, "" when none is forced.
func ForcedEffort(name string, env Getenv) string {
	t, ok := TierSpec(name)
	if !ok || !t.Effort {
		return ""
	}
	for _, key := range []string{"JEV_" + strings.ToUpper(name) + "_FORCE_EFFORT", "JEV_FORCE_EFFORT"} {
		if chosen, ok := lowerEnv(env, key); ok && isEffort(chosen) {
			return chosen
		}
	}
	return ""
}

var fableOff = regexp.MustCompile(`^(0|false|no|off)$`)

// FableAllowed is false only when JEV_ALLOW_FABLE, trimmed, is 0/false/no/off (any case).
func FableAllowed(env Getenv) bool {
	v, _ := env("JEV_ALLOW_FABLE")
	return !fableOff.MatchString(jsstr.ASCIILower(jsstr.Trim(v)))
}

// AvailableTiers is every tier name, minus fable when it is switched off.
func AvailableTiers(env Getenv) []string {
	var out []string
	for _, n := range TierNames {
		if n != "fable" || FableAllowed(env) {
			out = append(out, n)
		}
	}
	return out
}

// Thresholds (SPEC 4.3).
const (
	MinConfidence             = 0.6
	UncertainDefault          = "sonnet"
	DowngradeMaxContextTokens = 20000
	JevTimeoutMs              = 1500
	JevDeadlineMs             = 3000
	JevMaxRetries             = 1
	ContextWindowTokens       = 200000
)

var complexityScale = []string{
	"None",
	"Very low",
	"Low",
	"Some",
	"Moderate",
	"Moderate to high",
	"High",
	"Very high",
	"Severe",
	"Extreme",
}

// ComplexityMaxScore is the top of the score rubric.
var ComplexityMaxScore = float64(len(complexityScale) - 1)

func scale() []any {
	out := make([]any, len(complexityScale))
	for i, s := range complexityScale {
		out[i] = s
	}
	return out
}

func score(instructions string) *jsjson.Object {
	return jsjson.Obj("type", "score", "instructions", instructions, "criteria", scale())
}

// Questions returns the three score questions, in order, as a fresh object.
func Questions() *jsjson.Object {
	return jsjson.Obj(
		"task_complexity", score("How complex is the coding task overall, including ambiguity, scope, and blast radius?"),
		"reasoning_required", score("How much reasoning is required to complete the request correctly in one pass?"),
		"tool_complexity", score("How complex is the tool use required, from no tools to many coordinated or stateful operations?"),
	)
}

type guidance struct {
	what    string
	signals []string
	notFor  string
}

var guide = map[string]guidance{
	"haiku": {
		what:    "Trivial, mechanical, or purely factual work.",
		signals: []string{"Rename, reformat, comment, or run one obvious command"},
		notFor:  "Design judgement or multi-file reasoning.",
	},
	"sonnet": {
		what: "Well-scoped everyday work, and documents and knowledge work, where it does well for well under Opus's cost.",
		signals: []string{
			"Implement a specified function or change, add tests, or fix a bug whose cause is already known",
			"Write or edit documents, specs, summaries, or analysis",
		},
		notFor: "Open-ended or multi-step coding, changes that must not break existing behaviour, unknown-cause debugging, or judgement calls: Opus scores 5-21 points higher on agentic coding.",
	},
	"opus": {
		what: "Complex or open-ended coding and work that needs sustained judgement, where it clearly beats Sonnet, for about twice the cost per task.",
		signals: []string{
			"Unknown-cause or intermittent bugs, multi-step changes across a codebase, cross-module design, API or behaviour-preserving changes, security, auth, concurrency, or migrations",
		},
		notFor: "Well-scoped changes and routine document or knowledge work, where Sonnet does well for less.",
	},
	"fable": {
		what: "Long-horizon autonomous work, the most demanding reasoning, and adversarial review that hardens a plan, spec, or design by hunting for how it fails.",
		signals: []string{
			"Long-horizon autonomous work: a whole-repo migration, a large system built end to end from a spec, or a full deliverable such as financial analysis with spreadsheets and slides",
			"Adversarially review, red-team, stress-test, or poke holes in a plan, spec, or design to harden it",
			"A problem that has already defeated a strong model, such as a bug two attempts have missed",
		},
		notFor: "Writing the plan or spec itself, ordinary code review, or security-focused analysis, where Fable's safety classifiers can decline.",
	},
}

var cost = map[string]string{
	"haiku":  "$1 / $5 per million input / output tokens; the cheapest, but it does no reasoning",
	"sonnet": "$2 / $10 per million tokens; about 40-60% less per completed task than Opus",
	"opus":   "$4 / $20 per million tokens; about 1.6-2.6x Sonnet's cost per completed task",
	"fable":  "$10 / $50 per million tokens; the most expensive by far",
}

var choiceInstructions = []string{
	"Pick the cheapest exact model that can fully complete this coding request in one pass, without retrying on a stronger model.",
	"When you are unsure whether a cheaper model would get it right, pick the stronger one: a failed attempt wastes the whole turn and is rerun anyway, so it costs more than the difference. Speed does not matter.",
	"Each tier is offered as its newest version only. Judge required reasoning, not requested reply length.",
	"Reasoning effort is set per tier and is not something to choose between; judge only which model the work needs.",
	"Changing tier mid-conversation discards the prompt cache and re-reads the whole history, so prefer the current model where the work has not changed shape.",
}

// Model is one exact model offered to Jev.
type Model struct {
	ID, Tier, ReleasedAt, Description string
	// NoDescription marks a model whose description is undefined, so `description ?? id` is the id.
	NoDescription bool
}

// QuestionForModels builds the Jev choice over the exact models given.
func QuestionForModels(models []Model) *jsjson.Object {
	instructions := make([]any, len(choiceInstructions))
	for i, s := range choiceInstructions {
		instructions[i] = s
	}
	criteria := jsjson.NewObject()
	for _, m := range models {
		desc := m.Description
		if m.NoDescription {
			desc = m.ID
		}
		entry := jsjson.Obj("model", desc)
		if c, ok := cost[m.Tier]; ok {
			entry.Set("cost", c)
		} else {
			entry.Set("cost", jsjson.Undefined)
		}
		if g, ok := guide[m.Tier]; ok {
			signals := make([]any, len(g.signals))
			for i, s := range g.signals {
				signals[i] = s
			}
			entry.Set("what", g.what)
			entry.Set("signals", signals)
			entry.Set("not_for", g.notFor)
		}
		criteria.Set(m.ID, entry)
	}
	return jsjson.Obj("type", "choice", "instructions", instructions, "criteria", criteria)
}

// ShouldUseExactModel reports whether policy accepted Jev's exact model.
func ShouldUseExactModel(reason, chosenTier, finalTier string) bool {
	return (reason == "jev" || reason == "jev/no-change") && chosenTier == finalTier
}

// OverridePattern is one tier's explicit-request pattern (SPEC 4.5).
type OverridePattern struct {
	Tier string
	re   *regexp.Regexp // lower-case, without the leading \b and the trailing (?![-\w])
}

var overrideNames = map[string][2]string{
	"haiku":  {"haiku", "fast"},
	"sonnet": {"sonnet", "balanced"},
	"opus":   {"opus", "strong"},
	"fable":  {"fable", "long"},
}

// OverridePatterns, in tier order.
var OverridePatterns = func() []OverridePattern {
	ws := jsstr.JSWSClass
	var out []OverridePattern
	for _, t := range Tiers {
		n := overrideNames[t.Name]
		src := `(?:use|switch to|switch over to|route to)` + ws + `+(?:the` + ws + `+)?(?:claude(?:-|` + ws + `))?` +
			`(?:(?:` + n[0] + `)|` + n[1] + ws + `+(?:model|tier))`
		out = append(out, OverridePattern{Tier: t.Name, re: regexp.MustCompile(src)})
	}
	return out
}()

func isWordByte(c byte) bool {
	return c == '_' || (c >= '0' && c <= '9') || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')
}

// Test reports whether the pattern matches text the way the JavaScript `i`-flagged pattern
// with its `\b` and `(?![-\w])` would (SPEC 3.1): ASCII-lowered text, a restart at the failed
// match's start + 1, and both assertions checked against the real neighbouring characters.
func (p OverridePattern) Test(text string) bool {
	lower := jsstr.ASCIILower(text)
	for pos := 0; pos <= len(lower); {
		loc := p.re.FindStringIndex(lower[pos:])
		if loc == nil {
			return false
		}
		start, end := pos+loc[0], pos+loc[1]
		boundary := start == 0 || !isWordByte(lower[start-1])
		ahead := end == len(lower) || !(lower[end] == '-' || isWordByte(lower[end]))
		if boundary && ahead {
			return true
		}
		pos = start + 1
	}
	return false
}
