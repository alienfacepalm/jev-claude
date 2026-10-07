package jsjson

import (
	"math"
	"testing"
)

func mustParse(t *testing.T, s string) any {
	t.Helper()
	v, err := Parse(s)
	if err != nil {
		t.Fatalf("Parse(%q): %v", s, err)
	}
	return v
}

func TestRoundTripKeepsJavaScriptKeyOrder(t *testing.T) {
	cases := map[string]string{
		`{"b":1,"2":2,"a":3,"1":4,"01":5}`:                        `{"1":4,"2":2,"b":1,"a":3,"01":5}`,
		`{"x":1,"4294967295":2,"4294967294":3,"0":4}`:             `{"0":4,"4294967294":3,"x":1,"4294967295":2}`,
		`{"a":1,"b":2,"a":3}`:                                     `{"a":3,"b":2}`,
		`[1.0,-0,12345678901234567890,1e21,1e-7,0.000001,5e-324]`: `[1,0,12345678901234567000,1e+21,1e-7,0.000001,5e-324]`,
		`[123456789012345680000,1.7976931348623157e308,1e400]`:    `[123456789012345680000,1.7976931348623157e+308,null]`,
		`"\ud800 x \udfff 😀"`:                                     `"\ud800 x \udfff 😀"`,
		`"a/b<>& \u0001\u001f\b\f\n\r\t\"\\"`:                     "\"a/b<>& \\u0001\\u001f\\b\\f\\n\\r\\t\\\"\\\\\"",
		` { "nested" : [ { } , [ ] , null , true , false ] } `:    `{"nested":[{},[],null,true,false]}`,
	}
	for in, want := range cases {
		if got := Stringify(mustParse(t, in)); got != want {
			t.Errorf("round trip %s\n got  %s\n want %s", in, got, want)
		}
	}
}

func TestParseRejectsWhatJSONParseRejects(t *testing.T) {
	for _, in := range []string{"NaN", "Infinity", "-Infinity", "01", "1.", ".5", "+1", "[1,]", `{"a":1,}`, `"\x"`, "\"a\nb\"", "", "{} x", "\xef\xbb\xbf{}", "'a'", "tru"} {
		if _, err := Parse(in); err == nil {
			t.Errorf("Parse(%q) succeeded", in)
		}
	}
}

func TestParseBytesReplacesInvalidUTF8LikeBufferToString(t *testing.T) {
	// E2 82 is a truncated three-byte sequence: one U+FFFD (maximal subpart), not two.
	v, err := ParseBytes([]byte("\"a\xe2\x82b\xffc\xed\xa0\x80d\""))
	if err != nil {
		t.Fatal(err)
	}
	if want := "a�b�c���d"; v != want {
		t.Fatalf("got %q want %q", v, want)
	}
}

func TestNegativeZeroStringifiesAsZeroAndParsesWithSign(t *testing.T) {
	// The Go constant -0.0 is +0; a negative zero has to be made at run time.
	negZero := math.Copysign(0, -1)
	if got := Stringify(negZero); got != "0" {
		t.Fatalf("Stringify(-0) = %q, want \"0\"", got)
	}
	if got := Stringify(Obj("z", negZero)); got != `{"z":0}` {
		t.Fatalf("Stringify({z: -0}) = %q", got)
	}
	if f := mustParse(t, "-0").(float64); !math.Signbit(f) {
		t.Fatal("-0 parses to negative zero")
	}
}

func TestIndentMatchesStringifyWithTwoSpaces(t *testing.T) {
	v := mustParse(t, `{"model":"x","permissions":{"deny":["Bash(rm*)"]},"e":[],"o":{}}`)
	want := "{\n  \"model\": \"x\",\n  \"permissions\": {\n    \"deny\": [\n      \"Bash(rm*)\"\n    ]\n  },\n  \"e\": [],\n  \"o\": {}\n}"
	if got := Indent(v); got != want {
		t.Fatalf("got\n%s\nwant\n%s", got, want)
	}
}

func TestUndefinedMembersAreOmittedAndElementsNull(t *testing.T) {
	o := Obj("a", 1.0, "b", Undefined, "c", []any{Undefined, 2.0})
	if got := Stringify(o); got != `{"a":1,"c":[null,2]}` {
		t.Fatal(got)
	}
	// A spread that sets a key to undefined keeps its position and hides the old value.
	dst := Obj("x", 1.0, "confidence", 0.9, "y", 2.0)
	Spread(dst, Obj("confidence", Undefined))
	if got := Stringify(dst); got != `{"x":1,"y":2}` {
		t.Fatal(got)
	}
	s := NewObject()
	Spread(s, "a😀")
	if got := Stringify(s); got != `{"0":"a","1":"\ud83d","2":"\ude00"}` {
		t.Fatal(got)
	}
}

func TestNumberToString(t *testing.T) {
	point1 := 0.1
	point3 := point1 + 0.2
	cases := map[float64]string{
		0.1: "0.1", 100: "100", 1e21: "1e+21", 1e20: "100000000000000000000", 1.5e-7: "1.5e-7",
		123e-20: "1.23e-18", -2.5: "-2.5", math.NaN(): "NaN", math.Inf(-1): "-Infinity", 0.000001: "0.000001",
		1.7976931348623157e308: "1.7976931348623157e+308", 5e-324: "5e-324", point3: "0.30000000000000004",
	}
	for in, want := range cases {
		if got := NumberToString(in); got != want {
			t.Errorf("NumberToString(%v) = %s, want %s", in, got, want)
		}
	}
}

func TestMathRoundToFixedAndToNumber(t *testing.T) {
	for in, want := range map[float64]float64{2.5: 3, -2.5: -2, 0.49999999999999994: 0, 1.4: 1, -1.6: -2, 94.0: 94} {
		if got := MathRound(in); got != want {
			t.Errorf("MathRound(%v) = %v want %v", in, got, want)
		}
	}
	if r := MathRound(-0.4); r != 0 || !math.Signbit(r) {
		t.Error("MathRound(-0.4) is -0")
	}
	for in, want := range map[float64]string{0.125: "0.13", 0.375: "0.38", 1.005: "1.00", 0.82: "0.82", 0: "0.00", -0.001: "-0.00", math.NaN(): "NaN", 1e21: "1e+21", 2.345: "2.35"} {
		if got := ToFixed2(in); got != want {
			t.Errorf("ToFixed2(%v) = %s want %s", in, got, want)
		}
	}
	if ToFixed2(math.Copysign(0, -1)) != "0.00" {
		t.Error("-0 prints 0.00")
	}
	num := map[string]float64{
		`"0.9"`: 0.9, `[0.9]`: 0.9, `["0.9"]`: 0.9, `[[0.7]]`: 0.7, `[]`: 0, `[null]`: 0, `null`: 0, `true`: 1, `false`: 0,
		`" 0x1 "`: 1, `""`: 0, `"  "`: 0, `"0b101"`: 5, `"0o17"`: 15, `"1e3"`: 1000, `"-Infinity"`: math.Inf(-1),
		`"+Infinity"`: math.Inf(1), `".5"`: 0.5, `"5."`: 5, "\"\xc2\xa01\xef\xbb\xbf\"": 1,
	}
	for in, want := range num {
		if got := ToNumber(mustParse(t, in)); got != want {
			t.Errorf("ToNumber(%s) = %v want %v", in, got, want)
		}
	}
	for _, in := range []string{`"-0x1"`, `"abc"`, `[1,2]`, `{}`, `"0x"`, `"1_000"`, `"infinity"`, `"1e"`, `"."`, `"0xg"`} {
		if got := ToNumber(mustParse(t, in)); !math.IsNaN(got) {
			t.Errorf("ToNumber(%s) = %v want NaN", in, got)
		}
	}
	if !math.IsNaN(ToNumber(Undefined)) {
		t.Error("undefined is NaN")
	}
}

func TestTruthyAndJSString(t *testing.T) {
	for _, v := range []any{nil, Undefined, false, 0.0, math.NaN(), ""} {
		if Truthy(v) {
			t.Errorf("%v is falsy", v)
		}
	}
	for _, v := range []any{true, 1.0, "0", []any{}, NewObject()} {
		if !Truthy(v) {
			t.Errorf("%v is truthy", v)
		}
	}
	if JSString(Undefined) != "undefined" || JSString([]any{1.0, nil, []any{2.0, "x"}}) != "1,,2,x" || JSString(NewObject()) != "[object Object]" {
		t.Error("JSString")
	}
}
