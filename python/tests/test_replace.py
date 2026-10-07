"""Replacing a file (SPEC 3.11; `node/test/status.test.mjs`): the rename retries while another
process holds the target open, and a rename that never lands removes the temporary file."""

from __future__ import annotations

import os
import tempfile
import threading
import time
import unittest
from collections.abc import Iterator
from contextlib import contextmanager

from jev_router.status import replace_over


def _write(path: str, text: str) -> None:
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(text)


def _read(path: str) -> str:
    with open(path, encoding="utf-8") as handle:
        return handle.read()


def _leftovers(directory: str) -> list[str]:
    return [name for name in os.listdir(directory) if name.endswith(".tmp")]


@contextmanager
def _held_open(path: str) -> Iterator[object]:
    """Opens `path` with read sharing but no delete sharing, as antivirus, the indexer and a plain
    reader do on Windows, so nothing can be renamed over it until the handle is closed."""
    import ctypes  # noqa: PLC0415 - Windows only
    from ctypes import wintypes  # noqa: PLC0415 - Windows only

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)  # type: ignore[attr-defined,unused-ignore]
    kernel32.CreateFileW.restype = wintypes.HANDLE
    kernel32.CreateFileW.argtypes = [
        wintypes.LPCWSTR,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.LPVOID,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.HANDLE,
    ]
    generic_read, share_read, open_existing = 0x80000000, 0x1, 3
    handle = kernel32.CreateFileW(path, generic_read, share_read, None, open_existing, 0x80, None)
    if handle == wintypes.HANDLE(-1).value:
        raise OSError(ctypes.get_last_error(), "CreateFileW failed")  # type: ignore[attr-defined,unused-ignore]

    class Holder:
        closed = False

        def close(self) -> None:
            if not self.closed:
                self.closed = True
                kernel32.CloseHandle(handle)

    holder = Holder()
    try:
        yield holder
    finally:
        holder.close()


class TestReplaceOver(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.dir = self._tmp.name
        self.file = os.path.join(self.dir, "a.json")
        self.temp = os.path.join(self.dir, "a.json.1.tmp")

    def test_replaces_an_existing_file(self) -> None:
        _write(self.file, "old")
        _write(self.temp, "new")
        replace_over(self.temp, self.file)
        self.assertEqual(_read(self.file), "new")
        self.assertEqual(_leftovers(self.dir), [])

    def test_a_missing_temp_file_is_an_error(self) -> None:
        with self.assertRaises(OSError):
            replace_over(self.temp, self.file)

    @unittest.skipUnless(os.name == "nt", "only Windows refuses to rename over an open file")
    def test_lands_while_another_handle_briefly_holds_the_file_open(self) -> None:
        _write(self.file, "old")
        with _held_open(self.file) as held:
            _write(self.temp, "new")
            timer = threading.Timer(0.05, held.close)  # type: ignore[attr-defined]
            timer.start()
            try:
                replace_over(self.temp, self.file)
            finally:
                timer.join()
        self.assertEqual(_read(self.file), "new")
        self.assertEqual(_leftovers(self.dir), [])

    @unittest.skipUnless(os.name == "nt", "only Windows refuses to rename over an open file")
    def test_a_file_that_stays_held_is_an_error_and_removes_the_temp_file(self) -> None:
        _write(self.file, "old")
        with _held_open(self.file):
            _write(self.temp, "secret prompt")
            started = time.monotonic()
            with self.assertRaises(PermissionError):
                replace_over(self.temp, self.file)
            self.assertGreater(time.monotonic() - started, 0.1, "it retried before giving up")
            self.assertEqual(_leftovers(self.dir), [], "the temp file held prompt text and must be gone")
        self.assertEqual(_read(self.file), "old", "the held file kept its previous content")


if __name__ == "__main__":
    unittest.main()
