package proxy

import (
	"crypto/sha1"
	"encoding/hex"
	"errors"
	"regexp"
	"sort"
	"strconv"
	"strings"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
	"github.com/alienfacepalm/jev-claude/go/internal/modelnames"
)

// typeError stands for a JavaScript TypeError: the caller takes Node's catch path.
func typeError(msg string) error { return errors.New(msg) }

// SanitizeSchema rewrites draft-04 boolean exclusive bounds in place, recursively.
func SanitizeSchema(node any) {
	switch n := node.(type) {
	case []any:
		for _, v := range n {
			SanitizeSchema(v)
		}
	case *jsjson.Object:
		for _, pair := range [][2]string{{"exclusiveMinimum", "minimum"}, {"exclusiveMaximum", "maximum"}} {
			key, bound := pair[0], pair[1]
			if b, ok := n.Value(key).(bool); ok {
				if v, isNum := n.Value(bound).(float64); b && isNum {
					n.Set(key, v)
					n.Delete(bound)
				} else {
					n.Delete(key)
				}
			}
		}
		for _, k := range n.Keys() {
			SanitizeSchema(n.Value(k))
		}
	}
}

var reminder = regexp.MustCompile(`<system-reminder>(?s:.)*?</system-reminder>`)

// spreadList is `[...v]` for a list Node iterates.
func spreadList(v any) ([]any, error) {
	switch x := v.(type) {
	case []any:
		return x, nil
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
	return nil, typeError("object is not iterable")
}

// blockType is `b.type`, which throws on a null or undefined block.
func blockType(b any) (any, error) {
	if jsjson.IsNullish(b) {
		return nil, typeError("Cannot read properties of null (reading 'type')")
	}
	return jsjson.Prop(b, "type"), nil
}

// joinTexts is `blocks.filter(b => b.type === "text").map(b => b.text).join(sep)`.
func joinTexts(blocks []any, sep string) (string, error) {
	var parts []string
	for _, b := range blocks {
		t, err := blockType(b)
		if err != nil {
			return "", err
		}
		if t == "text" {
			text := jsjson.Prop(b, "text")
			if jsjson.IsNullish(text) {
				parts = append(parts, "")
			} else {
				parts = append(parts, jsjson.JSString(text))
			}
		}
	}
	return strings.Join(parts, sep), nil
}

func hasTools(body any) bool {
	tools, ok := jsjson.Prop(body, "tools").([]any)
	return ok && len(tools) > 0
}

// NewTurnPrompt is the text of a genuinely new user turn, or nil (SPEC 7.5).
func NewTurnPrompt(body any) (any, error) {
	if !hasTools(body) {
		return nil, nil
	}
	messages, err := spreadList(jsjson.Coalesce(jsjson.Prop(body, "messages"), []any{}))
	if err != nil {
		return nil, err
	}
	var last any = jsjson.Undefined
	for i := len(messages) - 1; i >= 0; i-- {
		if jsjson.Prop(messages[i], "role") != "system" {
			last = messages[i]
			break
		}
	}
	if !jsjson.Truthy(last) || jsjson.Prop(last, "role") != "user" {
		return nil, nil
	}
	var text string
	switch content := jsjson.Prop(last, "content").(type) {
	case string:
		text = content
	case []any:
		for _, b := range content {
			t, err := blockType(b)
			if err != nil {
				return nil, err
			}
			if t == "tool_result" {
				return nil, nil
			}
		}
		if text, err = joinTexts(content, "\n"); err != nil {
			return nil, err
		}
	default:
		return nil, nil
	}
	text = jsstr.Trim(reminder.ReplaceAllLiteralString(text, ""))
	if text == "" {
		return nil, nil
	}
	return text, nil
}

var thinking = regexp.MustCompile(`thinking`)

// keyCount is `Object.keys(v).length`.
func keyCount(v any) int {
	switch x := v.(type) {
	case *jsjson.Object:
		return x.Len()
	case []any:
		return len(x)
	case string:
		return jsstr.Len16(x)
	}
	return 0
}

// ApplyTier points a request body at a tier, removing fields that tier cannot take. model ""
// means the tier's own id. An unknown tier leaves the body alone.
func ApplyTier(body *jsjson.Object, tierName, model string, env config.Getenv) *jsjson.Object {
	tier, ok := config.TierSpec(tierName)
	if !ok {
		return body
	}
	if model == "" {
		model = tier.ID
	}
	body.Set("model", model)
	if !tier.Thinking {
		body.Delete("thinking")
		cm := body.Value("context_management")
		if edits, ok := jsjson.Prop(cm, "edits").([]any); ok {
			kept := []any{}
			for _, e := range edits {
				t := jsjson.JSString(jsjson.Coalesce(jsjson.Prop(e, "type"), ""))
				if !thinking.MatchString(jsstr.ASCIILower(t)) {
					kept = append(kept, e)
				}
			}
			cm.(*jsjson.Object).Set("edits", kept)
			if len(kept) == 0 {
				body.Delete("context_management")
			}
		}
	}
	oc := body.Value("output_config")
	if !tier.Effort && jsjson.Truthy(oc) {
		if o, ok := oc.(*jsjson.Object); ok {
			o.Delete("effort")
		}
		if keyCount(oc) == 0 {
			body.Delete("output_config")
		}
	} else if tier.Effort {
		effort := config.ForcedEffort(tierName, env)
		if effort == "" && !jsjson.Truthy(jsjson.Prop(oc, "effort")) {
			effort = config.EffortFloor(tierName, env)
		}
		if effort != "" {
			next := jsjson.NewObject()
			jsjson.Spread(next, oc)
			next.Set("effort", effort)
			body.Set("output_config", next)
		}
	}
	return body
}

// VersionOf is [major, minor] read from a model id of the given tier, [0, 0] when unreadable.
func VersionOf(id any, tier any) [2]float64 {
	name, _ := tier.(string)
	spec, ok := config.TierSpec(name)
	if !ok || spec.Family == "" {
		return [2]float64{}
	}
	text := "" // `id = ""` applies only when id is undefined
	if id != jsjson.Undefined {
		text = jsjson.JSString(id)
	}
	re := versionPatterns[spec.Family]
	m := re.FindStringSubmatchIndex(text)
	if m == nil {
		return [2]float64{}
	}
	major, _ := strconv.ParseFloat(text[m[2]:m[3]], 64)
	minor := 0.0
	if s := modelnames.MinorAfter(text, m[1]); s != "" {
		minor, _ = strconv.ParseFloat(s, 64)
	}
	return [2]float64{major, minor}
}

var versionPatterns = func() map[string]*regexp.Regexp {
	out := map[string]*regexp.Regexp{}
	for _, t := range config.Tiers {
		out[t.Family] = regexp.MustCompile(`^(?:[\w-]+\.)?claude-` + t.Family + `-(\d+)`)
	}
	return out
}()

func compareVersions(a, b [2]float64) float64 {
	if d := a[0] - b[0]; d != 0 {
		return d
	}
	return a[1] - b[1]
}

// CatalogModel is one exact model, with the releasedAt value as Node holds it.
type CatalogModel struct {
	config.Model
	Released any
}

// ClaudeModels lists the catalog's Claude models newest first, falling back to the four
// static tiers when there are none. It fails where Node throws (a non-string created_at).
func ClaudeModels(catalog []any) ([]CatalogModel, error) {
	var models []CatalogModel
	for _, m := range catalog {
		id := jsjson.Prop(m, "id")
		tier := config.TierOf(id)
		if tier == "" {
			continue
		}
		created := jsjson.Prop(m, "created_at")
		var parts []string
		if d := jsjson.Prop(m, "display_name"); jsjson.Truthy(d) {
			parts = append(parts, jsjson.JSString(d))
		}
		if jsjson.Truthy(created) {
			s, ok := created.(string)
			if !ok {
				return nil, typeError("model.created_at.slice is not a function")
			}
			parts = append(parts, "released "+jsstr.Slice16(s, 0, 10))
		}
		if t := jsjson.Prop(m, "max_input_tokens"); jsjson.Truthy(t) {
			parts = append(parts, jsjson.JSString(t)+" input tokens")
		}
		models = append(models, CatalogModel{
			Model:    config.Model{ID: id.(string), Tier: tier, Description: strings.Join(parts, "; ")},
			Released: jsjson.Coalesce(created, ""),
		})
	}
	var sortErr error
	sort.SliceStable(models, func(i, j int) bool {
		a, b := models[i], models[j]
		if d := compareVersions(VersionOf(b.ID, b.Tier), VersionOf(a.ID, a.Tier)); d != 0 {
			return d < 0
		}
		ra, okA := a.Released.(string)
		rb, okB := b.Released.(string)
		if !okA || !okB {
			sortErr = typeError("b.releasedAt.localeCompare is not a function")
			return false
		}
		// Code-unit order stands in for localeCompare (SPEC 7.5, 19.1).
		return compare16(rb, ra) < 0
	})
	if sortErr != nil {
		return nil, sortErr
	}
	for i := range models {
		if s, ok := models[i].Released.(string); ok {
			models[i].ReleasedAt = s
		}
	}
	if len(models) == 0 {
		for _, t := range config.Tiers {
			models = append(models, CatalogModel{Model: config.Model{ID: t.ID, Tier: t.Name, Description: t.ID}, Released: ""})
		}
	}
	return models, nil
}

// compare16 compares strings by UTF-16 code units.
func compare16(a, b string) int {
	ua, ub := units16(a), units16(b)
	for i := 0; i < len(ua) && i < len(ub); i++ {
		if ua[i] != ub[i] {
			if ua[i] < ub[i] {
				return -1
			}
			return 1
		}
	}
	return len(ua) - len(ub)
}

func units16(s string) []uint16 {
	var out []uint16
	for i := 0; i < len(s); {
		r, w := jsstr.Decode(s, i)
		i += w
		if r >= 0x10000 {
			r -= 0x10000
			out = append(out, uint16(0xD800+(r>>10)), uint16(0xDC00+(r&0x3FF)))
		} else {
			out = append(out, uint16(r))
		}
	}
	return out
}

// NewestPerTier keeps the first model of each tier, in first-seen order.
func NewestPerTier(models []CatalogModel) []CatalogModel {
	seen := map[string]bool{}
	var out []CatalogModel
	for _, m := range models {
		if !seen[m.Tier] {
			seen[m.Tier] = true
			out = append(out, m)
		}
	}
	return out
}

// NewerThanCalibrated lists the newest model ids per tier that are newer than the tuning.
func NewerThanCalibrated(catalog []any) ([]string, error) {
	models, err := ClaudeModels(catalog)
	if err != nil {
		return nil, err
	}
	out := []string{}
	for _, m := range NewestPerTier(models) {
		if compareVersions(VersionOf(m.ID, m.Tier), VersionOf(config.IDOf(m.Tier), m.Tier)) > 0 {
			out = append(out, m.ID)
		}
	}
	return out, nil
}

// SessionOf is the session id from metadata.user_id, "" on any failure.
func SessionOf(body any) any {
	uid := jsjson.Coalesce(jsjson.Path(body, "metadata", "user_id"), "{}")
	v, err := jsjson.Parse(jsjson.JSString(uid))
	if err != nil || v == nil {
		return ""
	}
	return jsjson.Coalesce(jsjson.Prop(v, "session_id"), "")
}

// firstContent is `body?.messages?.[0]?.content`.
func firstContent(body any) any {
	var first any = jsjson.Undefined
	switch m := jsjson.Prop(body, "messages").(type) {
	case []any:
		if len(m) > 0 {
			first = m[0]
		}
	case *jsjson.Object:
		first = m.Value("0")
	}
	return jsjson.Prop(first, "content")
}

func contentText(body any, sep string) (string, error) {
	switch c := firstContent(body).(type) {
	case string:
		return c, nil
	case []any:
		return joinTexts(c, sep)
	}
	return "", nil
}

// ConversationKey is the first 12 hex digits of SHA-1 over "<session>|<first message text>".
func ConversationKey(body any) (string, error) {
	session := SessionOf(body)
	text, err := contentText(body, "")
	if err != nil {
		return "", err
	}
	sum := sha1.Sum([]byte(jsstr.ToUTF8(jsjson.JSString(session) + "|" + text)))
	return hex.EncodeToString(sum[:])[:12], nil
}

var wsRun = regexp.MustCompile(jsstr.JSWSClass + `+`)

// AgentLabel is a short readable name for a conversation, from its first message.
func AgentLabel(body any, max int) (string, error) {
	text, err := contentText(body, " ")
	if err != nil {
		return "", err
	}
	clean := jsstr.Trim(wsRun.ReplaceAllLiteralString(reminder.ReplaceAllLiteralString(text, ""), " "))
	if jsstr.Len16(clean) > max {
		return jsstr.Slice16(clean, 0, max-1) + "\xe2\x80\xa6", nil
	}
	return clean, nil
}

// Mains maps a session id to its main conversation key, oldest insertion first.
type Mains struct {
	keys []string // jsjson.Stringify of the session, so 5 and "5" stay distinct
	vals map[string]string
	ids  map[string]any
}

// NewMains returns an empty map.
func NewMains() *Mains { return &Mains{vals: map[string]string{}, ids: map[string]any{}} }

func (m *Mains) get(session any) (string, bool) {
	v, ok := m.vals[jsjson.Stringify(session)]
	return v, ok
}

func (m *Mains) set(session any, key string) {
	k := jsjson.Stringify(session)
	if _, ok := m.vals[k]; !ok {
		m.keys = append(m.keys, k)
	}
	m.vals[k] = key
	m.ids[k] = session
}

func (m *Mains) deleteOldest() {
	if len(m.keys) == 0 {
		return
	}
	k := m.keys[0]
	m.keys = m.keys[1:]
	delete(m.vals, k)
	delete(m.ids, k)
}

// Entries are the [session, key] pairs in insertion order.
func (m *Mains) Entries() [][2]any {
	out := make([][2]any, len(m.keys))
	for i, k := range m.keys {
		out[i] = [2]any{m.ids[k], m.vals[k]}
	}
	return out
}

func (m *Mains) isMainKey(key string) bool {
	for _, v := range m.vals {
		if v == key {
			return true
		}
	}
	return false
}

// AgentOf says which agent in a session a request belongs to, recording the main thread.
func AgentOf(body any, mains *Mains) (key, label string, main bool, err error) {
	key, err = ConversationKey(body)
	if err != nil {
		return
	}
	session := SessionOf(body)
	if jsjson.Truthy(session) && hasTools(body) {
		if _, ok := mains.get(session); !ok {
			if len(mains.keys) > 50 {
				mains.deleteOldest()
			}
			mains.set(session, key)
		}
	}
	if !jsjson.Truthy(session) {
		main = true
	} else if k, ok := mains.get(session); ok && k == key {
		main = true
	}
	label, err = AgentLabel(body, 48)
	if err != nil {
		return
	}
	if label == "" {
		if main {
			label = "main"
		} else {
			label = key
		}
	}
	return
}
