"""Every routing decision knob (port of node/src/config.mjs, SPEC 4).

Every prose string sent to Jev is copied byte-for-byte from node/src/config.mjs.
"""

from __future__ import annotations

import os
import re

from .jsstr import JSWS, js_trim

# Model tiers, cheapest first (SPEC 4.1). `floor` is absent for Haiku.
TIERS = [
    {"name": "haiku", "id": "claude-haiku-4-5-20251001", "family": "haiku", "thinking": False, "effort": False},
    {"name": "sonnet", "id": "claude-sonnet-5-5", "family": "sonnet", "thinking": True, "effort": True, "floor": "high"},
    {"name": "opus", "id": "claude-opus-5-5", "family": "opus", "thinking": True, "effort": True, "floor": "medium"},
    {"name": "fable", "id": "claude-fable-5-1", "family": "fable", "thinking": True, "effort": True, "floor": "high"},
]

EFFORTS = ["low", "medium", "high", "xhigh", "max"]

TIER_NAMES = [t["name"] for t in TIERS]


def tier_spec(name):
    for tier in TIERS:
        if tier["name"] == name:
            return tier
    return None


def rank_of(name) -> int:
    return TIER_NAMES.index(name) if name in TIER_NAMES else -1


def id_of(name):
    tier = tier_spec(name)
    return tier["id"] if tier else None


def _chosen(env, key):
    value = env.get(key)
    if not isinstance(value, str):
        return None
    return js_trim(value).lower()


def effort_floor(name, env=None):
    """The effort a tier is given when the request names none."""
    env = os.environ if env is None else env
    tier = tier_spec(name)
    if not tier or not tier.get("floor"):
        return None
    chosen = _chosen(env, f"JEV_{name.upper()}_EFFORT")
    return chosen if chosen in EFFORTS else tier["floor"]


def forced_effort(name, env=None):
    """An effort that replaces whatever the request carries, or None."""
    env = os.environ if env is None else env
    tier = tier_spec(name)
    if not tier or not tier["effort"]:
        return None
    for key in (f"JEV_{name.upper()}_FORCE_EFFORT", "JEV_FORCE_EFFORT"):
        chosen = _chosen(env, key)
        if chosen in EFFORTS:
            return chosen
    return None


AUTO_MODEL = "jev-router"


def is_auto(model) -> bool:
    return isinstance(model, str) and model == AUTO_MODEL


def tier_of(model):
    """Tier name for a model string, or None."""
    if not isinstance(model, str):
        return None
    for tier in TIERS:
        if tier["family"] in model:
            return tier["name"]
    return None


_FABLE_OFF = re.compile(r"(0|false|no|off)\Z", re.ASCII | re.IGNORECASE)


def fable_allowed(env=None) -> bool:
    env = os.environ if env is None else env
    value = env.get("JEV_ALLOW_FABLE")
    text = js_trim(value) if isinstance(value, str) else ""
    return not _FABLE_OFF.match(text)


def available_tiers(env=None):
    return [n for n in TIER_NAMES if n != "fable" or fable_allowed(env)]


THRESHOLDS = {
    "minConfidence": 0.6,
    "uncertainDefault": "sonnet",
    "downgradeMaxContextTokens": 20000,
    "jevTimeoutMs": 1500,
    "jevDeadlineMs": 3000,
    "jevMaxRetries": 1,
}

CONTEXT_WINDOW_TOKENS = 200000

COMPLEXITY_SCALE = [
    "None",
    "Very low",
    "Low",
    "Some",
    "Moderate",
    "Moderate to high",
    "High",
    "Very high",
    "Severe",
    "Extreme",
]

COMPLEXITY_MAX_SCORE = len(COMPLEXITY_SCALE) - 1

_OVERRIDE_NAMES = {
    "haiku": {"names": "haiku", "generic": "fast"},
    "sonnet": {"names": "sonnet", "generic": "balanced"},
    "opus": {"names": "opus", "generic": "strong"},
    "fable": {"names": "fable", "generic": "long"},
}


def _override(names, generic):
    # SPEC 4.5: `\s` written out as JSWS; `\w`/`\b` ASCII via re.ASCII; ASCII-only case folding.
    return re.compile(
        rf"\b(?:use|switch to|switch over to|route to){JSWS}+(?:the{JSWS}+)?(?:claude[-{JSWS[1:-1]}])?"
        rf"(?:(?:{names})|{generic}{JSWS}+(?:model|tier))(?![-\w])",
        re.ASCII | re.IGNORECASE,
    )


OVERRIDE_PATTERNS = [
    {"tier": t["name"], "re": _override(**_OVERRIDE_NAMES[t["name"]])} for t in TIERS
]


def _score(instructions, criteria):
    return {"type": "score", "instructions": instructions, "criteria": criteria}


def _choice(instructions, criteria):
    return {"type": "choice", "instructions": instructions, "criteria": criteria}


QUESTIONS = {
    "task_complexity": _score(
        "How complex is the coding task overall, including ambiguity, scope, and blast radius?",
        COMPLEXITY_SCALE,
    ),
    "reasoning_required": _score(
        "How much reasoning is required to complete the request correctly in one pass?",
        COMPLEXITY_SCALE,
    ),
    "tool_complexity": _score(
        "How complex is the tool use required, from no tools to many coordinated or stateful operations?",
        COMPLEXITY_SCALE,
    ),
}

GUIDANCE = {
    "haiku": {
        "what": "Trivial, mechanical, or purely factual work.",
        "signals": ["Rename, reformat, comment, or run one obvious command"],
        "not_for": "Design judgement or multi-file reasoning.",
    },
    "sonnet": {
        "what": "Well-scoped everyday work, and documents and knowledge work, where it does well for well under Opus's cost.",
        "signals": [
            "Implement a specified function or change, add tests, or fix a bug whose cause is already known",
            "Write or edit documents, specs, summaries, or analysis",
        ],
        "not_for": "Open-ended or multi-step coding, changes that must not break existing behaviour, unknown-cause debugging, or judgement calls: Opus scores 5-21 points higher on agentic coding.",
    },
    "opus": {
        "what": "Complex or open-ended coding and work that needs sustained judgement, where it clearly beats Sonnet, for about twice the cost per task.",
        "signals": [
            "Unknown-cause or intermittent bugs, multi-step changes across a codebase, cross-module design, API or behaviour-preserving changes, security, auth, concurrency, or migrations",
        ],
        "not_for": "Well-scoped changes and routine document or knowledge work, where Sonnet does well for less.",
    },
    "fable": {
        "what": "Long-horizon autonomous work, the most demanding reasoning, and adversarial review that hardens a plan, spec, or design by hunting for how it fails.",
        "signals": [
            "Long-horizon autonomous work: a whole-repo migration, a large system built end to end from a spec, or a full deliverable such as financial analysis with spreadsheets and slides",
            "Adversarially review, red-team, stress-test, or poke holes in a plan, spec, or design to harden it",
            "A problem that has already defeated a strong model, such as a bug two attempts have missed",
        ],
        "not_for": "Writing the plan or spec itself, ordinary code review, or security-focused analysis, where Fable's safety classifiers can decline.",
    },
}

COST = {
    "haiku": "$1 / $5 per million input / output tokens; the cheapest, but it does no reasoning",
    "sonnet": "$2 / $10 per million tokens; about 40-60% less per completed task than Opus",
    "opus": "$4 / $20 per million tokens; about 1.6-2.6x Sonnet's cost per completed task",
    "fable": "$10 / $50 per million tokens; the most expensive by far",
}

_MODEL_INSTRUCTIONS = [
    "Pick the cheapest exact model that can fully complete this coding request in one pass, without retrying on a stronger model.",
    "When you are unsure whether a cheaper model would get it right, pick the stronger one: a failed attempt wastes the whole turn and is rerun anyway, so it costs more than the difference. Speed does not matter.",
    "Each tier is offered as its newest version only. Judge required reasoning, not requested reply length.",
    "Reasoning effort is set per tier and is not something to choose between; judge only which model the work needs.",
    "Changing tier mid-conversation discards the prompt cache and re-reads the whole history, so prefer the current model where the work has not changed shape.",
]


def question_for_models(models):
    """A Jev choice over the exact models available (SPEC 4.4)."""
    from .jsstr import UNDEFINED, coalesce

    criteria = {}
    for model in models:
        model_id = model.get("id", UNDEFINED)
        tier = model.get("tier")
        entry = {
            "model": coalesce(model.get("description", UNDEFINED), model_id),
            "cost": COST.get(tier, UNDEFINED) if isinstance(tier, str) else UNDEFINED,
        }
        entry.update(GUIDANCE.get(tier, {}) if isinstance(tier, str) else {})
        criteria[_key(model_id)] = entry
    return _choice(list(_MODEL_INSTRUCTIONS), criteria)


def _key(value):
    from .jsstr import to_string

    return to_string(value)


def should_use_exact_model(reason, chosen_tier, final_tier) -> bool:
    return reason in ("jev", "jev/no-change") and chosen_tier == final_tier
