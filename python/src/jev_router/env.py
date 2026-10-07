"""jev's settings from env files (port of node/src/env.mjs, SPEC 9.1).

`parse_env` is `Dotenv::ParseContent` from Node 24.21.0
(conformance/reference/node_dotenv_parse_content.cc), ported line for line. It works on the
decoded text: every split is on an ASCII character, so that is the same as working on the bytes.
"""

from __future__ import annotations

import os
import re
from collections.abc import Mapping, MutableMapping
from typing import Final

from . import osdirs
from .jsjson import is_index_key

PROJECT_KEYS: Final = {
    "JEV_API_KEY",
    "TYPESAFE_API_KEY",
    "JEV_DEBUG",
    "JEV_ALLOW_FABLE",
    "JEV_NO_STATUSLINE",
    "JEV_ICONS",
}
_EFFORT_KEY = re.compile(r"JEV_(?:[A-Z]+_)?(?:FORCE_)?EFFORT\Z")

PRIVATE_KEYS: Final = ["JEV_API_KEY", "TYPESAFE_API_KEY"]

_NPOS = -1


def is_project_key(key: str) -> bool:
    """Whether a project `.env` may set `key` (SPEC 9.1)."""
    return key in PROJECT_KEYS or bool(_EFFORT_KEY.match(key))


def _find_first_not_of(text: str, chars: str) -> int:
    for i, ch in enumerate(text):
        if ch not in chars:
            return i
    return _NPOS


def _find_last_not_of(text: str, chars: str) -> int:
    for i in range(len(text) - 1, -1, -1):
        if text[i] not in chars:
            return i
    return _NPOS


def _find_first_of(text: str, chars: str) -> int:
    for i, ch in enumerate(text):
        if ch in chars:
            return i
    return _NPOS


def trim_spaces(text: str) -> str:
    """Removes leading and trailing space, tab and newline (and nothing else)."""
    if not text:
        return ""
    pos_start = _find_first_not_of(text, " \t\n")
    if pos_start == _NPOS:
        return ""
    pos_end = _find_last_not_of(text, " \t\n")
    if pos_end == _NPOS:
        return text[pos_start:]
    return text[pos_start : pos_end + 1]


def parse_content(source: str) -> dict[str, str]:
    """`Dotenv::ParseContent`: the store as a key -> value map (insertion order)."""
    store: dict[str, str] = {}
    lines = source.replace("\r", "")
    content = trim_spaces(lines)

    while content:
        if content[0] == "\n" or content[0] == "#":
            newline = content.find("\n")
            if newline != _NPOS:
                content = content[newline + 1 :]
            else:
                content = ""
            continue

        equal_or_newline = _find_first_of(content, "=\n")
        if equal_or_newline == _NPOS or content[equal_or_newline] == "\n":
            if equal_or_newline != _NPOS:
                content = content[equal_or_newline + 1 :]
                content = trim_spaces(content)
                continue
            break

        key = content[:equal_or_newline]
        content = content[equal_or_newline + 1 :]
        key = trim_spaces(key)

        if not content or content[0] == "\n":
            store[key] = ""
            continue

        content = trim_spaces(content)

        if not key:
            continue

        if key.startswith("export "):
            key = key[7:]
            key = trim_spaces(key)

        if not content:
            store[key] = ""
            break

        if content[0] == '"':
            closing_quote = content.find(content[0], 1)
            if closing_quote != _NPOS:
                value = content[1:closing_quote]
                store[key] = value.replace("\\n", "\n")
                newline = content.find("\n", closing_quote + 1)
                if newline != _NPOS:
                    content = content[newline + 1 :]
                else:
                    content = ""
                continue

        if content[0] in ("'", '"', "`"):
            closing_quote = content.find(content[0], 1)
            if closing_quote == _NPOS:
                newline = content.find("\n")
                if newline != _NPOS:
                    store[key] = content[:newline]
                    content = content[newline + 1 :]
                else:
                    store[key] = content
                    break
            else:
                store[key] = content[1:closing_quote]
                newline = content.find("\n", closing_quote + 1)
                if newline != _NPOS:
                    content = content[newline + 1 :]
                else:
                    content = ""
                continue
        else:
            newline = content.find("\n")
            if newline != _NPOS:
                value = content[:newline]
                hash_character = value.find("#")
                if hash_character != _NPOS:
                    value = value[:hash_character]
                store[key] = trim_spaces(value)
                content = content[newline + 1 :]
            else:
                value = content
                hash_char = value.find("#")
                if hash_char != _NPOS:
                    value = content[:hash_char]
                store[key] = trim_spaces(value)
                content = ""

        content = trim_spaces(content)

    return store


def parse_env(source: str | bytes) -> dict[str, str]:
    """`util.parseEnv(text)`: the parsed map as a JavaScript object.

    Node's store is a `std::map`, so keys come out in byte order; JavaScript then puts canonical
    array-index keys first, which `jsjson.js_keys` applies wherever the object is iterated.
    """
    if isinstance(source, bytes):
        source = source.decode("utf-8", "replace")
    store = parse_content(source)
    ordered = sorted(store.keys(), key=lambda k: k.encode("utf-8", "surrogatepass"))
    index = sorted((k for k in ordered if is_index_key(k)), key=int)
    return {k: store[k] for k in index + [k for k in ordered if not is_index_key(k)]}


def _read(file: str) -> dict[str, str]:
    try:
        with open(file, "rb") as handle:
            return parse_env(handle.read())
    except Exception:  # noqa: BLE001 - missing or unreadable; the key may still come from the real environment
        return {}


def load_env(
    cwd: str | None = None, home: str | None = None, env: MutableMapping[str, str] | None = None
) -> MutableMapping[str, str]:
    """Loads jev's settings into `env` and returns it.

    Existing variables win, then the project's `.env` (allow-listed keys only),
    `<home>/.jev-router.env`, then `<home>/.jev-claude.env`.
    """
    cwd = os.getcwd() if cwd is None else cwd
    home = osdirs.home() if home is None else home
    env = os.environ if env is None else env
    project = [(k, v) for k, v in _read(os.path.join(cwd, ".env")).items() if is_project_key(k)]
    entries = project + list(_read(os.path.join(home, ".jev-router.env")).items())
    entries += list(_read(os.path.join(home, ".jev-claude.env")).items())
    for key, value in entries:
        if value != "" and key not in env:
            try:
                env[key] = value
            except (ValueError, OSError):
                # os.environ refuses some keys Node accepts (an empty name, one with "=").
                pass
    return env


def child_env(env: Mapping[str, str] | None = None) -> dict[str, str]:
    """A copy of `env` without the Jev key."""
    env = os.environ if env is None else env
    out = dict(env)
    for key in PRIVATE_KEYS:
        out.pop(key, None)
    return out
