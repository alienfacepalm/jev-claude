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
	var id any = jsjson.Undefined
	if len(os.Args) > 1 {
		id = os.Args[1]
	}
	st := status.ReadStatus(id)
	main := status.MainDecision(st)
	shown := main
	if jsjson.Truthy(main) && jsjson.Truthy(jsjson.Prop(st, "manual")) {
		o := jsjson.NewObject()
		jsjson.Spread(o, main)
		o.Set("manual", true)
		shown = o
	}
	out := explain.FormatExplanation(shown) + "\n"
	if agents := explain.FormatAgents(st, status.NowMs()); agents != "" {
		out += agents + "\n"
	}
	os.Stdout.WriteString(jsstr.ToUTF8(out))
}
