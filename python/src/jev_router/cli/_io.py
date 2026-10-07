"""Byte-exact console output: UTF-8 whatever the console code page (the harness compares bytes)."""

from __future__ import annotations

import sys

from ..jsstr import well_formed


def write_stdout(text: str) -> None:
    """Writes text to stdout as UTF-8 bytes, lone surrogates replaced, and flushes."""
    data = well_formed(text).encode("utf-8")
    stream = getattr(sys.stdout, "buffer", None)
    if stream is None:
        sys.stdout.write(text)
        sys.stdout.flush()
        return
    sys.stdout.flush()
    stream.write(data)
    stream.flush()


def read_stdin() -> bytes:
    """All of stdin as bytes (empty when there is no stdin)."""
    stream = getattr(sys.stdin, "buffer", None)
    if stream is None:
        return (sys.stdin.read() if sys.stdin else "").encode("utf-8")
    data: bytes = stream.read()
    return data
