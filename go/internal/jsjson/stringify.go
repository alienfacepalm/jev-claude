package jsjson

import (
	"math"
	"strings"
	"unicode/utf8"

	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
)

// Stringify is compact JSON.stringify. A top-level Undefined yields "".
func Stringify(v any) string {
	var b strings.Builder
	write(&b, v, "", "")
	return b.String()
}

// Indent is JSON.stringify(v, null, 2).
func Indent(v any) string {
	var b strings.Builder
	write(&b, v, "  ", "")
	return b.String()
}

// Quote writes s as a JSON string literal.
func Quote(s string) string {
	var b strings.Builder
	quote(&b, s)
	return b.String()
}

const hexdigits = "0123456789abcdef"

func quote(b *strings.Builder, s string) {
	b.WriteByte('"')
	for i := 0; i < len(s); {
		r, w := jsstr.Decode(s, i)
		switch {
		case r == '"':
			b.WriteString(`\"`)
		case r == '\\':
			b.WriteString(`\\`)
		case r == '\b':
			b.WriteString(`\b`)
		case r == '\f':
			b.WriteString(`\f`)
		case r == '\n':
			b.WriteString(`\n`)
		case r == '\r':
			b.WriteString(`\r`)
		case r == '\t':
			b.WriteString(`\t`)
		case r < 0x20 || jsstr.IsSurrogate(r):
			b.WriteString(`\u`)
			b.WriteByte(hexdigits[r>>12&0xF])
			b.WriteByte(hexdigits[r>>8&0xF])
			b.WriteByte(hexdigits[r>>4&0xF])
			b.WriteByte(hexdigits[r&0xF])
		case r == utf8.RuneError && w == 1:
			b.WriteString("�")
		default:
			b.WriteString(s[i : i+w])
		}
		i += w
	}
	b.WriteByte('"')
}

func write(b *strings.Builder, v any, step, indent string) {
	switch x := v.(type) {
	case nil:
		b.WriteString("null")
	case undefined:
		// Only reachable at top level; members and elements are handled by the callers.
	case bool:
		if x {
			b.WriteString("true")
		} else {
			b.WriteString("false")
		}
	case float64:
		if math.IsNaN(x) || math.IsInf(x, 0) {
			b.WriteString("null")
		} else {
			b.WriteString(NumberToString(x))
		}
	case int:
		b.WriteString(NumberToString(float64(x)))
	case string:
		quote(b, x)
	case []any:
		if len(x) == 0 {
			b.WriteString("[]")
			return
		}
		inner := indent + step
		b.WriteByte('[')
		for i, e := range x {
			if i > 0 {
				b.WriteByte(',')
			}
			if step != "" {
				b.WriteString("\n" + inner)
			}
			if e == Undefined {
				b.WriteString("null")
			} else {
				write(b, e, step, inner)
			}
		}
		if step != "" {
			b.WriteString("\n" + indent)
		}
		b.WriteByte(']')
	case *Object:
		inner := indent + step
		first := true
		b.WriteByte('{')
		for _, k := range x.Keys() {
			e := x.vals[k]
			if e == Undefined {
				continue
			}
			if !first {
				b.WriteByte(',')
			}
			first = false
			if step != "" {
				b.WriteString("\n" + inner)
			}
			quote(b, k)
			b.WriteByte(':')
			if step != "" {
				b.WriteByte(' ')
			}
			write(b, e, step, inner)
		}
		if !first && step != "" {
			b.WriteString("\n" + indent)
		}
		b.WriteByte('}')
	default:
		b.WriteString("null")
	}
}
