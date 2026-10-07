// Package proxy is the local routing proxy (SPEC 7), ported from node/src/proxy.mjs.
package proxy

import (
	"bytes"
	"crypto/tls"
	"errors"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/jev"
	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
	"github.com/alienfacepalm/jev-claude/go/internal/logx"
	"github.com/alienfacepalm/jev-claude/go/internal/policy"
	"github.com/alienfacepalm/jev-claude/go/internal/router"
	"github.com/alienfacepalm/jev-claude/go/internal/status"
)

// AnthropicBaseURL is the default upstream.
const AnthropicBaseURL = "https://api.anthropic.com"

// MaxConversations is how many conversations' routing state is kept.
const MaxConversations = 50

// Route answers one routing question. A nil object with a nil error is "no answer" (null);
// an error is a throw, which takes the body-processing catch path (SPEC 7.2).
type Route func(router.Args) (*jsjson.Object, error)

// Options configure Start.
type Options struct {
	UpstreamURL     string // default AnthropicBaseURL
	Route           Route  // default the real router (and a Jev prewarm)
	CalibrationFile string // default status.CalibrationFile()
}

// Proxy is a running proxy.
type Proxy struct {
	Port   int
	server *http.Server
}

// Close stops the proxy.
func (p *Proxy) Close() { p.server.Close() }

type convState struct {
	tier  string // "" is null
	model string // "" is undefined
}

type proxy struct {
	upstream     *url.URL
	upstreamHost string
	basePath     string
	route        Route
	calibration  string
	transport    *http.Transport

	mu       sync.Mutex // guards convos, order, mains, catalog and every convState (SPEC 3.10)
	convos   map[string]*convState
	order    []string // least recently used first
	mains    *Mains
	catalog  []string
	catModel map[string]any
}

// Start listens on 127.0.0.1 on a free port.
func Start(opts Options) (*Proxy, error) {
	if opts.UpstreamURL == "" {
		opts.UpstreamURL = AnthropicBaseURL
	}
	target, err := url.Parse(opts.UpstreamURL)
	if err != nil || (target.Scheme != "http" && target.Scheme != "https") || target.Host == "" {
		return nil, errors.New("invalid upstream URL: " + opts.UpstreamURL)
	}
	p := &proxy{
		upstream:     target,
		upstreamHost: hostOf(target),
		basePath:     strings.TrimSuffix(target.EscapedPath(), "/"),
		route:        opts.Route,
		calibration:  opts.CalibrationFile,
		convos:       map[string]*convState{},
		mains:        NewMains(),
		catModel:     map[string]any{},
		transport: &http.Transport{
			Proxy:               nil,
			ForceAttemptHTTP2:   false,
			TLSNextProto:        map[string]func(string, *tls.Conn) http.RoundTripper{},
			DisableCompression:  true,
			MaxIdleConnsPerHost: 16,
			IdleConnTimeout:     90 * time.Second,
		},
	}
	if p.calibration == "" {
		p.calibration = status.CalibrationFile()
	}
	if p.route == nil {
		p.route = func(a router.Args) (*jsjson.Object, error) { return router.AskJev(a), nil }
		jev.Prewarm()
	}
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return nil, err
	}
	srv := &http.Server{Handler: p, ErrorLog: nil}
	srv.SetKeepAlivesEnabled(true)
	// Serve returns http.ErrServerClosed once Close is called; nothing else stops it.
	go func() { _ = srv.Serve(ln) }()
	return &Proxy{Port: ln.Addr().(*net.TCPAddr).Port, server: srv}, nil
}

// hostOf is WHATWG url.host: the port only when the URL names a non-default one.
func hostOf(u *url.URL) string {
	port := u.Port()
	if port == "" || (u.Scheme == "http" && port == "80") || (u.Scheme == "https" && port == "443") {
		return u.Hostname()
	}
	if strings.Contains(u.Hostname(), ":") {
		return "[" + u.Hostname() + "]:" + port
	}
	return u.Hostname() + ":" + port
}

func debugf(line func() string) { logx.Debug(line) }

// stateFor finds or creates a conversation's state (SPEC 7.4). Caller holds mu.
func (p *proxy) stateFor(key, fallback string) *convState {
	s := p.convos[key]
	if s == nil && fallback != "" {
		if f := p.convos[fallback]; f != nil {
			s = f
			p.remove(fallback)
		}
	}
	if s != nil {
		p.remove(key)
	} else if len(p.order) >= MaxConversations {
		victim := p.order[0]
		for _, k := range p.order {
			if !p.mains.isMainKey(k) {
				victim = k
				break
			}
		}
		p.remove(victim)
	}
	if s == nil {
		s = &convState{}
	}
	p.convos[key] = s
	p.order = append(p.order, key)
	return s
}

func (p *proxy) remove(key string) {
	if _, ok := p.convos[key]; !ok {
		return
	}
	delete(p.convos, key)
	for i, k := range p.order {
		if k == key {
			p.order = append(p.order[:i:i], p.order[i+1:]...)
			break
		}
	}
}

func (p *proxy) catalogValues() []any {
	p.mu.Lock()
	defer p.mu.Unlock()
	out := make([]any, len(p.catalog))
	for i, id := range p.catalog {
		out[i] = p.catModel[id]
	}
	return out
}

func modelForTier(models []CatalogModel, tier string) string {
	for _, m := range models {
		if m.Tier == tier {
			return m.ID
		}
	}
	return config.IDOf(tier)
}

// process rewrites a /v1/messages body (SPEC 7.3) and returns the bytes to forward.
func (p *proxy) process(raw []byte) []byte {
	body := jsjson.Undefined
	var state *convState
	out, err := func() ([]byte, error) {
		v, err := jsjson.ParseBytes(raw)
		if err != nil {
			return nil, err
		}
		body = v
		if v == nil {
			return nil, typeError("Cannot read properties of null (reading 'tools')")
		}
		status.DumpBody(body, os.Getenv("JEV_DUMP"))
		if tools := jsjson.Prop(body, "tools"); !jsjson.IsNullish(tools) {
			list, ok := tools.([]any)
			if !ok {
				return nil, typeError("body.tools?.forEach is not a function")
			}
			for _, t := range list {
				if jsjson.IsNullish(t) {
					return nil, typeError("Cannot read properties of null (reading 'input_schema')")
				}
				SanitizeSchema(jsjson.Prop(t, "input_schema"))
			}
		}
		model := jsjson.Prop(body, "model")
		if !config.IsAuto(model) {
			debugf(func() string { return "passthrough, user selected " + jsjson.JSString(model) })
			if hasTools(body) {
				p.mu.Lock()
				key, label, main, err := AgentOf(body, p.mains)
				p.mu.Unlock()
				if err != nil {
					return nil, err
				}
				debugf(func() string { return key + " passthrough " + mainOrSub(main) + " " + jsjson.JSString(model) })
				prompt, err := NewTurnPrompt(body)
				if err != nil {
					return nil, err
				}
				if jsjson.Truthy(prompt) {
					status.MarkManual(SessionOf(body), model, &status.Agent{Key: key, Label: label, Main: main})
				}
			}
			return []byte(jsjson.Stringify(body)), nil
		}
		obj := body.(*jsjson.Object)
		return p.routed(obj, &state)
	}()
	if err == nil {
		return out
	}
	debugf(func() string { return "could not process body: " + err.Error() })
	if config.IsAuto(jsjson.Prop(body, "model")) {
		obj := body.(*jsjson.Object)
		tier, model := config.UncertainDefault, ""
		if state != nil {
			p.mu.Lock()
			if state.tier != "" {
				tier = state.tier
			}
			model = state.model
			p.mu.Unlock()
		}
		if model == "" {
			model = config.IDOf(tier)
		}
		ApplyTier(obj, tier, model, config.ProcessEnv)
		return []byte(jsjson.Stringify(obj))
	}
	return raw
}

func mainOrSub(main bool) string {
	if main {
		return "main"
	}
	return "sub"
}

// routed handles a request for the sentinel (SPEC 7.3 step 3). *statep is set as soon as the
// conversation state exists, for the catch path.
func (p *proxy) routed(body *jsjson.Object, statep **convState) ([]byte, error) {
	session := SessionOf(body)
	fallback := ""
	if jsjson.Truthy(session) {
		bare := body.Clone()
		bare.Set("metadata", jsjson.Undefined)
		k, err := ConversationKey(bare)
		if err != nil {
			return nil, err
		}
		fallback = k
	}
	p.mu.Lock()
	key, label, main, err := AgentOf(body, p.mains)
	if err != nil {
		p.mu.Unlock()
		return nil, err
	}
	state := p.stateFor(key, fallback)
	*statep = state
	// One snapshot of the state, read in the same critical section as stateFor (SPEC 3.10):
	// another request for this conversation may route while this one awaits Jev, and Node
	// reads tier, model and "routed before" together, with no await between them.
	current := state.tier
	currentModel := state.model
	routedBefore := state.tier != ""
	if current == "" {
		current = config.UncertainDefault
	}
	p.mu.Unlock()

	promptV, err := NewTurnPrompt(body)
	if err != nil {
		return nil, err
	}
	prompt, hasPrompt := promptV.(string)
	explaining := hasPrompt && strings.Contains(prompt, "<jev-explain>")
	var fresh *jsjson.Object
	if hasPrompt && !explaining {
		all, err := ClaudeModels(p.catalogValues())
		if err != nil {
			return nil, err
		}
		allowed := config.AvailableTiers(config.ProcessEnv)
		var filtered []CatalogModel
		for _, m := range all {
			for _, t := range allowed {
				if m.Tier == t {
					filtered = append(filtered, m)
					break
				}
			}
		}
		models := NewestPerTier(filtered)
		var available []string
		menu := make([]config.Model, len(models))
		for i, m := range models {
			menu[i] = m.Model
			seen := false
			for _, a := range available {
				seen = seen || a == m.Tier
			}
			if !seen {
				available = append(available, m.Tier)
			}
		}
		if currentModel == "" {
			currentModel = modelForTier(models, current)
		}
		contextTokens := jsjson.MathRound(float64(jsstr.Len16(jsjson.Stringify(body.Value("messages")))) / 4)
		answer, err := p.route(router.Args{Prompt: prompt, Current: currentModel, ContextTokens: contextTokens, Models: menu})
		if err != nil {
			return nil, err
		}
		var jevV any // nil is null
		if answer != nil {
			jevV = answer
		}
		var chosen *CatalogModel
		if answer != nil {
			if choice, ok := answer.Value("choice").(string); ok {
				for i := range models {
					if models[i].ID == choice {
						chosen = &models[i]
						break
					}
				}
			}
		}
		var tierAnswer any
		if chosen != nil {
			t := answer.Clone()
			t.Set("choice", chosen.Tier)
			tierAnswer = t
		}
		policyTokens := 0.0
		if routedBefore {
			policyTokens = contextTokens
		}
		decision := policy.Decide(policy.Input{Prompt: prompt, Jev: tierAnswer, Current: current, Available: available, ContextTokens: policyTokens})
		chosenTier := ""
		if chosen != nil {
			chosenTier = chosen.Tier
		}
		var model string
		switch {
		case chosen != nil && config.ShouldUseExactModel(decision.Reason, chosenTier, decision.Tier):
			model = chosen.ID
		case decision.Tier == current:
			model = currentModel
		default:
			model = modelForTier(models, decision.Tier)
		}
		p.mu.Lock()
		state.tier = decision.Tier
		state.model = model
		p.mu.Unlock()
		var jevRecord any // nil is null
		if answer != nil {
			jevRecord = jsjson.Obj("request", answer.Value("request"), "response", answer.Value("response"))
		}
		fresh = jsjson.Obj(
			"prompt", prompt,
			"model", model,
			"confidence", jsjson.Coalesce(jsjson.Prop(jevV, "confidence"), nil),
			"metrics", jsjson.Coalesce(jsjson.Prop(jevV, "metrics"), nil),
			"reason", decision.Reason,
			"jev", jevRecord,
		)
		debugf(func() string {
			who := "main"
			if !main {
				who = "sub[" + label + "]"
			}
			how := "no-jev"
			if answer != nil {
				how = jsjson.JSString(answer.Value("ms")) + "ms p=" + jsjson.ToFixed2(jsjson.ToNumber(answer.Value("confidence")))
			}
			return key + " " + who + " " + how + " " + current + " -> " + decision.Tier + " (" + decision.Reason + ") ctx~" +
				jsjson.NumberToString(contextTokens) + " | " + jsstr.Slice16(prompt, 0, 60)
		})
	}
	p.mu.Lock()
	tier := state.tier
	if tier == "" {
		tier = current
	}
	model := state.model
	p.mu.Unlock()
	if model == "" {
		model = config.IDOf(tier)
	}
	debugf(func() string { return key + " rewrite " + jsjson.JSString(body.Value("model")) + " -> " + model })
	ApplyTier(body, tier, model, config.ProcessEnv)
	if fresh != nil && !explaining {
		effort := jsjson.Coalesce(jsjson.Prop(body.Value("output_config"), "effort"), nil)
		decision := jsjson.Obj("tier", tier)
		jsjson.Spread(decision, fresh)
		decision.Set("effort", effort)
		decision.Set("at", status.NowMs())
		id := SessionOf(body)
		if !jsjson.Truthy(id) {
			id = key
		}
		if err := status.WriteDecision(id, decision, &status.Agent{Key: key, Label: label, Main: main}); err != nil {
			return nil, err
		}
	}
	return []byte(jsjson.Stringify(body)), nil
}

var (
	messagesTarget = regexp.MustCompile(`^/v1/messages`)
	modelsTarget   = regexp.MustCompile(`^/v1/models(?:\?|$)`)
	servedBy       = regexp.MustCompile(`"model"` + jsstr.JSWSClass + `*:` + jsstr.JSWSClass + `*"([^"]+)"`)
)

var hopByHop = []string{"Content-Length", "Transfer-Encoding", "Connection", "Keep-Alive"}

func (p *proxy) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	if r.Method == http.MethodHead {
		w.WriteHeader(http.StatusOK)
		return
	}
	raw, err := io.ReadAll(r.Body)
	if err != nil {
		return
	}
	target := r.RequestURI
	out := raw
	if messagesTarget.MatchString(target) {
		out = p.process(raw)
	}
	// The user gave up while Jev was being asked; there is nobody to send this turn for.
	if r.Context().Err() != nil {
		return
	}
	isModels := r.Method == http.MethodGet && modelsTarget.MatchString(target)

	header := http.Header{}
	for k, v := range r.Header {
		header[http.CanonicalHeaderKey(k)] = append([]string(nil), v...)
	}
	for _, h := range hopByHop {
		header.Del(h)
	}
	if isModels || os.Getenv("JEV_DEBUG") != "" {
		header.Del("Accept-Encoding")
	}
	if _, ok := header["User-Agent"]; !ok {
		header["User-Agent"] = []string{""} // keeps Go from adding its own
	}
	path, query, hasQuery := strings.Cut(target, "?")
	upURL := &url.URL{
		Scheme:     p.upstream.Scheme,
		Host:       p.upstream.Host,
		Opaque:     p.basePath + path,
		RawQuery:   query,
		ForceQuery: hasQuery && query == "",
	}
	req := &http.Request{
		Method:     r.Method,
		URL:        upURL,
		Proto:      "HTTP/1.1",
		ProtoMajor: 1,
		ProtoMinor: 1,
		Header:     header,
		Host:       p.upstreamHost,
	}
	req = req.WithContext(r.Context())
	if len(out) > 0 {
		req.Body = io.NopCloser(bytes.NewReader(out))
		req.ContentLength = int64(len(out))
		req.GetBody = func() (io.ReadCloser, error) { return io.NopCloser(bytes.NewReader(out)), nil }
	}
	res, err := p.transport.RoundTrip(req)
	if err != nil {
		if r.Context().Err() != nil {
			return
		}
		debugf(func() string { return "upstream error: " + err.Error() })
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusBadGateway)
		// A failed write means the client has gone; there is no one left to tell.
		_, _ = io.WriteString(w, jsjson.Stringify(jsjson.Obj("type", "error", "error", jsjson.Obj("message", err.Error()))))
		return
	}
	defer res.Body.Close()

	if isModels {
		data, err := io.ReadAll(res.Body)
		if err != nil {
			panic(http.ErrAbortHandler)
		}
		p.readCatalog(data)
		copyHeaders(w.Header(), res.Header, true)
		w.WriteHeader(res.StatusCode)
		_, _ = w.Write(data) // a failed write means the client has gone
		return
	}

	copyHeaders(w.Header(), res.Header, false)
	w.WriteHeader(res.StatusCode)
	flusher, _ := w.(http.Flusher)
	if flusher != nil {
		flusher.Flush()
	}
	seen := os.Getenv("JEV_DEBUG") == ""
	buf := make([]byte, 32*1024)
	for {
		n, rerr := res.Body.Read(buf)
		if n > 0 {
			if !seen {
				if m := servedBy.FindSubmatch([]byte(jsstr.DecodeBytes(buf[:n]))); m != nil {
					seen = true
					model := string(m[1])
					debugf(func() string { return strconv.Itoa(res.StatusCode) + " served by " + model })
				}
			}
			if _, werr := w.Write(buf[:n]); werr != nil {
				return // the client left; returning closes the upstream body
			}
			if flusher != nil {
				flusher.Flush()
			}
		}
		if rerr == io.EOF {
			return
		}
		if rerr != nil {
			if r.Context().Err() == nil {
				debugf(func() string { return "stream ended early: " + rerr.Error() })
			}
			// Mid-stream: cut the client connection rather than end the response cleanly.
			panic(http.ErrAbortHandler)
		}
	}
}

// copyHeaders copies upstream response headers to the client, leaving out the framing the
// server manages itself, and keeps Go from sniffing a content-type the upstream did not send.
func copyHeaders(dst, src http.Header, dropLength bool) {
	for k, v := range src {
		switch http.CanonicalHeaderKey(k) {
		case "Connection", "Keep-Alive", "Transfer-Encoding":
			continue
		case "Content-Length":
			if dropLength {
				continue
			}
		}
		dst[k] = append([]string(nil), v...)
	}
	if _, ok := dst["Content-Type"]; !ok {
		dst["Content-Type"] = nil
	}
}

// readCatalog records the account's models from a /v1/models body and writes the calibration
// file (SPEC 7.2 step 5). Failures are debug-logged and write nothing.
func (p *proxy) readCatalog(data []byte) {
	fail := func(err error) {
		debugf(func() string { return "could not read Claude model catalog: " + err.Error() })
	}
	v, err := jsjson.ParseBytes(data)
	if err != nil {
		fail(err)
		return
	}
	if v == nil {
		fail(typeError("Cannot read properties of null (reading 'data')"))
		return
	}
	list := jsjson.Coalesce(jsjson.Prop(v, "data"), []any{})
	var entries []any
	switch x := list.(type) {
	case []any:
		entries = x
	case string:
		// Iterating a string yields characters, none of which has a tier.
	default:
		fail(typeError("data is not iterable"))
		return
	}
	p.mu.Lock()
	for _, m := range entries {
		id := jsjson.Prop(m, "id")
		if config.TierOf(id) == "" {
			continue
		}
		s := id.(string)
		if _, ok := p.catModel[s]; !ok {
			p.catalog = append(p.catalog, s)
		}
		p.catModel[s] = m
	}
	p.mu.Unlock()
	values := p.catalogValues()
	newer, err := NewerThanCalibrated(values)
	if err != nil {
		fail(err)
		return
	}
	models, err := ClaudeModels(values)
	if err != nil {
		fail(err)
		return
	}
	newerList := make([]any, len(newer))
	for i, id := range newer {
		newerList[i] = id
	}
	var ids []any
	for _, m := range NewestPerTier(models) {
		ids = append(ids, m.ID)
	}
	status.WriteCalibration(newerList, ids, p.calibration)
}
