"""Tiny versions of the `/usage` meters (port of node/src/usage.mjs, SPEC 12)."""

from __future__ import annotations

import math
from collections.abc import Mapping
from typing import Final

from .jsstr import math_round, to_string

DIM: Final = "\x1b[2m"
BOLD: Final = "\x1b[1m"
RESET: Final = "\x1b[0m"

# Claude Code's `rate_limits` windows, in the order the line shows them, with the icon-set name of
# each window's label. Only claude.ai Pro and Max accounts get them, and only after the first
# response, so each one is simply left out when it is missing.
WINDOWS: Final = (("five_hour", "fiveHour"), ("seven_day", "sevenDay"))
CELLS: Final = 4
# From this share of a limit used, its value is bold instead of dimmed.
NEAR: Final = 70


def _get(obj: object, key: str) -> object:
    """`obj?.[key]`: undefined (None here) for anything that is not an object with that key."""
    return obj.get(key) if isinstance(obj, dict) else None


def usage_meters(rate_limits: object, marks: Mapping[str, str]) -> str:
    """The 5-hour and 7-day limits Claude Code reports, drawn tiny.

    Each shows a label, a bar of four cells when the icon set has one (each cell a quarter of the
    limit, any use that shows as at least 1% filling the first) and the percentage used: dimmed
    while there is plenty left, bold from 70%. Each item is led by the line's ` · ` separator; the
    result is empty when no window has a usable number.
    """
    out = ""
    for key, label in WINDOWS:
        used = _get(_get(rate_limits, key), "used_percentage")
        # Only a finite JSON number counts: a string, null, boolean or array is left out.
        if isinstance(used, bool) or not isinstance(used, (int, float)) or not math.isfinite(used):
            continue
        # `Math.min(100, Math.max(0, used))`, which turns -0 into +0.
        clamped = 100.0 if used > 100 else float(used) if used > 0 else 0.0
        shown = math_round(clamped)
        filled = math.ceil(shown / (100 / CELLS))
        cell = marks["meterFull"]
        bar = f"{cell * filled}{marks['meterEmpty'] * (CELLS - filled)} " if cell else ""
        emphasis = BOLD if shown >= NEAR else DIM
        out += f" {DIM}·{RESET} {BOLD}{marks[label]}{RESET} {emphasis}{bar}{to_string(shown)}%{RESET}"
    return out
