"""Where a session is working (port of node/src/worktree.mjs, SPEC 12.2)."""

from __future__ import annotations

import subprocess
import sys
from collections.abc import Callable

from .jsstr import UNDEFINED, coalesce, js_trim, truthy

_NO_WINDOW = 0x08000000 if sys.platform == "win32" else 0


def git_branch(directory: object) -> str | None:
    """The branch checked out in `directory`, "" when detached, None outside a checkout."""
    if not truthy(directory):
        return None
    if not isinstance(directory, str):
        return None  # Node's spawn rejects a non-string cwd, and the catch turns that into null
    try:
        out = subprocess.run(
            ["git", "branch", "--show-current"],
            cwd=directory,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            timeout=1,
            creationflags=_NO_WINDOW,
            check=False,  # a failing git means "not a checkout", read from the exit code below
        )
    except Exception:  # noqa: BLE001 - no git, a missing directory or a timeout all mean "not a checkout"
        return None
    if out.returncode != 0:
        return None
    return js_trim(out.stdout.decode("utf-8", "replace"))


def _get(obj: object, key: str) -> object:
    """`obj?.key`: undefined for anything that is not an object holding the key."""
    if isinstance(obj, dict):
        return obj.get(key, UNDEFINED)
    return UNDEFINED


def location_info(info: object, branch_of: Callable[[object], object] = git_branch) -> dict[str, object] | None:
    """`{branch, worktree}`, or None outside a git checkout."""
    worktree = coalesce(_get(_get(info, "worktree"), "name"), _get(_get(info, "workspace"), "git_worktree"), None)
    directory = coalesce(
        _get(_get(info, "workspace"), "current_dir"), _get(info, "cwd"), _get(_get(info, "worktree"), "path")
    )
    branch = coalesce(_get(_get(info, "worktree"), "branch"), UNDEFINED)
    if branch is UNDEFINED or branch is None:
        branch = branch_of(directory)
    if worktree is None and branch is None:
        return None
    return {"branch": branch, "worktree": worktree}
