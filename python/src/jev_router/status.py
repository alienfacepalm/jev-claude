"""Per-session status files (port of node/src/status.mjs, SPEC 8).

The directory is fixed when this module is imported (SPEC 3.9). Every read-modify-write of a
status file, and every calibration write, holds one process-wide lock (SPEC 3.10).
"""

from __future__ import annotations

import functools
import itertools
import math
import os
import re
import threading
import time
from collections.abc import Iterable, Mapping
from typing import Final, TypedDict

from . import jsjson, osdirs
from .jsjson import js_items, js_keys
from .jsstr import UNDEFINED, JsObject, JsValue, coalesce, coalesce_js, to_number, to_string, truthy

DIR: Final = os.environ.get("JEV_STATUS_DIR") or os.path.join(osdirs.temp(), "jev-claude")
STATUS_DIR: Final = DIR

DIR_MODE: Final = 0o700
FILE_MODE: Final = 0o600

STALE_AFTER_MS: Final = 7 * 24 * 60 * 60 * 1000
_pruned = False

SETTINGS_FILE: Final = os.path.join(DIR, "settings.json")
CALIBRATION_FILE: Final = os.path.join(DIR, "calibration.json")

MAX_AGENTS: Final = 12

LOCK: Final = threading.RLock()
_SEQUENCE: Final = itertools.count()
_dumped: Final = itertools.count()

_UNSAFE: Final = re.compile(r"[^A-Za-z0-9_-]")


class AgentView(TypedDict):
    """The main thread's agent entry, and the live sub-agents newest first."""

    main: JsObject | None
    subagents: list[JsObject]


class Calibration(TypedDict):
    """What the last model list said (`at` is None when no session has read one yet)."""

    newer: list[JsValue]
    models: list[JsValue]
    at: float | None


def now_ms() -> int:
    """`Date.now()`."""
    return time.time_ns() // 1_000_000


def file_for(session_id: object) -> str:
    """The status file of a session; raises TypeError for a non-string id, as Node's `.replace` does."""
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


_REPLACE_ATTEMPTS: Final = 10
_REPLACE_PAUSE_S: Final = 0.02


def replace_over(temp: str, file: str) -> None:
    """Renames `temp` over `file`, retrying briefly while Windows reports the target as in use.

    Antivirus, the search indexer and any reader that opened the file without delete sharing make
    the rename fail for a moment (SPEC 3.11). Any other error, or one that outlasts the retries
    (about 180 ms), removes `temp` and is raised: the temporary file can hold prompt text, and
    nothing else ever cleans it up.
    """
    for attempt in range(1, _REPLACE_ATTEMPTS + 1):
        try:
            os.replace(temp, file)
            return
        except PermissionError:
            if os.name == "nt" and attempt < _REPLACE_ATTEMPTS:
                time.sleep(_REPLACE_PAUSE_S)
                continue
            _remove_quietly(temp)
            raise
        except BaseException:
            _remove_quietly(temp)
            raise


def _remove_quietly(path: str) -> None:
    try:
        os.unlink(path)
    except OSError:
        pass


def write_private(file: str, text: str | bytes) -> None:
    """Writes `text` to `file` inside the status directory, owner-only, renamed into place."""
    ensure_dir()
    data = text if isinstance(text, bytes) else text.encode("utf-8")
    temp = temp_name(file)
    try:
        _write_new(temp, data, FILE_MODE)
    except BaseException:
        _remove_quietly(temp)
        raise
    replace_over(temp, file)
    os.chmod(file, FILE_MODE)


def write_status(session_id: object, status: object) -> None:
    """Publish the latest routing decision so the status line can display it."""
    global _pruned  # noqa: PLW0603 - stale files are pruned once per process (SPEC 8)
    if not truthy(session_id):
        return
    try:
        write_private(file_for(session_id), jsjson.dumps_bytes(status))
        if not _pruned:
            _pruned = True
            prune_stale()
    except Exception:  # noqa: BLE001 - status display is cosmetic and must never interfere with a request
        pass


def read_status(session_id: object) -> JsValue | None:
    """Latest routing decision for a session, or None."""
    try:
        with open(file_for(session_id), "rb") as handle:
            return jsjson.parse(handle.read())
    except Exception:  # noqa: BLE001 - missing, unreadable or malformed all mean "no decision yet"
        return None


def _get(obj: object, key: str) -> JsValue:
    """`obj?.key` over JSON values: undefined for anything that is not an object."""
    if isinstance(obj, dict):
        value: JsValue = obj.get(key, UNDEFINED)
        return value
    return UNDEFINED


def _prop(obj: object, key: str) -> JsValue:
    """`obj.key` where `obj` must not be null or undefined (JavaScript throws there)."""
    if obj is None or obj is UNDEFINED:
        raise TypeError(f"Cannot read properties of {obj!r} (reading '{key}')")
    return _get(obj, key)


def _spread(obj: object) -> JsObject:
    """`{...obj}` for a JSON value: objects copy their keys, strings their indices, others nothing."""
    if isinstance(obj, dict):
        return {k: obj[k] for k in js_keys(obj)}
    if isinstance(obj, str):
        return {str(i): ch for i, ch in enumerate(obj)}
    if isinstance(obj, list):
        return {str(i): v for i, v in enumerate(obj)}
    return {}


def _defined(entries: Iterable[tuple[str, JsValue]]) -> JsObject:
    """An object literal: keys whose value is undefined are kept as UNDEFINED (omitted when written)."""
    return dict(entries)


def _at_key(entry: object) -> float:
    value = coalesce(_get(entry, "at"), 0)
    return to_number(value)


def _newest_first(a: object, b: object) -> int:
    diff = _at_key(b) - _at_key(a)
    if math.isnan(diff) or diff == 0:
        return 0
    return -1 if diff < 0 else 1


def merge_agent(existing: object, agent: object, entry: Mapping[str, JsValue]) -> JsObject:
    """Newest-wins merge of one agent into the map, trimmed to the most recent MAX_AGENTS."""
    agents = _spread(coalesce(existing, {}))
    key = _key(_prop(agent, "key"))
    agents[key] = {**_spread(agents.get(key, UNDEFINED)), **entry}
    keys = js_keys(agents)
    if len(keys) > MAX_AGENTS:
        ordered = [k for k in keys if not truthy(_get(agents[k], "main"))]

        def by_newest(a: str, b: str) -> int:
            return _newest_first(agents[a], agents[b])

        ordered.sort(key=functools.cmp_to_key(by_newest))
        for stale in ordered[MAX_AGENTS - 1 :]:
            del agents[stale]
    return agents


def _key(value: object) -> str:
    return to_string(value)


def write_decision(session_id: object, decision: JsObject, agent: object = None) -> None:
    """Publish a routed prompt, retaining recent exact Jev exchanges for diagnosis."""
    with LOCK:
        previous = read_status(session_id)
        entry: JsObject
        if truthy(agent):
            entry = {
                **_spread(decision),
                "agent": {"key": _get(agent, "key"), "label": _get(agent, "label"), "main": _get(agent, "main")},
            }
        else:
            entry = decision
        history_before = coalesce(_get(previous, "history"), [])
        history: list[JsValue] = [*_iterable(history_before), entry][-20:]
        agents: JsValue
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
                        ("at", coalesce_js(_get(decision, "at"), now_ms())),
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


def _iterable(value: object) -> list[JsValue]:
    """`[...value]`: arrays and strings iterate; anything else throws."""
    if isinstance(value, list):
        return list(value)
    if isinstance(value, str):
        return list(value)
    raise TypeError("value is not iterable")


def mark_manual(session_id: object, model: JsValue, agent: object = None) -> None:
    """Records that an agent is running a model the user chose rather than a routed one."""
    with LOCK:
        previous = read_status(session_id)
        agents: JsValue
        manual: JsValue
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
            manual = True if truthy(_get(agent, "main")) else coalesce_js(_get(previous, "manual"), False)
        else:
            agents = _get(previous, "agents")
            manual = True
        out = _spread(previous)
        if truthy(agents):
            out["agents"] = agents
        out["manual"] = manual
        out["at"] = now_ms()
        write_status(session_id, out)


def main_decision(status: JsValue) -> JsValue:
    """The main thread's most recent full decision, else the status itself."""
    if not truthy(status):
        return None
    history = coalesce(_get(status, "history"), [])
    for entry in reversed(_iterable(history)):
        if truthy(_get(_get(entry, "agent"), "main")):
            return entry
    return status


def agent_view(status: object, fresh_ms: float = 90_000, now: float | None = None) -> AgentView:
    """The main thread's entry and the live sub-agents, newest first."""
    now = now_ms() if now is None else now
    agents = coalesce(_get(status, "agents"), {})
    entries: list[JsObject] = [{"key": key, **_spread(value)} for key, value in js_items(_spread(agents))]
    main = next((a for a in entries if truthy(a.get("main", UNDEFINED))), None)
    subagents = [a for a in entries if not truthy(a.get("main", UNDEFINED)) and to_number(now) - _at_key(a) <= fresh_ms]
    subagents.sort(key=functools.cmp_to_key(_newest_first))
    return {"main": main, "subagents": subagents}


def write_calibration(newer: JsValue = UNDEFINED, models: JsValue = UNDEFINED, file: str | None = None) -> None:
    """Records the newest model per tier, and which are newer than the router's calibration."""
    file = CALIBRATION_FILE if file is None else file
    newer = [] if newer is UNDEFINED else newer
    models = [] if models is UNDEFINED else models
    with LOCK:
        try:
            write_private(file, jsjson.dumps_bytes({"newer": newer, "models": models, "at": now_ms()}))
        except Exception:  # noqa: BLE001 - a missed notice is cosmetic and must never interfere with a request
            pass


def read_calibration(file: str | None = None) -> Calibration:
    """What the last model list said; `at` is None when no session has read one yet."""
    file = CALIBRATION_FILE if file is None else file
    try:
        with open(file, "rb") as handle:
            data = jsjson.parse(handle.read())
        newer, models, at = _prop(data, "newer"), _prop(data, "models"), _prop(data, "at")
        known = isinstance(models, list) and isinstance(at, (int, float)) and not isinstance(at, bool)
        return {
            "newer": newer if isinstance(newer, list) else [],
            "models": models if known and isinstance(models, list) else [],
            "at": at if known and isinstance(at, (int, float)) else None,
        }
    except Exception:  # noqa: BLE001 - no calibration yet, or an unreadable one, reads as "none"
        return {"newer": [], "models": [], "at": None}


_TRUE_SETTING: Final = re.compile(r"(1|true|yes)\Z", re.IGNORECASE | re.ASCII)


def dump_body(body: object, setting: object = UNDEFINED) -> str | None:
    """Saves a request body for diagnosis (`JEV_DUMP`); returns the file, or None."""
    if setting is UNDEFINED:
        setting = os.environ.get("JEV_DUMP")
    if not truthy(setting):
        return None
    # Node tests and interpolates the setting as a string.
    text_setting = to_string(setting)
    prefix = os.path.join(DIR, "dump") if _TRUE_SETTING.match(text_setting) else text_setting
    file = f"{prefix}.{now_ms()}-{next(_dumped)}.json"
    try:
        if prefix.startswith(DIR):
            ensure_dir()
        text = jsjson.stringify(body, 2)
        if not isinstance(text, str):
            raise TypeError("nothing to write")
        _write_new(file, text.encode("utf-8"), FILE_MODE)
        return file
    except Exception:  # noqa: BLE001 - a dump is diagnostic only and must never interfere with a request
        return None


def prune_stale(max_age_ms: float = STALE_AFTER_MS, now: float | None = None) -> int:
    """Delete status files untouched for `max_age_ms`."""
    now = now_ms() if now is None else now
    removed = 0
    try:
        names = os.listdir(DIR)
    except Exception:  # noqa: BLE001 - missing or unreadable directory: nothing to prune
        return 0
    for name in names:
        if not name.endswith(".json") or name == "settings.json":
            continue
        file = os.path.join(DIR, name)
        try:
            if now - os.stat(file).st_mtime_ns / 1_000_000 > max_age_ms:
                os.unlink(file)
                removed += 1
        except Exception:  # noqa: BLE001 - another session may have removed or replaced it; ignore
            pass
    return removed


__all__ = [
    "CALIBRATION_FILE",
    "DIR",
    "LOCK",
    "SETTINGS_FILE",
    "STATUS_DIR",
    "AgentView",
    "Calibration",
    "agent_view",
    "dump_body",
    "ensure_dir",
    "main_decision",
    "mark_manual",
    "merge_agent",
    "prune_stale",
    "read_calibration",
    "read_status",
    "write_calibration",
    "write_decision",
    "write_private",
    "write_status",
]
