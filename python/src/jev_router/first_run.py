"""The once-per-user setup-check offer (port of node/src/first-run.mjs, SPEC 11)."""

from __future__ import annotations

import os
import re
import sys
from collections.abc import Sequence
from typing import Literal, Protocol

from . import jsjson, osdirs
from .jsstr import js_trim
from .log import iso_now

FIRST_RUN_FILE = os.path.join(osdirs.home(), ".jev-router", "first-run.json")


class LineSource(Protocol):
    """Where an answer is read from: a text or binary stream (sys.stdin by default)."""

    def readline(self) -> str | bytes:
        """One line, with its newline; empty at end of input."""
        ...


class TextSink(Protocol):
    """Where the question is written: a text stream (sys.stderr by default)."""

    def write(self, text: str, /) -> object:
        """Writes text."""
        ...

    def flush(self) -> object:
        """Flushes what was written."""
        ...


def was_offered(file: str | None = None) -> bool:
    """Whether the setup check has been offered on this machine before."""
    file = FIRST_RUN_FILE if file is None else file
    try:
        with open(file, "rb") as handle:
            handle.read()
        return True
    except Exception:  # noqa: BLE001 - missing or unreadable both mean "not offered yet", as in Node
        return False


def mark_offered(accepted: bool, file: str | None = None) -> None:
    """Records that the offer was made and the answer."""
    file = FIRST_RUN_FILE if file is None else file
    try:
        os.makedirs(os.path.dirname(file), exist_ok=True)
        with open(file, "wb") as handle:
            handle.write(jsjson.dumps_bytes({"offeredAt": iso_now(), "accepted": accepted}))
    except Exception:  # noqa: BLE001 - unwritable home directory: the offer comes back next time, which is harmless
        pass


def should_offer(args: Sequence[str], interactive: object, offered: bool, shadowed: bool = False) -> bool:
    """Whether to offer the setup check: interactive, first time, no arguments, skill not shadowed."""
    return bool(interactive) and not offered and not shadowed and len(args) == 0


def shadows_skill(cwd: str, root: str) -> bool:
    """Whether the launch directory defines its own `jev-calibrate` skill."""
    return os.path.abspath(cwd) != os.path.abspath(root) and os.path.exists(
        os.path.join(cwd, ".claude", "skills", "jev-calibrate")
    )


_NO = re.compile(r"(n|no)\Z", re.IGNORECASE | re.ASCII)


def ask_yes_no(
    question: str,
    input: LineSource | None = None,  # noqa: A002 - mirrors Node's askYesNo(question, {input, output})
    output: TextSink | None = None,
) -> bool | Literal["interrupt"] | None:
    """Asks a yes/no question and always settles.

    True or False for an answer (empty is yes), None when input closes or fails, and
    "interrupt" on Ctrl+C.
    """
    source: LineSource = sys.stdin if input is None else input
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
    except Exception:  # noqa: BLE001 - a question that cannot be shown still waits for its answer
        pass
    try:
        raw: str | bytes = source.readline()
    except KeyboardInterrupt:
        return "interrupt"
    except Exception:  # noqa: BLE001 - input that fails is input that closed: the question settles as None
        return None
    line = raw.decode("utf-8", "replace") if isinstance(raw, bytes) else raw
    if line == "":
        return None
    if line.endswith("\n"):
        line = line[:-1].removesuffix("\r")
    return not _NO.match(js_trim(line))
