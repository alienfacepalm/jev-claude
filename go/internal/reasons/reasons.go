// Package reasons says a routing decision in words a person reads (SPEC 12.4), ported from
// node/src/reasons.mjs.
package reasons

import "strings"

type reason struct {
	match string
	short string // "" means null
	long  string
}

var table = []reason{
	{match: "override", long: "you named this model in the prompt"},
	{match: "jev-unavailable", short: "router offline", long: "the router could not be reached, so the model was left alone"},
	{match: "low-confidence-default", long: "the router was unsure, so this ran one tier below its pick, and no lower than the default model"},
	{match: "downgrade-not-worth-cache-rebuild", short: "keeping the cache", long: "a cheaper model would have to re-read the whole conversation, which costs more than it saves"},
	{match: "unavailable", short: "nearest available", long: "the chosen tier is not available on this account, so the nearest one was used"},
}

func find(r any) (reason, bool) {
	s, ok := r.(string)
	if !ok || s == "" {
		return reason{}, false
	}
	for _, x := range table {
		if strings.Contains(s, x.match) {
			return x, true
		}
	}
	return reason{}, false
}

// Short is the status line's few words; ok is false when the decision speaks for itself.
func Short(r any) (string, bool) {
	x, ok := find(r)
	if !ok || x.short == "" {
		return "", false
	}
	return x.short, true
}

// Long is the explanation panel's sentence.
func Long(r any) string {
	if x, ok := find(r); ok {
		return x.long
	}
	return "the router's recommendation"
}

// IsNoChange reports whether a decision left the model where it was.
func IsNoChange(r any) bool {
	s, ok := r.(string)
	return ok && strings.Contains(s, "no-change")
}
