// Cases for the JavaScript-compatibility helpers every port writes once (SPEC 3.3, 3.6, 3.7):
// JSON.stringify, JSON.parse over raw bytes, Math.round, toFixed(2), and ToNumber.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { attempt, THROWS } from "./tagged.mjs";

export default function jsonCases({ root }) {
  const fixture = JSON.parse(readFileSync(join(root, "conformance", "fixtures", "claude-code-print-request.json"), "utf8"));

  const controls = Array.from({ length: 0x20 }, (_, i) => String.fromCharCode(i)).join("");
  const values = [
    ["zero", 0], ["negative zero", -0], ["one", 1], ["one point five", 1.5], ["minus one", -1],
    ["0.1 + 0.2", 0.1 + 0.2], ["1e21", 1e21], ["1e20", 1e20], ["just under 1e21", 999999999999999900000],
    ["123456789012345680000", 123456789012345680000], ["2^53", 2 ** 53], ["2^53 + 2", 2 ** 53 + 2],
    ["0.000001", 0.000001], ["1e-7", 1e-7], ["1.5e-7", 1.5e-7], ["1.25e-6", 0.00000125], ["5e-324", 5e-324],
    ["max value", Number.MAX_VALUE], ["-1e21", -1e21], ["-1.5e-7", -1.5e-7], ["100", 100], ["1e300", 1e300],
    ["one third", 1 / 3], ["two thirds", 2 / 3], ["metric 5/9", 5 / 9], ["context size", 2765 / 200000],
    ["NaN", NaN], ["Infinity", Infinity], ["-Infinity", -Infinity],
    ["empty string", ""], ["control characters", controls], ["delete and C1", "\u007f\u0080\u0085\u009f"],
    ["line and paragraph separators", "  "], ["html and slash stay literal", "</script> & <b> / \\"],
    ["quotes and backslash", 'say "hi" \\ there'], ["non-ascii", "naïve café 日本"], ["emoji", "🚀"],
    ["lone high surrogate", "a\ud800b"], ["lone low surrogate", "a\udc00b"], ["reversed pair", "\ude00\ud83d"],
    ["high surrogate at end", "end\udbff"], ["bom", "﻿"],
    ["null", null], ["true", true], ["false", false], ["undefined", undefined],
    ["empty array", []], ["empty object", {}], ["nested", { a: [1, { b: [] }, {}], c: { d: null } }],
    ["undefined member omitted", { a: 1, b: undefined, c: 3 }], ["undefined element is null", [1, undefined, NaN, -0]],
    ["integer-like keys", { b: 1, 2: 2, a: 3, 1: 4, "01": 5, 0: 6, 4294967294: 7, 4294967295: 8, "-1": 9, "1.5": 10 }],
    ["only undefined member", { x: undefined }],
    ["deep array", [[[[["deep"]]]]]],
    ["captured messages", fixture.body.messages],
    ["captured body", fixture.body],
  ];
  const stringify = [];
  for (const [name, value] of values) {
    const compact = JSON.stringify(value);
    stringify.push({ name, input: { value }, expected: { text: compact, utf16Length: compact?.length } });
  }
  for (const [name, value] of [
    ["empty containers", { a: [], b: {}, c: [{}], d: [[]] }],
    ["nested", { model: "claude-opus-5-5", list: [1, "two", null, true], obj: { k: { j: "v" } } }],
    ["undefined members", { a: undefined, b: [undefined], c: 1 }],
    ["integer-like keys", { z: 1, 3: 2, 1: 3 }],
    ["settings file", { model: "claude-opus-4-6", permissions: { deny: ["Bash(rm*)"] } }],
    ["scalar", 42],
    ["string with escapes", "a\n\"b\"\ud800"],
    ["empty array", []],
  ]) {
    const text = JSON.stringify(value, null, 2);
    stringify.push({ name: `indented ${name}`, input: { value, indent: 2 }, expected: { text, utf16Length: text.length } });
  }

  // Input bytes, decoded as Buffer.toString() does (invalid UTF-8 -> U+FFFD), then JSON.parse.
  const raw = [
    ["object", '{"a":1,"b":[true,false,null],"c":"x"}'],
    ["whitespace everywhere", ' \t\n\r{ "a" : [ 1 , 2 ] } \n'],
    ["duplicate keys", '{"a":1,"b":2,"a":3}'],
    ["integer-like keys reorder", '{"b":1,"2":2,"a":3,"1":4,"01":5}'],
    ["array index bounds", '{"x":0,"4294967295":1,"4294967294":2,"0":3,"-0":4,"00":5}'],
    ["duplicate integer-like key", '{"5":1,"a":2,"5":3}'],
    ["big integer", "12345678901234567890"],
    ["one point zero", "1.0"],
    ["negative zero", "-0"],
    ["negative zero float", "-0.0"],
    ["exponents", "[1e2,1E-2,1.5e+3,-2.5e-7,1e21,1e-7]"],
    ["overflow to Infinity", "1e400"],
    ["underflow to zero", "1e-400"],
    ["max safe and beyond", "[9007199254740991,9007199254740993]"],
    ["escapes", '"\\" \\\\ \\/ \\b \\f \\n \\r \\t \\u0041 \\u00e9 \\u2028"'],
    ["lone high surrogate escape", '"x\\ud800y"'],
    ["lone low surrogate escape", '"\\udc00"'],
    ["escaped pair", '"\\ud83d\\ude80"'],
    ["uppercase hex escape", '"\\uD83D\\uDE80"'],
    ["raw emoji", '"🚀"'],
    ["NaN literal fails", "NaN"],
    ["Infinity literal fails", "Infinity"],
    ["-Infinity literal fails", "-Infinity"],
    ["NaN in array fails", "[NaN]"],
    ["trailing comma fails", "[1,2,]"],
    ["single quotes fail", "{'a':1}"],
    ["leading zero fails", "01"],
    ["plus sign fails", "+1"],
    ["empty input fails", ""],
    ["unescaped control character fails", '"a\u0001b"'],
    ["raw tab in string fails", '"a\tb"'],
    ["raw line separator allowed", '"a b"'],
    ["top-level string", '"text"'],
    ["top-level null", "null"],
    ["top-level number with spaces", "  42  "],
    ["nested", '{"a":{"b":{"c":[[],{}]}}}'],
  ];
  const bytes = raw.map(([name, text]) => [name, Buffer.from(text, "utf8")]);
  // Raw invalid UTF-8, which no JS string literal can spell.
  bytes.push(
    ["invalid byte in string", Buffer.from([0x22, 0x61, 0xff, 0x62, 0x22])],
    ["truncated sequence", Buffer.from([0x22, 0xe2, 0x82, 0x22])],
    ["overlong encoding", Buffer.from([0x22, 0xc0, 0xaf, 0x22])],
    ["utf-8 encoded surrogate", Buffer.from([0x22, 0xed, 0xa0, 0x80, 0x22])],
    ["continuation bytes alone", Buffer.from([0x22, 0x80, 0x80, 0xbf, 0x22])],
    ["four-byte beyond U+10FFFF", Buffer.from([0x22, 0xf4, 0x90, 0x80, 0x80, 0x22])],
    ["invalid byte in key", Buffer.from([0x7b, 0x22, 0xfe, 0x22, 0x3a, 0x31, 0x7d])],
    ["utf-8 bom fails", Buffer.from([0xef, 0xbb, 0xbf, 0x7b, 0x7d])],
    ["invalid byte outside string fails", Buffer.from([0x5b, 0x31, 0xff, 0x5d])],
    ["nul byte in string fails", Buffer.from([0x22, 0x00, 0x22])],
  );
  bytes.push(["captured request file", readFileSync(join(root, "conformance", "fixtures", "claude-code-print-request.json"))]);
  const parse = bytes.map(([name, buffer]) => {
    const value = attempt(() => JSON.parse(buffer.toString()));
    return {
      name,
      input: { bytes: buffer },
      expected: value === THROWS ? THROWS : { value, text: JSON.stringify(value) },
    };
  });

  const rounds = [2.5, -2.5, 0.49999999999999994, -0.5, -0.4, 0.5, 1.5, -1.5, -0.6, 12.5, 8, 99.99, 0, -0,
    4503599627370495.5, 4503599627370497, 2 ** 53, -(2 ** 53), 1e21, NaN, Infinity, -Infinity, 0.2 * 3, 1 / 3, 94.49999999999999];
  const fixed = [0.125, 0.375, 1.005, 0, -0, -0.125, 1, 0.5, 0.005, 0.015, 2.675, 123.456, -1.5, 1 / 3, 2 / 3, 0.92,
    0.995, 9.995, 0.5555555555555556, 1e20, 1e21, 1.5e21, NaN, Infinity, -Infinity, 1e-7, 0.045, 1.45, 8.345, 99.995];
  const toNumber = [0.9, null, true, false, "", " ", "0.9", " 0x1 ", "0x1", "0X1F", "0o7", "0O7", "0b1", "0B11", "-0x1", "+0x1",
    "Infinity", "+Infinity", "-Infinity", "infinity", "1e3", ".5", "5.", "+.5", "-.5e1", "1_000", "١", " 1 ",
    "﻿2　", " 3 ", "\u00851", "\u001f1", undefined, {}, [], [null], [undefined], [0.9], ["0.9"], [[0.7]],
    [1, 2], "0x", "0xg", "1e", "-", "+", ".", "00012", "-0", "  12  ", "12px", [" 3 "], [[]], [true], [null, null], "0.6",
    "6e-1", "1e1000", "-1e1000", NaN, -0, "0b2", "0o8", "0x1.5", "1,5", "\t\n\v\f\r 7 "];
  const percents = [0.92, 0.945, 0.005, 0.015, 0.125, 1, 0, -0.005, 0.285, 0.575, 0.94, 0.97, 0.5, 0.995];
  const label = (v) => (v === undefined ? "undefined" : Object.is(v, -0) ? "-0" : typeof v === "number" ? String(v) : JSON.stringify(v));
  const math = [
    ...rounds.map((value, i) => ({ name: `MathRound #${i} ${label(value)}`, input: { op: "MathRound", value }, expected: Math.round(value) })),
    ...fixed.map((value, i) => ({ name: `toFixed2 #${i} ${label(value)}`, input: { op: "toFixed2", value }, expected: value.toFixed(2) })),
    ...toNumber.map((value, i) => ({ name: `ToNumber #${i} ${label(value)}`, input: { op: "ToNumber", value }, expected: Number(value) })),
    ...percents.map((value, i) => ({ name: `roundPercent #${i} ${label(value)}`, input: { op: "roundPercent", value }, expected: Math.round(value * 100) })),
    ...toNumber.map((value, i) => ({ name: `toFixed2 of ToNumber #${i} ${label(value)}`, input: { op: "toFixed2OfToNumber", value }, expected: Number(value).toFixed(2) })),
  ];

  return { stringify, parse, math };
}
