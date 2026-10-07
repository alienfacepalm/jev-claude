package proxy

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sync"
	"testing"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/router"
	"github.com/alienfacepalm/jev-claude/go/internal/status"
)

// TestMain points the status store at a throwaway directory: the real one holds live
// sessions' decisions (node/test/isolate-status.mjs does the same).
func TestMain(m *testing.M) {
	dir, err := os.MkdirTemp("", "jev-status-test-")
	if err != nil {
		panic(err)
	}
	status.Dir = dir
	os.Unsetenv("JEV_DUMP")
	os.Unsetenv("JEV_DEBUG")
	code := m.Run()
	os.RemoveAll(dir)
	os.Exit(code)
}

func parse(t *testing.T, s string) any {
	t.Helper()
	v, err := jsjson.Parse(s)
	if err != nil {
		t.Fatalf("parse %s: %v", s, err)
	}
	return v
}

func obj(t *testing.T, s string) *jsjson.Object { return parse(t, s).(*jsjson.Object) }

// answer is a fake route that always answers choice with the given confidence.
func answer(choice string, confidence float64) Route {
	return func(router.Args) (*jsjson.Object, error) {
		return jsjson.Obj("choice", choice, "confidence", confidence, "ms", 1.0), nil
	}
}

func metadata(session string) *jsjson.Object {
	return jsjson.Obj("user_id", jsjson.Stringify(jsjson.Obj("session_id", session)))
}

// seen is what an upstream received.
type seen struct {
	method, url string
	header      http.Header
	body        any
}

type recorder struct {
	mu   sync.Mutex
	list []seen
}

func (r *recorder) add(s seen) {
	r.mu.Lock()
	r.list = append(r.list, s)
	r.mu.Unlock()
}

func (r *recorder) all() []seen {
	r.mu.Lock()
	defer r.mu.Unlock()
	return append([]seen(nil), r.list...)
}

// recordingUpstream records each request and answers with a small JSON message, or with the
// given handler's reply.
func recordingUpstream(t *testing.T, reply func(w http.ResponseWriter, r *http.Request, body any)) (*recorder, string) {
	rec := &recorder{}
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		data, _ := io.ReadAll(r.Body)
		var body any
		if len(data) > 0 {
			body, _ = jsjson.ParseBytes(data)
		}
		rec.add(seen{method: r.Method, url: r.RequestURI, header: r.Header.Clone(), body: body})
		if reply != nil {
			reply(w, r, body)
			return
		}
		w.Header().Set("content-type", "application/json")
		writeBody(w, `{"id":"msg_1","type":"message"}`)
	}))
	t.Cleanup(srv.Close)
	return rec, srv.URL
}

func startProxy(t *testing.T, opts Options) string {
	t.Helper()
	p, err := Start(opts)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(p.Close)
	return fmt.Sprintf("http://127.0.0.1:%d", p.Port)
}

// post sends body as JSON and returns the response text.
func post(t *testing.T, url string, body any, headers map[string]string) (int, string) {
	t.Helper()
	req, err := http.NewRequestWithContext(context.Background(), http.MethodPost, url, bytes.NewReader([]byte(jsjson.Stringify(body))))
	if err != nil {
		t.Fatal(err)
	}
	req.Header.Set("content-type", "application/json")
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	res, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatal(err)
	}
	defer res.Body.Close()
	data, _ := io.ReadAll(res.Body)
	return res.StatusCode, string(data)
}

// get sends a GET and returns the status and body text.
func get(t *testing.T, url string, headers map[string]string) (int, string) {
	t.Helper()
	req, err := http.NewRequestWithContext(context.Background(), http.MethodGet, url, http.NoBody)
	if err != nil {
		t.Fatal(err)
	}
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	res, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatal(err)
	}
	defer res.Body.Close()
	data, err := io.ReadAll(res.Body)
	if err != nil {
		t.Fatal(err)
	}
	return res.StatusCode, string(data)
}

// writeBody writes a fake upstream's reply. A failed write surfaces as the client's error, so
// the handler has nothing to do with it.
func writeBody(w io.Writer, s string) { _, _ = io.WriteString(w, s) }

// mustWrite writes a test file and fails the test if it cannot: a setup step that silently
// fails would let a negative assertion pass without testing anything.
func mustWrite(t *testing.T, path string, data []byte) {
	t.Helper()
	if err := os.WriteFile(path, data, 0o644); err != nil {
		t.Fatal(err)
	}
}

// mustMkdirAll creates a test directory with the given mode or fails the test.
func mustMkdirAll(t *testing.T, path string, perm os.FileMode) {
	t.Helper()
	if err := os.MkdirAll(path, perm); err != nil {
		t.Fatal(err)
	}
}

func fixture(t *testing.T) *jsjson.Object {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("..", "..", "..", "conformance", "fixtures", "claude-code-print-request.json"))
	if err != nil {
		t.Fatal(err)
	}
	v, err := jsjson.ParseBytes(data)
	if err != nil {
		t.Fatal(err)
	}
	return v.(*jsjson.Object)
}

// clone deep-copies a value (structuredClone).
func clone(t *testing.T, v any) any { return parse(t, jsjson.Stringify(v)) }

func modelOf(s seen) any { return jsjson.Prop(s.body, "model") }
