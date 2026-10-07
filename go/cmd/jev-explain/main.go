// Command jev-explain prints the explanation panel for a session (SPEC 13).
package main

import (
	"os"

	"github.com/alienfacepalm/jev-claude/go/internal/explain"
	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
	"github.com/alienfacepalm/jev-claude/go/internal/status"
)

func main() {
	id := jsjson.Undefined
	if len(os.Args) > 1 {
		id = os.Args[1]
	}
	st := status.ReadStatus(id)
	decision := status.MainDecision(st)
	shown := decision
	if jsjson.Truthy(decision) && jsjson.Truthy(jsjson.Prop(st, "manual")) {
		o := jsjson.NewObject()
		jsjson.Spread(o, decision)
		o.Set("manual", true)
		shown = o
	}
	out := explain.FormatExplanation(shown) + "\n"
	if agents := explain.FormatAgents(st, status.NowMs()); agents != "" {
		out += agents + "\n"
	}
	os.Stdout.WriteString(jsstr.ToUTF8(out))
}
