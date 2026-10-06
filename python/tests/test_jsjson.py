"""JSON with JavaScript semantics. Expected bytes were produced by Node.js 24."""

import math
import unittest

from jev_router import jsjson
from jev_router.jsstr import UNDEFINED


class Parse(unittest.TestCase):
    def test_numbers_are_doubles(self):
        value = jsjson.parse('[12345678901234567890, 1.0, -0, 7]')
        self.assertEqual(jsjson.stringify(value), "[12345678901234567000,1,0,7]")
        self.assertTrue(all(isinstance(v, float) for v in value))
        self.assertEqual(math.copysign(1, value[2]), -1)

    def test_non_json_literals_fail(self):
        for text in ["NaN", "Infinity", "-Infinity", "[NaN]", '{"a": Infinity}', "", "01", "{'a':1}", "﻿{}", "[1,]"]:
            with self.assertRaises(jsjson.ParseError, msg=text):
                jsjson.parse(text)

    def test_key_order_is_javascript_object_order(self):
        value = jsjson.parse('{"b":1,"2":2,"a":3,"1":4,"01":5}')
        self.assertEqual(jsjson.stringify(value), '{"1":4,"2":2,"b":1,"a":3,"01":5}')
        mixed = jsjson.parse('{"b":1,"4294967295":7,"0":0,"4294967294":6,"a":3}')
        self.assertEqual(jsjson.js_keys(mixed), ["0", "4294967294", "b", "4294967295", "a"])

    def test_repeated_key_keeps_first_position_and_last_value(self):
        self.assertEqual(jsjson.stringify(jsjson.parse('{"a":1,"b":2,"a":3}')), '{"a":3,"b":2}')

    def test_invalid_utf8_becomes_replacement_characters(self):
        cases = [
            ("7b2261223a2280227d", "7b2261223a22efbfbd227d"),
            ("7b2261223a22eda080227d", "7b2261223a22efbfbdefbfbdefbfbd227d"),
            ("5b22c328225d", "5b22efbfbd28225d"),
            ("22e28222", "22efbfbd22"),
            ("22f09f9822", "22efbfbd22"),
            ("22fffe22", "22efbfbdefbfbd22"),
        ]
        for given, expected in cases:
            self.assertEqual(jsjson.dumps_bytes(jsjson.parse(bytes.fromhex(given))).hex(), expected, given)

    def test_utf16_bom_bytes_are_not_sniffed(self):
        with self.assertRaises(jsjson.ParseError):
            jsjson.parse(b"\xff\xfe{\x00}\x00")

    def test_lone_surrogates_survive_a_round_trip(self):
        value = jsjson.parse('["\\ud800", "x\\udc00y", "\\ud83d\\ude00"]')
        self.assertEqual(value[0], "\ud800")
        self.assertEqual(jsjson.stringify(value), '["\\ud800","x\\udc00y","\U0001f600"]')


class Stringify(unittest.TestCase):
    def test_escapes_match_json_stringify(self):
        cases = [
            ('a"b\\c', '"a\\"b\\\\c"'),
            ("\b\f\n\r\t", '"\\b\\f\\n\\r\\t"'),
            ("\u0000\u001f\u007f", '"\\u0000\\u001f\u007f"'),
            ("\ud800", '"\\ud800"'),
            ("/<>&  ", '"/<>&  "'),
            ("é", '"é"'),
        ]
        for value, expected in cases:
            self.assertEqual(jsjson.stringify(value), expected, repr(value))

    def test_adjacent_surrogates_from_concatenation_form_a_pair(self):
        self.assertEqual(jsjson.dumps_bytes("\ud83d" + "\ude00"), b'"\xf0\x9f\x98\x80"')

    def test_numbers_and_non_finite_values(self):
        value = [1e21, 1e-7, 5e-324, -0.0, math.nan, math.inf, -math.inf, 1760000000000, True, None]
        self.assertEqual(jsjson.stringify(value), "[1e+21,1e-7,5e-324,0,null,null,null,1760000000000,true,null]")

    def test_undefined_is_omitted_from_objects_and_null_in_arrays(self):
        self.assertEqual(jsjson.stringify({"a": UNDEFINED, "b": [UNDEFINED, 1]}), '{"b":[null,1]}')
        self.assertIs(jsjson.stringify(UNDEFINED), UNDEFINED)

    def test_indented_form(self):
        value = {"a": [], "b": {}, "c": [1, {"d": None}], "e": "x"}
        self.assertEqual(
            jsjson.stringify(value, 2),
            '{\n  "a": [],\n  "b": {},\n  "c": [\n    1,\n    {\n      "d": null\n    }\n  ],\n  "e": "x"\n}',
        )

    def test_utf16_length_of_messages_for_context_tokens(self):
        from jev_router import jsstr

        text = jsjson.stringify([{"role": "user", "content": "hé\U0001f600\n"}])
        self.assertEqual(jsstr.u16_len(text), 36)


if __name__ == "__main__":
    unittest.main()
