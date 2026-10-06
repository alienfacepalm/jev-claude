"""jev-check: the read-only setup report behind /jev-calibrate (port of node/bin/jev-check.mjs)."""

from __future__ import annotations

import datetime
import os

from .. import repo
from ..config import AUTO_MODEL, TIERS, fable_allowed, tier_of
from ..jsstr import to_string
from ..status import read_calibration
from ._io import write_stdout


def _row(label: str, text: str) -> str:
    return f"{label.ljust(13)}{text}"


def _local_time(at) -> str:
    # `new Date(at).toLocaleString()`: the platform's local date-time format (excluded from
    # byte comparison, SPEC 13).
    try:
        return datetime.datetime.fromtimestamp(at / 1000).strftime("%x, %X")
    except (OverflowError, OSError, ValueError):
        return "Invalid Date"


def report(env=None, root=repo.ROOT) -> str:
    env = os.environ if env is None else env
    repository = bool(root) and os.path.exists(os.path.join(root, ".git")) and os.path.exists(
        os.path.join(root, "node", "scripts", "calibrate.mjs")
    )
    routing = env.get("ANTHROPIC_CUSTOM_MODEL_OPTION") == AUTO_MODEL
    calibration = read_calibration()
    newer, models, at = calibration["newer"], calibration["models"], calibration["at"]

    lines = ["Jev Router setup check (read-only: nothing was changed)", ""]
    pinned = env.get("ANTHROPIC_MODEL") and env.get("ANTHROPIC_MODEL") != AUTO_MODEL
    if not routing:
        text = "off - no JEV_API_KEY found. Add JEV_API_KEY=... to ~/.jev-router.env and restart jev-claude."
    elif pinned:
        text = (
            f"available, but this session started on {env['ANTHROPIC_MODEL']} because ANTHROPIC_MODEL is set. "
            "Choose Jev Router in /model to route."
        )
    else:
        text = "on - Jev Router picks a model for each turn"
    lines.append(_row("Routing", text))
    if env.get("ANTHROPIC_AUTH_TOKEN"):
        claude = "auth token (ANTHROPIC_AUTH_TOKEN)"
    elif env.get("ANTHROPIC_API_KEY"):
        claude = (
            "API key (ANTHROPIC_API_KEY), billed per token - if you approved it when Claude Code asked; "
            "otherwise your sign-in. Change it with 'Use custom API key' in /config."
        )
    else:
        claude = "your Claude Code sign-in"
    lines.append(_row("Claude", claude))
    lines.append(_row("Tuned for", ", ".join(f"{t['name']} {t['id']}" for t in TIERS)))

    if at is None:
        lines.append(
            _row("Your account", "not read yet - Claude Code has not loaded the model list. Run /jev-calibrate again shortly.")
        )
    else:
        listed = ", ".join(to_string(m) for m in models) or "no Claude models listed"
        lines.append(_row("Your account", f"{listed} (as of {_local_time(at)})"))
        offered = {tier_of(m) for m in models}
        missing = [t["name"] for t in TIERS if t["name"] not in offered]
        if missing:
            lines.append(_row("", f"not offered to this account: {', '.join(missing)} (routing steps around them)"))
        if newer:
            text = (
                f"{', '.join(to_string(n) for n in newer)} - routing already uses them, but the router was tuned on "
                "the versions before. Update jev-router to get tuning for them."
            )
        else:
            text = "none - the router is tuned for the newest models your account offers"
        lines.append(_row("Newer models", text))

    if fable_allowed(env):
        fable = "offered when the work calls for it (bills extra usage credits; JEV_ALLOW_FABLE=0 turns it off)"
    else:
        fable = "off (JEV_ALLOW_FABLE is set to turn it off)"
    lines.append(_row("Fable", fable))
    lines.append(_row("Mode", "repository" if repository else "installed"))
    return "\n".join(lines) + "\n"


def main() -> int:
    write_stdout(report())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
