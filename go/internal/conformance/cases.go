// Package conformance loads the golden cases in conformance/cases (SPEC 16.2) and decodes
// their tagged encoding (conformance/cases/README.md). The golden test lives beside it.
package conformance

import (
	"bytes"
	"encoding/hex"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
)

// Hex is a $hex tag: raw bytes.
type Hex []byte

type throwsTag struct{}
type clockTag struct{}

// Throws is the $throws tag: the Node call threw.
var Throws any = throwsTag{}

// Clock is the $clock tag: any number.
var Clock any = clockTag{}

// Case is one golden case.
type Case struct {
	Name     string
	Input    any
	Expected any
}

// Dir is the repository's conformance/cases directory.
func Dir() string {
	_, file, _, _ := runtime.Caller(0)
	return filepath.Join(filepath.Dir(file), "..", "..", "..", "conformance", "cases")
}

// Load reads and decodes one case file.
func Load(name string) ([]Case, error) {
	data, err := os.ReadFile(filepath.Join(Dir(), name))
	if err != nil {
		return nil, err
	}
	v, err := jsjson.ParseBytes(data)
	if err != nil {
		return nil, err
	}
	list, ok := v.([]any)
	if !ok {
		return nil, fmt.Errorf("%s is not an array", name)
	}
	var out []Case
	for _, item := range list {
		o := item.(*jsjson.Object)
		out = append(out, Case{
			Name:     o.Value("name").(string),
			Input:    Decode(o.Value("input")),
			Expected: Decode(o.Value("expected")),
		})
	}
	return out, nil
}

// Decode replaces every tag in v, at any depth.
func Decode(v any) any {
	switch x := v.(type) {
	case []any:
		out := make([]any, len(x))
		for i, e := range x {
			out[i] = Decode(e)
		}
		return out
	case *jsjson.Object:
		if x.Len() == 1 {
			k := x.Keys()[0]
			val := x.Value(k)
			switch k {
			case "$undefined":
				return jsjson.Undefined
			case "$number":
				switch val {
				case "NaN":
					return math.NaN()
				case "Infinity":
					return math.Inf(1)
				case "-Infinity":
					return math.Inf(-1)
				case "-0":
					return math.Copysign(0, -1)
				}
			case "$utf16":
				var units []uint16
				for _, u := range val.([]any) {
					units = append(units, uint16(u.(float64)))
				}
				return FromUTF16(units)
			case "$hex":
				b, _ := hex.DecodeString(val.(string))
				return Hex(b)
			case "$throws":
				return Throws
			case "$clock":
				return Clock
			}
		}
		out := jsjson.NewObject()
		for _, k := range x.Keys() {
			out.Set(k, Decode(x.Value(k)))
		}
		return out
	}
	return v
}

// FromUTF16 builds a WTF-8 string from UTF-16 units, pairing surrogates where they pair.
func FromUTF16(units []uint16) string {
	var b []byte
	for i := 0; i < len(units); i++ {
		u := rune(units[i])
		if u >= 0xD800 && u <= 0xDBFF && i+1 < len(units) && units[i+1] >= 0xDC00 && units[i+1] <= 0xDFFF {
			u = 0x10000 + (u-0xD800)<<10 + (rune(units[i+1]) - 0xDC00)
			i++
		}
		b = jsstr.AppendRune(b, u)
	}
	return string(b)
}

// Equal compares two values: objects key by key in order, NaN equal to NaN, -0 distinct from
// 0, Clock matching any number.
func Equal(got, want any) bool {
	if want == Clock {
		_, ok := got.(float64)
		return ok
	}
	switch w := want.(type) {
	case float64:
		g, ok := got.(float64)
		if !ok {
			return false
		}
		if math.IsNaN(w) {
			return math.IsNaN(g)
		}
		return g == w && math.Signbit(g) == math.Signbit(w)
	case []any:
		g, ok := got.([]any)
		if !ok || len(g) != len(w) {
			return false
		}
		for i := range w {
			if !Equal(g[i], w[i]) {
				return false
			}
		}
		return true
	case *jsjson.Object:
		g, ok := got.(*jsjson.Object)
		if !ok {
			return false
		}
		gk, wk := g.Keys(), w.Keys()
		if len(gk) != len(wk) {
			return false
		}
		for i := range wk {
			if gk[i] != wk[i] || !Equal(g.Value(gk[i]), w.Value(wk[i])) {
				return false
			}
		}
		return true
	case Hex:
		g, ok := got.(Hex)
		return ok && bytes.Equal(g, w)
	}
	return got == want
}

// Show renders a value for a failure message.
func Show(v any) string {
	switch v {
	case Throws:
		return "<throws>"
	case Clock:
		return "<clock>"
	case jsjson.Undefined:
		return "undefined"
	}
	if f, ok := v.(float64); ok && (math.IsNaN(f) || math.IsInf(f, 0) || (f == 0 && math.Signbit(f))) {
		return jsjson.NumberToString(f) + map[bool]string{true: " (-0)", false: ""}[f == 0]
	}
	return strings.ReplaceAll(jsjson.Stringify(v), "\n", `\n`)
}

// Diff names the first place got and want differ, or "".
func Diff(got, want any, path string) string {
	if Equal(got, want) {
		return ""
	}
	switch w := want.(type) {
	case []any:
		if g, ok := got.([]any); ok && len(g) == len(w) {
			for i := range w {
				if d := Diff(g[i], w[i], fmt.Sprintf("%s[%d]", path, i)); d != "" {
					return d
				}
			}
		}
	case *jsjson.Object:
		if g, ok := got.(*jsjson.Object); ok {
			gk, wk := g.Keys(), w.Keys()
			if strings.Join(gk, ",") != strings.Join(wk, ",") {
				return fmt.Sprintf("%s: keys %v, want %v", path, gk, wk)
			}
			for _, k := range wk {
				if d := Diff(g.Value(k), w.Value(k), path+"."+k); d != "" {
					return d
				}
			}
		}
	}
	return fmt.Sprintf("%s: got %s, want %s", path, Show(got), Show(want))
}
