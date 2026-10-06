package jsjson

import (
	"math"
	"math/big"
	"strconv"
	"strings"

	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
)

// NumberToString is Number.prototype.toString(): shortest round-trip digits, fixed notation
// for decimal exponents -7 < n < 21, exponent notation with an explicit sign otherwise.
func NumberToString(f float64) string {
	switch {
	case math.IsNaN(f):
		return "NaN"
	case math.IsInf(f, 1):
		return "Infinity"
	case math.IsInf(f, -1):
		return "-Infinity"
	case f == 0:
		return "0"
	case f < 0:
		return "-" + NumberToString(-f)
	}
	e := strconv.FormatFloat(f, 'e', -1, 64) // d.ddde±XX
	mant, exp, _ := strings.Cut(e, "e")
	digits := strings.Replace(mant, ".", "", 1)
	x, _ := strconv.Atoi(exp)
	k := len(digits)
	n := x + 1
	switch {
	case k <= n && n <= 21:
		return digits + strings.Repeat("0", n-k)
	case 0 < n && n <= 21:
		return digits[:n] + "." + digits[n:]
	case -6 < n && n <= 0:
		return "0." + strings.Repeat("0", -n) + digits
	}
	sign := "+"
	if n-1 < 0 {
		sign = "-"
	}
	ex := strconv.Itoa(abs(n - 1))
	if k == 1 {
		return digits + "e" + sign + ex
	}
	return digits[:1] + "." + digits[1:] + "e" + sign + ex
}

func abs(n int) int {
	if n < 0 {
		return -n
	}
	return n
}

// JSString is String(v), the conversion a template literal applies.
func JSString(v any) string {
	switch x := v.(type) {
	case nil:
		return "null"
	case undefined:
		return "undefined"
	case bool:
		if x {
			return "true"
		}
		return "false"
	case float64:
		return NumberToString(x)
	case int:
		return strconv.Itoa(x)
	case string:
		return x
	case []any:
		parts := make([]string, len(x))
		for i, e := range x {
			if !IsNullish(e) {
				parts[i] = JSString(e)
			}
		}
		return strings.Join(parts, ",")
	}
	return "[object Object]"
}

// ToNumber is ECMAScript ToNumber over a JSON value (SPEC 3.6).
func ToNumber(v any) float64 {
	switch x := v.(type) {
	case float64:
		return x
	case int:
		return float64(x)
	case nil:
		return 0
	case bool:
		if x {
			return 1
		}
		return 0
	case string:
		return StringToNumber(x)
	case []any:
		return StringToNumber(JSString(x))
	}
	return math.NaN()
}

func allDigits(s string, ok func(byte) bool) bool {
	if s == "" {
		return false
	}
	for i := 0; i < len(s); i++ {
		if !ok(s[i]) {
			return false
		}
	}
	return true
}

func isDec(c byte) bool { return c >= '0' && c <= '9' }

// StringToNumber is ECMAScript StringToNumber.
func StringToNumber(s string) float64 {
	s = jsstr.Trim(s)
	if s == "" {
		return 0
	}
	switch s {
	case "Infinity", "+Infinity":
		return math.Inf(1)
	case "-Infinity":
		return math.Inf(-1)
	}
	if len(s) > 2 && s[0] == '0' {
		base := 0
		var ok func(byte) bool
		switch s[1] {
		case 'x', 'X':
			base, ok = 16, func(c byte) bool { return isDec(c) || (c|0x20 >= 'a' && c|0x20 <= 'f') }
		case 'o', 'O':
			base, ok = 8, func(c byte) bool { return c >= '0' && c <= '7' }
		case 'b', 'B':
			base, ok = 2, func(c byte) bool { return c == '0' || c == '1' }
		}
		if base != 0 {
			if !allDigits(s[2:], ok) {
				return math.NaN()
			}
			n, _ := new(big.Int).SetString(s[2:], base)
			f, _ := new(big.Float).SetInt(n).Float64()
			return f
		}
	}
	// StrDecimalLiteral: [+-] (digits [. digits?] | . digits) [(e|E) [+-] digits]
	i := 0
	if s[i] == '+' || s[i] == '-' {
		i++
	}
	intEnd := digits(s, i)
	fracEnd := intEnd
	hasDigits := intEnd > i
	if fracEnd < len(s) && s[fracEnd] == '.' {
		fracEnd = digits(s, fracEnd+1)
		hasDigits = hasDigits || fracEnd > intEnd+1
	}
	if !hasDigits {
		return math.NaN()
	}
	end := fracEnd
	if end < len(s) && (s[end] == 'e' || s[end] == 'E') {
		j := end + 1
		if j < len(s) && (s[j] == '+' || s[j] == '-') {
			j++
		}
		k := digits(s, j)
		if k == j {
			return math.NaN()
		}
		end = k
	}
	if end != len(s) {
		return math.NaN()
	}
	lit := s
	// strconv does not take a bare trailing "." before an exponent or at the end ("1." / "1.e5").
	lit = strings.Replace(lit, ".e", "e", 1)
	lit = strings.Replace(lit, ".E", "E", 1)
	lit = strings.TrimSuffix(lit, ".")
	f, err := strconv.ParseFloat(lit, 64)
	if err != nil && !math.IsInf(f, 0) && f != 0 {
		return math.NaN()
	}
	return f
}

// MathRound is Math.round (SPEC 3.7).
func MathRound(x float64) float64 {
	if math.IsNaN(x) || math.IsInf(x, 0) || x == 0 {
		return x
	}
	r := math.Floor(x)
	if x-r >= 0.5 {
		r++
	}
	if r == 0 && x < 0 {
		return math.Copysign(0, -1)
	}
	return r
}

// ToFixed2 is x.toFixed(2) (SPEC 3.7): exact round-half-up on the double's real value.
func ToFixed2(x float64) string {
	if math.IsNaN(x) {
		return "NaN"
	}
	sign := ""
	if x < 0 {
		sign, x = "-", -x
	}
	if x >= 1e21 {
		return sign + NumberToString(x)
	}
	v := new(big.Float).SetPrec(256).SetFloat64(x)
	v.Mul(v, big.NewFloat(100).SetPrec(256))
	v.Add(v, big.NewFloat(0.5).SetPrec(256))
	n, _ := v.Int(nil) // truncation toward zero == floor for non-negative values
	s := n.String()
	for len(s) < 3 {
		s = "0" + s
	}
	return sign + s[:len(s)-2] + "." + s[len(s)-2:]
}
