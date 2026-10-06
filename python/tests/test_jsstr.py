"""JavaScript string and number semantics. Expected values were produced by Node.js 24."""

import math
import re
import unittest

from jev_router import jsstr
from jev_router.jsstr import UNDEFINED


class NumberToString(unittest.TestCase):
    def test_matches_number_prototype_to_string(self):
        cases = [
            (0, "0"), (-0.0, "0"), (1, "1"), (-1, "-1"), (0.1, "0.1"), (1e21, "1e+21"),
            (1e-7, "1e-7"), (1e-6, "0.000001"), (123456789012345680000, "123456789012345680000"),
            (5e-324, "5e-324"), (1.7976931348623157e308, "1.7976931348623157e+308"),
            (1.5, "1.5"), (-2.5, "-2.5"), (100, "100"), (1 / 3, "0.3333333333333333"),
            (2**53, "9007199254740992"), (1e20, "100000000000000000000"), (123e-20, "1.23e-18"),
            (0.1 + 0.2, "0.30000000000000004"), (4.35, "4.35"), (1e300, "1e+300"),
            (1.5e-323, "1.5e-323"), (999999999999999999999, "1e+21"), (1.2e-6, "0.0000012"),
            (12345678901234567890, "12345678901234567000"), (1760000000000, "1760000000000"),
            (math.nan, "NaN"), (math.inf, "Infinity"), (-math.inf, "-Infinity"),
        ]
        for value, expected in cases:
            self.assertEqual(jsstr.number_to_string(value), expected, repr(value))

    def test_to_string_of_json_values(self):
        self.assertEqual(jsstr.to_string(None), "null")
        self.assertEqual(jsstr.to_string(UNDEFINED), "undefined")
        self.assertEqual(jsstr.to_string(True), "true")
        self.assertEqual(jsstr.to_string(8.0), "8")
        self.assertEqual(jsstr.to_string([1.0, None, "a", [2.0, UNDEFINED]]), "1,,a,2,")
        self.assertEqual(jsstr.to_string({"a": 1}), "[object Object]")


class FixedAndRound(unittest.TestCase):
    def test_to_fixed_2_ties_and_exact_values(self):
        cases = [
            (0.125, "0.13"), (0.375, "0.38"), (1.005, "1.00"), (-0.005, "-0.01"), (2.675, "2.67"),
            (1.45, "1.45"), (0.5, "0.50"), (-1.5, "-1.50"), (123.456, "123.46"), (0.045, "0.04"),
            (10.235, "10.23"), (0, "0.00"), (1e20, "100000000000000000000.00"), (-0.001, "-0.00"),
            (0.994999, "0.99"), (0.995, "0.99"), (99.995, "100.00"), (0.615, "0.61"),
            (-0.0, "0.00"), (math.nan, "NaN"), (1e21, "1e+21"),
        ]
        for value, expected in cases:
            self.assertEqual(jsstr.to_fixed2(value), expected, repr(value))

    def test_math_round(self):
        cases = [
            (2.5, 3, False), (-2.5, -2, False), (0.49999999999999994, 0, False), (-0.5, 0, True),
            (-0.4, 0, True), (0.5, 1, False), (1.5, 2, False), (-1.5, -1, False), (3.7, 4, False),
            (-3.7, -4, False), (4503599627370495.5, 4503599627370496, False), (-0.6, -1, False),
            (-0.0, 0, True),
        ]
        for value, expected, negative_zero in cases:
            result = jsstr.math_round(value)
            self.assertEqual(result, expected, repr(value))
            self.assertEqual(result == 0 and math.copysign(1, result) < 0, negative_zero, repr(value))
        self.assertTrue(math.isnan(jsstr.math_round(math.nan)))
        self.assertEqual(jsstr.math_round(math.inf), math.inf)


class ToNumber(unittest.TestCase):
    def test_strings(self):
        cases = [
            ("0.9", 0.9), (" 0x1 ", 1), ("", 0), ("abc", None), (" ", 0), ("-0x1", None),
            ("0b101", 5), ("0o17", 15), ("1e3", 1000), (".5", 0.5), ("5.", 5),
            ("+Infinity", math.inf), ("-Infinity", -math.inf), ("Infinity", math.inf),
            ("infinity", None), ("1_000", None), (" 12﻿", 12), ("0x", None),
            ("+0x1", None), ("1e", None), ("\u001f5", None), ("nan", None), ("inf", None),
        ]
        for text, expected in cases:
            result = jsstr.to_number(text)
            if expected is None:
                self.assertTrue(math.isnan(result), repr(text))
            else:
                self.assertEqual(result, expected, repr(text))

    def test_other_values(self):
        self.assertEqual(jsstr.to_number(None), 0)
        self.assertEqual(jsstr.to_number(True), 1)
        self.assertEqual(jsstr.to_number(False), 0)
        self.assertTrue(math.isnan(jsstr.to_number(UNDEFINED)))
        self.assertTrue(math.isnan(jsstr.to_number({})))
        arrays = [([], 0), ([None], 0), ([0.9], 0.9), (["0.9"], 0.9), ([[0.7]], 0.7), ([1, 2], None),
                  ([True], None), ([UNDEFINED], 0), ([[]], 0), ([[None]], 0)]
        for value, expected in arrays:
            result = jsstr.to_number(value)
            if expected is None:
                self.assertTrue(math.isnan(result), repr(value))
            else:
                self.assertEqual(result, expected, repr(value))

    def test_parse_int(self):
        cases = [("5", 5), (" 7x", 7), ("-3", -3), ("x", None), ("", None), ("+4", 4), ("0x10", 0)]
        for text, expected in cases:
            result = jsstr.parse_int(text)
            if expected is None:
                self.assertTrue(math.isnan(result))
            else:
                self.assertEqual(result, expected)


class Truthiness(unittest.TestCase):
    def test_javascript_truthiness(self):
        for value in [None, UNDEFINED, False, 0, 0.0, -0.0, math.nan, ""]:
            self.assertFalse(jsstr.truthy(value), repr(value))
        for value in [True, 1, -1.5, "0", " ", [], {}, math.inf]:
            self.assertTrue(jsstr.truthy(value), repr(value))

    def test_coalesce_only_skips_null_and_undefined(self):
        self.assertEqual(jsstr.coalesce(None, UNDEFINED, 0, 5), 0)
        self.assertEqual(jsstr.coalesce("", "x"), "")
        self.assertIs(jsstr.coalesce(None, UNDEFINED), UNDEFINED)


class Whitespace(unittest.TestCase):
    def test_trim_strips_exactly_the_javascript_set(self):
        for ch in "\t\n\x0b\x0c\r          　﻿":
            self.assertEqual(jsstr.js_trim(ch + "x" + ch), "x", hex(ord(ch)))
        # Python's str.strip removes these too; JavaScript's trim does not.
        for ch in "\x1c\x1d\x1e\x1f\x85​":
            self.assertEqual(jsstr.js_trim(ch + "x" + ch), ch + "x" + ch, hex(ord(ch)))

    def test_jsws_class_is_the_javascript_backslash_s(self):
        pattern = re.compile(jsstr.JSWS + "+", re.ASCII)
        self.assertEqual(pattern.sub(" ", "a　﻿ b\u001fc\u0085d"), "a b\u001fc\u0085d")


class Utf16(unittest.TestCase):
    def test_length_counts_code_units(self):
        self.assertEqual(jsstr.u16_len("abc"), 3)
        self.assertEqual(jsstr.u16_len("\U0001f600"), 2)
        self.assertEqual(jsstr.u16_len("\ud800"), 1)
        self.assertEqual(jsstr.code_points("\U0001f600x"), 2)

    def test_slice_and_pad(self):
        self.assertEqual(jsstr.u16_slice("\U0001f600abc", 0, 3), "\U0001f600a")
        self.assertEqual(jsstr.u16_slice("a\U0001f600", 0, 2), "a", "a split pair drops the high surrogate")
        self.assertEqual(jsstr.u16_slice("a\ud800b", 0, 2), "a\ud800", "a real lone surrogate stays")
        self.assertEqual(jsstr.u16_pad_end("\U0001f600", 4), "\U0001f600  ")

    def test_utf8_replaces_lone_surrogates(self):
        self.assertEqual(jsstr.utf8("a\ud800b"), b"a\xef\xbf\xbdb")
        self.assertEqual(jsstr.utf8("😀"), "\U0001f600".encode("utf-8"), "a pair is a pair")


if __name__ == "__main__":
    unittest.main()
