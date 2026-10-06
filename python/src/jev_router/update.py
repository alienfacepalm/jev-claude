"""Update checks and fast-forward updates (port of node/src/update.mjs, SPEC 14)."""

from __future__ import annotations

import datetime
import os
import re
import subprocess
import sys

from . import jsjson, osdirs
from .jsstr import UNDEFINED, coalesce, is_nullish, js_trim, parse_int, to_string, truthy
from .log import iso_from_ms
from .status import now_ms, temp_name

UPDATE_FILE = os.path.join(osdirs.home(), ".jev-router", "update.json")
CHECK_EVERY_MS = 6 * 60 * 60 * 1000
FETCH_TIMEOUT_S = 20
LOCAL_TIMEOUT_S = 5

_NO_WINDOW = 0x08000000 if sys.platform == "win32" else 0


class GitError(Exception):
    pass


def git(root, args, timeout=LOCAL_TIMEOUT_S) -> str:
    """`git -C <root> ...`, stdout trimmed; raises GitError on failure or timeout."""
    command = ["git", "-C", root, *args]
    env = dict(os.environ)
    env["GIT_TERMINAL_PROMPT"] = "0"
    try:
        out = subprocess.run(
            command,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            env=env,
            creationflags=_NO_WINDOW,
        )
    except subprocess.TimeoutExpired:
        raise GitError(f"Command failed: {' '.join(command)} (timed out)") from None
    except OSError as error:
        raise GitError(f"spawn git {error}") from None
    if out.returncode != 0:
        stderr = out.stderr.decode("utf-8", "replace")
        raise GitError(f"Command failed: {' '.join(command)}\n{stderr}")
    return js_trim(out.stdout.decode("utf-8", "replace"))


def read_state(file=None):
    file = UPDATE_FILE if file is None else file
    try:
        with open(file, "rb") as handle:
            state = jsjson.parse(handle.read())
        return state if isinstance(state, (dict, list)) else None
    except Exception:
        return None


def write_state(state, file=None) -> None:
    file = UPDATE_FILE if file is None else file
    try:
        os.makedirs(os.path.dirname(file), exist_ok=True)
        temp = temp_name(file)
        with open(temp, "wb") as handle:
            handle.write(jsjson.dumps_bytes(state))
        os.replace(temp, file)
    except Exception:
        pass


_ISO = re.compile(r"(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})\.(\d{3})Z\Z", re.ASCII)


def parse_iso(text):
    """Milliseconds for a `toISOString()` string, or None for anything else."""
    if not isinstance(text, str):
        return None
    match = _ISO.match(text)
    if not match:
        return None
    year, month, day, hour, minute, second, millis = (int(g) for g in match.groups())
    try:
        moment = datetime.datetime(year, month, day, hour, minute, second, tzinfo=datetime.timezone.utc)
    except ValueError:
        return None
    epoch = datetime.datetime(1970, 1, 1, tzinfo=datetime.timezone.utc)
    return (moment - epoch) // datetime.timedelta(milliseconds=1) + millis


def _get(obj, key):
    return obj.get(key, UNDEFINED) if isinstance(obj, dict) else UNDEFINED


def is_check_due(state, now=None, every_ms=CHECK_EVERY_MS) -> bool:
    """Whether the last check is old enough (or missing, unreadable, or in the future)."""
    now = now_ms() if now is None else now
    at = parse_iso(coalesce(_get(state, "checkedAt"), ""))
    return at is None or now - at >= every_ms or at > now


def compare_versions(a, b) -> float:
    """Dotted release numbers compared as numbers; pre-release tags ignored."""

    def parts(value):
        out = []
        for piece in to_string(value).split("-")[0].split("."):
            number = parse_int(piece)
            out.append(0.0 if number != number or number == 0 else number)
        return out

    x, y = parts(a), parts(b)
    for i in range(max(len(x), len(y))):
        diff = (x[i] if i < len(x) else 0) - (y[i] if i < len(y) else 0)
        if diff != 0:
            return diff
    return 0


def update_notice(state, current_version):
    if not truthy(_get(state, "available")) or not truthy(_get(state, "latest")) or not truthy(current_version):
        return None
    latest = state["latest"]
    if compare_versions(latest, current_version) <= 0:
        return None
    return f"[jev] Update available: {to_string(current_version)} -> {to_string(latest)}. Run `jev-claude --update`."


def installed_version(root):
    try:
        with open(os.path.join(root, "package.json"), "rb") as handle:
            data = jsjson.parse(handle.read())
        if is_nullish(data):
            return None
        return coalesce(_get(data, "version"), None)
    except Exception:
        return None


def _canonical(path: str) -> str:
    real = os.path.realpath(os.path.abspath(path), strict=True)
    real = real.replace("\\", "/")
    return real.lower() if sys.platform == "win32" else real


def inspect_clone(root) -> dict:
    """Whether `root` is a clean clone this tool may fast-forward; never raises."""
    try:
        top = git(root, ["rev-parse", "--show-toplevel"])
        if _canonical(top) != _canonical(root):
            return {"ok": False, "reason": "this folder is not a git clone of its own"}
    except Exception:
        return {"ok": False, "reason": "this folder is not a git clone"}
    try:
        branch = git(root, ["symbolic-ref", "--short", "HEAD"])
    except Exception:
        return {"ok": False, "reason": "the checkout is not on a branch"}
    try:
        if git(root, ["status", "--porcelain", "--untracked-files=no"]):
            return {"ok": False, "reason": "there are local changes in the checkout"}
        git(root, ["fetch", "--quiet", "origin", branch], FETCH_TIMEOUT_S)
        head = git(root, ["rev-parse", "HEAD"])
        remote = git(root, ["rev-parse", "FETCH_HEAD"])
        if head == remote:
            return {"ok": True, "branch": branch, "head": head, "remote": remote, "behind": False}

        def is_ancestor(older, newer):
            try:
                git(root, ["merge-base", "--is-ancestor", older, newer])
                return True
            except Exception:
                return False

        if is_ancestor("FETCH_HEAD", "HEAD"):
            return {"ok": True, "branch": branch, "head": head, "remote": remote, "behind": False}
        if not is_ancestor("HEAD", "FETCH_HEAD"):
            return {"ok": False, "reason": f"local {branch} has commits that origin/{branch} does not"}
        return {"ok": True, "branch": branch, "head": head, "remote": remote, "behind": True}
    except Exception as error:
        return {"ok": False, "reason": f"could not reach origin ({str(error).split(chr(10))[0]})"}


def _version_at(root, ref):
    try:
        data = jsjson.parse(git(root, ["show", f"{ref}:package.json"]))
        if is_nullish(data):
            return None
        return coalesce(_get(data, "version"), None)
    except Exception:
        return None


def check_for_update(root, now=None) -> dict:
    """One update check, as the state file records it."""
    now = now_ms() if now is None else now
    checked_at = iso_from_ms(now)
    clone = inspect_clone(root)
    if not clone["ok"] or not clone["behind"]:
        return {"checkedAt": checked_at, "available": False}
    latest = _version_at(root, "FETCH_HEAD")
    return {"checkedAt": checked_at, "available": truthy(latest), "latest": latest, "remote": clone["remote"]}


def needs_install(changed_files) -> bool:
    """Only the lockfile means the dependencies need installing again."""
    return "pnpm-lock.yaml" in changed_files


def apply_update(root, install=None) -> dict:
    """Fast-forwards the clone and, when the lockfile changed, runs `install(root)`."""
    clone = inspect_clone(root)
    if not clone["ok"]:
        return {"status": "refused", "reason": clone["reason"]}
    source = installed_version(root)
    if not clone["behind"]:
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
    except Exception as error:
        return {"status": "failed", "reason": str(error).split("\n")[0], "from": source}
