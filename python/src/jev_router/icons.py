"""Labels for the status line's items (port of node/src/icons.mjs, SPEC 12.1)."""

from __future__ import annotations

import os
import sys

SYMBOLS = {
    "model": "\u2727\u2726",
    "effort": "\u25d4",
    "agents": "\u2726",
    "dir": "\u2750",
    "branch": "\ue0a0",
    "worktree": "\u2302",
    "context": "\u2261",
}

TEXT = {
    "model": "model",
    "effort": "effort",
    "agents": "agents",
    "dir": "dir",
    "branch": "branch",
    "worktree": "worktree",
    "context": "ctx",
}


def icons(env=None, platform=None):
    env = os.environ if env is None else env
    platform = sys.platform if platform is None else platform
    choice = (env.get("JEV_ICONS") or "").lower()
    if choice == "symbols":
        return SYMBOLS
    if choice in ("text", "ascii"):
        return TEXT
    modern = env.get("WT_SESSION") or env.get("TERM_PROGRAM") or env.get("ConEmuPID")
    return TEXT if platform == "win32" and not modern else SYMBOLS
