"""Turns a Jev answer into the model actually run (port of node/src/policy.mjs, SPEC 7.6)."""

from __future__ import annotations

import re

from .config import OVERRIDE_PATTERNS, THRESHOLDS, TIER_NAMES, rank_of
from .jsstr import UNDEFINED, coalesce, is_nullish, to_number, to_string

_OWN_WORDS = [
    re.compile(r"<agent-message.*?</agent-message>", re.DOTALL),
    re.compile(r"<system-reminder>.*?</system-reminder>", re.DOTALL),
    re.compile(r"```.*?```", re.DOTALL),
    re.compile(r"`[^`\n]*`"),
    re.compile(r'"[^"\n]*"'),
]


def own_words(prompt) -> str:
    """The part of a prompt the user wrote themselves."""
    text = to_string(coalesce(prompt, ""))
    for pattern in _OWN_WORDS:
        text = pattern.sub(" ", text)
    return text


def detect_override(prompt):
    """The tier the user named explicitly in the prompt, or None."""
    text = own_words(prompt)
    for entry in OVERRIDE_PATTERNS:
        if entry["re"].search(text):
            return entry["tier"]
    return None


def clamp_to_available(tier, available):
    if tier in available:
        return tier
    rank = rank_of(tier)
    up = [t for i, t in enumerate(TIER_NAMES) if i > rank and t in available and (t != "fable" or tier == "fable")]
    if up:
        return up[0]
    down = [t for i, t in enumerate(TIER_NAMES) if i < rank and t in available]
    return down[-1] if down else None


def decide(prompt=UNDEFINED, jev=None, current=None, available=(), context_tokens=0):
    """`decide({prompt, jev, current, available, contextTokens})` -> `{tier, reason, changed}`."""
    if context_tokens is UNDEFINED:
        context_tokens = 0
    available = list(available)

    def settle(tier, reason):
        final = coalesce(clamp_to_available(tier, available), current)
        why = reason if final == tier else f"{reason}+unavailable"
        return {
            "tier": final,
            "reason": f"{why}/no-change" if final == current else why,
            "changed": final != current,
        }

    override = detect_override(prompt)
    if override:
        return settle(override, "override")

    if not isinstance(jev, dict) or jev.get("choice", UNDEFINED) not in TIER_NAMES:
        return settle(current, "jev-unavailable")

    target = jev["choice"]
    confidence = jev.get("confidence", UNDEFINED)
    if not (to_number(confidence) >= THRESHOLDS["minConfidence"]):
        step_down = max(rank_of(target) - 1, rank_of(THRESHOLDS["uncertainDefault"]), rank_of(current))
        return settle(TIER_NAMES[step_down], "low-confidence-default")

    if rank_of(target) < rank_of(current) and to_number(context_tokens) > THRESHOLDS["downgradeMaxContextTokens"]:
        return settle(current, "downgrade-not-worth-cache-rebuild")

    return settle(target, "jev")


__all__ = ["decide", "detect_override", "own_words", "clamp_to_available", "is_nullish"]
