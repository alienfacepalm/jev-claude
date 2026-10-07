// Package policy turns a Jev answer into the tier that runs (SPEC 7.6), ported from
// node/src/policy.mjs.
package policy

import (
	"regexp"
	"strings"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
)

var carried = []*regexp.Regexp{
	regexp.MustCompile(`<agent-message(?s:.)*?</agent-message>`),
	regexp.MustCompile(`<system-reminder>(?s:.)*?</system-reminder>`),
	regexp.MustCompile("```(?s:.)*?```"),
	regexp.MustCompile("`[^`\\n]*`"),
	regexp.MustCompile(`"[^"\n]*"`),
}

// negatedVerb is an instruction verb a negation sits directly in front of ("do not use",
// "don't switch to", "never use"): the JavaScript `/gi` pattern, written lower-case for text that
// ASCIILower has folded (SPEC 3.1). U+2019 is the typographic apostrophe.
var negatedVerb = regexp.MustCompile(
	`(?:\bnot|\bcannot|n['\x{2019}]t|\bnever|\bno|\bavoid|\bwithout|\bdont)` + jsstr.JSWSClass + `+` +
		`(?:(?:ever|really|actually|just|simply)` + jsstr.JSWSClass + `+)?` +
		`(?:use|switch to|switch over to|route to)`)

// question is a sentence that ends in a question mark: from the previous . ! ? or line break up
// to the mark.
var question = regexp.MustCompile(`[^.!?\n]*\?`)

// blankMatches replaces every match of the lower-case pattern re with one space. The pattern is
// matched against an ASCII-lowered copy (same length, same offsets), and the replacement is
// applied to the original text at those offsets, so only ASCII letters fold, as with a JavaScript
// non-`u` `i` flag.
func blankMatches(re *regexp.Regexp, text string) string {
	locs := re.FindAllStringIndex(jsstr.ASCIILower(text), -1)
	if locs == nil {
		return text
	}
	var b strings.Builder
	last := 0
	for _, loc := range locs {
		b.WriteString(text[last:loc[0]])
		b.WriteByte(' ')
		last = loc[1]
	}
	b.WriteString(text[last:])
	return b.String()
}

// OwnWords is the part of a prompt the user wrote: carried text, negated instructions and
// questions replaced with one space.
func OwnWords(prompt any) string {
	text := jsjson.JSString(jsjson.Coalesce(prompt, ""))
	for _, re := range carried {
		text = re.ReplaceAllLiteralString(text, " ")
	}
	text = blankMatches(negatedVerb, text)
	return blankMatches(question, text)
}

// DetectOverride is the tier the user named explicitly, "" when none.
func DetectOverride(prompt any) string {
	text := OwnWords(prompt)
	for _, p := range config.OverridePatterns {
		if p.Test(text) {
			return p.Tier
		}
	}
	return ""
}

func contains(list []string, s string) bool {
	for _, x := range list {
		if x == s {
			return true
		}
	}
	return false
}

// ClampToAvailable is the nearest tier the account can run, "" when none.
func ClampToAvailable(tier string, available []string) string {
	if contains(available, tier) {
		return tier
	}
	rank := config.RankOf(tier)
	for i, t := range config.TierNames {
		if i > rank && contains(available, t) && (t != "fable" || tier == "fable") {
			return t
		}
	}
	down := ""
	for i, t := range config.TierNames {
		if i < rank && contains(available, t) {
			down = t
		}
	}
	return down
}

// Input is what decide reads.
type Input struct {
	Prompt        any
	Jev           any // the Jev answer with `choice` as a tier name, or nil
	Current       string
	Available     []string
	ContextTokens float64
}

// Decision is the tier that runs and why.
type Decision struct {
	Tier    string
	Reason  string
	Changed bool
}

// Decide is pure and total: any missing or malformed input falls back to the current tier.
func Decide(in Input) Decision {
	settle := func(tier, reason string) Decision {
		final := ClampToAvailable(tier, in.Available)
		if final == "" {
			final = in.Current
		}
		why := reason
		if final != tier {
			why = reason + "+unavailable"
		}
		if final == in.Current {
			why += "/no-change"
		}
		return Decision{Tier: final, Reason: why, Changed: final != in.Current}
	}
	if o := DetectOverride(in.Prompt); o != "" {
		return settle(o, "override")
	}
	choice, ok := jsjson.Prop(in.Jev, "choice").(string)
	if !jsjson.Truthy(in.Jev) || !ok || config.RankOf(choice) < 0 {
		return settle(in.Current, "jev-unavailable")
	}
	if !(jsjson.ToNumber(jsjson.Prop(in.Jev, "confidence")) >= config.MinConfidence) {
		step := max(config.RankOf(choice)-1, config.RankOf(config.UncertainDefault), config.RankOf(in.Current))
		return settle(config.TierNames[step], "low-confidence-default")
	}
	if config.RankOf(choice) < config.RankOf(in.Current) && in.ContextTokens > config.DowngradeMaxContextTokens {
		return settle(in.Current, "downgrade-not-worth-cache-rebuild")
	}
	return settle(choice, "jev")
}
