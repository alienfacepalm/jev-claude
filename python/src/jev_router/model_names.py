"""A model's short display name with its version (port of node/src/model-names.mjs, SPEC 12.3)."""

from __future__ import annotations

import re

from .jsstr import coalesce, to_string

_SHORT = re.compile(r"claude-([a-z]+)-(\d+)(?:-(\d{1,2})(?!\d))?", re.ASCII)


def short_name(model: object) -> str | None:
    """`claude-opus-5-5` is "Opus 5.5", `claude-haiku-4-5-20251001` "Haiku 4.5"; None otherwise."""
    match = _SHORT.search(to_string(coalesce(model, "")))
    if not match:
        return None
    family, major, minor = match.groups()
    version = f"{major}.{minor}" if minor else major
    return f"{family[0].upper()}{family[1:]} {version}"
