// Package status is the per-session status store (SPEC 8), ported from node/src/status.mjs.
package status

import (
	"errors"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/alienfacepalm/jev-claude/go/internal/fsx"
	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
	"github.com/alienfacepalm/jev-claude/go/internal/osdirs"
)

// Dir is the status directory, fixed at program start (SPEC 3.9): JEV_STATUS_DIR when
// non-empty, else <temp>/jev-claude. Tests may point it elsewhere before any write.
var Dir = func() string {
	if v := os.Getenv("JEV_STATUS_DIR"); v != "" {
		return v
	}
	return filepath.Join(osdirs.Temp(), "jev-claude")
}()

// StaleAfterMs is how old a status file must be before it is pruned.
const StaleAfterMs = 7 * 24 * 60 * 60 * 1000

// MaxAgents is how many agents a session file keeps.
const MaxAgents = 12

// NowMs is the clock (Date.now()), replaceable in tests.
var NowMs = func() float64 { return float64(time.Now().UnixMilli()) }

var (
	mu      sync.Mutex // one process-wide lock around each read-modify-write (SPEC 3.10)
	seq     atomic.Int64
	dumped  atomic.Int64
	pruneMu sync.Mutex
	pruned  bool
)

// SettingsFile is the launcher's --settings file.
func SettingsFile() string { return filepath.Join(Dir, "settings.json") }

// CalibrationFile is the shared calibration notice.
func CalibrationFile() string { return filepath.Join(Dir, "calibration.json") }

var notWord = regexp.MustCompile(`[^A-Za-z0-9_-]`)

func fileFor(sessionID any) (string, error) {
	s, ok := sessionID.(string)
	if !ok {
		return "", fmt.Errorf("sessionId.replace is not a function")
	}
	return filepath.Join(Dir, notWord.ReplaceAllString(s, "")+".json"), nil
}

// EnsureDir creates the status directory owner-only.
func EnsureDir() error {
	if err := os.MkdirAll(Dir, 0o700); err != nil {
		return err
	}
	return os.Chmod(Dir, 0o700)
}

// WritePrivate writes text to file through a unique temporary file and a rename (SPEC 3.11).
func WritePrivate(file, text string) error {
	if err := EnsureDir(); err != nil {
		return err
	}
	temp := fmt.Sprintf("%s.%d.%d.tmp", file, os.Getpid(), seq.Add(1))
	if err := os.WriteFile(temp, []byte(text), 0o600); err != nil {
		return err
	}
	if err := fsx.RenameOver(temp, file); err != nil {
		return err
	}
	return os.Chmod(file, 0o600)
}

// WriteStatus publishes a status; an empty (falsy) id writes nothing. Errors are swallowed.
func WriteStatus(sessionID any, status any) {
	if !jsjson.Truthy(sessionID) {
		return
	}
	file, err := fileFor(sessionID)
	if err != nil {
		return
	}
	if WritePrivate(file, jsjson.Stringify(status)) != nil {
		return
	}
	pruneMu.Lock()
	first := !pruned
	pruned = true
	pruneMu.Unlock()
	if first {
		PruneStale(StaleAfterMs, NowMs())
	}
}

// ReadStatus is the parsed session file, or nil on any error.
func ReadStatus(sessionID any) any {
	file, err := fileFor(sessionID)
	if err != nil {
		return nil
	}
	data, err := os.ReadFile(file)
	if err != nil {
		return nil
	}
	v, err := jsjson.ParseBytes(data)
	if err != nil {
		return nil
	}
	return v
}

// Agent identifies which conversation inside a session a decision belongs to.
type Agent struct {
	Key   string
	Label string
	Main  bool
}

// spreadArray is `[...v]`: an array's elements, a string's code points, [] for nullish, and
// a TypeError for anything else that is not iterable.
func spreadArray(v any) ([]any, error) {
	switch x := v.(type) {
	case []any:
		return append([]any(nil), x...), nil
	case string:
		var out []any
		for i := 0; i < len(x); {
			_, w := jsstr.Decode(x, i)
			out = append(out, x[i:i+w])
			i += w
		}
		return out, nil
	}
	if jsjson.IsNullish(v) {
		return nil, nil
	}
	return nil, errors.New("object is not iterable")
}

// WriteDecision publishes a routed decision (SPEC 8.2). An error is what Node would throw.
func WriteDecision(sessionID any, decision *jsjson.Object, agent *Agent) error {
	mu.Lock()
	defer mu.Unlock()
	previous := ReadStatus(sessionID)
	var entry any = decision
	if agent != nil {
		e := decision.Clone()
		e.Set("agent", jsjson.Obj("key", agent.Key, "label", agent.Label, "main", agent.Main))
		entry = e
	}
	history, err := spreadArray(jsjson.Coalesce(jsjson.Prop(previous, "history"), []any{}))
	if err != nil {
		return err
	}
	history = append(history, entry)
	if len(history) > 20 {
		history = history[len(history)-20:]
	}
	agents := jsjson.Prop(previous, "agents")
	if agent != nil {
		agents = mergeAgent(agents, agent, jsjson.Obj(
			"label", agent.Label,
			"main", agent.Main,
			"tier", decision.Value("tier"),
			"model", decision.Value("model"),
			"confidence", decision.Value("confidence"),
			"effort", decision.Value("effort"),
			"reason", decision.Value("reason"),
			"at", jsjson.Coalesce(decision.Value("at"), NowMs()),
		))
	}
	out := decision.Clone()
	if jsjson.Truthy(agents) {
		out.Set("agents", agents)
	}
	out.Set("history", history)
	WriteStatus(sessionID, out)
	return nil
}

// MarkManual records that an agent runs a model the user chose (SPEC 8.2).
func MarkManual(sessionID any, model any, agent *Agent) {
	mu.Lock()
	defer mu.Unlock()
	previous := ReadStatus(sessionID)
	agents := jsjson.Prop(previous, "agents")
	if agent != nil {
		agents = mergeAgent(agents, agent, jsjson.Obj(
			"label", agent.Label, "main", agent.Main, "model", model, "manual", true, "at", NowMs(),
		))
	}
	var manual any = true
	if agent != nil && !agent.Main {
		manual = jsjson.Coalesce(jsjson.Prop(previous, "manual"), false)
	}
	out := jsjson.NewObject()
	jsjson.Spread(out, previous)
	if jsjson.Truthy(agents) {
		out.Set("agents", agents)
	}
	out.Set("manual", manual)
	out.Set("at", NowMs())
	WriteStatus(sessionID, out)
}

// MainDecision is the main thread's newest history entry, else the status; nil for a falsy status.
func MainDecision(status any) any {
	if !jsjson.Truthy(status) {
		return nil
	}
	history, _ := spreadArray(jsjson.Coalesce(jsjson.Prop(status, "history"), []any{}))
	for i := len(history) - 1; i >= 0; i-- {
		if jsjson.Truthy(jsjson.Path(history[i], "agent", "main")) {
			return history[i]
		}
	}
	return status
}

// byAtDesc sorts stably, newest `at` first, as `(b.at ?? 0) - (a.at ?? 0)`; NaN counts as a tie.
func byAtDesc(items []any) {
	at := func(v any) float64 { return jsjson.ToNumber(jsjson.Coalesce(jsjson.Prop(v, "at"), 0.0)) }
	sort.SliceStable(items, func(i, j int) bool {
		d := at(items[j]) - at(items[i])
		return !math.IsNaN(d) && d < 0
	})
}

func mergeAgent(existing any, agent *Agent, entry *jsjson.Object) *jsjson.Object {
	agents := jsjson.NewObject()
	jsjson.Spread(agents, jsjson.Coalesce(existing, jsjson.NewObject()))
	merged := jsjson.NewObject()
	jsjson.Spread(merged, agents.Value(agent.Key))
	jsjson.Spread(merged, entry)
	agents.Set(agent.Key, merged)
	keys := agents.Keys()
	if len(keys) > MaxAgents {
		var ordered []any
		for _, k := range keys {
			if !jsjson.Truthy(jsjson.Prop(agents.Value(k), "main")) {
				ordered = append(ordered, k)
			}
		}
		// Sort the keys by their entries' `at`.
		entries := make([]any, len(ordered))
		for i, k := range ordered {
			entries[i] = jsjson.Obj("k", k, "at", jsjson.Prop(agents.Value(k.(string)), "at"))
		}
		byAtDesc(entries)
		for i := MaxAgents - 1; i < len(entries); i++ {
			agents.Delete(jsjson.Prop(entries[i], "k").(string))
		}
	}
	return agents
}

// View is the main agent and the live sub-agents, newest first.
type View struct {
	Main      *jsjson.Object
	Subagents []*jsjson.Object
}

func entriesOf(v any) [][2]any {
	var out [][2]any
	switch x := v.(type) {
	case *jsjson.Object:
		for _, k := range x.Keys() {
			out = append(out, [2]any{k, x.Value(k)})
		}
	case []any:
		for i, e := range x {
			out = append(out, [2]any{strconv.Itoa(i), e})
		}
	case string:
		tmp := jsjson.NewObject()
		jsjson.Spread(tmp, x)
		return entriesOf(tmp)
	}
	return out
}

// AgentView splits a status's agents into the main one and the fresh sub-agents (SPEC 8.2).
func AgentView(status any, freshMs, now float64) View {
	var view View
	var subs []any
	for _, kv := range entriesOf(jsjson.Coalesce(jsjson.Prop(status, "agents"), jsjson.NewObject())) {
		a := jsjson.Obj("key", kv[0])
		jsjson.Spread(a, kv[1])
		main := jsjson.Truthy(a.Value("main"))
		if main && view.Main == nil {
			view.Main = a
		}
		if !main && now-jsjson.ToNumber(jsjson.Coalesce(a.Value("at"), 0.0)) <= freshMs {
			subs = append(subs, a)
		}
	}
	byAtDesc(subs)
	for _, s := range subs {
		view.Subagents = append(view.Subagents, s.(*jsjson.Object))
	}
	return view
}

// Calibration is what the last model list said.
type Calibration struct {
	Newer  []any
	Models []any
	At     any // float64, or nil when no session has read a model list
}

// WriteCalibration records the newest model per tier and which are newer than the tuning.
func WriteCalibration(newer, models []any, file string) {
	mu.Lock()
	defer mu.Unlock()
	if newer == nil {
		newer = []any{}
	}
	if models == nil {
		models = []any{}
	}
	_ = WritePrivate(file, jsjson.Stringify(jsjson.Obj("newer", newer, "models", models, "at", NowMs())))
}

// ReadCalibration reads the calibration file (SPEC 8.3).
func ReadCalibration(file string) Calibration {
	empty := Calibration{Newer: []any{}, Models: []any{}, At: nil}
	data, err := os.ReadFile(file)
	if err != nil {
		return empty
	}
	v, err := jsjson.ParseBytes(data)
	if err != nil || v == nil {
		return empty
	}
	newer, _ := jsjson.Prop(v, "newer").([]any)
	models, isArray := jsjson.Prop(v, "models").([]any)
	at, isNumber := jsjson.Prop(v, "at").(float64)
	out := Calibration{Newer: []any{}, Models: []any{}, At: nil}
	if newer != nil {
		out.Newer = newer
	}
	if isArray && isNumber {
		out.Models, out.At = models, at
	}
	return out
}

var dumpYes = regexp.MustCompile(`^(1|true|yes)$`)

// DumpBody saves a request body for diagnosis (SPEC 8.4) and returns the file, or "".
func DumpBody(body any, setting string) string {
	if setting == "" {
		return ""
	}
	prefix := setting
	if dumpYes.MatchString(jsstr.ASCIILower(setting)) {
		prefix = filepath.Join(Dir, "dump")
	}
	file := fmt.Sprintf("%s.%d-%d.json", prefix, int64(NowMs()), dumped.Add(1)-1)
	if strings.HasPrefix(prefix, Dir) {
		if EnsureDir() != nil {
			return ""
		}
	}
	if os.WriteFile(file, []byte(jsjson.Indent(body)), 0o600) != nil {
		return ""
	}
	return file
}

// PruneStale deletes status files untouched for maxAgeMs, settings.json excepted.
func PruneStale(maxAgeMs, now float64) int {
	removed := 0
	names, err := os.ReadDir(Dir)
	if err != nil {
		return 0
	}
	for _, e := range names {
		name := e.Name()
		if !strings.HasSuffix(name, ".json") || name == "settings.json" {
			continue
		}
		file := filepath.Join(Dir, name)
		info, err := os.Stat(file)
		if err != nil {
			continue
		}
		if now-float64(info.ModTime().UnixNano())/1e6 > maxAgeMs {
			if os.Remove(file) == nil {
				removed++
			}
		}
	}
	return removed
}
