"""JavaScript string, number and regular-expression semantics (SPEC 3.1, 3.2, 3.5, 3.6, 3.7).

Everything that must behave exactly as the Node.js implementation does when it measures,
trims, converts or formats a value lives here, so it exists once and is tested directly.
"""

from __future__ import annotations

import math
import re
from decimal import ROUND_HALF_UP, Decimal


class _Undefined:
    """JavaScript `undefined`: distinct from `None` (`null`), falsy, printed as "undefined"."""

    _instance = None

    def __new__(cls):
        if cls._instance is None:
            cls._instance = super().__new__(cls)
        return cls._instance

    def __bool__(self) -> bool:
        return False

    def __repr__(self) -> str:
        return "UNDEFINED"


UNDEFINED = _Undefined()

# The JavaScript whitespace set `\s` matches without the `u` flag (SPEC 3.1), written out so no
# engine's own `\s` is ever used. JSWS_CLASS goes inside a regex character class.
JSWS_CHARS = (
    "\t\n\x0b\x0c\r   "
    + "".join(chr(c) for c in range(0x2000, 0x200B))
    + "    　﻿"
)
JSWS_CLASS = "\t\n\x0b\x0c\r    -     　﻿"
JSWS = "[" + JSWS_CLASS + "]"


def is_nullish(value) -> bool:
    """`value == null` in JavaScript: null or undefined."""
    return value is None or value is UNDEFINED


def coalesce(*values):
    """`a ?? b ?? c`: the first value that is neither null nor undefined (else the last)."""
    for value in values[:-1]:
        if not is_nullish(value):
            return value
    return values[-1]


def js_trim(text: str) -> str:
    """`String.prototype.trim()`: strips exactly the JSWS characters (SPEC 3.5)."""
    return text.strip(JSWS_CHARS)


# --- UTF-16 measurement (SPEC 3.2) ------------------------------------------------------------


def _units(text: str) -> bytes:
    return text.encode("utf-16-le", "surrogatepass")


def u16_len(text: str) -> int:
    """`text.length`: UTF-16 code units."""
    return len(_units(text)) // 2


def u16_slice(text: str, start: int, end: int | None = None) -> str:
    """`text.slice(start, end)` in UTF-16 units, for non-negative bounds.

    A cut that would split a surrogate pair drops the lone high surrogate (SPEC 3.2).
    """
    units = _units(text)
    count = len(units) // 2
    end = count if end is None else max(0, min(end, count))
    start = max(0, min(start, count))
    if start >= end:
        return ""
    piece = units[2 * start : 2 * end]
    if end < count:
        last = int.from_bytes(units[2 * end - 2 : 2 * end], "little")
        after = int.from_bytes(units[2 * end : 2 * end + 2], "little")
        if 0xD800 <= last <= 0xDBFF and 0xDC00 <= after <= 0xDFFF:
            piece = piece[:-2]
    return piece.decode("utf-16-le", "surrogatepass")


def u16_pad_end(text: str, width: int) -> str:
    """`text.padEnd(width)` with spaces, measured in UTF-16 units."""
    return text + " " * max(0, width - u16_len(text))


def code_points(text: str) -> int:
    """`[...text].length`: code points, a surrogate pair counting once."""
    return len(_units(text).decode("utf-16-le", "surrogatepass"))


def well_formed(text: str) -> str:
    """The text as UTF-8 would carry it: pairs joined, each lone surrogate replaced by U+FFFD."""
    return _units(text).decode("utf-16-le", "replace")


def utf8(text: str) -> bytes:
    """UTF-8 bytes for hashing or a terminal, lone surrogates as `EF BF BD` (SPEC 3.3)."""
    return well_formed(text).encode("utf-8")


# --- Numbers (SPEC 3.3 number form, 3.6, 3.7) ------------------------------------------------


def _is_number(value) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def is_number(value) -> bool:
    """`typeof value === "number"`."""
    return _is_number(value)


def is_finite_number(value) -> bool:
    """`Number.isFinite(value)`: a number, not NaN or an infinity, no coercion."""
    return _is_number(value) and math.isfinite(float(value))


def number_to_string(value) -> str:
    """`Number.prototype.toString()`: shortest round-trip digits in JavaScript's layout."""
    x = float(value)
    if x != x:
        return "NaN"
    if x == math.inf:
        return "Infinity"
    if x == -math.inf:
        return "-Infinity"
    if x == 0:
        return "0"
    if x < 0:
        return "-" + number_to_string(-x)
    text = repr(x)
    if "e" in text:
        mantissa, exponent = text.split("e")
        exp = int(exponent)
    else:
        mantissa, exp = text, 0
    if "." in mantissa:
        whole, fraction = mantissa.split(".")
    else:
        whole, fraction = mantissa, ""
    digits = whole + fraction
    n = len(whole) + exp
    stripped = digits.lstrip("0")
    n -= len(digits) - len(stripped)
    digits = stripped.rstrip("0") or "0"
    k = len(digits)
    if k <= n <= 21:
        return digits + "0" * (n - k)
    if 0 < n <= 21:
        return digits[:n] + "." + digits[n:]
    if -6 < n <= 0:
        return "0." + "0" * (-n) + digits
    e = n - 1
    sign = "+" if e >= 0 else "-"
    if k == 1:
        return f"{digits}e{sign}{abs(e)}"
    return f"{digits[0]}.{digits[1:]}e{sign}{abs(e)}"


def to_string(value) -> str:
    """ECMAScript `ToString` for the values JSON produces (and `${value}`)."""
    if isinstance(value, str):
        return value
    if value is None:
        return "null"
    if value is UNDEFINED:
        return "undefined"
    if isinstance(value, bool):
        return "true" if value else "false"
    if _is_number(value):
        return number_to_string(value)
    if isinstance(value, list):
        return array_join(value, ",")
    if isinstance(value, dict):
        return "[object Object]"
    return str(value)


def array_join(items, separator: str) -> str:
    """`Array.prototype.join`: null and undefined elements are empty."""
    return separator.join("" if is_nullish(item) else to_string(item) for item in items)


def truthy(value) -> bool:
    """JavaScript truthiness: `[]` and `{}` are truthy, NaN is falsy."""
    if value is None or value is UNDEFINED:
        return False
    if isinstance(value, bool):
        return value
    if _is_number(value):
        x = float(value)
        return not (x == 0 or x != x)
    if isinstance(value, str):
        return len(value) > 0
    return True


_DECIMAL = re.compile(r"[+-]?(?:\d+\.?\d*(?:[eE][+-]?\d+)?|\.\d+(?:[eE][+-]?\d+)?)", re.ASCII)
_RADIX = {"x": (16, re.compile(r"[0-9a-fA-F]+", re.ASCII)), "o": (8, re.compile(r"[0-7]+")), "b": (2, re.compile(r"[01]+"))}


def string_to_number(text: str) -> float:
    """ECMAScript `StringToNumber` (SPEC 3.6)."""
    t = js_trim(text)
    if t == "":
        return 0.0
    if t in ("Infinity", "+Infinity"):
        return math.inf
    if t == "-Infinity":
        return -math.inf
    if len(t) > 2 and t[0] == "0" and t[1] in "xXoObB":
        base, pattern = _RADIX[t[1].lower()]
        if pattern.fullmatch(t[2:]):
            return float(int(t[2:], base))
        return math.nan
    if _DECIMAL.fullmatch(t):
        return float(t)
    return math.nan


def to_number(value) -> float:
    """ECMAScript `ToNumber` over JSON values (SPEC 3.6)."""
    if isinstance(value, bool):
        return 1.0 if value else 0.0
    if _is_number(value):
        return float(value)
    if value is None:
        return 0.0
    if value is UNDEFINED:
        return math.nan
    if isinstance(value, str):
        return string_to_number(value)
    if isinstance(value, list):
        return string_to_number(array_join(value, ","))
    return math.nan


def math_round(value) -> float:
    """`Math.round` (SPEC 3.7): halves go up, `-0` kept for -0.5 <= x <= -0."""
    x = float(value)
    if x != x or math.isinf(x):
        return x
    if x == 0:
        return x
    r = float(math.floor(x))
    result = r + 1 if x - r >= 0.5 else r
    if result == 0 and x < 0:
        return -0.0
    return result


def math_max(*values) -> float:
    """`Math.max` over numbers: NaN wins."""
    result = -math.inf
    for value in values:
        x = float(value)
        if x != x:
            return math.nan
        if x > result or (x == 0 and result == 0 and math.copysign(1, result) < 0):
            result = x
    return result


def to_fixed2(value) -> str:
    """`Number.prototype.toFixed(2)` (SPEC 3.7): exact value, ties to the larger magnitude."""
    x = float(value)
    if x != x:
        return "NaN"
    if abs(x) >= 1e21:
        return number_to_string(x)
    if x < 0:
        return "-" + to_fixed2(-x)
    if x == 0:
        x = 0.0
    quantized = Decimal(x).quantize(Decimal("0.01"), rounding=ROUND_HALF_UP)
    return format(quantized, "f")


_PARSE_INT = re.compile(r"[+-]?\d+", re.ASCII)


def parse_int(text) -> float:
    """`Number.parseInt(text, 10)`: leading whitespace, sign, then the longest digit run."""
    t = to_string(text).lstrip(JSWS_CHARS)
    match = _PARSE_INT.match(t)
    if not match:
        return math.nan
    return float(int(match.group(0)))
