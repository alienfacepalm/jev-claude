// Package jev is the client for TypeSafe's Jev endpoint, written against the wire contract
// in SPEC 5 (it replaces @typesafe-ai/sdk). It never logs the key or a body.
package jev

import (
	"bytes"
	"context"
	"crypto/tls"
	"errors"
	"fmt"
	"io"
	"math"
	"math/rand"
	"net/http"
	"net/url"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
	"github.com/alienfacepalm/jev-claude/go/internal/repo"
)

// DefaultBaseURL is the Jev API root used when TYPESAFE_BASE_URL is blank.
const DefaultBaseURL = "https://api.typesafe.ai"

// Timing (SPEC 5.3).
var (
	AttemptTimeout = 1500 * time.Millisecond
	MaxRetries     = 1
)

// Transport is shared by every call and the prewarm, so the warmed connection is reused.
var Transport = &http.Transport{
	Proxy:               nil, // Node's fetch ignores HTTP_PROXY
	ForceAttemptHTTP2:   false,
	TLSNextProto:        map[string]func(string, *tls.Conn) http.RoundTripper{},
	DisableCompression:  true,
	MaxIdleConnsPerHost: 4,
	IdleConnTimeout:     90 * time.Second,
}

// Config is what a call reads from the environment.
type Config struct {
	Key, BaseURL, Model string
}

var (
	configMu sync.Mutex
	fixed    *Config
)

// trimmed is the SDK's readEnv: trimmed, blank counting as unset.
func trimmed(key string) string {
	v, _ := os.LookupEnv(key)
	return jsstr.Trim(v)
}

// CurrentConfig reads the configuration the way Node's lazily built client does: once a
// client could be built (a key was present), its settings are kept for the process.
func CurrentConfig() (Config, error) {
	configMu.Lock()
	defer configMu.Unlock()
	if fixed != nil {
		return *fixed, nil
	}
	key, ok := os.LookupEnv("JEV_API_KEY")
	if !ok {
		key, ok = os.LookupEnv("TYPESAFE_API_KEY")
	}
	if !ok {
		return Config{}, errors.New("missing API key: set JEV_API_KEY or TYPESAFE_API_KEY")
	}
	base := trimmed("TYPESAFE_BASE_URL")
	if base == "" {
		base = DefaultBaseURL
	}
	base = strings.TrimRight(base, "/")
	model := trimmed("TYPESAFE_DEFAULT_MODEL")
	if model == "" {
		model = "jev-latest"
	}
	fixed = &Config{Key: key, BaseURL: base, Model: model}
	return *fixed, nil
}

// ResetConfig forgets the fixed configuration (tests).
func ResetConfig() {
	configMu.Lock()
	fixed = nil
	configMu.Unlock()
}

// UserAgent is jev-router-go/<release version>.
func UserAgent() string { return "jev-router-go/" + repo.Version(repo.Root()) }

type statusError struct {
	status int
	header http.Header
}

func (e *statusError) Error() string { return fmt.Sprintf("Jev returned HTTP %d", e.status) }

func retryableStatus(code int) bool {
	return code == 408 || code == 429 || (code >= 500 && code <= 599)
}

// retryAfter is the server's requested delay in ms, ok false when absent or invalid.
func retryAfter(h http.Header, now time.Time) (float64, bool) {
	if vals, present := h[http.CanonicalHeaderKey("retry-after-ms")]; present {
		ms := jsjson.StringToNumber(strings.Join(vals, ", "))
		if !math.IsNaN(ms) && !math.IsInf(ms, 0) && ms >= 0 {
			return ms, true
		}
	}
	vals, present := h[http.CanonicalHeaderKey("retry-after")]
	if !present {
		return 0, false
	}
	raw := strings.Join(vals, ", ")
	if s := jsjson.StringToNumber(raw); !math.IsNaN(s) && !math.IsInf(s, 0) {
		if s >= 0 {
			return s * 1000, true
		}
		return 0, false
	}
	if t, err := http.ParseTime(raw); err == nil {
		return math.Max(0, float64(t.Sub(now).Milliseconds())), true
	}
	return 0, false
}

// Backoff is the delay before zero-based retry n.
func Backoff(n int, h http.Header, random func() float64) time.Duration {
	if h != nil {
		if ms, ok := retryAfter(h, time.Now()); ok && ms <= 60000 {
			return time.Duration(ms * float64(time.Millisecond))
		}
	}
	exp := math.Min(150*math.Pow(2, float64(n)), 400)
	return time.Duration(jsjson.MathRound(exp*(1-random()*0.25))) * time.Millisecond
}

// attempt is one round trip including the whole body, under the per-attempt timeout.
func attempt(ctx context.Context, cfg Config, body []byte, retry int) (int, http.Header, []byte, error) {
	actx, cancel := context.WithTimeout(ctx, AttemptTimeout)
	defer cancel()
	req, err := http.NewRequestWithContext(actx, http.MethodPost, cfg.BaseURL+"/v1/systemone", bytes.NewReader(body))
	if err != nil {
		return 0, nil, nil, err
	}
	req.Header.Set("Authorization", "Bearer "+cfg.Key)
	req.Header.Set("Accept", "application/json")
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("User-Agent", UserAgent())
	if retry > 0 {
		req.Header.Set("X-TypeSafe-Retry-Count", fmt.Sprint(retry))
	}
	res, err := Transport.RoundTrip(req)
	if err != nil {
		return 0, nil, nil, err
	}
	defer res.Body.Close()
	data, err := io.ReadAll(res.Body)
	if err != nil {
		return 0, nil, nil, err
	}
	return res.StatusCode, res.Header, data, nil
}

// SystemOne posts the request (with "model" appended) and returns the parsed response body:
// a value, a string when the body is not JSON, or Undefined for an empty body. The context
// carries the overall deadline.
func SystemOne(ctx context.Context, request *jsjson.Object) (any, error) {
	cfg, err := CurrentConfig()
	if err != nil {
		return nil, err
	}
	payload := request.Clone()
	if jsjson.IsNullish(payload.Value("model")) {
		payload.Set("model", cfg.Model)
	}
	body := []byte(jsjson.Stringify(payload))
	for n := 0; ; n++ {
		left := MaxRetries - n
		status, header, data, err := attempt(ctx, cfg, body, n)
		var delayHeader http.Header
		if err != nil {
			if ctx.Err() != nil {
				return nil, fmt.Errorf("request aborted: %w", ctx.Err())
			}
			if left <= 0 {
				return nil, fmt.Errorf("Connection error: %w", err)
			}
		} else {
			if status >= 200 && status <= 299 {
				return parseBody(data), nil
			}
			if left <= 0 || !retryableStatus(status) {
				return nil, &statusError{status, header}
			}
			delayHeader = header
		}
		timer := time.NewTimer(Backoff(n, delayHeader, rand.Float64))
		select {
		case <-ctx.Done():
			timer.Stop()
			return nil, fmt.Errorf("request aborted while waiting to retry: %w", ctx.Err())
		case <-timer.C:
		}
	}
}

// parseBody is the SDK's parseBody: empty is Undefined, JSON is parsed (a leading BOM is
// dropped, as fetch's text() does), anything else is the text.
func parseBody(data []byte) any {
	text := jsstr.DecodeBytes(data)
	text = strings.TrimPrefix(text, "\xef\xbb\xbf")
	if text == "" {
		return jsjson.Undefined
	}
	if v, err := jsjson.Parse(text); err == nil {
		return v
	}
	return text
}

// Prewarm sends one fire-and-forget HEAD to the origin of TYPESAFE_BASE_URL (untrimmed) or
// the default, so the first real call finds a warm connection.
func Prewarm() {
	base, ok := os.LookupEnv("TYPESAFE_BASE_URL")
	if !ok {
		base = DefaultBaseURL
	}
	u, err := url.Parse(base)
	if err != nil || u.Scheme == "" || u.Host == "" {
		return
	}
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		req, err := http.NewRequestWithContext(ctx, http.MethodHead, u.Scheme+"://"+u.Host, http.NoBody)
		if err != nil {
			return
		}
		if res, err := Transport.RoundTrip(req); err == nil {
			_, _ = io.Copy(io.Discard, res.Body) // fire and forget: every outcome is ignored
			res.Body.Close()
		}
	}()
}
