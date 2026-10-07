"""Update checks and fast-forward updates (port of node/src/update.mjs, SPEC 14)."""

from __future__ import annotations

import datetime
import math
import os
import re
import subprocess
import sys
from collections.abc import Callable, Sequence
from typing import NotRequired, TypedDict

from . import jsjson, osdirs
from .jsstr import UNDEFINED, JsObject, JsValue, coalesce_js, is_nullish, js_trim, parse_int, to_string, truthy
from .log import iso_from_ms
from .status import now_ms, replace_over, temp_name

UPDATE_FILE = os.path.join(osdirs.home(), ".jev-router", "update.json")
CHECK_EVERY_MS = 6 * 60 * 60 * 1000
FETCH_TIMEOUT_S = 20
LOCAL_TIMEOUT_S = 5

_NO_WINDOW = 0x08000000 if sys.platform == "win32" else 0


class GitError(Exception):
    """A git command that failed, timed out or could not start."""


class Clone(TypedDict):
    """What `inspect_clone` found: `ok` with the refs, or not `ok` with a reason."""

    ok: bool
    reason: NotRequired[str]
    branch: NotRequired[str]
    head: NotRequired[str]
    remote: NotRequired[str]
    behind: NotRequired[bool]


def git(root: str, args: Sequence[str], timeout: float = LOCAL_TIMEOUT_S) -> str:
    """`git -C <root> ...`, stdout trimmed; raises GitError on failure or timeout."""
    command = ["git", "-C", root, *args]
    env = dict(os.environ)
    env["GIT_TERMINAL_PROMPT"] = "0"
    try:
        out = subprocess.run(
            command,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=timeout,
            env=env,
            creationflags=_NO_WINDOW,
            check=False,  # the exit code is checked below, with git's stderr in the error
        )
    except subprocess.TimeoutExpired:
        raise GitError(f"Command failed: {' '.join(command)} (timed out)") from None
    except OSError as error:
        raise GitError(f"spawn git {error}") from None
    if out.returncode != 0:
        stderr = out.stderr.decode("utf-8", "replace")
        raise GitError(f"Command failed: {' '.join(command)}\n{stderr}")
    return js_trim(out.stdout.decode("utf-8", "replace"))


def read_state(file: str | None = None) -> JsValue:
    """The last update check's record, or None when there is none or it is unreadable."""
    file = UPDATE_FILE if file is None else file
    try:
        with open(file, "rb") as handle:
            state = jsjson.parse(handle.read())
        return state if isinstance(state, (dict, list)) else None
    except Exception:  # noqa: BLE001 - no record, or an unreadable one, means "never checked"
        return None


def write_state(state: object, file: str | None = None) -> None:
    """Saves an update check's record, renamed into place; errors are swallowed."""
    file = UPDATE_FILE if file is None else file
    try:
        os.makedirs(os.path.dirname(file), exist_ok=True)
        temp = temp_name(file)
        try:
            with open(temp, "wb") as handle:
                handle.write(jsjson.dumps_bytes(state))
        except BaseException:
            # Never leave `<file>.<pid>.<seq>.tmp` behind, whatever stopped the write.
            try:
                os.unlink(temp)
            except OSError:
                pass
            raise
        replace_over(temp, file)
    except Exception:  # noqa: BLE001 - the update check is best-effort and must never fail the launcher
        pass


_ISO = re.compile(r"(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})\.(\d{3})Z\Z", re.ASCII)


def parse_iso(text: object) -> int | None:
    """Milliseconds for a `toISOString()` string, or None for anything else."""
    if not isinstance(text, str):
        return None
    match = _ISO.match(text)
    if not match:
        return None
    year, month, day, hour, minute, second, millis = (int(g) for g in match.groups())
    try:
        moment = datetime.datetime(year, month, day, hour, minute, second, tzinfo=datetime.UTC)
    except ValueError:
        return None
    epoch = datetime.datetime(1970, 1, 1, tzinfo=datetime.UTC)
    return (moment - epoch) // datetime.timedelta(milliseconds=1) + millis


def _get(obj: object, key: str) -> JsValue:
    if isinstance(obj, dict):
        value: JsValue = obj.get(key, UNDEFINED)
        return value
    return UNDEFINED


def is_check_due(state: object, now: float | None = None, every_ms: float = CHECK_EVERY_MS) -> bool:
    """Whether the last check is old enough (or missing, unreadable, or in the future)."""
    now = now_ms() if now is None else now
    at = parse_iso(coalesce_js(_get(state, "checkedAt"), ""))
    return at is None or now - at >= every_ms or at > now


def compare_versions(a: object, b: object) -> float:
    """Dotted release numbers compared as numbers; pre-release tags ignored."""

    def parts(value: object) -> list[float]:
        out: list[float] = []
        for piece in to_string(value).split("-")[0].split("."):
            number = parse_int(piece)
            out.append(0.0 if math.isnan(number) or number == 0 else number)
        return out

    x, y = parts(a), parts(b)
    for i in range(max(len(x), len(y))):
        diff = (x[i] if i < len(x) else 0) - (y[i] if i < len(y) else 0)
        if diff != 0:
            return diff
    return 0


def update_notice(state: object, current_version: object) -> str | None:
    """The launcher's one-line notice when the last check found a newer release, else None."""
    if not truthy(_get(state, "available")) or not truthy(_get(state, "latest")) or not truthy(current_version):
        return None
    latest = _get(state, "latest")
    if compare_versions(latest, current_version) <= 0:
        return None
    return f"[jev] Update available: {to_string(current_version)} -> {to_string(latest)}. Run `jev-claude --update`."


def installed_version(root: str) -> JsValue:
    """The `version` in a checkout's package.json, or None."""
    try:
        with open(os.path.join(root, "package.json"), "rb") as handle:
            data = jsjson.parse(handle.read())
        if is_nullish(data):
            return None
        return coalesce_js(_get(data, "version"), None)
    except Exception:  # noqa: BLE001 - no package.json, or an unreadable one, means no known version
        return None


def _canonical(path: str) -> str:
    real = os.path.realpath(os.path.abspath(path), strict=True)
    real = real.replace("\\", "/")
    return real.lower() if sys.platform == "win32" else real


def inspect_clone(root: str) -> Clone:
    """Whether `root` is a clean clone this tool may fast-forward; never raises."""
    try:
        top = git(root, ["rev-parse", "--show-toplevel"])
        if _canonical(top) != _canonical(root):
            return {"ok": False, "reason": "this folder is not a git clone of its own"}
    except Exception:  # noqa: BLE001 - no git, no repository or an unresolvable path: not a clone
        return {"ok": False, "reason": "this folder is not a git clone"}
    try:
        branch = git(root, ["symbolic-ref", "--short", "HEAD"])
    except Exception:  # noqa: BLE001 - a detached HEAD fails here, and so does anything else: no branch
        return {"ok": False, "reason": "the checkout is not on a branch"}
    try:
        if git(root, ["status", "--porcelain", "--untracked-files=no"]):
            return {"ok": False, "reason": "there are local changes in the checkout"}
        git(root, ["fetch", "--quiet", "origin", branch], FETCH_TIMEOUT_S)
        head = git(root, ["rev-parse", "HEAD"])
        remote = git(root, ["rev-parse", "FETCH_HEAD"])
        if head == remote:
            return {"ok": True, "branch": branch, "head": head, "remote": remote, "behind": False}

        def is_ancestor(older: str, newer: str) -> bool:
            try:
                git(root, ["merge-base", "--is-ancestor", older, newer])
                return True
            except Exception:  # noqa: BLE001 - git exits 1 for "not an ancestor"; any failure reads the same
                return False

        if is_ancestor("FETCH_HEAD", "HEAD"):
            return {"ok": True, "branch": branch, "head": head, "remote": remote, "behind": False}
        if not is_ancestor("HEAD", "FETCH_HEAD"):
            return {"ok": False, "reason": f"local {branch} has commits that origin/{branch} does not"}
        return {"ok": True, "branch": branch, "head": head, "remote": remote, "behind": True}
    except Exception as error:  # noqa: BLE001 - reported in the result: this function never raises
        return {"ok": False, "reason": f"could not reach origin ({str(error).split(chr(10))[0]})"}


def _version_at(root: str, ref: str) -> JsValue:
    try:
        data = jsjson.parse(git(root, ["show", f"{ref}:package.json"]))
        if is_nullish(data):
            return None
        return coalesce_js(_get(data, "version"), None)
    except Exception:  # noqa: BLE001 - no package.json at that ref, or an unreadable one: version unknown
        return None


def check_for_update(root: str, now: float | None = None) -> JsObject:
    """One update check, as the state file records it."""
    now = now_ms() if now is None else now
    checked_at = iso_from_ms(now)
    clone = inspect_clone(root)
    if not clone["ok"] or not clone.get("behind"):
        return {"checkedAt": checked_at, "available": False}
    latest = _version_at(root, "FETCH_HEAD")
    return {"checkedAt": checked_at, "available": truthy(latest), "latest": latest, "remote": clone.get("remote")}


def needs_install(changed_files: Sequence[str]) -> bool:
    """Only the lockfile means the dependencies need installing again."""
    return "pnpm-lock.yaml" in changed_files


def apply_update(root: str, install: Callable[[str], object] | None = None) -> JsObject:
    """Fast-forwards the clone and, when the lockfile changed, runs `install(root)`."""
    clone = inspect_clone(root)
    if not clone["ok"]:
        return {"status": "refused", "reason": clone.get("reason")}
    source = installed_version(root)
    if not clone.get("behind"):
        return {"status": "current", "version": source}
    try:
        changed = git(root, ["diff", "--name-only", "HEAD", "FETCH_HEAD"]).split("\n")
        git(root, ["merge", "--ff-only", "FETCH_HEAD"], FETCH_TIMEOUT_S)
        target = installed_version(root)
        if needs_install(changed):
            code = install(root) if install is not None else UNDEFINED
            if truthy(code):
                return {
                    "status": "failed",
                    "reason": f"installing dependencies exited with {to_string(code)}",
                    "from": source,
                    "to": target,
                }
        return {"status": "updated", "from": source, "to": target}
    except Exception as error:  # noqa: BLE001 - reported in the result, as Node's catch does
        return {"status": "failed", "reason": str(error).split("\n")[0], "from": source}
