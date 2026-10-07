"""jev-statusline: the status line Claude Code runs (port of node/bin/jev-statusline.mjs, SPEC 12)."""

from __future__ import annotations

import re

from .. import jsjson
from ..icons import icons
from ..jsstr import UNDEFINED, coalesce, is_nullish, math_round, to_number, to_string, truthy, u16_len, u16_slice
from ..model_names import short_name
from ..reasons import short_reason
from ..status import agent_view, read_calibration, read_status
from ..worktree import location_info
from ._io import read_stdin, write_stdout

_FAMILY = re.compile(r"claude-([a-z]+)-", re.ASCII)
MAX_BRANCH = 28

DIM = "\x1b[2m"
BOLD = "\x1b[1m"
RESET = "\x1b[0m"
COLOR = {"haiku": "\x1b[32m", "sonnet": "\x1b[36m", "opus": "\x1b[35m", "fable": "\x1b[33m"}


def _get(obj, key):
    if isinstance(obj, dict):
        return obj.get(key, UNDEFINED)
    return UNDEFINED


def _prop(obj, key):
    if is_nullish(obj):
        raise TypeError(f"Cannot read properties of {to_string(obj)} (reading '{key}')")
    return _get(obj, key)


def _status_tier_of(model):
    match = _FAMILY.search(to_string(coalesce(model, "")))
    return match.group(1) if match else UNDEFINED


def _color(tier) -> str:
    return COLOR.get(tier, "") if isinstance(tier, str) else ""


def _clip(text: str, limit: int) -> str:
    return f"{u16_slice(text, 0, limit - 1)}\u2026" if u16_len(text) > limit else text


def render(input_bytes: bytes) -> str:
    marks = {name: f"{BOLD}{mark}{RESET}" for name, mark in icons().items()}

    try:
        text = jsjson.decode_bytes(input_bytes) or "{}"
        data = jsjson.parse(text)
    except jsjson.ParseError:
        data = {}

    status = read_status(_prop(data, "session_id"))
    directory = coalesce(_get(_get(data, "workspace"), "current_dir"), _get(data, "cwd"), "")
    if not isinstance(directory, str):
        raise TypeError("current_dir.split is not a function")
    directory = re.split(r"[/\\]", directory)[-1]
    pct = math_round(to_number(coalesce(_get(_get(data, "context_window"), "used_percentage"), 0)))
    view = agent_view(status)
    main, subagents = view["main"], view["subagents"]

    def main_line(entry) -> str:
        color = _color(_get(entry, "tier"))
        confidence = _get(entry, "confidence")
        p = "" if is_nullish(confidence) else f" {DIM}({to_string(math_round(to_number(confidence) * 100))}%){RESET}"
        effort = _get(entry, "effort")
        level = f" {DIM}\u00b7{RESET} {marks['effort']} {to_string(effort)}" if truthy(effort) else ""
        said = short_reason(_get(entry, "reason"))
        why = f" {DIM}({said}){RESET}" if said else ""
        name = coalesce(short_name(_get(entry, "model")), _get(entry, "model"), _get(entry, "tier"))
        return f"{marks['model']} {color}{to_string(name)}{RESET}{p}{level}{why}"

    routed = f"{DIM}jev: waiting for first prompt{RESET}"
    if truthy(_get(main, "manual")) or (not truthy(main) and truthy(_get(status, "manual"))):
        shown = coalesce(_get(_get(data, "model"), "display_name"), _get(main, "model"), "")
        routed = f"{DIM}\u23f8 manual{RESET} {to_string(shown)}".rstrip(_TRIM_END)
    elif truthy(main):
        routed = main_line(main)
    elif truthy(status):
        routed = main_line(status)

    agents = ""
    if subagents:
        first = subagents[:3]
        names = []
        for a in first:
            color = _color(coalesce(a.get("tier", UNDEFINED), _status_tier_of(a.get("model", UNDEFINED))))
            pause = "\u23f8 " if truthy(a.get("manual", UNDEFINED)) else ""
            name = coalesce(short_name(a.get("model", UNDEFINED)), a.get("tier", UNDEFINED), a.get("model", UNDEFINED), "?")
            names.append(f"{color}{pause}{to_string(name)}{RESET}")
        more = f"{DIM}+{len(subagents) - len(first)}{RESET}" if len(subagents) > len(first) else ""
        agents = f" {DIM}\u00b7{RESET} {marks['agents']} " + f"{DIM},{RESET}".join(n for n in [*names, more] if n)

    newer = read_calibration()["newer"]
    notice = ""
    if newer:
        extra = f" +{len(newer) - 1}" if len(newer) > 1 else ""
        notice = f" {DIM}\u00b7{RESET} \x1b[33mnew {to_string(newer[0])}{extra}: /jev-calibrate{RESET}"

    loc = location_info(data)
    branch = _get(loc, "branch")
    worktree = _get(loc, "worktree")
    branch_part = ""
    if not is_nullish(branch):
        label = branch if truthy(branch) else "(detached)"
        branch_part = f" {DIM}\u00b7{RESET} {marks['branch']} \x1b[34m{_clip(to_string(label), MAX_BRANCH)}{RESET}"
    worktree_part = f" {DIM}\u00b7{RESET} {marks['worktree']} \x1b[32m{to_string(worktree)}{RESET}" if truthy(worktree) else ""
    where = branch_part + worktree_part
    dir_part = f" {DIM}\u00b7{RESET} {marks['dir']} {directory}" if directory and directory != worktree else ""

    return f"{routed}{agents}{dir_part}{where} {DIM}\u00b7{RESET} {marks['context']} {to_string(pct)}%{notice}\n"


# `trimEnd()` strips the JavaScript whitespace set.
from ..jsstr import JSWS_CHARS as _TRIM_END  # noqa: E402


def main() -> int:
    write_stdout(render(read_stdin()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
