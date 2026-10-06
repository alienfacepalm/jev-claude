"""Per-session status files (port of node/src/status.mjs, SPEC 8).

The directory is fixed when this module is imported (SPEC 3.9). Every read-modify-write of a
status file, and every calibration write, holds one process-wide lock (SPEC 3.10).
"""

from __future__ import annotations

import functools
import itertools
import os
import re
import threading
import time

from . import jsjson, osdirs
from .jsjson import js_items, js_keys
from .jsstr import UNDEFINED, coalesce, is_nullish, to_number, truthy

DIR = os.environ.get("JEV_STATUS_DIR") or os.path.join(osdirs.temp(), "jev-claude")
STATUS_DIR = DIR

DIR_MODE = 0o700
FILE_MODE = 0o600

STALE_AFTER_MS = 7 * 24 * 60 * 60 * 1000
_pruned = False

SETTINGS_FILE = os.path.join(DIR, "settings.json")
CALIBRATION_FILE = os.path.join(DIR, "calibration.json")

MAX_AGENTS = 12

LOCK = threading.RLock()
_SEQUENCE = itertools.count()
_dumped = itertools.count()

_UNSAFE = re.compile(r"[^A-Za-z0-9_-]")


def now_ms() -> int:
    """`Date.now()`."""
    return time.time_ns() // 1_000_000


def file_for(session_id) -> str:
    if not isinstance(session_id, str):
        # `sessionId.replace` throws for anything but a string; callers swallow it.
        raise TypeError("sessionId.replace is not a function")
    return os.path.join(DIR, _UNSAFE.sub("", session_id) + ".json")


def ensure_dir() -> None:
    """Creates the status directory, owner-only; errors propagate to the caller."""
    os.makedirs(DIR, mode=DIR_MODE, exist_ok=True)
    os.chmod(DIR, DIR_MODE)


def temp_name(file: str) -> str:
    """A temporary name no other writer in this process or another shares (SPEC 3.10)."""
    return f"{file}.{os.getpid()}.{next(_SEQUENCE)}.tmp"


def _write_new(path: str, data: bytes, mode: int) -> None:
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | getattr(os, "O_BINARY", 0), mode)
    try:
        view = memoryview(data)
        while view:
            written = os.write(fd, view)
            view = view[written:]
    finally:
        os.close(fd)


def _replace(source: str, target: str) -> None:
    # On Windows a reader that opened the target without FILE_SHARE_DELETE blocks the rename for
    # a moment; Node's libuv readers do not, but a Python reader may, so retry briefly.
    for attempt in range(20):
        try:
            os.replace(source, target)
            return
        except PermissionError:
            if os.name != "nt" or attempt == 19:
                raise
            time.sleep(0.01)


def write_private(file: str, text) -> None:
    """Writes `text` to `file` inside the status directory, owner-only, renamed into place."""
    ensure_dir()
    data = text if isinstance(text, bytes) else text.encode("utf-8")
    temp = temp_name(file)
    try:
        _write_new(temp, data, FILE_MODE)
        _replace(temp, file)
    except BaseException:
        try:
            os.unlink(temp)
        except OSError:
            pass
        raise
    os.chmod(file, FILE_MODE)


def write_status(session_id, status) -> None:
    """Publish the latest routing decision so the status line can display it."""
    global _pruned
    if not truthy(session_id):
        return
    try:
        write_private(file_for(session_id), jsjson.dumps_bytes(status))
        if not _pruned:
            _pruned = True
            prune_stale()
    except Exception:
        pass


def read_status(session_id):
    """Latest routing decision for a session, or None."""
    try:
        with open(file_for(session_id), "rb") as handle:
            return jsjson.parse(handle.read())
    except Exception:
        return None


def _get(obj, key):
    if isinstance(obj, dict):
        return obj.get(key, UNDEFINED)
    if obj is None or obj is UNDEFINED:
        return UNDEFINED
    return UNDEFINED


def _prop(obj, key):
    """`obj.key` where `obj` must not be null or undefined (JavaScript throws there)."""
    if obj is None or obj is UNDEFINED:
        raise TypeError(f"Cannot read properties of {obj!r} (reading '{key}')")
    return _get(obj, key)


def _spread(obj) -> dict:
    """`{...obj}` for a JSON value: objects copy their keys, strings their indices, others nothing."""
    if isinstance(obj, dict):
        return {k: obj[k] for k in js_keys(obj)}
    if isinstance(obj, str):
        return {str(i): ch for i, ch in enumerate(obj)}
    if isinstance(obj, list):
        return {str(i): v for i, v in enumerate(obj)}
    return {}


def _defined(entries) -> dict:
    """An object literal: keys whose value is undefined are kept as UNDEFINED (omitted when written)."""
    return dict(entries)


def _at_key(entry) -> float:
    value = coalesce(_get(entry, "at"), 0)
    return to_number(value)


def _newest_first(a, b) -> int:
    diff = _at_key(b) - _at_key(a)
    if diff != diff or diff == 0:
        return 0
    return -1 if diff < 0 else 1


def merge_agent(existing, agent, entry) -> dict:
    """Newest-wins merge of one agent into the map, trimmed to the most recent MAX_AGENTS."""
    agents = _spread(coalesce(existing, {}))
    key = _key(_prop(agent, "key"))
    agents[key] = {**_spread(agents.get(key, UNDEFINED)), **entry}
    keys = js_keys(agents)
    if len(keys) > MAX_AGENTS:
        ordered = [k for k in keys if not truthy(_get(agents[k], "main"))]
        ordered.sort(key=functools.cmp_to_key(lambda a, b: _newest_first(agents[a], agents[b])))
        for stale in ordered[MAX_AGENTS - 1 :]:
            del agents[stale]
    return agents


def _key(value) -> str:
    from .jsstr import to_string

    return to_string(value)


def write_decision(session_id, decision, agent=None) -> None:
    """Publish a routed prompt, retaining recent exact Jev exchanges for diagnosis."""
    with LOCK:
        previous = read_status(session_id)
        if truthy(agent):
            entry = {
                **_spread(decision),
                "agent": {"key": _get(agent, "key"), "label": _get(agent, "label"), "main": _get(agent, "main")},
            }
        else:
            entry = decision
        history_before = coalesce(_get(previous, "history"), [])
        history = (_iterable(history_before) + [entry])[-20:]
        if truthy(agent):
            agents = merge_agent(
                _get(previous, "agents"),
                agent,
                _defined(
                    [
                        ("label", _get(agent, "label")),
                        ("main", _get(agent, "main")),
                        ("tier", _get(decision, "tier")),
                        ("model", _get(decision, "model")),
                        ("confidence", _get(decision, "confidence")),
                        ("effort", _get(decision, "effort")),
                        ("reason", _get(decision, "reason")),
                        ("at", coalesce(_get(decision, "at"), now_ms())),
                    ]
                ),
            )
        else:
            agents = _get(previous, "agents")
        out = {**_spread(decision)}
        if truthy(agents):
            out["agents"] = agents
        out["history"] = history
        write_status(session_id, out)


def _iterable(value) -> list:
    """`[...value]`: arrays and strings iterate; anything else throws."""
    if isinstance(value, list):
        return list(value)
    if isinstance(value, str):
        return list(value)
    raise TypeError("value is not iterable")


def mark_manual(session_id, model, agent=None) -> None:
    """Records that an agent is running a model the user chose rather than a routed one."""
    with LOCK:
        previous = read_status(session_id)
        if truthy(agent):
            agents = merge_agent(
                _get(previous, "agents"),
                agent,
                {
                    "label": _get(agent, "label"),
                    "main": _get(agent, "main"),
                    "model": model,
                    "manual": True,
                    "at": now_ms(),
                },
            )
            manual = True if truthy(_get(agent, "main")) else coalesce(_get(previous, "manual"), False)
        else:
            agents = _get(previous, "agents")
            manual = True
        out = _spread(previous)
        if truthy(agents):
            out["agents"] = agents
        out["manual"] = manual
        out["at"] = now_ms()
        write_status(session_id, out)


def main_decision(status):
    """The main thread's most recent full decision, else the status itself."""
    if not truthy(status):
        return None
    history = coalesce(_get(status, "history"), [])
    for entry in reversed(_iterable(history)):
        if truthy(_get(_get(entry, "agent"), "main")):
            return entry
    return status


def agent_view(status, fresh_ms=90_000, now=None):
    """The main thread's entry and the live sub-agents, newest first."""
    now = now_ms() if now is None else now
    agents = coalesce(_get(status, "agents"), {})
    entries = [{"key": key, **_spread(value)} for key, value in js_items(_spread(agents))]
    main = next((a for a in entries if truthy(a.get("main", UNDEFINED))), None)
    subagents = [
        a for a in entries if not truthy(a.get("main", UNDEFINED)) and to_number(now) - _at_key(a) <= fresh_ms
    ]
    subagents.sort(key=functools.cmp_to_key(_newest_first))
    return {"main": main, "subagents": subagents}


def write_calibration(newer=UNDEFINED, models=UNDEFINED, file=None) -> None:
    """Records the newest model per tier, and which are newer than the router's calibration."""
    file = CALIBRATION_FILE if file is None else file
    newer = [] if newer is UNDEFINED else newer
    models = [] if models is UNDEFINED else models
    with LOCK:
        try:
            write_private(file, jsjson.dumps_bytes({"newer": newer, "models": models, "at": now_ms()}))
        except Exception:
            pass


def read_calibration(file=None) -> dict:
    """What the last model list said; `at` is None when no session has read one yet."""
    file = CALIBRATION_FILE if file is None else file
    try:
        with open(file, "rb") as handle:
            data = jsjson.parse(handle.read())
        newer, models, at = _prop(data, "newer"), _prop(data, "models"), _prop(data, "at")
        known = isinstance(models, list) and isinstance(at, (int, float)) and not isinstance(at, bool)
        return {
            "newer": newer if isinstance(newer, list) else [],
            "models": models if known else [],
            "at": at if known else None,
        }
    except Exception:
        return {"newer": [], "models": [], "at": None}


_TRUE_SETTING = re.compile(r"(1|true|yes)\Z", re.IGNORECASE | re.ASCII)


def dump_body(body, setting=UNDEFINED):
    """Saves a request body for diagnosis (`JEV_DUMP`); returns the file, or None."""
    if setting is UNDEFINED:
        setting = os.environ.get("JEV_DUMP")
    if not truthy(setting):
        return None
    prefix = os.path.join(DIR, "dump") if _TRUE_SETTING.match(setting) else setting
    file = f"{prefix}.{now_ms()}-{next(_dumped)}.json"
    try:
        if prefix.startswith(DIR):
            ensure_dir()
        text = jsjson.stringify(body, 2)
        if text is UNDEFINED:
            raise TypeError("nothing to write")
        _write_new(file, text.encode("utf-8"), FILE_MODE)
        return file
    except Exception:
        return None


def prune_stale(max_age_ms=STALE_AFTER_MS, now=None) -> int:
    """Delete status files untouched for `max_age_ms`."""
    now = now_ms() if now is None else now
    removed = 0
    try:
        names = os.listdir(DIR)
    except Exception:
        return 0
    for name in names:
        if not name.endswith(".json") or name == "settings.json":
            continue
        file = os.path.join(DIR, name)
        try:
            if now - os.stat(file).st_mtime_ns / 1_000_000 > max_age_ms:
                os.unlink(file)
                removed += 1
        except Exception:
            pass
    return removed


__all__ = [
    "DIR", "STATUS_DIR", "SETTINGS_FILE", "CALIBRATION_FILE", "LOCK", "write_private", "write_status",
    "read_status", "write_decision", "mark_manual", "main_decision", "agent_view", "write_calibration",
    "read_calibration", "dump_body", "prune_stale", "ensure_dir", "merge_agent", "is_nullish",
]
