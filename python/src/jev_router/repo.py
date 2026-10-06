"""The repository root and release version (SPEC 2.3, 2.4), resolved once at start."""

from __future__ import annotations

import os

from . import jsjson

_MARKER = os.path.join(".claude", "skills", "jev-calibrate", "SKILL.md")


def find_root(start: str | None = None, env=None) -> str | None:
    """`JEV_ROOT` when set and non-empty, else the nearest ancestor of `start` (default this
    package's own file) holding `.claude/skills/jev-calibrate/SKILL.md`, else None."""
    env = os.environ if env is None else env
    value = env.get("JEV_ROOT")
    if value:
        return value
    here = os.path.dirname(os.path.realpath(start or __file__))
    while True:
        if os.path.isfile(os.path.join(here, _MARKER)):
            return here
        parent = os.path.dirname(here)
        if parent == here:
            return None
        here = parent


ROOT = find_root()


def release_version(root: str | None = ROOT) -> str:
    """The root `package.json` version, or `0.0.0` without a root or a readable version."""
    if not root:
        return "0.0.0"
    try:
        with open(os.path.join(root, "package.json"), "rb") as handle:
            version = jsjson.parse(handle.read()).get("version")
        return version if isinstance(version, str) else "0.0.0"
    except Exception:
        return "0.0.0"
