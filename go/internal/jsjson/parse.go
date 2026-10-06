package jsjson

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
)

// SyntaxError is a JSON.parse failure. The message is free-form.
type SyntaxError struct{ Msg string }

func (e *SyntaxError) Error() string { return e.Msg }

// ParseBytes is `JSON.parse(buffer.toString())`: bytes decode as UTF-8 with each invalid
// sequence replaced by U+FFFD, then parse.
func ParseBytes(p []byte) (any, error) { return Parse(jsstr.DecodeBytes(p)) }

// Parse is JSON.parse over a (WTF-8) string.
func Parse(s string) (any, error) {
	p := &parser{s: s}
	p.ws()
	v, err := p.value()
	if err != nil {
		return nil, err
	}
	p.ws()
	if p.i != len(p.s) {
		return nil, p.fail("unexpected non-whitespace character after JSON")
	}
	return v, nil
}

type parser struct {
	s string
	i int
}

func (p *parser) fail(msg string) error {
	return &SyntaxError{fmt.Sprintf("%s at position %d", msg, p.i)}
}

func (p *parser) ws() {
	for p.i < len(p.s) {
		switch p.s[p.i] {
		case ' ', '\t', '\n', '\r':
			p.i++
		default:
			return
		}
	}
}

func (p *parser) value() (any, error) {
	if p.i >= len(p.s) {
		return nil, p.fail("unexpected end of JSON input")
	}
	switch c := p.s[p.i]; {
	case c == '{':
		return p.object()
	case c == '[':
		return p.array()
	case c == '"':
		return p.str()
	case c == '-' || (c >= '0' && c <= '9'):
		return p.number()
	case strings.HasPrefix(p.s[p.i:], "true"):
		p.i += 4
		return true, nil
	case strings.HasPrefix(p.s[p.i:], "false"):
		p.i += 5
		return false, nil
	case strings.HasPrefix(p.s[p.i:], "null"):
		p.i += 4
		return nil, nil
	}
	return nil, p.fail("unexpected token")
}

func (p *parser) object() (any, error) {
	p.i++
	o := NewObject()
	p.ws()
	if p.i < len(p.s) && p.s[p.i] == '}' {
		p.i++
		return o, nil
	}
	for {
		p.ws()
		if p.i >= len(p.s) || p.s[p.i] != '"' {
			return nil, p.fail("expected property name")
		}
		k, err := p.str()
		if err != nil {
			return nil, err
		}
		p.ws()
		if p.i >= len(p.s) || p.s[p.i] != ':' {
			return nil, p.fail("expected ':'")
		}
		p.i++
		p.ws()
		v, err := p.value()
		if err != nil {
			return nil, err
		}
		o.Set(k.(string), v)
		p.ws()
		if p.i >= len(p.s) {
			return nil, p.fail("unterminated object")
		}
		if p.s[p.i] == ',' {
			p.i++
			continue
		}
		if p.s[p.i] == '}' {
			p.i++
			return o, nil
		}
		return nil, p.fail("expected ',' or '}'")
	}
}

func (p *parser) array() (any, error) {
	p.i++
	a := []any{}
	p.ws()
	if p.i < len(p.s) && p.s[p.i] == ']' {
		p.i++
		return a, nil
	}
	for {
		p.ws()
		v, err := p.value()
		if err != nil {
			return nil, err
		}
		a = append(a, v)
		p.ws()
		if p.i >= len(p.s) {
			return nil, p.fail("unterminated array")
		}
		if p.s[p.i] == ',' {
			p.i++
			continue
		}
		if p.s[p.i] == ']' {
			p.i++
			return a, nil
		}
		return nil, p.fail("expected ',' or ']'")
	}
}

func hex4(s string) (rune, bool) {
	if len(s) < 4 {
		return 0, false
	}
	n, err := strconv.ParseUint(s[:4], 16, 32)
	return rune(n), err == nil
}

func (p *parser) str() (any, error) {
	p.i++
	var b []byte
	for {
		if p.i >= len(p.s) {
			return nil, p.fail("unterminated string")
		}
		c := p.s[p.i]
		switch {
		case c == '"':
			p.i++
			return string(b), nil
		case c < 0x20:
			return nil, p.fail("bad control character in string literal")
		case c == '\\':
			if p.i+1 >= len(p.s) {
				return nil, p.fail("unterminated string")
			}
			e := p.s[p.i+1]
			p.i += 2
			switch e {
			case '"', '\\', '/':
				b = append(b, e)
			case 'b':
				b = append(b, '\b')
			case 'f':
				b = append(b, '\f')
			case 'n':
				b = append(b, '\n')
			case 'r':
				b = append(b, '\r')
			case 't':
				b = append(b, '\t')
			case 'u':
				r, ok := hex4(p.s[p.i:])
				if !ok || strings.ContainsAny(p.s[p.i:p.i+4], "+-xX_") {
					return nil, p.fail("bad unicode escape")
				}
				p.i += 4
				if r >= 0xD800 && r <= 0xDBFF && p.i+6 <= len(p.s) && p.s[p.i] == '\\' && p.s[p.i+1] == 'u' {
					if lo, ok := hex4(p.s[p.i+2:]); ok && lo >= 0xDC00 && lo <= 0xDFFF && !strings.ContainsAny(p.s[p.i+2:p.i+6], "+-xX_") {
						p.i += 6
						r = 0x10000 + (r-0xD800)<<10 + (lo - 0xDC00)
					}
				}
				b = jsstr.AppendRune(b, r)
			default:
				return nil, p.fail("bad escaped character")
			}
		default:
			b = append(b, c)
			p.i++
		}
	}
}

func digits(s string, i int) int {
	j := i
	for j < len(s) && s[j] >= '0' && s[j] <= '9' {
		j++
	}
	return j
}

func (p *parser) number() (any, error) {
	start := p.i
	if p.s[p.i] == '-' {
		p.i++
	}
	if p.i >= len(p.s) {
		return nil, p.fail("no number after minus sign")
	}
	if p.s[p.i] == '0' {
		p.i++
	} else if p.s[p.i] >= '1' && p.s[p.i] <= '9' {
		p.i = digits(p.s, p.i)
	} else {
		return nil, p.fail("no number after minus sign")
	}
	if p.i < len(p.s) && p.s[p.i] == '.' {
		j := digits(p.s, p.i+1)
		if j == p.i+1 {
			return nil, p.fail("unterminated fractional number")
		}
		p.i = j
	}
	if p.i < len(p.s) && (p.s[p.i] == 'e' || p.s[p.i] == 'E') {
		j := p.i + 1
		if j < len(p.s) && (p.s[j] == '+' || p.s[j] == '-') {
			j++
		}
		k := digits(p.s, j)
		if k == j {
			return nil, p.fail("exponent part is missing a number")
		}
		p.i = k
	}
	f, _ := strconv.ParseFloat(p.s[start:p.i], 64)
	return f, nil
}

// utf16Units splits a string into its UTF-16 code units, each as a one-unit string.
func utf16Units(s string) []any {
	var out []any
	for i := 0; i < len(s); {
		r, w := jsstr.Decode(s, i)
		i += w
		if r >= 0x10000 {
			r -= 0x10000
			out = append(out, string(jsstr.AppendRune(nil, 0xD800+(r>>10))), string(jsstr.AppendRune(nil, 0xDC00+(r&0x3FF))))
			continue
		}
		out = append(out, s[i-w:i])
	}
	return out
}
