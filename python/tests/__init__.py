"""Test package setup, run before any test module imports `jev_router`.

The status directory is fixed when `jev_router.status` is imported (SPEC 3.9), so it is pointed
at a throwaway directory first, as `node/test/isolate-status.mjs` does: the real one holds live
sessions' decisions, and `pruneStale` would delete any of them older than a week. Then `../src`
goes on `sys.path` so the tests run without installing the package.
"""

import atexit
import os
import shutil
import sys
import tempfile

_STATUS_DIR = tempfile.mkdtemp(prefix="jev-status-test-")
os.environ["JEV_STATUS_DIR"] = _STATUS_DIR
atexit.register(shutil.rmtree, _STATUS_DIR, True)

SRC = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src"))
if SRC not in sys.path:
    sys.path.insert(0, SRC)

REPO_ROOT = os.path.normpath(os.path.join(SRC, "..", ".."))
FIXTURES = os.path.join(REPO_ROOT, "conformance", "fixtures")
