// Package jsstr holds the JavaScript string semantics the port needs (SPEC 3.1, 3.2, 3.5).
//
// Strings are Go strings holding WTF-8: valid UTF-8 plus lone UTF-16 surrogates encoded as the
// three bytes ED A0-BF 80-BF, which is how a JavaScript string with a lone surrogate survives
// in Go. Lengths, slices and padding count UTF-16 code units, as JavaScript does.
package jsstr

import (
	"strings"
	"unicode"
	"unicode/utf8"
)

// JSWSClass is the JavaScript `\s` set written out as a Go regexp class (SPEC 3.1). Every `\s`
// in a ported pattern is spelled with this; Go's own `\s` is a different set.
const JSWSClass = `[\t\n\x0B\f\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]`

// IsJSWS reports whether r is in the JavaScript whitespace set.
func IsJSWS(r rune) bool {
	switch r {
	case '\t', '\n', 0x0B, '\f', '\r', ' ', 0xA0, 0x1680, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000, 0xFEFF:
		return true
	}
	return r >= 0x2000 && r <= 0x200A
}

// Decode returns the code point at s[i:] and its byte width. A WTF-8 lone surrogate decodes
// to its surrogate code point; any other invalid byte decodes as U+FFFD with width 1.
func Decode(s string, i int) (rune, int) {
	if i+2 < len(s) && s[i] == 0xED && s[i+1] >= 0xA0 && s[i+1] <= 0xBF && s[i+2] >= 0x80 && s[i+2] <= 0xBF {
		return rune(0xD000 | rune(s[i+1]&0x3F)<<6 | rune(s[i+2]&0x3F)), 3
	}
	r, n := utf8.DecodeRuneInString(s[i:])
	return r, n
}

// IsSurrogate reports whether r is a UTF-16 surrogate code point.
func IsSurrogate(r rune) bool { return r >= 0xD800 && r <= 0xDFFF }

// AppendRune appends r to b as WTF-8 (surrogates as three bytes).
func AppendRune(b []byte, r rune) []byte {
	if IsSurrogate(r) {
		return append(b, 0xED, byte(0x80|(r>>6)&0x3F), byte(0x80|r&0x3F))
	}
	return utf8.AppendRune(b, r)
}

func units(r rune) int {
	if r >= 0x10000 {
		return 2
	}
	return 1
}

// Len16 is the JavaScript `length` of s: its UTF-16 code units.
func Len16(s string) int {
	n := 0
	for i := 0; i < len(s); {
		r, w := Decode(s, i)
		n += units(r)
		i += w
	}
	return n
}

// Slice16 is `s.slice(start, end)` in UTF-16 units for 0 <= start <= end. A cut that would
// split a surrogate pair drops that half (SPEC 3.2; Node would keep a lone surrogate).
func Slice16(s string, start, end int) string {
	var b strings.Builder
	pos := 0
	for i := 0; i < len(s) && pos < end; {
		r, w := Decode(s, i)
		u := units(r)
		if pos >= start && pos+u <= end {
			b.WriteString(s[i : i+w])
		}
		pos += u
		i += w
	}
	return b.String()
}

// PadEnd16 is `s.padEnd(n)` with spaces, measured in UTF-16 units.
func PadEnd16(s string, n int) string {
	if l := Len16(s); l < n {
		return s + strings.Repeat(" ", n-l)
	}
	return s
}

// CodePoints is `[...s].length`.
func CodePoints(s string) int {
	n := 0
	for i := 0; i < len(s); {
		_, w := Decode(s, i)
		n++
		i += w
	}
	return n
}

// Trim is `String.prototype.trim()`: strips exactly the JSWS set from both ends (SPEC 3.5).
func Trim(s string) string {
	start := 0
	for start < len(s) {
		r, w := Decode(s, start)
		if !IsJSWS(r) {
			break
		}
		start += w
	}
	end := len(s)
	for end > start {
		// Walk back to the start of the last code point.
		j := end - 1
		for j > start && !isStart(s, j) {
			j--
		}
		r, _ := Decode(s, j)
		if !IsJSWS(r) {
			break
		}
		end = j
	}
	return s[start:end]
}

func isStart(s string, j int) bool { return s[j]&0xC0 != 0x80 }

// ASCIILower lowers A-Z only, keeping every byte offset: the stand-in for a JavaScript
// non-`u` `i` flag, which folds ASCII letters only (SPEC 3.1).
func ASCIILower(s string) string {
	b := []byte(s)
	for i, c := range b {
		if c >= 'A' && c <= 'Z' {
			b[i] = c + 32
		}
	}
	return string(b)
}

// specialUpper holds the unconditional SpecialCasing.txt upper-case expansions that
// String.prototype.toUpperCase applies and unicode.ToUpper does not.
var specialUpper = map[rune]string{
	0x00DF: "SS", 0x0149: "ʼN", 0x01F0: "J̌", 0x0390: "Ϊ́",
	0x03B0: "Ϋ́", 0x0587: "ԵՒ", 0x1E96: "H̱", 0x1E97: "T̈",
	0x1E98: "W̊", 0x1E99: "Y̊", 0x1E9A: "Aʾ", 0xFB00: "FF", 0xFB01: "FI",
	0xFB02: "FL", 0xFB03: "FFI", 0xFB04: "FFL", 0xFB05: "ST", 0xFB06: "ST",
}

// ToUpper is `String.prototype.toUpperCase()` for the cases that matter here: Unicode simple
// upper-casing plus the common unconditional expansions (ß -> SS, ligatures). Lone surrogates
// pass through.
func ToUpper(s string) string {
	b := make([]byte, 0, len(s))
	for i := 0; i < len(s); {
		r, w := Decode(s, i)
		i += w
		if sp, ok := specialUpper[r]; ok {
			b = append(b, sp...)
			continue
		}
		if !IsSurrogate(r) {
			r = unicode.ToUpper(r)
		}
		b = AppendRune(b, r)
	}
	return string(b)
}

// ToUTF8 converts WTF-8 to UTF-8, each lone surrogate (and any invalid byte) becoming U+FFFD:
// what Node writes when a string reaches a terminal, a file as text, or a hash (SPEC 3.3).
func ToUTF8(s string) string {
	b := make([]byte, 0, len(s))
	for i := 0; i < len(s); {
		r, w := Decode(s, i)
		i += w
		if IsSurrogate(r) {
			r = utf8.RuneError
		}
		b = utf8.AppendRune(b, r)
	}
	return string(b)
}

// DecodeBytes decodes bytes as UTF-8 the way Node's Buffer.toString() and TextDecoder do
// (the WHATWG decoder): each maximal invalid subpart becomes one U+FFFD.
func DecodeBytes(p []byte) string {
	if utf8.Valid(p) {
		return string(p)
	}
	b := make([]byte, 0, len(p)+8)
	for i := 0; i < len(p); {
		c := p[i]
		if c < 0x80 {
			b = append(b, c)
			i++
			continue
		}
		need, lo, hi := 0, byte(0x80), byte(0xBF)
		switch {
		case c >= 0xC2 && c <= 0xDF:
			need = 1
		case c == 0xE0:
			need, lo = 2, 0xA0
		case c >= 0xE1 && c <= 0xEC, c == 0xEE, c == 0xEF:
			need = 2
		case c == 0xED:
			need, hi = 2, 0x9F
		case c == 0xF0:
			need, lo = 3, 0x90
		case c >= 0xF1 && c <= 0xF3:
			need = 3
		case c == 0xF4:
			need, hi = 3, 0x8F
		default:
			b = utf8.AppendRune(b, utf8.RuneError)
			i++
			continue
		}
		j := i + 1
		ok := true
		for k := 0; k < need; k++ {
			if j >= len(p) {
				ok = false
				break
			}
			cl, ch := byte(0x80), byte(0xBF)
			if k == 0 {
				cl, ch = lo, hi
			}
			if p[j] < cl || p[j] > ch {
				ok = false
				break
			}
			j++
		}
		if ok {
			b = append(b, p[i:j]...)
		} else {
			b = utf8.AppendRune(b, utf8.RuneError)
		}
		i = j
	}
	return string(b)
}
