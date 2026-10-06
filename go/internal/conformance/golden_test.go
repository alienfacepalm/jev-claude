package conformance

import (
	"io"
	"math"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"testing"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/env"
	"github.com/alienfacepalm/jev-claude/go/internal/explain"
	"github.com/alienfacepalm/jev-claude/go/internal/icons"
	"github.com/alienfacepalm/jev-claude/go/internal/jev"
	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
	"github.com/alienfacepalm/jev-claude/go/internal/launch"
	"github.com/alienfacepalm/jev-claude/go/internal/legend"
	"github.com/alienfacepalm/jev-claude/go/internal/modelnames"
	"github.com/alienfacepalm/jev-claude/go/internal/policy"
	"github.com/alienfacepalm/jev-claude/go/internal/proxy"
	"github.com/alienfacepalm/jev-claude/go/internal/reasons"
	"github.com/alienfacepalm/jev-claude/go/internal/router"
	"github.com/alienfacepalm/jev-claude/go/internal/status"
	"github.com/alienfacepalm/jev-claude/go/internal/update"
	"github.com/alienfacepalm/jev-claude/go/internal/worktree"
)

// in reads an input member.
func in(c Case, key string) any { return jsjson.Prop(c.Input, key) }

// envOf turns a case's env object into a lookup.
func envOf(v any) config.Getenv {
	m := map[string]string{}
	if o, ok := v.(*jsjson.Object); ok {
		for _, k := range o.Keys() {
			if s, ok := o.Value(k).(string); ok {
				m[k] = s
			}
		}
	}
	return config.MapEnv(m)
}

func orNull(s string, ok bool) any {
	if !ok {
		return nil
	}
	return s
}

func emptyNull(s string) any {
	if s == "" {
		return nil
	}
	return s
}

func strs(v any) []string {
	var out []string
	for _, e := range v.([]any) {
		if s, ok := e.(string); ok {
			out = append(out, s)
		} else {
			out = append(out, "\x00not-a-string")
		}
	}
	return out
}

func modelObjects(models []proxy.CatalogModel) []any {
	out := []any{}
	for _, m := range models {
		out = append(out, jsjson.Obj("id", m.ID, "tier", m.Tier, "releasedAt", m.Released, "description", m.Description))
	}
	return out
}

func anyList(ids []string) []any {
	out := []any{}
	for _, s := range ids {
		out = append(out, s)
	}
	return out
}

func iconSet(v any) icons.Set {
	get := func(k string) string { s, _ := jsjson.Prop(v, k).(string); return s }
	return icons.Set{Model: get("model"), Effort: get("effort"), Agents: get("agents"), Dir: get("dir"), Branch: get("branch"), Worktree: get("worktree"), Context: get("context")}
}

func setObject(s icons.Set) *jsjson.Object {
	o := jsjson.NewObject()
	for _, e := range s.Entries() {
		o.Set(e[0], e[1])
	}
	return o
}

func platform(v any) string {
	switch v {
	case "win32":
		return "windows"
	case "darwin":
		return "darwin"
	}
	return "linux"
}

func toNumberOr(v any, dflt float64) float64 {
	if v == jsjson.Undefined {
		return dflt
	}
	return jsjson.ToNumber(v)
}

func viewObject(v status.View) *jsjson.Object {
	var main any = nil
	if v.Main != nil {
		main = v.Main
	}
	subs := []any{}
	for _, s := range v.Subagents {
		subs = append(subs, s)
	}
	return jsjson.Obj("main", main, "subagents", subs)
}

// run computes a case's actual result.
type run func(t *testing.T, c Case) any

var runners = map[string]run{
	"detect-override.json": func(t *testing.T, c Case) any { return emptyNull(policy.DetectOverride(in(c, "prompt"))) },
	"decide.json": func(t *testing.T, c Case) any {
		d := policy.Decide(policy.Input{
			Prompt:        in(c, "prompt"),
			Jev:           in(c, "jev"),
			Current:       in(c, "current").(string),
			Available:     strs(in(c, "available")),
			ContextTokens: toNumberOr(in(c, "contextTokens"), 0),
		})
		return jsjson.Obj("tier", d.Tier, "reason", d.Reason, "changed", d.Changed)
	},
	"effort-floor.json": func(t *testing.T, c Case) any {
		return emptyNull(config.EffortFloor(in(c, "name").(string), envOf(in(c, "env"))))
	},
	"forced-effort.json": func(t *testing.T, c Case) any {
		return emptyNull(config.ForcedEffort(in(c, "name").(string), envOf(in(c, "env"))))
	},
	"fable-allowed.json": func(t *testing.T, c Case) any { return config.FableAllowed(envOf(in(c, "env"))) },
	"new-turn-prompt.json": func(t *testing.T, c Case) any {
		v, err := proxy.NewTurnPrompt(in(c, "body"))
		if err != nil {
			return Throws
		}
		return v
	},
	"agent-label.json": func(t *testing.T, c Case) any {
		max := 48
		if m, ok := in(c, "max").(float64); ok {
			max = int(m)
		}
		v, err := proxy.AgentLabel(in(c, "body"), max)
		if err != nil {
			return Throws
		}
		return v
	},
	"apply-tier.json": func(t *testing.T, c Case) any {
		model, _ := in(c, "model").(string)
		return proxy.ApplyTier(in(c, "body").(*jsjson.Object), in(c, "tier").(string), model, envOf(in(c, "env")))
	},
	"sanitize-schema.json": func(t *testing.T, c Case) any {
		node := in(c, "node")
		proxy.SanitizeSchema(node)
		return node
	},
	"version-of.json": func(t *testing.T, c Case) any {
		v := proxy.VersionOf(in(c, "id"), in(c, "tier"))
		return []any{v[0], v[1]}
	},
	"short-name.json": func(t *testing.T, c Case) any { return orNull(modelnames.ShortName(in(c, "model"))) },
	"claude-models.json": func(t *testing.T, c Case) any {
		m, err := proxy.ClaudeModels(in(c, "catalog").([]any))
		if err != nil {
			return Throws
		}
		return modelObjects(m)
	},
	"newest-per-tier.json": func(t *testing.T, c Case) any {
		out := proxy.FirstPerTier(in(c, "models").([]any), func(m any) any { return jsjson.Prop(m, "tier") })
		if out == nil {
			return []any{}
		}
		return out
	},
	"newer-than-calibrated.json": func(t *testing.T, c Case) any {
		ids, err := proxy.NewerThanCalibrated(in(c, "catalog").([]any))
		if err != nil {
			return Throws
		}
		return anyList(ids)
	},
	"session-of.json": func(t *testing.T, c Case) any { return proxy.SessionOf(in(c, "body")) },
	"conversation-key.json": func(t *testing.T, c Case) any {
		k, err := proxy.ConversationKey(in(c, "body"))
		if err != nil {
			return Throws
		}
		return k
	},
	"agent-of.json": func(t *testing.T, c Case) any {
		mains := proxy.NewMains()
		results := []any{}
		for _, body := range in(c, "steps").([]any) {
			key, label, main, err := proxy.AgentOf(body, mains)
			if err != nil {
				results = append(results, Throws)
				continue
			}
			results = append(results, jsjson.Obj("key", key, "label", label, "main", main))
		}
		pairs := []any{}
		for _, e := range mains.Entries() {
			pairs = append(pairs, []any{e[0], e[1]})
		}
		return jsjson.Obj("results", results, "mains", pairs)
	},
	"write-decision.json": func(t *testing.T, c Case) any {
		status.Dir = t.TempDir()
		// The generator pins Date.now to 1_800_000_000_000 while a sequence runs. Its first write
		// also runs the once-per-process pruneStale under that clock, which deletes the file just
		// written (see the first case's null first step); pinning the same clock reproduces it.
		savedNow := status.NowMs
		status.NowMs = func() float64 { return 1_800_000_000_000 }
		defer func() { status.NowMs = savedNow }()
		files := []any{}
		for _, s := range in(c, "steps").([]any) {
			var agent *status.Agent
			if a, ok := jsjson.Prop(s, "agent").(*jsjson.Object); ok {
				agent = &status.Agent{Key: a.Value("key").(string), Label: jsjson.JSString(a.Value("label")), Main: jsjson.Truthy(a.Value("main"))}
			}
			id := jsjson.Prop(s, "id")
			switch jsjson.Prop(s, "op") {
			case "writeDecision":
				if err := status.WriteDecision(id, jsjson.Prop(s, "decision").(*jsjson.Object), agent); err != nil {
					t.Errorf("writeDecision: %v", err)
				}
			case "markManual":
				status.MarkManual(id, jsjson.Prop(s, "model"), agent)
			}
			files = append(files, jsjson.Obj("file", status.ReadStatus(id)))
		}
		return files
	},
	"agent-view.json": func(t *testing.T, c Case) any {
		return viewObject(status.AgentView(in(c, "status"), toNumberOr(in(c, "freshMs"), 90000), jsjson.ToNumber(in(c, "now"))))
	},
	"main-decision.json": func(t *testing.T, c Case) any { return status.MainDecision(in(c, "status")) },
	"icons.json": func(t *testing.T, c Case) any {
		return setObject(icons.Icons(envOf(in(c, "env")), platform(in(c, "platform"))))
	},
	"reasons.json": func(t *testing.T, c Case) any {
		r := in(c, "reason")
		return jsjson.Obj("short", orNull(reasons.Short(r)), "long", reasons.Long(r), "noChange", reasons.IsNoChange(r))
	},
	"format-explanation.json": func(t *testing.T, c Case) any { return explain.FormatExplanation(in(c, "status")) },
	"format-agents.json": func(t *testing.T, c Case) any {
		return explain.FormatAgents(in(c, "status"), jsjson.ToNumber(in(c, "now")))
	},
	"format-legend.json": func(t *testing.T, c Case) any { return legend.Format(iconSet(in(c, "set"))) },
	"location-info.json": func(t *testing.T, c Case) any {
		asked := []any{}
		branch := in(c, "branch")
		loc := worktree.LocationInfo(in(c, "input"), func(dir any) any { asked = append(asked, dir); return branch })
		var result any = nil
		if loc != nil {
			result = jsjson.Obj("branch", loc.Branch, "worktree", loc.Worktree)
		}
		return jsjson.Obj("result", result, "asked", asked)
	},
	"compare-versions.json": func(t *testing.T, c Case) any { return update.CompareVersions(in(c, "a"), in(c, "b")) },
	"update-notice.json": func(t *testing.T, c Case) any {
		return orNull(update.UpdateNotice(in(c, "state"), in(c, "currentVersion")))
	},
	"is-check-due.json": func(t *testing.T, c Case) any {
		return update.IsCheckDue(in(c, "state"), jsjson.ToNumber(in(c, "now")), toNumberOr(in(c, "everyMs"), update.CheckEveryMs))
	},
	"needs-install.json": func(t *testing.T, c Case) any { return update.NeedsInstall(strs(in(c, "changedFiles"))) },
	"quote-for-cmd.json": func(t *testing.T, c Case) any { return launch.QuoteForCmd(in(c, "arg")) },
	"parse-env.json": func(t *testing.T, c Case) any {
		o := jsjson.NewObject()
		for _, p := range env.ParseContent(in(c, "text").(string)) {
			o.Set(p.Key, p.Value)
		}
		return o
	},
	"stringify.json": func(t *testing.T, c Case) any {
		v := in(c, "value")
		if v == jsjson.Undefined {
			return jsjson.Obj("text", jsjson.Undefined)
		}
		text := jsjson.Stringify(v)
		if in(c, "indent") == 2.0 {
			text = jsjson.Indent(v)
		}
		return jsjson.Obj("text", text, "utf16Length", float64(jsstr.Len16(text)))
	},
	"parse.json": func(t *testing.T, c Case) any {
		v, err := jsjson.ParseBytes(in(c, "bytes").(Hex))
		if err != nil {
			return Throws
		}
		return jsjson.Obj("value", v, "text", jsjson.Stringify(v))
	},
	"math.json": func(t *testing.T, c Case) any {
		v := in(c, "value")
		switch in(c, "op") {
		case "MathRound":
			return jsjson.MathRound(jsjson.ToNumber(v))
		case "toFixed2":
			return jsjson.ToFixed2(jsjson.ToNumber(v))
		case "ToNumber":
			return jsjson.ToNumber(v)
		case "roundPercent":
			return jsjson.MathRound(jsjson.ToNumber(v) * 100)
		case "toFixed2OfToNumber":
			return jsjson.ToFixed2(jsjson.ToNumber(v))
		}
		t.Fatalf("unknown op %v", in(c, "op"))
		return nil
	},
}

// stripped compares a stringify case whose expected text is $undefined.
func normalizeStringify(c Case) any {
	if jsjson.Prop(c.Expected, "text") == jsjson.Undefined {
		return jsjson.Obj("text", jsjson.Undefined)
	}
	return c.Expected
}

func TestGoldenCases(t *testing.T) {
	saved := status.Dir
	t.Cleanup(func() { status.Dir = saved })
	files, err := filepath.Glob(filepath.Join(Dir(), "*.json"))
	if err != nil || len(files) == 0 {
		t.Fatalf("no case files in %s", Dir())
	}
	covered := map[string]bool{"jev-request.json": true, "status-line.json": true}
	total := 0
	for _, file := range files {
		name := filepath.Base(file)
		r, ok := runners[name]
		if !ok {
			if !covered[name] {
				t.Errorf("no runner for %s", name)
			}
			continue
		}
		cases, err := Load(name)
		if err != nil {
			t.Fatal(err)
		}
		t.Run(strings.TrimSuffix(name, ".json"), func(t *testing.T) {
			for _, c := range cases {
				total++
				got := r(t, c)
				want := c.Expected
				if name == "stringify.json" {
					want = normalizeStringify(c)
				}
				if !Equal(got, want) {
					t.Errorf("%s\n  input    %.300s\n  diff     %s", c.Name, Show(c.Input), Diff(got, want, "$"))
				}
			}
		})
	}
	t.Logf("%d cases checked", total)
}

func TestGoldenJevRequest(t *testing.T) {
	cases, err := Load("jev-request.json")
	if err != nil {
		t.Fatal(err)
	}
	t.Setenv("JEV_API_KEY", "test-key")
	t.Setenv("TYPESAFE_DEFAULT_MODEL", "")
	os.Unsetenv("TYPESAFE_DEFAULT_MODEL")
	for _, c := range cases {
		t.Run(c.Name, func(t *testing.T) {
			var method, path, body any = nil, nil, nil
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				data, _ := io.ReadAll(r.Body)
				method, path, body = r.Method, r.URL.Path, jsstr.DecodeBytes(data)
				w.Header().Set("content-type", "application/json")
				io.WriteString(w, in(c, "response").(string))
			}))
			defer srv.Close()
			t.Setenv("TYPESAFE_BASE_URL", srv.URL)
			jev.ResetConfig()
			defer jev.ResetConfig()

			var models []config.Model
			for _, m := range in(c, "models").([]any) {
				o := m.(*jsjson.Object)
				desc, hasDesc := o.Value("description").(string)
				released, _ := o.Value("releasedAt").(string)
				models = append(models, config.Model{ID: o.Value("id").(string), Tier: jsjson.JSString(o.Value("tier")), ReleasedAt: released, Description: desc, NoDescription: !hasDesc})
			}
			result := router.AskJev(router.Args{
				Prompt:        in(c, "prompt").(string),
				Current:       in(c, "current").(string),
				ContextTokens: jsjson.ToNumber(in(c, "contextTokens")),
				Models:        models,
			})
			want := c.Expected
			if wantURL := jsjson.Prop(want, "url"); wantURL == nil {
				if method != nil {
					t.Errorf("a request was sent")
				}
			} else {
				if method != jsjson.Prop(want, "method") || path != "/v1/systemone" || !strings.HasSuffix(wantURL.(string), "/v1/systemone") {
					t.Errorf("sent %v %v", method, path)
				}
				if body != jsjson.Prop(want, "body") {
					t.Errorf("body\n got  %v\n want %v", body, jsjson.Prop(want, "body"))
				}
			}
			var got any = nil
			if result != nil {
				if _, ok := result.Value("ms").(float64); !ok {
					t.Errorf("result has no integer ms")
				}
				r := result.Clone()
				r.Delete("ms")
				got = r
			}
			if !Equal(got, jsjson.Prop(want, "result")) {
				t.Errorf("result\n got  %s\n want %s", Show(got), Show(jsjson.Prop(want, "result")))
			}
		})
	}
}

// buildStatusLine compiles jev-statusline into a temporary directory.
func buildStatusLine(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	exe := filepath.Join(dir, "jev-statusline")
	if runtime.GOOS == "windows" {
		exe += ".exe"
	}
	cmd := exec.Command("go", "build", "-o", exe, "github.com/alienfacepalm/jev-claude/go/cmd/jev-statusline")
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("go build: %v\n%s", err, out)
	}
	return exe
}

// cleanEnv is the process environment without JEV_*, TYPESAFE_*, ANTHROPIC_*, CLAUDE_* and
// the home and temp variables, which each run sets itself.
func cleanEnv() []string {
	var out []string
	for _, kv := range os.Environ() {
		k, _, _ := strings.Cut(kv, "=")
		u := strings.ToUpper(k)
		skip := false
		for _, p := range []string{"JEV_", "TYPESAFE_", "ANTHROPIC_", "CLAUDE_"} {
			skip = skip || strings.HasPrefix(u, p)
		}
		for _, n := range []string{"HOME", "USERPROFILE", "TEMP", "TMP", "TMPDIR", "WT_SESSION", "TERM_PROGRAM", "CONEMUPID"} {
			skip = skip || u == n
		}
		if !skip {
			out = append(out, kv)
		}
	}
	return out
}

func TestGoldenStatusLine(t *testing.T) {
	cases, err := Load("status-line.json")
	if err != nil {
		t.Fatal(err)
	}
	exe := buildStatusLine(t)
	for _, c := range cases {
		t.Run(c.Name, func(t *testing.T) {
			statusDir, work, scratch := t.TempDir(), t.TempDir(), t.TempDir()
			write := func(file string, value, text any) {
				if s, ok := text.(string); ok {
					os.WriteFile(filepath.Join(statusDir, file), []byte(s), 0o600)
				} else if value != jsjson.Undefined {
					os.WriteFile(filepath.Join(statusDir, file), []byte(jsjson.Stringify(value)), 0o600)
				}
			}
			if f, ok := in(c, "statusFile").(string); ok {
				write(f, in(c, "status"), in(c, "statusText"))
			}
			write("calibration.json", in(c, "calibration"), in(c, "calibrationText"))
			cmd := exec.Command(exe)
			cmd.Dir = work
			cmd.Env = append(cleanEnv(), "JEV_STATUS_DIR="+statusDir, "HOME="+scratch, "USERPROFILE="+scratch, "TEMP="+scratch, "TMP="+scratch, "TMPDIR="+scratch)
			if icons, ok := in(c, "icons").(string); ok {
				cmd.Env = append(cmd.Env, "JEV_ICONS="+icons)
			}
			cmd.Stdin = strings.NewReader(in(c, "stdin").(string))
			var stderr strings.Builder
			cmd.Stderr = &stderr
			out, err := cmd.Output()
			if err != nil || stderr.Len() > 0 {
				t.Fatalf("exit %v, stderr %q", err, stderr.String())
			}
			if want := jsjson.Prop(c.Expected, "stdout"); string(out) != want {
				t.Errorf("\n got  %q\n want %q", out, want)
			}
		})
	}
}

// TestGoldenFilesAreAllCovered keeps a new case file from going unchecked.
func TestGoldenFilesAreAllCovered(t *testing.T) {
	files, _ := filepath.Glob(filepath.Join(Dir(), "*.json"))
	var missing []string
	for _, f := range files {
		n := filepath.Base(f)
		if _, ok := runners[n]; !ok && n != "jev-request.json" && n != "status-line.json" {
			missing = append(missing, n)
		}
	}
	sort.Strings(missing)
	if len(missing) > 0 {
		t.Fatalf("case files without a runner: %v", missing)
	}
	_ = math.NaN
}
