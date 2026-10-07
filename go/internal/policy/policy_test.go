package policy

import (
	"os"
	"path/filepath"
	"regexp"
	"testing"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
)

// Ported from node/test/policy.test.mjs.

var all = []string{"haiku", "sonnet", "opus", "fable"}

func sure(choice string) any   { return jsjson.Obj("choice", choice, "confidence", 0.95) }
func unsure(choice string) any { return jsjson.Obj("choice", choice, "confidence", 0.2) }

func base(jev any) Input {
	return Input{Prompt: "refactor the parser", Jev: jev, Current: "sonnet", Available: all}
}

func with(in Input, f func(*Input)) Input { f(&in); return in }

func has(reason, pattern string) bool { return regexp.MustCompile(pattern).MatchString(reason) }

func TestPolicy(t *testing.T) {
	t.Run("score rubrics contain only API-valid descriptions", func(t *testing.T) {
		q := config.Questions()
		for _, k := range q.Keys() {
			criteria := jsjson.Prop(q.Value(k), "criteria").([]any)
			if len(criteria) > 10 {
				t.Errorf("%s has %d criteria", k, len(criteria))
			}
			for _, c := range criteria {
				if _, ok := c.(string); !ok {
					t.Errorf("%s: %v", k, c)
				}
			}
		}
	})

	t.Run("follows a confident Jev answer", func(t *testing.T) {
		if d := Decide(base(sure("opus"))); d != (Decision{"opus", "jev", true}) {
			t.Errorf("%+v", d)
		}
	})

	t.Run("an explicit user override beats Jev", func(t *testing.T) {
		d := Decide(with(base(sure("opus")), func(i *Input) { i.Prompt = "use haiku to fix this typo" }))
		if d.Tier != "haiku" || d.Reason != "override" {
			t.Errorf("%+v", d)
		}
	})

	t.Run("a sub-agent report quoting an override phrase does not force a model", func(t *testing.T) {
		// Captured from a real session: a test-review report delivered as a message quoted
		// `"use strong" -> opus`, and that turn was forced onto Opus.
		data, err := os.ReadFile(filepath.Join("..", "..", "..", "conformance", "fixtures", "subagent-handback-prompt.txt"))
		if err != nil {
			t.Fatal(err)
		}
		prompt := string(data)
		if DetectOverride(prompt) != "" {
			t.Fatal("the quoted phrase was read as an override")
		}
		d := Decide(with(base(sure("sonnet")), func(i *Input) { i.Prompt = prompt; i.Current = "opus" }))
		if d.Tier != "sonnet" || d.Reason != "jev" {
			t.Errorf("Jev's confident answer is acted on, not the quoted phrase: %+v", d)
		}
	})

	t.Run("detectOverride only fires on a real instruction", func(t *testing.T) {
		for prompt, want := range map[string]string{
			"switch to opus": "opus", "use haiku": "haiku", "use the strong model": "opus",
			"Use Claude Haiku for this one": "haiku", "the opus of his career": "",
		} {
			if got := DetectOverride(prompt); got != want {
				t.Errorf("%q = %q want %q", prompt, got, want)
			}
		}
	})

	t.Run("detectOverride ignores ordinary prose that mentions a tier word", func(t *testing.T) {
		for _, prompt := range []string{
			"help me with fast fourier transform code",
			"replace the polling loop with long polling",
			"the test only fails on fast CI runners",
			"write tests with long input strings",
			"turn on fast refresh in vite",
			"refactor this to rely on strong typing",
			"use haiku-style commit messages",
			"use long variable names",
		} {
			if got := DetectOverride(prompt); got != "" {
				t.Errorf("%q = %q", prompt, got)
			}
		}
	})

	t.Run("a tier named in a negated instruction is not a request for it", func(t *testing.T) {
		for _, prompt := range []string{
			"do not use fable",
			"never use fable for this",
			"please don't switch to fable",
			"we cannot use opus on this account",
			"Don't ever use Opus here",
			"do NOT route to sonnet",
		} {
			if got := DetectOverride(prompt); got != "" {
				t.Errorf("%q = %q", prompt, got)
			}
		}
	})

	t.Run("a negated tier does not hide a real instruction beside it", func(t *testing.T) {
		for prompt, want := range map[string]string{
			"don't use haiku, use opus":                  "opus",
			"do not use fable. use sonnet for the tests": "sonnet",
			"never use haiku for this but use opus":      "opus",
		} {
			if got := DetectOverride(prompt); got != want {
				t.Errorf("%q = %q want %q", prompt, got, want)
			}
		}
	})

	t.Run("a tier named in a question is asking about it, not asking for it", func(t *testing.T) {
		for prompt, want := range map[string]string{
			"why does the planner use opus?":                "",
			"should we switch to haiku for the lint step?":  "",
			"Use opus for the migration. Is that too slow?": "opus",
			"what does the router do when I say\nuse opus":  "opus",
		} {
			if got := DetectOverride(prompt); got != want {
				t.Errorf("%q = %q want %q", prompt, got, want)
			}
		}
	})

	t.Run("a negated or questioned tier lets Jev decide", func(t *testing.T) {
		d := Decide(with(base(sure("opus")), func(i *Input) { i.Prompt = "do not use fable" }))
		if d.Tier != "opus" || d.Reason != "jev" {
			t.Errorf("%+v", d)
		}
	})

	t.Run("keeps the current model when Jev is unreachable", func(t *testing.T) {
		d := Decide(base(nil))
		if d.Tier != "sonnet" || d.Changed || !has(d.Reason, `jev-unavailable`) {
			t.Errorf("%+v", d)
		}
	})

	t.Run("ignores a tier name Jev invented", func(t *testing.T) {
		if d := Decide(base(sure("mystery-9"))); d.Tier != "sonnet" {
			t.Errorf("%+v", d)
		}
	})

	t.Run("an unsure pick of Opus runs one tier lower, on the default", func(t *testing.T) {
		d := Decide(with(base(unsure("opus")), func(i *Input) { i.Current = "haiku" }))
		if d.Tier != "sonnet" || d.Reason != "low-confidence-default" {
			t.Errorf("%+v", d)
		}
	})

	t.Run("never downgrades on a low-confidence answer", func(t *testing.T) {
		d := Decide(base(unsure("haiku")))
		if d.Tier != "sonnet" || !has(d.Reason, `low-confidence-default`) {
			t.Errorf("an unsure downgrade is not a reason to leave the default: %+v", d)
		}
	})

	t.Run("a middling answer is not followed down to a weaker model", func(t *testing.T) {
		middling := Decide(base(jsjson.Obj("choice", "haiku", "confidence", 0.45)))
		if middling.Tier != "sonnet" || !has(middling.Reason, `low-confidence-default`) {
			t.Errorf("%+v", middling)
		}
		if d := Decide(base(jsjson.Obj("choice", "haiku", "confidence", 0.78))); d.Tier != "haiku" {
			t.Errorf("a measured 0.78 pick is followed: %+v", d)
		}
	})

	t.Run("an unsure answer keeps Opus when Opus is already in use", func(t *testing.T) {
		d := Decide(with(base(unsure("haiku")), func(i *Input) { i.Current = "opus" }))
		if d.Tier != "opus" || !has(d.Reason, `no-change`) {
			t.Errorf("%+v", d)
		}
	})

	t.Run("keeps a tier stronger than the default on a low-confidence answer", func(t *testing.T) {
		d := Decide(with(base(unsure("haiku")), func(i *Input) { i.Current = "fable" }))
		if d.Tier != "fable" || !has(d.Reason, `no-change`) {
			t.Errorf("%+v", d)
		}
	})

	t.Run("an answer without a confidence is treated as unsure", func(t *testing.T) {
		d := Decide(with(base(jsjson.Obj("choice", "haiku")), func(i *Input) { i.Current = "haiku" }))
		if d.Tier != "sonnet" || d.Reason != "low-confidence-default" {
			t.Errorf("%+v", d)
		}
	})

	t.Run("an unsure answer runs one tier below its pick", func(t *testing.T) {
		cases := []struct{ current, pick, want string }{{"sonnet", "opus", "sonnet"}, {"sonnet", "fable", "opus"}, {"haiku", "sonnet", "sonnet"}}
		for _, c := range cases {
			if d := Decide(with(base(unsure(c.pick)), func(i *Input) { i.Current = c.current })); d.Tier != c.want {
				t.Errorf("%+v: %+v", c, d)
			}
		}
	})

	t.Run("a low-confidence answer cannot reach fable", func(t *testing.T) {
		d := Decide(with(base(unsure("fable")), func(i *Input) { i.Current = "haiku" }))
		if d.Tier != "opus" || d.Reason != "low-confidence-default" {
			t.Errorf("one step below fable, never fable itself: %+v", d)
		}
	})

	t.Run("still allows a confident upgrade to fable", func(t *testing.T) {
		if d := Decide(base(sure("fable"))); d.Tier != "fable" {
			t.Errorf("%+v", d)
		}
	})

	t.Run("refuses a downgrade once the cache rebuild costs more than it saves", func(t *testing.T) {
		d := Decide(with(base(sure("haiku")), func(i *Input) { i.Current = "opus"; i.ContextTokens = 80000 }))
		if d.Tier != "opus" || !has(d.Reason, `cache-rebuild`) {
			t.Errorf("%+v", d)
		}
	})

	t.Run("allows the same downgrade early in a conversation", func(t *testing.T) {
		if d := Decide(with(base(sure("haiku")), func(i *Input) { i.Current = "opus" })); d.Tier != "haiku" {
			t.Errorf("%+v", d)
		}
	})

	t.Run("substitutes upward when the chosen tier is unavailable", func(t *testing.T) {
		d := Decide(with(base(sure("sonnet")), func(i *Input) { i.Current = "haiku"; i.Available = []string{"haiku", "opus"} }))
		if d.Tier != "opus" || !has(d.Reason, `unavailable`) {
			t.Errorf("%+v", d)
		}
	})

	t.Run("never substitutes upward into paid fable", func(t *testing.T) {
		d := Decide(with(base(sure("opus")), func(i *Input) { i.Current = "haiku"; i.Available = []string{"haiku", "fable"} }))
		if d.Tier != "haiku" {
			t.Errorf("%+v", d)
		}
	})

	t.Run("accepts exact model changes within the same tier", func(t *testing.T) {
		if !config.ShouldUseExactModel("jev/no-change", "opus", "opus") || config.ShouldUseExactModel("low-confidence-default/no-change", "opus", "opus") {
			t.Error("shouldUseExactModel")
		}
	})

	t.Run("fable is on offer by default and can be switched off", func(t *testing.T) {
		offered := func(env map[string]string) bool {
			for _, name := range config.AvailableTiers(config.MapEnv(env)) {
				if name == "fable" {
					return true
				}
			}
			return false
		}
		if !offered(nil) || !offered(map[string]string{"JEV_ALLOW_FABLE": "1"}) {
			t.Error("fable is on by default")
		}
		for _, off := range []string{"0", "false", "No", " off "} {
			if offered(map[string]string{"JEV_ALLOW_FABLE": off}) {
				t.Error(off)
			}
		}
		if got := config.AvailableTiers(config.MapEnv(map[string]string{"JEV_ALLOW_FABLE": "0"})); len(got) != 3 || got[2] != "opus" {
			t.Errorf("%v", got)
		}
	})
}
