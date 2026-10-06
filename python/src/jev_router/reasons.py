"""How a routing decision is said to a person (port of node/src/reasons.mjs, SPEC 12.4)."""

from __future__ import annotations

REASONS = [
    {"match": "override", "short": None, "long": "you named this model in the prompt"},
    {
        "match": "jev-unavailable",
        "short": "router offline",
        "long": "the router could not be reached, so the model was left alone",
    },
    {
        "match": "low-confidence-default",
        "short": None,
        "long": "the router was unsure, so this ran one tier below its pick, and no lower than the default model",
    },
    {
        "match": "downgrade-not-worth-cache-rebuild",
        "short": "keeping the cache",
        "long": "a cheaper model would have to re-read the whole conversation, which costs more than it saves",
    },
    {
        "match": "unavailable",
        "short": "nearest available",
        "long": "the chosen tier is not available on this account, so the nearest one was used",
    },
]


def _includes(reason, needle) -> bool:
    # `reason.includes(x)`: a string search; an array's `includes` compares elements instead.
    if isinstance(reason, str):
        return needle in reason
    if isinstance(reason, list):
        return any(isinstance(item, str) and item == needle for item in reason)
    raise TypeError("reason.includes is not a function")


def _find(reason):
    from .jsstr import truthy

    if not truthy(reason):
        return None
    for entry in REASONS:
        if _includes(reason, entry["match"]):
            return entry
    return None


def short_reason(reason):
    """A few words for the status line, or None."""
    entry = _find(reason)
    return entry["short"] if entry else None


def long_reason(reason):
    """A sentence for the explanation panel."""
    entry = _find(reason)
    return entry["long"] if entry else "the router's recommendation"


def is_no_change(reason) -> bool:
    from .jsstr import is_nullish

    if is_nullish(reason):
        return False
    return _includes(reason, "no-change")
