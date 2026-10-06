package jsstr

import (
	"regexp"
	"testing"
)

// Built from bytes so the source holds no invisible characters.
var (
	nbsp     = "\xc2\xa0"
	ideo     = "\xe3\x80\x80"
	bom      = "\xef\xbb\xbf"
	nel      = "\xc2\x85" // U+0085: not JavaScript whitespace
	sep      = "\xe2\x80\xa8"
	loneHigh = "\xed\xa0\xbd" // U+D83D as WTF-8
	emoji    = "\xf0\x9f\x98\x80"
)

func TestTrimStripsExactlyJavaScriptWhitespace(t *testing.T) {
	cases := map[string]string{
		" \t\n\v\f\r" + nbsp + ideo + bom + sep + "x y" + nbsp: "x y",
		nel + "x" + nel: nel + "x" + nel,
		"\x1cx\x1f":     "\x1cx\x1f",
		"":              "",
		"   ":           "",
		loneHigh + " ":  loneHigh,
	}
	for in, want := range cases {
		if got := Trim(in); got != want {
			t.Errorf("Trim(%q) = %q want %q", in, got, want)
		}
	}
}

func TestLengthsAndSlicesCountUTF16Units(t *testing.T) {
	s := "a" + emoji + "b" + loneHigh
	if Len16(s) != 5 {
		t.Fatalf("Len16 = %d", Len16(s))
	}
	if got := Slice16(s, 0, 3); got != "a"+emoji {
		t.Errorf("Slice16 0..3 = %q", got)
	}
	if got := Slice16(s, 0, 2); got != "a" {
		t.Errorf("a cut inside a pair drops the half: %q", got)
	}
	if got := Slice16(s, 3, 5); got != "b"+loneHigh {
		t.Errorf("lone surrogate survives slicing: %q", got)
	}
	if PadEnd16(emoji, 4) != emoji+"  " || CodePoints(emoji+"x") != 2 {
		t.Error("padEnd / code points")
	}
}

func TestCaseHelpers(t *testing.T) {
	// The Kelvin sign and the long s keep their bytes: only ASCII folds.
	if got := ASCIILower("USE Opus \xe2\x84\xaa\xc5\xbf"); got != "use opus \xe2\x84\xaa\xc5\xbf" {
		t.Errorf("ASCIILower = %q", got)
	}
	if got := ToUpper("stra\xc3\x9fe claude-opus"); got != "STRASSE CLAUDE-OPUS" {
		t.Errorf("ToUpper = %q", got)
	}
}

func TestToUTF8ReplacesLoneSurrogates(t *testing.T) {
	if got := ToUTF8("a" + loneHigh + emoji); got != "a\xef\xbf\xbd"+emoji {
		t.Errorf("ToUTF8 = %q", got)
	}
}

func TestDecodeBytesFollowsTheWHATWGDecoder(t *testing.T) {
	cases := map[string]string{
		"ok " + emoji:          "ok " + emoji,
		"\xe2\x82":             "\xef\xbf\xbd",
		"\xe2\x82x":            "\xef\xbf\xbdx",
		"\xed\xa0\x80":         "\xef\xbf\xbd\xef\xbf\xbd\xef\xbf\xbd",
		"\xf0\x9f\x98":         "\xef\xbf\xbd",
		"\xc0\xaf":             "\xef\xbf\xbd\xef\xbf\xbd",
		"\xf4\x90\x80\x80":     "\xef\xbf\xbd\xef\xbf\xbd\xef\xbf\xbd\xef\xbf\xbd",
		"a\xffb":               "a\xef\xbf\xbdb",
		"\xe0\x80\x80":         "\xef\xbf\xbd\xef\xbf\xbd\xef\xbf\xbd",
		"\xf1\x80\x80\xe1\x80": "\xef\xbf\xbd\xef\xbf\xbd",
	}
	for in, want := range cases {
		if got := DecodeBytes([]byte(in)); got != want {
			t.Errorf("DecodeBytes(%q) = %q want %q", in, got, want)
		}
	}
}

func TestJSWSClassMatchesTheSameSetAsIsJSWS(t *testing.T) {
	re := regexp.MustCompile("^" + JSWSClass + "$")
	for r := rune(0); r < 0x3100; r++ {
		if re.MatchString(string(r)) != IsJSWS(r) {
			t.Errorf("U+%04X disagrees", r)
		}
	}
	if IsJSWS(0xFEFF) != re.MatchString(bom) || !IsJSWS(0xFEFF) {
		t.Error("BOM is JSWS")
	}
}
