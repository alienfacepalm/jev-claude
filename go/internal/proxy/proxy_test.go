package proxy

import (
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/router"
	"github.com/alienfacepalm/jev-claude/go/internal/status"
)

// Ported from node/test/proxy.test.mjs.

func ids(models []CatalogModel) string {
	var out []string
	for _, m := range models {
		out = append(out, m.ID)
	}
	return strings.Join(out, ",")
}

func catalog(t *testing.T, s string) []any { return parse(t, s).([]any) }

func prompt(t *testing.T, body string) any {
	t.Helper()
	v, err := NewTurnPrompt(parse(t, body))
	if err != nil {
		t.Fatal(err)
	}
	return v
}

func TestRoutingBasics(t *testing.T) {
	t.Run("only the sentinel model is routed", func(t *testing.T) {
		if !config.IsAuto("jev-router") || config.IsAuto("claude-opus-4-6") || config.IsAuto("claude-haiku-4-5-20251001") || config.IsAuto(jsjson.Undefined) {
			t.Error("isAuto")
		}
	})
	t.Run("the sentinel is not mistaken for a real tier", func(t *testing.T) {
		if config.TierOf("jev-router") != "" {
			t.Error("tierOf(jev-router)")
		}
	})
	t.Run("reads the session id out of Claude Code's metadata", func(t *testing.T) {
		sid := "11111111-2222-4333-8444-555555555555"
		if SessionOf(jsjson.Obj("metadata", metadata(sid))) != sid ||
			SessionOf(jsjson.Obj("metadata", jsjson.Obj("user_id", "not-json"))) != "" || SessionOf(jsjson.NewObject()) != "" {
			t.Error("sessionOf")
		}
	})
}

func TestStatusStore(t *testing.T) {
	t.Run("status round-trips per session and misses cleanly", func(t *testing.T) {
		sid := fmt.Sprintf("test-%d", os.Getpid())
		status.WriteStatus(sid, jsjson.Obj("tier", "opus", "confidence", 0.87, "reason", "jev"))
		if got := jsjson.Stringify(status.ReadStatus(sid)); got != `{"tier":"opus","confidence":0.87,"reason":"jev"}` {
			t.Errorf("%s", got)
		}
		if status.ReadStatus("no-such-session") != nil {
			t.Error("a miss is null")
		}
		status.WriteStatus("", jsjson.Obj("tier", "opus"))
	})

	t.Run("status files are private to their owner", func(t *testing.T) {
		if runtime.GOOS == "windows" {
			t.Skip("modes are no-ops on Windows")
		}
		sid := fmt.Sprintf("perm-%d", os.Getpid())
		status.WriteStatus(sid, jsjson.Obj("tier", "opus"))
		if info, _ := os.Stat(status.Dir); info.Mode().Perm() != 0o700 {
			t.Errorf("dir %v", info.Mode())
		}
		if info, _ := os.Stat(filepath.Join(status.Dir, sid+".json")); info.Mode().Perm() != 0o600 {
			t.Errorf("file %v", info.Mode())
		}
	})

	t.Run("stale status files are pruned and fresh ones kept", func(t *testing.T) {
		os.MkdirAll(status.Dir, 0o700)
		stale := filepath.Join(status.Dir, fmt.Sprintf("stale-%d.json", os.Getpid()))
		fresh := filepath.Join(status.Dir, fmt.Sprintf("fresh-%d.json", os.Getpid()))
		os.WriteFile(stale, []byte("{}"), 0o600)
		os.WriteFile(fresh, []byte("{}"), 0o600)
		old := time.Now().Add(-8 * 24 * time.Hour)
		os.Chtimes(stale, old, old)
		if status.PruneStale(status.StaleAfterMs, status.NowMs()) < 1 {
			t.Error("nothing pruned")
		}
		if _, err := os.Stat(stale); err == nil {
			t.Error("stale file kept")
		}
		if _, err := os.Stat(fresh); err != nil {
			t.Error("fresh file removed")
		}
	})

	t.Run("routing status retains the exact recent Jev exchanges", func(t *testing.T) {
		sid := fmt.Sprintf("history-%d", os.Getpid())
		status.WriteDecision(sid, obj(t, `{"prompt":"first","jev":{"request":{"id":1},"response":{"confidence":0.6}}}`), nil)
		status.WriteDecision(sid, obj(t, `{"prompt":"second","jev":{"request":{"id":2},"response":{"confidence":0.8}}}`), nil)
		st := status.ReadStatus(sid)
		history := jsjson.Prop(st, "history").([]any)
		if jsjson.Prop(st, "prompt") != "second" || len(history) != 2 || jsjson.Prop(history[0], "prompt") != "first" ||
			jsjson.Path(history[0], "jev", "response", "confidence") != 0.6 {
			t.Errorf("%s", jsjson.Stringify(st))
		}
	})
}

func TestModelCatalog(t *testing.T) {
	t.Run("recognises older model versions within a tier", func(t *testing.T) {
		for model, want := range map[any]string{
			"claude-sonnet-4-6": "sonnet", "claude-sonnet-5": "sonnet", "claude-haiku-4-5-20251001": "haiku",
			"claude-opus-4-1": "opus", "claude-fable-5-1[1m]": "fable", "mystery-9": "", jsjson.Undefined: "",
		} {
			if got := config.TierOf(model); got != want {
				t.Errorf("tierOf(%v) = %q", model, got)
			}
		}
	})

	t.Run("keeps available Claude model versions as separate Jev choices", func(t *testing.T) {
		m, err := ClaudeModels(catalog(t, `[{"id":"claude-opus-5-5","display_name":"Claude Opus 5"},{"id":"claude-opus-4-8","display_name":"Claude Opus 4.8"}]`))
		if err != nil || ids(m) != "claude-opus-5-5,claude-opus-4-8" || m[0].Tier != "opus" || m[1].Tier != "opus" {
			t.Errorf("%v %v", m, err)
		}
	})

	newest := func(s string) string {
		m, err := ClaudeModels(catalog(t, s))
		if err != nil {
			t.Fatal(err)
		}
		return ids(NewestPerTier(m))
	}

	t.Run("a new major version is picked as the newest of its tier, dated or not", func(t *testing.T) {
		if got := newest(`[{"id":"claude-opus-5-5"},{"id":"claude-opus-6"},{"id":"claude-opus-4-8"}]`); got != "claude-opus-6" {
			t.Error(got)
		}
		if got := newest(`[{"id":"claude-sonnet-5-5"},{"id":"claude-sonnet-5-10"}]`); got != "claude-sonnet-5-10" {
			t.Error(got)
		}
		if got := newest(`[{"id":"claude-haiku-4-5-20251001"},{"id":"claude-haiku-4-6"}]`); got != "claude-haiku-4-6" {
			t.Error("a date suffix is not a minor version: " + got)
		}
	})

	t.Run("an id in the old version-first naming never outranks a current model", func(t *testing.T) {
		list := `[{"id":"claude-3-7-sonnet-20250219"},{"id":"claude-sonnet-5-5"},{"id":"claude-3-5-haiku-20241022"},{"id":"claude-haiku-4-5-20251001"}]`
		if got := newest(list); got != "claude-sonnet-5-5,claude-haiku-4-5-20251001" {
			t.Error(got)
		}
		if newer, _ := NewerThanCalibrated(catalog(t, list)); len(newer) != 0 {
			t.Errorf("a retired model is not news: %v", newer)
		}
	})

	t.Run("a provider prefix does not hide the version", func(t *testing.T) {
		if got := newest(`[{"id":"anthropic.claude-opus-5-5"},{"id":"anthropic.claude-opus-6"}]`); got != "anthropic.claude-opus-6" {
			t.Error(got)
		}
	})

	t.Run("flags a model newer than the router was calibrated for, and nothing else", func(t *testing.T) {
		for list, want := range map[string]string{
			`[]`: "",
			`[{"id":"claude-opus-5-5"},{"id":"claude-sonnet-5-5"},{"id":"claude-opus-4-8"}]`: "",
			`[{"id":"claude-opus-6"},{"id":"claude-opus-5-5"},{"id":"claude-sonnet-5-5"}]`:   "claude-opus-6",
		} {
			newer, err := NewerThanCalibrated(catalog(t, list))
			if err != nil || strings.Join(newer, ",") != want {
				t.Errorf("%s: %v %v", list, newer, err)
			}
		}
	})

	t.Run("the calibration notice round-trips and reads empty when absent", func(t *testing.T) {
		file := filepath.Join(status.Dir, fmt.Sprintf("calibration-test-%d.json", os.Getpid()))
		if c := status.ReadCalibration(file); len(c.Newer) != 0 || len(c.Models) != 0 || c.At != nil {
			t.Fatalf("%+v", c)
		}
		status.WriteCalibration([]any{"claude-opus-6"}, []any{"claude-opus-6", "claude-sonnet-5-5"}, file)
		read := status.ReadCalibration(file)
		if jsjson.Stringify(read.Newer) != `["claude-opus-6"]` || jsjson.Stringify(read.Models) != `["claude-opus-6","claude-sonnet-5-5"]` {
			t.Fatalf("%+v", read)
		}
		if _, ok := read.At.(float64); !ok {
			t.Fatal("at is a number")
		}
		status.WriteCalibration(nil, nil, file)
		if len(status.ReadCalibration(file).Newer) != 0 {
			t.Fatal("newer cleared")
		}
		os.Remove(file)
	})
}

func TestProxyEndToEnd(t *testing.T) {
	t.Run("a Claude API key reaches Anthropic untouched, on routed and manual requests alike", func(t *testing.T) {
		rec, url := recordingUpstream(t, func(w http.ResponseWriter, r *http.Request, _ any) {
			w.Header().Set("content-type", "application/json")
			if strings.HasPrefix(r.RequestURI, "/v1/models") {
				io.WriteString(w, `{"data":[]}`)
			} else {
				io.WriteString(w, `{"id":"msg_1","type":"message"}`)
			}
		})
		cal := filepath.Join(status.Dir, fmt.Sprintf("calibration-key-test-%d.json", os.Getpid()))
		t.Cleanup(func() { os.Remove(cal) })
		base := startProxy(t, Options{UpstreamURL: url, Route: answer("claude-sonnet-5-5", 0.9), CalibrationFile: cal})

		req, _ := http.NewRequest(http.MethodGet, base+"/v1/models", nil)
		req.Header.Set("x-api-key", "sk-ant-api03-test")
		res, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatal(err)
		}
		res.Body.Close()
		key := map[string]string{"x-api-key": "sk-ant-api03-test"}
		for _, model := range []string{"jev-router", "claude-opus-5-5"} {
			post(t, base+"/v1/messages", jsjson.Obj("model", model, "tools", parse(t, `[{"name":"Bash"}]`), "messages", parse(t, `[{"role":"user","content":"hi"}]`)), key)
		}
		// An auth token, as ANTHROPIC_AUTH_TOKEN or a gateway sends it, passes through the same way.
		post(t, base+"/v1/messages", jsjson.Obj("model", "claude-opus-5-5", "messages", parse(t, `[{"role":"user","content":"hi"}]`)), map[string]string{"authorization": "Bearer gateway-token"})

		all := rec.all()
		var keys []string
		for _, s := range all {
			keys = append(keys, s.header.Get("x-api-key"))
		}
		if strings.Join(keys, ",") != "sk-ant-api03-test,sk-ant-api03-test,sk-ant-api03-test," {
			t.Errorf("keys %v", keys)
		}
		if _, sent := all[3].header["X-Api-Key"]; sent || all[3].header.Get("authorization") != "Bearer gateway-token" {
			t.Errorf("auth %v", all[3].header)
		}
	})

	t.Run("Claude proxy sends exact account models to Jev and routes the chosen version", func(t *testing.T) {
		rec, url := recordingUpstream(t, func(w http.ResponseWriter, r *http.Request, _ any) {
			w.Header().Set("content-type", "application/json")
			if strings.HasPrefix(r.RequestURI, "/v1/models") {
				io.WriteString(w, `{"data":[{"id":"claude-opus-4-8","display_name":"Claude Opus 4.8","created_at":"2026-01-05"},{"id":"claude-opus-5-5","display_name":"Claude Opus 5.5","created_at":"2026-09-02"},{"id":"claude-sonnet-5-5","display_name":"Claude Sonnet 5.5","created_at":"2026-08-11"}]}`)
				return
			}
			io.WriteString(w, `{"id":"msg_1","type":"message","model":"claude-opus-5-5"}`)
		})
		cal := filepath.Join(status.Dir, fmt.Sprintf("calibration-proxy-test-%d.json", os.Getpid()))
		t.Cleanup(func() { os.Remove(cal) })
		var menu string
		base := startProxy(t, Options{UpstreamURL: url, CalibrationFile: cal, Route: func(a router.Args) (*jsjson.Object, error) {
			var list []string
			for _, m := range a.Models {
				list = append(list, m.ID)
			}
			menu = strings.Join(list, ",")
			return jsjson.Obj("choice", "claude-opus-5-5", "confidence", 0.91, "ms", 1.0), nil
		}})
		res, err := http.Get(base + "/v1/models")
		if err != nil {
			t.Fatal(err)
		}
		io.ReadAll(res.Body)
		res.Body.Close()
		recorded := status.ReadCalibration(cal)
		if jsjson.Stringify(recorded.Models) != `["claude-opus-5-5","claude-sonnet-5-5"]` || len(recorded.Newer) != 0 {
			t.Errorf("the account's newest per tier, nothing newer than the tuning: %+v", recorded)
		}
		post(t, base+"/v1/messages", jsjson.Obj("model", "jev-router", "tools", parse(t, `[{"name":"Bash"}]`), "messages", parse(t, `[{"role":"user","content":"debug this race"}]`)), nil)
		if menu != "claude-opus-5-5,claude-sonnet-5-5" {
			t.Errorf("only the newest of each tier is on the menu, newest first: %s", menu)
		}
		var routed []seen
		for _, s := range rec.all() {
			if s.method == http.MethodPost {
				routed = append(routed, s)
			}
		}
		if len(routed) != 1 || modelOf(routed[0]) != "claude-opus-5-5" {
			t.Errorf("%v", routed)
		}
	})

	t.Run("a routed request without metadata is recorded under the conversation key", func(t *testing.T) {
		_, url := recordingUpstream(t, nil)
		base := startProxy(t, Options{UpstreamURL: url, Route: answer("claude-sonnet-5-5", 0.77)})
		// Exactly what `claude -p` sends first: no metadata, so no session id.
		body := jsjson.Obj("model", "jev-router", "tools", parse(t, `[{"name":"Bash"}]`),
			"messages", []any{jsjson.Obj("role", "user", "content", fmt.Sprintf("rename this variable %d", os.Getpid()))})
		post(t, base+"/v1/messages", body, nil)
		if SessionOf(body) != "" {
			t.Fatal("the request carries no session id")
		}
		key, _ := ConversationKey(body)
		st := status.ReadStatus(key)
		if st == nil {
			t.Fatal("the decision is filed under the conversation key instead of being dropped")
		}
		if jsjson.Prop(st, "tier") != "sonnet" || jsjson.Prop(st, "confidence") != 0.77 {
			t.Errorf("%s", jsjson.Stringify(st))
		}
		if jsjson.Prop(st, "effort") != "high" {
			t.Error("records the effort that went out, here Sonnet's own since the request named none")
		}
		if v := status.AgentView(st, 90000, status.NowMs()); v.Main == nil || v.Main.Value("effort") != "high" {
			t.Error("and carries it to the per-agent entry the status line reads")
		}
	})
}

func TestBodyRewriting(t *testing.T) {
	sanitized := func(schema string) string {
		node := parse(t, schema)
		SanitizeSchema(node)
		return jsjson.Stringify(node)
	}
	t.Run("converts a draft-04 boolean exclusiveMinimum into a draft 2020-12 number", func(t *testing.T) {
		if got := sanitized(`{"type":"object","properties":{"topN":{"minimum":0,"exclusiveMinimum":true}}}`); got != `{"type":"object","properties":{"topN":{"exclusiveMinimum":0}}}` {
			t.Error(got)
		}
	})
	t.Run("drops a false exclusiveMaximum and keeps the bound", func(t *testing.T) {
		if got := sanitized(`{"properties":{"n":{"maximum":10,"exclusiveMaximum":false}}}`); got != `{"properties":{"n":{"maximum":10}}}` {
			t.Error(got)
		}
	})
	t.Run("leaves an already-valid numeric bound alone", func(t *testing.T) {
		if got := sanitized(`{"properties":{"n":{"exclusiveMinimum":5}}}`); got != `{"properties":{"n":{"exclusiveMinimum":5}}}` {
			t.Error(got)
		}
	})
	t.Run("reaches schemas nested in arrays and sub-objects", func(t *testing.T) {
		if got := sanitized(`{"anyOf":[{"items":{"minimum":1,"exclusiveMinimum":true}}]}`); got != `{"anyOf":[{"items":{"exclusiveMinimum":1}}]}` {
			t.Error(got)
		}
	})
	t.Run("survives null and primitive nodes", func(t *testing.T) {
		SanitizeSchema(nil)
		if got := sanitized(`{"a":null,"b":3,"c":"x"}`); got != `{"a":null,"b":3,"c":"x"}` {
			t.Error(got)
		}
	})

	tools := `"tools":[{"name":"Bash"}]`
	t.Run("reads a plain string prompt as a new turn", func(t *testing.T) {
		if prompt(t, `{`+tools+`,"messages":[{"role":"user","content":"fix the bug"}]}`) != "fix the bug" {
			t.Fail()
		}
	})
	t.Run("reads a text block prompt as a new turn", func(t *testing.T) {
		if prompt(t, `{`+tools+`,"messages":[{"role":"user","content":[{"type":"text","text":"fix the bug"}]}]}`) != "fix the bug" {
			t.Fail()
		}
	})
	t.Run("hook context after the prompt does not hide the turn", func(t *testing.T) {
		if prompt(t, `{`+tools+`,"messages":[{"role":"user","content":"refactor the parser"},{"role":"system","content":[{"type":"text","text":"SessionStart hook additional context: ..."}]}]}`) != "refactor the parser" {
			t.Fail()
		}
	})
	t.Run("ignores a tool_result continuation mid-turn", func(t *testing.T) {
		if prompt(t, `{`+tools+`,"messages":[{"role":"user","content":"fix the bug"},{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"done"}]}]}`) != nil {
			t.Fail()
		}
	})
	t.Run("ignores auxiliary calls that carry no tools", func(t *testing.T) {
		if prompt(t, `{"messages":[{"role":"user","content":"summarise this"}]}`) != nil {
			t.Fail()
		}
	})
	t.Run("ignores a request whose last message is from the assistant", func(t *testing.T) {
		if prompt(t, `{`+tools+`,"messages":[{"role":"assistant","content":"thinking"}]}`) != nil {
			t.Fail()
		}
	})
	t.Run("ignores an empty prompt", func(t *testing.T) {
		if prompt(t, `{`+tools+`,"messages":[{"role":"user","content":"   "}]}`) != nil {
			t.Fail()
		}
	})
	t.Run("survives a malformed body", func(t *testing.T) {
		for _, b := range []any{jsjson.Undefined, jsjson.NewObject(), parse(t, `{"tools":[],"messages":[]}`)} {
			if v, err := NewTurnPrompt(b); v != nil || err != nil {
				t.Errorf("%v %v", v, err)
			}
		}
	})
	t.Run("strips system reminders Claude Code injects into the prompt", func(t *testing.T) {
		if prompt(t, `{`+tools+`,"messages":[{"role":"user","content":"fix the bug\n<system-reminder>be careful\nabout things</system-reminder>"}]}`) != "fix the bug" {
			t.Fail()
		}
	})
	t.Run("a prompt that is only a system reminder is not a turn", func(t *testing.T) {
		if prompt(t, `{`+tools+`,"messages":[{"role":"user","content":"<system-reminder>noise</system-reminder>"}]}`) != nil {
			t.Fail()
		}
	})

	none := config.MapEnv(nil)
	apply := func(body, tier string, env config.Getenv) string {
		return jsjson.Stringify(ApplyTier(obj(t, body), tier, "", env))
	}
	t.Run("routing to haiku strips fields haiku cannot accept", func(t *testing.T) {
		got := apply(`{"model":"claude-sonnet-4-6","thinking":{"type":"adaptive"},"output_config":{"effort":"medium"},"context_management":{"edits":[{"type":"clear_thinking_20251015","keep":"all"}]}}`, "haiku", none)
		if got != `{"model":"claude-haiku-4-5-20251001"}` {
			t.Error(got)
		}
	})
	t.Run("routing to haiku keeps context-management strategies unrelated to thinking", func(t *testing.T) {
		got := apply(`{"model":"claude-sonnet-4-6","context_management":{"edits":[{"type":"clear_tool_uses_20250919"},{"type":"clear_thinking_20251015"}]}}`, "haiku", none)
		if got != `{"model":"claude-haiku-4-5-20251001","context_management":{"edits":[{"type":"clear_tool_uses_20250919"}]}}` {
			t.Error(got)
		}
	})
	t.Run("routing to opus leaves thinking and effort intact", func(t *testing.T) {
		got := apply(`{"model":"claude-sonnet-4-6","thinking":{"type":"adaptive"},"output_config":{"effort":"medium"}}`, "opus", none)
		if got != `{"model":"claude-opus-5-5","thinking":{"type":"adaptive"},"output_config":{"effort":"medium"}}` {
			t.Error(got)
		}
	})
	t.Run("an unknown tier leaves the request untouched", func(t *testing.T) {
		if got := apply(`{"model":"claude-sonnet-4-6","thinking":{"type":"adaptive"}}`, "nonsense", none); got != `{"model":"claude-sonnet-4-6","thinking":{"type":"adaptive"}}` {
			t.Error(got)
		}
	})
	t.Run("names each tier's own effort when the request does not", func(t *testing.T) {
		if got := apply(`{"model":"jev-router","thinking":{"type":"adaptive"}}`, "opus", none); !strings.HasSuffix(got, `"output_config":{"effort":"medium"}}`) {
			t.Error(got)
		}
		if got := apply(`{"model":"jev-router","thinking":{"type":"adaptive"}}`, "sonnet", none); !strings.HasSuffix(got, `"output_config":{"effort":"high"}}`) {
			t.Error(got)
		}
	})
	t.Run("JEV_<TIER>_EFFORT overrides a tier's effort, and a bad value is ignored", func(t *testing.T) {
		env := func(k, v string) config.Getenv { return config.MapEnv(map[string]string{k: v}) }
		if config.EffortFloor("opus", env("JEV_OPUS_EFFORT", "High")) != "high" ||
			config.EffortFloor("sonnet", env("JEV_SONNET_EFFORT", "low")) != "low" ||
			config.EffortFloor("opus", env("JEV_OPUS_EFFORT", "turbo")) != "medium" ||
			config.EffortFloor("opus", none) != "medium" ||
			config.EffortFloor("haiku", env("JEV_HAIKU_EFFORT", "high")) != "" {
			t.Error("effortFloor")
		}
	})
	t.Run("keeps an effort the request already carries", func(t *testing.T) {
		if got := apply(`{"model":"jev-router","thinking":{"type":"adaptive"},"output_config":{"effort":"low"}}`, "opus", none); !strings.HasSuffix(got, `"output_config":{"effort":"low"}}`) {
			t.Error("the user's own choice outranks the floor: " + got)
		}
	})
	t.Run("never names an effort for a tier that cannot take one", func(t *testing.T) {
		if got := apply(`{"model":"jev-router","output_config":{"effort":"high"}}`, "haiku", none); got != `{"model":"claude-haiku-4-5-20251001"}` {
			t.Error(got)
		}
	})

	// A request captured from the real Claude Code CLI: it sends effort `high` and adaptive thinking.
	captured := func() *jsjson.Object { return clone(t, fixture(t).Value("body")).(*jsjson.Object) }
	effortOf := func(b *jsjson.Object) any { return jsjson.Prop(b.Value("output_config"), "effort") }
	t.Run("JEV_FORCE_EFFORT replaces the effort Claude Code sent, and a per-tier one wins over it", func(t *testing.T) {
		if effortOf(captured()) != "high" {
			t.Fatal("the capture really carries an effort")
		}
		if e := effortOf(ApplyTier(captured(), "opus", "", config.MapEnv(map[string]string{"JEV_FORCE_EFFORT": "low"}))); e != "low" {
			t.Error("outranks the effort Claude Code sent")
		}
		if e := effortOf(ApplyTier(captured(), "opus", "", config.MapEnv(map[string]string{"JEV_FORCE_EFFORT": "low", "JEV_OPUS_FORCE_EFFORT": "xhigh"}))); e != "xhigh" {
			t.Error("the tier setting beats the global one")
		}
		if e := effortOf(ApplyTier(captured(), "sonnet", "", config.MapEnv(map[string]string{"JEV_OPUS_FORCE_EFFORT": "xhigh"}))); e != "high" {
			t.Error("another tier keeps the effort Claude Code sent")
		}
	})
	t.Run("a forced effort is ignored when unrecognised and never reaches Haiku", func(t *testing.T) {
		if e := effortOf(ApplyTier(captured(), "opus", "", config.MapEnv(map[string]string{"JEV_FORCE_EFFORT": "turbo"}))); e != "high" {
			t.Error("a bad value leaves the request alone")
		}
		if ApplyTier(captured(), "haiku", "", config.MapEnv(map[string]string{"JEV_FORCE_EFFORT": "max"})).Has("output_config") {
			t.Error("Haiku takes no effort, forced or not")
		}
		if config.ForcedEffort("haiku", config.MapEnv(map[string]string{"JEV_HAIKU_FORCE_EFFORT": "max"})) != "" ||
			config.ForcedEffort("fable", config.MapEnv(map[string]string{"JEV_FORCE_EFFORT": " Max "})) != "max" {
			t.Error("case and spaces are forgiven")
		}
	})
}

func TestConversations(t *testing.T) {
	key := func(s string) string {
		k, err := ConversationKey(parse(t, s))
		if err != nil {
			t.Fatal(err)
		}
		return k
	}
	t.Run("a conversation keeps one key as it grows, and differs from a sub-agent", func(t *testing.T) {
		main := key(`{"messages":[{"role":"user","content":"main task"}]}`)
		grown := key(`{"messages":[{"role":"user","content":"main task"},{"role":"assistant","content":"ok"}]}`)
		sub := key(`{"messages":[{"role":"user","content":"sub-agent task"}]}`)
		if main != grown || main == sub {
			t.Error(main, grown, sub)
		}
	})
	t.Run("the key ignores the cache_control breakpoint Claude Code moves between requests", func(t *testing.T) {
		first := key(`{"messages":[{"role":"user","content":[{"type":"text","text":"<system-reminder>x</system-reminder>"},{"type":"text","text":"do the thing","cache_control":{"type":"ephemeral","ttl":"1h"}}]}]}`)
		later := key(`{"messages":[{"role":"user","content":[{"type":"text","text":"<system-reminder>x</system-reminder>"},{"type":"text","text":"do the thing"}]},{"role":"assistant","content":"working"}]}`)
		if first != later {
			t.Error(first, later)
		}
	})
	t.Run("the same opening text in two sessions gets two keys", func(t *testing.T) {
		mk := func(id string) string {
			k, _ := ConversationKey(jsjson.Obj("metadata", metadata(id), "messages", parse(t, `[{"role":"user","content":"same opening"}]`)))
			return k
		}
		if mk("a") == mk("b") {
			t.Fail()
		}
	})
	t.Run("the key survives metadata that is not JSON", func(t *testing.T) {
		if _, err := ConversationKey(parse(t, `{"metadata":{"user_id":"not-json"},"messages":[{"role":"user","content":"hi"}]}`)); err != nil {
			t.Fatal(err)
		}
	})

	t.Run("the first tool-bearing conversation in a session is the main thread", func(t *testing.T) {
		mains := NewMains()
		mk := func(text string) any {
			return jsjson.Obj("metadata", metadata("s-main"), "tools", parse(t, `[{"name":"Read"}]`), "messages", []any{jsjson.Obj("role", "user", "content", text)})
		}
		mainKey, _, isMain, _ := AgentOf(mk("the user's opening prompt"), mains)
		subKey, subLabel, subMain, _ := AgentOf(mk("search the repo for conversationKey"), mains)
		_, _, again, _ := AgentOf(mk("the user's opening prompt"), mains)
		if !isMain || subMain || mainKey == subKey || subLabel != "search the repo for conversationKey" || !again {
			t.Errorf("main %v, sub %v %q, main again %v", isMain, subMain, subLabel, again)
		}
	})
	t.Run("auxiliary calls without tools never claim the main slot", func(t *testing.T) {
		mains := NewMains()
		auxKey, _, auxMain, _ := AgentOf(jsjson.Obj("metadata", metadata("s-aux"), "messages", parse(t, `[{"role":"user","content":"summarise this"}]`)), mains)
		realKey, _, realMain, _ := AgentOf(jsjson.Obj("metadata", metadata("s-aux"), "tools", parse(t, `[{"name":"Read"}]`), "messages", parse(t, `[{"role":"user","content":"the real prompt"}]`)), mains)
		k, _ := mains.get("s-aux")
		if auxMain || !realMain || k == auxKey || k != realKey {
			t.Errorf("aux %v real %v slot %s", auxMain, realMain, k)
		}
	})
	t.Run("agent labels are trimmed of reminders and length", func(t *testing.T) {
		label := func(v any) string { l, _ := AgentLabel(v, 48); return l }
		if label(parse(t, `{"messages":[{"role":"user","content":"<system-reminder>noise</system-reminder> real task"}]}`)) != "real task" {
			t.Error("reminder")
		}
		if l := label(jsjson.Obj("messages", []any{jsjson.Obj("role", "user", "content", strings.Repeat("x", 80))})); len([]rune(l)) != 48 {
			t.Errorf("length %d", len([]rune(l)))
		}
		if label(jsjson.NewObject()) != "" {
			t.Error("empty")
		}
	})

	t.Run("each agent's model is recorded separately within one session", func(t *testing.T) {
		sid := fmt.Sprintf("agents-%d", os.Getpid())
		status.WriteDecision(sid, obj(t, `{"tier":"opus","model":"claude-opus-5-5","confidence":0.94,"at":1000}`), &status.Agent{Key: "k-main", Label: "fix the race", Main: true})
		status.WriteDecision(sid, obj(t, `{"tier":"haiku","model":"claude-haiku-4-5","confidence":0.81,"at":2000}`), &status.Agent{Key: "k-sub", Label: "grep for callers"})
		st := status.ReadStatus(sid)
		v := status.AgentView(st, 90000, 2000)
		if v.Main.Value("tier") != "opus" || v.Main.Value("label") != "fix the race" || len(v.Subagents) != 1 || v.Subagents[0].Value("tier") != "haiku" {
			t.Errorf("a sub-agent's choice does not overwrite the main thread: %s", jsjson.Stringify(st))
		}
		if len(jsjson.Prop(st, "history").([]any)) != 2 {
			t.Error("history still records every decision")
		}
	})
	t.Run("stale sub-agents drop out of the live view but the main thread stays", func(t *testing.T) {
		sid := fmt.Sprintf("stale-agents-%d", os.Getpid())
		status.WriteDecision(sid, obj(t, `{"tier":"opus","at":0}`), &status.Agent{Key: "m", Label: "main", Main: true})
		status.WriteDecision(sid, obj(t, `{"tier":"haiku","at":0}`), &status.Agent{Key: "s", Label: "old sub"})
		v := status.AgentView(status.ReadStatus(sid), 90000, 10*60000)
		if v.Main.Value("tier") != "opus" || len(v.Subagents) != 0 {
			t.Error("a sub-agent that has not been routed recently is not live")
		}
	})
	t.Run("a sub-agent pinned to its own model does not pause the session", func(t *testing.T) {
		sid := fmt.Sprintf("manual-agents-%d", os.Getpid())
		status.WriteDecision(sid, obj(t, `{"tier":"opus","model":"claude-opus-5-5","at":1000}`), &status.Agent{Key: "m", Label: "main", Main: true})
		status.MarkManual(sid, "claude-haiku-4-5", &status.Agent{Key: "s", Label: "pinned sub"})
		st := status.ReadStatus(sid)
		v := status.AgentView(st, 90000, status.NowMs())
		if jsjson.Prop(st, "manual") != false || v.Main.Value("tier") != "opus" || v.Subagents[0].Value("manual") != true {
			t.Errorf("%s", jsjson.Stringify(st))
		}
		status.MarkManual(sid, "claude-sonnet-5", &status.Agent{Key: "m", Label: "main", Main: true})
		if jsjson.Prop(status.ReadStatus(sid), "manual") != true {
			t.Error("the main thread picking a model does pause it")
		}
	})
}
