"""The user's saved default model (port of node/src/settings.mjs, SPEC 9.2)."""

from __future__ import annotations

import os

from . import jsjson, osdirs
from .config import AUTO_MODEL
from .jsstr import UNDEFINED, coalesce, is_nullish
from .status import DIR, FILE_MODE, ensure_dir

USER_SETTINGS = os.path.join(osdirs.home(), ".claude", "settings.json")
SAVED_MODEL_MEMO = os.path.join(DIR, "saved-model.json")


def _read_json(file):
    with open(file, "rb") as handle:
        return jsjson.parse(handle.read())


def _model_of(value):
    """`value.model`, which throws for null and undefined."""
    if is_nullish(value):
        raise TypeError("Cannot read properties of null (reading 'model')")
    return value.get("model", UNDEFINED) if isinstance(value, dict) else UNDEFINED


def _memo_of(memo):
    try:
        return _model_of(_read_json(memo))
    except Exception:
        return UNDEFINED


def _write(file, data: bytes, mode=None) -> None:
    flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC | getattr(os, "O_BINARY", 0)
    fd = os.open(file, flags, 0o666 if mode is None else mode)
    try:
        view = memoryview(data)
        while view:
            view = view[os.write(fd, view):]
    finally:
        os.close(fd)


def read_saved_model(file=None, memo=None):
    """The model saved as the user's default (UNDEFINED when unreadable or absent)."""
    file = USER_SETTINGS if file is None else file
    memo = SAVED_MODEL_MEMO if memo is None else memo
    try:
        model = _model_of(_read_json(file))
    except Exception:
        return UNDEFINED
    if model == AUTO_MODEL and isinstance(model, str):
        return _memo_of(memo)
    try:
        if memo == SAVED_MODEL_MEMO:
            ensure_dir()
        _write(memo, jsjson.dumps_bytes({"model": coalesce(model, None)}), FILE_MODE)
    except Exception:
        pass
    return model


def restore_saved_model(previous, file=None) -> bool:
    """Puts `previous` back if the settings file now holds the sentinel."""
    file = USER_SETTINGS if file is None else file
    try:
        settings = _read_json(file)
        if not (isinstance(_model_of(settings), str) and settings["model"] == AUTO_MODEL):
            return False
        if is_nullish(previous):
            del settings["model"]
        else:
            settings["model"] = previous
        _write(file, (jsjson.stringify(settings, 2) + "\n").encode("utf-8"))
        return True
    except Exception:
        return False
