"""Debug and failure logging (port of node/src/log.mjs, SPEC 15).

`LOG_FILE` and whether stdout is a terminal are fixed at start (SPEC 3.9).
"""

from __future__ import annotations

import datetime
import os
import sys
import threading

from . import osdirs
from .jsstr import truthy, well_formed

LOG_FILE = os.path.join(osdirs.home(), ".jev-claude.log")


def _isatty() -> bool:
    try:
        return sys.stdout is not None and sys.stdout.isatty()
    except Exception:
        return False


INTERACTIVE = _isatty()

FILE_MODE = 0o600
_tightened = False
_lock = threading.Lock()


def iso_now() -> str:
    """`new Date().toISOString()`."""
    return iso(datetime.datetime.now(datetime.timezone.utc))


def iso(moment: datetime.datetime) -> str:
    moment = moment.astimezone(datetime.timezone.utc)
    return moment.strftime("%Y-%m-%dT%H:%M:%S.") + f"{moment.microsecond // 1000:03d}Z"


def iso_from_ms(ms) -> str:
    moment = datetime.datetime(1970, 1, 1, tzinfo=datetime.timezone.utc) + datetime.timedelta(milliseconds=ms)
    return iso(moment)


def write_stderr(text: str) -> None:
    """Write text to stderr as UTF-8 bytes, whatever the console code page."""
    try:
        data = well_formed(text).encode("utf-8")
        stream = getattr(sys.stderr, "buffer", None)
        if stream is not None:
            sys.stderr.flush()
            stream.write(data)
            stream.flush()
        else:
            sys.stderr.write(text)
    except Exception:
        pass


def log(line: str) -> None:
    global _tightened
    text = f"[jev] {line}\n"
    if not INTERACTIVE:
        write_stderr(text)
        return
    try:
        with _lock:
            fd = os.open(LOG_FILE, os.O_WRONLY | os.O_CREAT | os.O_APPEND | getattr(os, "O_BINARY", 0), FILE_MODE)
            try:
                os.write(fd, well_formed(f"{iso_now()} {text}").encode("utf-8"))
            finally:
                os.close(fd)
            if not _tightened:
                _tightened = True
                os.chmod(LOG_FILE, FILE_MODE)
    except Exception:
        pass


def debug(line: str) -> None:
    if truthy(os.environ.get("JEV_DEBUG")):
        log(line)
