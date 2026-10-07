"""JSON with JavaScript semantics (SPEC 3.3): `JSON.parse` and `JSON.stringify` as Node does them.

Objects are plain `dict`s. JavaScript orders an object's keys with canonical array-index keys
first, ascending, then every other key in insertion order; a `dict` already keeps insertion
order, so `js_keys` applies the index rule wherever keys are iterated. Every number parses to a
`float`. `jsstr.UNDEFINED` marks `undefined`, which `stringify` omits from objects.
"""

from __future__ import annotations

import json
import math
from collections.abc import Mapping
from typing import NoReturn

from .jsstr import UNDEFINED, JsObject, JsValue, Undefined, is_number, number_to_string

_INDEX_LIMIT = 2**32 - 1


def is_index_key(key: str) -> bool:
    """A canonical array index: decimal, no leading zeros (except "0"), below 2^32 - 1."""
    if not key or not key.isascii() or not key.isdigit():
        return False
    if len(key) > 1 and key[0] == "0":
        return False
    return int(key) < _INDEX_LIMIT


def js_keys(obj: Mapping[str, object]) -> list[str]:
    """`Object.keys(obj)` order."""
    keys = list(obj.keys())
    index = sorted((k for k in keys if is_index_key(k)), key=int)
    if not index:
        return keys
    return index + [k for k in keys if not is_index_key(k)]


def js_items[V](obj: Mapping[str, V]) -> list[tuple[str, V]]:
    """`Object.entries(obj)` order."""
    return [(k, obj[k]) for k in js_keys(obj)]


def js_values[V](obj: Mapping[str, V]) -> list[V]:
    """`Object.values(obj)` order."""
    return [obj[k] for k in js_keys(obj)]


def spread(value: object) -> JsObject:
    """`{...value}` for a JSON value: an object's own keys, a string's or array's indices."""
    if isinstance(value, dict):
        return {k: value[k] for k in js_keys(value)}
    if isinstance(value, (str, list)):
        return {str(i): item for i, item in enumerate(value)}
    return {}


class ParseError(ValueError):
    """Raised where `JSON.parse` throws a SyntaxError."""


def _reject_constant(name: str) -> NoReturn:
    raise ParseError(f"Unexpected token {name} in JSON")


def _object(pairs: list[tuple[str, JsValue]]) -> JsObject:
    out: JsObject = {}
    for key, value in pairs:
        out[key] = value
    return out


def decode_bytes(data: bytes) -> str:
    """Node's `Buffer.toString()`: UTF-8, each invalid sequence replaced by U+FFFD."""
    return data.decode("utf-8", "replace")


def parse(text: str | bytes | bytearray | memoryview) -> JsValue:
    """`JSON.parse`. Accepts `str`, or `bytes` decoded as `Buffer.toString()` does first."""
    if isinstance(text, (bytes, bytearray, memoryview)):
        text = decode_bytes(bytes(text))
    try:
        value: JsValue = json.loads(
            text,
            parse_int=float,
            parse_float=float,
            parse_constant=_reject_constant,
            object_pairs_hook=_object,
        )
    except ParseError:
        raise
    except (ValueError, RecursionError) as error:
        raise ParseError(str(error)) from None
    return value


_SHORT = {'"': '\\"', "\\": "\\\\", "\b": "\\b", "\f": "\\f", "\n": "\\n", "\r": "\\r", "\t": "\\t"}


def quote(text: str) -> str:
    """A JSON string literal as `JSON.stringify` writes it (well-formed: lone surrogates escaped)."""
    out = ['"']
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]
        code = ord(ch)
        if ch in _SHORT:
            out.append(_SHORT[ch])
        elif code < 0x20:
            out.append(f"\\u{code:04x}")
        elif 0xD800 <= code <= 0xDBFF:
            if i + 1 < n and 0xDC00 <= ord(text[i + 1]) <= 0xDFFF:
                out.append(chr(0x10000 + ((code - 0xD800) << 10) + (ord(text[i + 1]) - 0xDC00)))
                i += 1
            else:
                out.append(f"\\u{code:04x}")
        elif 0xDC00 <= code <= 0xDFFF:
            out.append(f"\\u{code:04x}")
        else:
            out.append(ch)
        i += 1
    out.append('"')
    return "".join(out)


def _number(value: float) -> str:
    x = float(value)
    if math.isnan(x) or math.isinf(x):
        return "null"
    return number_to_string(x)


def stringify(value: object, indent: int | None = None) -> str | Undefined:
    """`JSON.stringify(value)` or `JSON.stringify(value, null, indent)`.

    Returns `UNDEFINED` where JavaScript returns `undefined` (the value itself is undefined).
    """
    gap = " " * min(10, int(indent)) if indent else ""
    result = _serialize(value, gap, "")
    return UNDEFINED if result is None else result


def _serialize(value: object, gap: str, current: str) -> str | None:
    if value is UNDEFINED:
        return None
    if value is None:
        return "null"
    if value is True:
        return "true"
    if value is False:
        return "false"
    if isinstance(value, str):
        return quote(value)
    if is_number(value):
        return _number(value)
    if isinstance(value, (list, tuple)):
        if not value:
            return "[]"
        inner = current + gap
        parts = []
        for item in value:
            text = _serialize(item, gap, inner)
            parts.append("null" if text is None else text)
        if gap:
            return "[\n" + inner + (",\n" + inner).join(parts) + "\n" + current + "]"
        return "[" + ",".join(parts) + "]"
    if isinstance(value, dict):
        inner = current + gap
        parts = []
        separator = ": " if gap else ":"
        for key in js_keys(value):
            text = _serialize(value[key], gap, inner)
            if text is None:
                continue
            parts.append(quote(key) + separator + text)
        if not parts:
            return "{}"
        if gap:
            return "{\n" + inner + (",\n" + inner).join(parts) + "\n" + current + "}"
        return "{" + ",".join(parts) + "}"
    raise TypeError(f"cannot serialise {type(value).__name__}")


def stringify_object(value: Mapping[str, object], indent: int | None = None) -> str:
    """`JSON.stringify` of an object, which always has a JSON form (never `undefined`)."""
    text = stringify(value, indent)
    if isinstance(text, Undefined):
        raise TypeError("an object always has a JSON form")
    return text


def dumps_bytes(value: object, indent: int | None = None) -> bytes:
    """`Buffer.from(JSON.stringify(value))`: the UTF-8 bytes Node would send or write.

    A lone surrogate never reaches the bytes: `quote` escapes it.
    """
    text = stringify(value, indent)
    if text is UNDEFINED:
        raise TypeError("undefined has no JSON form")
    return text.encode("utf-8")
