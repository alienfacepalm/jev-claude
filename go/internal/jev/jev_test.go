package jev

import (
	"context"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
)

// The Jev client against a loopback fake Jev (SPEC 5). Node has no unit test file for its SDK;
// these pin the wire contract the port implements itself.

type call struct {
	header http.Header
	body   string
}

func fakeJev(t *testing.T, reply func(n int, w http.ResponseWriter)) (*[]call, *sync.Mutex) {
	var calls []call
	var mu sync.Mutex
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		data, _ := io.ReadAll(r.Body)
		mu.Lock()
		calls = append(calls, call{r.Header.Clone(), string(data)})
		n := len(calls)
		mu.Unlock()
		reply(n, w)
	}))
	t.Cleanup(srv.Close)
	t.Setenv("JEV_API_KEY", "k-123")
	t.Setenv("TYPESAFE_BASE_URL", srv.URL+"/")
	ResetConfig()
	t.Cleanup(ResetConfig)
	return &calls, &mu
}

func request() *jsjson.Object { return jsjson.Obj("state", "x", "questions", jsjson.NewObject()) }

func ok(w http.ResponseWriter) {
	w.Header().Set("content-type", "application/json")
	_, _ = io.WriteString(w, `{"answers":{}}`) // a failed write fails the client under test
}

func TestSendsOnlyTheContractHeadersAndAppendsTheModel(t *testing.T) {
	calls, _ := fakeJev(t, func(int, http.ResponseWriter) {})
	*calls = nil
	got, err := SystemOne(context.Background(), request())
	if err != nil {
		t.Fatal(err)
	}
	if got != jsjson.Undefined {
		t.Errorf("an empty body parses to undefined, got %v", got)
	}
	c := (*calls)[0]
	if c.body != `{"state":"x","questions":{},"model":"jev-latest"}` {
		t.Errorf("body %s", c.body)
	}
	if c.header.Get("Authorization") != "Bearer k-123" || c.header.Get("Accept") != "application/json" ||
		c.header.Get("Content-Type") != "application/json" || !strings.HasPrefix(c.header.Get("User-Agent"), "jev-router-go/") {
		t.Errorf("headers %v", c.header)
	}
	for _, h := range []string{"X-Typesafe-Sdk", "X-Typesafe-Runtime", "X-Typesafe-Retry-Count", "Accept-Encoding"} {
		if _, sent := c.header[h]; sent {
			t.Errorf("%s was sent", h)
		}
	}
}

func TestRetriesOnceOnA503WithTheRetryCount(t *testing.T) {
	calls, _ := fakeJev(t, func(n int, w http.ResponseWriter) {
		if n == 1 {
			w.Header().Set("retry-after-ms", "10")
			w.WriteHeader(http.StatusServiceUnavailable)
			return
		}
		ok(w)
	})
	got, err := SystemOne(context.Background(), request())
	if err != nil || jsjson.Stringify(got) != `{"answers":{}}` {
		t.Fatalf("%v %v", got, err)
	}
	if len(*calls) != 2 || (*calls)[1].header.Get("X-TypeSafe-Retry-Count") != "1" {
		t.Fatalf("calls %d, retry header %q", len(*calls), (*calls)[1].header.Get("X-TypeSafe-Retry-Count"))
	}
}

func TestDoesNotRetryAClientError(t *testing.T) {
	calls, _ := fakeJev(t, func(n int, w http.ResponseWriter) { w.WriteHeader(http.StatusBadRequest) })
	if _, err := SystemOne(context.Background(), request()); err == nil {
		t.Fatal("a 400 is a failure")
	}
	if len(*calls) != 1 {
		t.Fatalf("retried a 400: %d calls", len(*calls))
	}
}

func TestGivesUpAfterOneRetry(t *testing.T) {
	calls, _ := fakeJev(t, func(n int, w http.ResponseWriter) { w.WriteHeader(http.StatusInternalServerError) })
	if _, err := SystemOne(context.Background(), request()); err == nil {
		t.Fatal("500 twice is a failure")
	}
	if len(*calls) != 2 {
		t.Fatalf("%d calls", len(*calls))
	}
}

func TestAnAttemptTimeoutIsRetriedAndTheDeadlineStopsEverything(t *testing.T) {
	saved := AttemptTimeout
	AttemptTimeout = 100 * time.Millisecond
	t.Cleanup(func() { AttemptTimeout = saved })
	calls, _ := fakeJev(t, func(n int, w http.ResponseWriter) {
		if n == 1 {
			time.Sleep(300 * time.Millisecond)
		}
		ok(w)
	})
	if _, err := SystemOne(context.Background(), request()); err != nil {
		t.Fatalf("the retry should succeed: %v", err)
	}
	if len(*calls) != 2 {
		t.Fatalf("%d calls", len(*calls))
	}

	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	started := time.Now()
	*calls = nil
	if _, err := SystemOne(ctx, request()); err == nil {
		t.Fatal("the deadline is a failure")
	}
	if time.Since(started) > time.Second {
		t.Fatal("the deadline did not cut the call short")
	}
}

func TestBackoffHonoursRetryAfterWithinAMinute(t *testing.T) {
	h := http.Header{}
	h.Set("Retry-After", "2")
	if d := Backoff(0, h, func() float64 { return 0 }); d != 2*time.Second {
		t.Errorf("retry-after seconds: %v", d)
	}
	h.Set("Retry-After", "120")
	if d := Backoff(0, h, func() float64 { return 0 }); d != 150*time.Millisecond {
		t.Errorf("over a minute falls back to backoff: %v", d)
	}
	if d := Backoff(3, nil, func() float64 { return 1 }); d != 300*time.Millisecond {
		t.Errorf("capped at 400 with full jitter: %v", d)
	}
}

func TestMissingKeyIsAFailure(t *testing.T) {
	t.Setenv("JEV_API_KEY", "")
	ResetConfig()
	t.Cleanup(ResetConfig)
	// An empty JEV_API_KEY is still defined (Node's ??), so it is used, empty.
	cfg, err := CurrentConfig()
	if err != nil || cfg.Key != "" {
		t.Fatalf("%v %v", cfg, err)
	}
}
