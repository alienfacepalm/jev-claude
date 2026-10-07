"""The explanation panel behind /jev-explain (port of node/src/explain.mjs, SPEC 13)."""

from __future__ import annotations

import math
import re

from .config import tier_of
from .jsstr import (
    JSWS,
    UNDEFINED,
    JsValue,
    coalesce,
    is_finite_number,
    is_nullish,
    js_trim,
    math_max,
    math_round,
    to_fixed2,
    to_number,
    to_string,
    truthy,
    u16_len,
    u16_pad_end,
    u16_slice,
)
from .reasons import is_no_change, long_reason
from .status import agent_view, now_ms


def _get(obj: object, key: str) -> JsValue:
    if isinstance(obj, dict):
        value: JsValue = obj.get(key, UNDEFINED)
        return value
    return UNDEFINED


def _prop(obj: object, key: str) -> JsValue:
    if is_nullish(obj):
        raise TypeError(f"Cannot read properties of {obj!r} (reading '{key}')")
    return _get(obj, key)


def _upper(value: object) -> str:
    """`value.toUpperCase()`: only strings have it."""
    if not isinstance(value, str):
        raise TypeError("toUpperCase is not a function")
    return value.upper()


def _recommendation_of(status: object) -> object:
    answers = _get(_get(_get(status, "jev"), "response"), "answers")
    choice = coalesce(_get(_get(answers, "model"), "choice"), _get(_get(answers, "model_tier"), "choice"))
    if not truthy(choice):
        return coalesce(_get(status, "tier"), "unknown")
    return coalesce(tier_of(choice), choice)


WIDTH = 33
AGENT_WIDTH = 52
_SPACES = re.compile(JSWS + "+")


def _row(text: str = "") -> str:
    return f"│ {u16_pad_end(u16_slice(text, 0, WIDTH - 2), WIDTH - 2)} │"


def _metric(value: object) -> str:
    return to_fixed2(value) if is_finite_number(value) else "n/a"


def _wrapped(label: str, value: object) -> list[str]:
    words = js_trim(_SPACES.sub(" ", f"{label}{to_string(value)}")).split(" ")
    lines: list[str] = []
    for word in words:
        if not lines or u16_len(f"{lines[-1]} {word}") > WIDTH - 2:
            lines.append(word)
        else:
            lines[-1] += f" {word}"
    return [_row(line) for line in lines]


def _decision(reason: object = UNDEFINED) -> str:
    if reason is UNDEFINED:
        reason = ""
    return f"{'kept this model - ' if is_no_change(reason) else ''}{long_reason(reason)}"


def _agent_row(text: str = "") -> str:
    return f"│ {u16_pad_end(u16_slice(text, 0, AGENT_WIDTH - 2), AGENT_WIDTH - 2)} │"


def _age(at: object, now: object) -> str:
    seconds = math_max(0, math_round((to_number(now) - to_number(coalesce(at, now))) / 1000))
    if seconds < 60:
        return f"{to_string(seconds)}s"
    if seconds < 3600:
        return f"{to_string(math_round(seconds / 60))}m"
    return f"{to_string(math_round(seconds / 3600))}h"


def _percent(confidence: object) -> str:
    return f"{to_string(math_round(to_number(confidence) * 100))}%"


def format_agents(status: object, now: float | None = None) -> str:
    """Every agent routed in this session and the model each one got, or ""."""
    now = now_ms() if now is None else now
    view = agent_view(status, fresh_ms=math.inf, now=now)
    everyone = [a for a in [view["main"], *view["subagents"]] if a is not None and truthy(a)]
    if not everyone:
        return ""
    lines = [
        f"┌{'─' * AGENT_WIDTH}┐",
        _agent_row("Jev Router · agents this session"),
        _agent_row(),
    ]
    for a in everyone:
        role = u16_pad_end("main" if truthy(a.get("main", UNDEFINED)) else "sub", 5)
        model = u16_pad_end(_upper(coalesce(a.get("model", UNDEFINED), a.get("tier", UNDEFINED), "unknown")), 22)
        confidence = a.get("confidence", UNDEFINED)
        if truthy(a.get("manual", UNDEFINED)):
            p = "manual"
        elif is_nullish(confidence):
            p = ""
        else:
            p = _percent(confidence)
        lines.append(_agent_row(f"{role} {model} {u16_pad_end(p, 7)} {_age(a.get('at', UNDEFINED), now)}"))
        label = a.get("label", UNDEFINED)
        if truthy(label) and label != "main":
            lines.append(_agent_row(f"      {to_string(label)}"))
    lines.append(f"└{'─' * AGENT_WIDTH}┘")
    return "\n".join(lines)


def format_explanation(status: object) -> str:
    """The panel for one routing decision, or a one-line notice when there is none to show."""
    if not truthy(status):
        return "Jev Router: no routing decision has been recorded for this session."
    if truthy(_get(status, "manual")):
        return "Jev Router: routing is paused because you selected a model manually."

    m = coalesce(_get(status, "metrics"), {})
    request = _get(_get(_get(status, "jev"), "request"), "state")
    session = _get(request, "session")
    recommendation = _recommendation_of(status)
    confidence = _get(status, "confidence")
    return "\n".join(
        [
            f"┌{'─' * WIDTH}┐",
            _row("Jev Router"),
            _row(),
            _row("Jev request"),
            *_wrapped("Prompt: ", coalesce(_get(status, "prompt"), "not recorded")),
            _row(f"Current model: {_upper(coalesce(_get(session, 'current_model'), 'unknown'))}"),
            _row(f"Context tokens: {to_string(coalesce(_get(session, 'context_tokens'), 'unknown'))}"),
            _row(),
            _row("Jev response"),
            _row(f"Task complexity     {_metric(_prop(m, 'taskComplexity'))}"),
            _row(f"Reasoning required  {_metric(_prop(m, 'reasoningRequired'))}"),
            _row(f"Tool complexity     {_metric(_prop(m, 'toolComplexity'))}"),
            _row(f"Context size        {_metric(_prop(m, 'contextSize'))}"),
            _row(),
            _row(f"Recommended tier: {_upper(recommendation)}"),
            _row(f"Selected model: {_upper(coalesce(_get(status, 'model'), _get(status, 'tier'), 'unknown'))}"),
            _row(),
            _row(f"Confidence: {'n/a' if is_nullish(confidence) else _percent(confidence)}"),
            *_wrapped("Decision: ", _decision(_get(status, "reason"))),
            f"└{'─' * WIDTH}┘",
        ]
    )
