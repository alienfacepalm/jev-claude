// Package modelnames reads a model's short display name from its id (SPEC 12.3), ported
// from node/src/model-names.mjs.
package modelnames

import (
	"regexp"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
)

var pattern = regexp.MustCompile(`claude-([a-z]+)-(\d+)`)

// MinorAfter applies the `(?:-(\d{1,2})(?!\d))?` tail by hand (SPEC 3.1): when text[end:]
// is "-" plus a maximal digit run of one or two digits, that run is the minor; otherwise
// there is none and the match still stands.
func MinorAfter(text string, end int) string {
	if end >= len(text) || text[end] != '-' {
		return ""
	}
	j := end + 1
	for j < len(text) && text[j] >= '0' && text[j] <= '9' {
		j++
	}
	if n := j - end - 1; n >= 1 && n <= 2 {
		return text[end+1 : j]
	}
	return ""
}

// ShortName is "Opus 5.5" for claude-opus-5-5; ok is false when nothing matches.
func ShortName(model any) (string, bool) {
	text := jsjson.JSString(jsjson.Coalesce(model, ""))
	m := pattern.FindStringSubmatchIndex(text)
	if m == nil {
		return "", false
	}
	family, major := text[m[2]:m[3]], text[m[4]:m[5]]
	name := string(family[0]-32) + family[1:] + " " + major
	if minor := MinorAfter(text, m[1]); minor != "" {
		name += "." + minor
	}
	return name, true
}
