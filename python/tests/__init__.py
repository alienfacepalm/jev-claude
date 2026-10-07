"""Test package setup, run before any test module imports `jev_router`.

The status directory is fixed when `jev_router.status` is imported (SPEC 3.9), so it is pointed
at a throwaway directory first, as `node/test/isolate-status.mjs` does: the real one holds live
sessions' decisions, and `pruneStale` would delete any of them older than a week. Then `../src`
goes on `sys.path` so the tests run without installing the package.

The helpers at the end narrow a result the tests are about to look into, and fail the test when
it is not of that shape. Decoded JSON is typed `Any` from there on, as `json.loads` types it.
"""

import atexit
import os
import shutil
import stat
import sys
import tempfile
from collections.abc import Callable
from typing import Any

_STATUS_DIR = tempfile.mkdtemp(prefix="jev-status-test-")
os.environ["JEV_STATUS_DIR"] = _STATUS_DIR
atexit.register(shutil.rmtree, _STATUS_DIR, True)

SRC = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src"))
if SRC not in sys.path:
    sys.path.insert(0, SRC)

REPO_ROOT = os.path.normpath(os.path.join(SRC, "..", ".."))
FIXTURES = os.path.join(REPO_ROOT, "conformance", "fixtures")


def force_rmtree(path: str) -> None:
    """`rmSync(path, {recursive: true, force: true})`, git's read-only object files included."""

    def retry(function: Callable[[str], object], name: str, _error: BaseException) -> None:
        try:
            os.chmod(name, stat.S_IWRITE)
            function(name)
        except OSError:
            pass

    shutil.rmtree(path, onexc=retry)


def present[T](value: T | None) -> T:
    """The value, which must not be None."""
    if value is None:
        raise AssertionError("expected a value, got None")
    return value


def as_dict(value: object) -> dict[str, Any]:
    """The value, which must be a JSON object."""
    if not isinstance(value, dict):
        raise AssertionError(f"expected an object, got {value!r}")
    return value


def as_list(value: object) -> list[Any]:
    """The value, which must be a JSON array."""
    if not isinstance(value, list):
        raise AssertionError(f"expected an array, got {value!r}")
    return value


def as_str(value: object) -> str:
    """The value, which must be a string."""
    if not isinstance(value, str):
        raise AssertionError(f"expected a string, got {value!r}")
    return value
