package proxy

import (
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/router"
	"github.com/alienfacepalm/jev-claude/go/internal/status"
)

// Ported from node/test/proxy-routing.test.mjs: driven by a request captured from the real
// Claude Code CLI (see the fixture's `_source`). Assertions are on what the upstream received.

// continuation is the same conversation one step later: Claude ran a tool and sends its result.
func continuation(t *testing.T, body *jsjson.Object) *jsjson.Object {
	t.Helper()
	messages := body.Value("messages").([]any)
	next := []any{messages[0],
		parse(t, `{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"ls"}}]}`),
		parse(t, `{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"utils.js"}]}`)}
	next = append(next, messages[1:]...)
	out := body.Clone()
	out.Set("messages", next)
	return out
}

type routingHarness struct {
	rec     *recorder
	mu      sync.Mutex
	prompts []string
	base    string
	url     string
}

// newRoutingHarness puts a proxy in front of an upstream that, like the API, rejects the sentinel.
func newRoutingHarness(t *testing.T, route Route) *routingHarness {
	t.Helper()
	h := &routingHarness{}
	h.rec, h.url = recordingUpstream(t, func(w http.ResponseWriter, r *http.Request, body any) {
		w.Header().Set("content-type", "application/json")
		if jsjson.Prop(body, "model") == "jev-router" {
			w.WriteHeader(http.StatusBadRequest)
			writeBody(w, `{"type":"error","error":{"type":"invalid_request_error","message":"model: jev-router"}}`)
			return
		}
		writeBody(w, `{"id":"msg_1","type":"message"}`)
	})
	h.base = startProxy(t, Options{UpstreamURL: h.url, Route: func(a router.Args) (*jsjson.Object, error) {
		h.mu.Lock()
		h.prompts = append(h.prompts, a.Prompt)
		h.mu.Unlock()
		return route(a)
	}})
	return h
}

func (h *routingHarness) post(t *testing.T, body any) int {
	code, _ := post(t, h.base+fixture(t).Value("url").(string), body, nil)
	return code
}

func TestProxyRouting(t *testing.T) {
	realRequest := func(t *testing.T) *jsjson.Object {
		t.Helper()
		return clone(t, fixture(t).Value("body")).(*jsjson.Object)
	}
	session := SessionOf(fixture(t).Value("body"))

	t.Run("a real print-mode request is routed on the user's prompt", func(t *testing.T) {
		h := newRoutingHarness(t, answer("claude-haiku-4-5-20251001", 0.92))
		if code := h.post(t, realRequest(t)); code != 200 {
			t.Fatalf("status %d", code)
		}
		if strings.Join(h.prompts, "|") != "rename the variable x to count in utils.js" {
			t.Errorf("reminders stripped, prompt intact: %q", h.prompts)
		}
		sent := h.rec.all()[0]
		body := sent.body.(*jsjson.Object)
		if sent.url != "/v1/messages?beta=true" || body.Value("model") != "claude-haiku-4-5-20251001" {
			t.Errorf("%s %v", sent.url, body.Value("model"))
		}
		// Haiku takes neither adaptive thinking nor effort; sending them as captured would be rejected.
		if body.Has("thinking") || body.Has("context_management") || jsjson.Prop(body.Value("output_config"), "effort") != jsjson.Undefined {
			t.Errorf("%s", jsjson.Stringify(body))
		}
		if jsjson.Prop(status.ReadStatus(session), "tier") != "haiku" {
			t.Error("the decision reaches the status line")
		}
	})

	t.Run("an unwritable JEV_DUMP path does not stop the turn being routed", func(t *testing.T) {
		// A real misconfiguration: the dump directory does not exist, so the dump cannot be written.
		t.Setenv("JEV_DUMP", filepath.Join(os.TempDir(), "jev-no-such-dir", "nested", "dump"))
		h := newRoutingHarness(t, answer("claude-haiku-4-5-20251001", 0.92))
		if code := h.post(t, realRequest(t)); code != 200 {
			t.Fatalf("the API would reject the sentinel with a 400, got %d", code)
		}
		if modelOf(h.rec.all()[0]) != "claude-haiku-4-5-20251001" {
			t.Error("routed as usual, dump or no dump")
		}
	})

	t.Run("tool-call continuations keep the tier the turn was routed to", func(t *testing.T) {
		h := newRoutingHarness(t, answer("claude-sonnet-5-5", 0.92))
		opening := realRequest(t)
		h.post(t, opening)
		h.post(t, continuation(t, opening))
		h.post(t, continuation(t, opening))
		if len(h.prompts) != 1 {
			t.Errorf("Jev is asked once per turn, not once per tool call: %d", len(h.prompts))
		}
		for _, s := range h.rec.all() {
			if modelOf(s) != "claude-sonnet-5-5" {
				t.Errorf("%v", modelOf(s))
			}
		}
	})

	t.Run("the main thread keeps its tier after more than 50 sub-agents start", func(t *testing.T) {
		h := newRoutingHarness(t, func(a router.Args) (*jsjson.Object, error) {
			choice := "claude-sonnet-5-5"
			if strings.HasPrefix(a.Prompt, "Search the codebase") {
				choice = "claude-haiku-4-5-20251001"
			}
			return jsjson.Obj("choice", choice, "confidence", 0.9, "ms", 1.0), nil
		})
		main := realRequest(t)
		h.post(t, main)
		// Sub-agents share the session id and differ by their opening task. The main thread waits
		// on their results, so it is the oldest, least recently used conversation when it resumes.
		for i := 0; i < 55; i++ {
			sub := realRequest(t)
			content := jsjson.Prop(sub.Value("messages").([]any)[0], "content").([]any)
			content[len(content)-1].(*jsjson.Object).Set("text", "Search the codebase for callers of handler "+jsjson.NumberToString(float64(i))+" and report them")
			h.post(t, sub)
		}
		h.post(t, continuation(t, main))
		all := h.rec.all()
		if modelOf(all[len(all)-1]) != "claude-sonnet-5-5" {
			t.Errorf("not reset to the default: %v", modelOf(all[len(all)-1]))
		}
	})
}
