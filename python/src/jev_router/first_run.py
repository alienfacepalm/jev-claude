"""The once-per-user setup-check offer (port of node/src/first-run.mjs, SPEC 11)."""

from __future__ import annotations

import os
import re
import sys

from . import jsjson, osdirs
from .jsstr import js_trim
from .log import iso_now

FIRST_RUN_FILE = os.path.join(osdirs.home(), ".jev-router", "first-run.json")


def was_offered(file=None) -> bool:
    file = FIRST_RUN_FILE if file is None else file
    try:
        with open(file, "rb") as handle:
            handle.read()
        return True
    except Exception:
        return False


def mark_offered(accepted, file=None) -> None:
    file = FIRST_RUN_FILE if file is None else file
    try:
        os.makedirs(os.path.dirname(file), exist_ok=True)
        with open(file, "wb") as handle:
            handle.write(jsjson.dumps_bytes({"offeredAt": iso_now(), "accepted": accepted}))
    except Exception:
        pass


def should_offer(args, interactive, offered, shadowed=False) -> bool:
    return bool(interactive) and not offered and not shadowed and len(args) == 0


def shadows_skill(cwd, root) -> bool:
    """Whether the launch directory defines its own `jev-calibrate` skill."""
    return os.path.abspath(cwd) != os.path.abspath(root) and os.path.exists(
        os.path.join(cwd, ".claude", "skills", "jev-calibrate")
    )


_NO = re.compile(r"(n|no)\Z", re.IGNORECASE)


def ask_yes_no(question, input=None, output=None):
    """True or False for an answer (empty is yes), None when input closes or fails, and
    "interrupt" on Ctrl+C. Always settles."""
    input = sys.stdin if input is None else input
    output = sys.stderr if output is None else output
    try:
        buffer = getattr(output, "buffer", None)
        if buffer is not None:
            output.flush()
            buffer.write(question.encode("utf-8"))
            buffer.flush()
        else:
            output.write(question)
            output.flush()
    except Exception:
        pass
    try:
        line = input.readline()
    except KeyboardInterrupt:
        return "interrupt"
    except Exception:
        return None
    if isinstance(line, bytes):
        line = line.decode("utf-8", "replace")
    if line == "":
        return None
    if line.endswith("\n"):
        line = line[:-1]
        if line.endswith("\r"):
            line = line[:-1]
    return not _NO.match(js_trim(line))
