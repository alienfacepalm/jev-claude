"""Home and temp directories by Node's rules (SPEC 3.8).

`os.homedir()` and `os.tmpdir()` differ from `pathlib.Path.home()` and `tempfile.gettempdir()`,
and every implementation must agree on where the shared files live, so these never use the
platform helpers.
"""

from __future__ import annotations

import os
import sys

WINDOWS = sys.platform == "win32"


def _env(env, name):
    value = env.get(name)
    return value if value else None


def _windows_profile_dir() -> str | None:
    try:
        import ctypes
        from ctypes import wintypes

        advapi32 = ctypes.WinDLL("advapi32", use_last_error=True)
        userenv = ctypes.WinDLL("userenv", use_last_error=True)
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        token = wintypes.HANDLE()
        TOKEN_READ = 0x20008
        kernel32.GetCurrentProcess.restype = wintypes.HANDLE
        if not advapi32.OpenProcessToken(kernel32.GetCurrentProcess(), TOKEN_READ, ctypes.byref(token)):
            return None
        try:
            size = wintypes.DWORD(0)
            userenv.GetUserProfileDirectoryW(token, None, ctypes.byref(size))
            buffer = ctypes.create_unicode_buffer(size.value or 260)
            if not userenv.GetUserProfileDirectoryW(token, buffer, ctypes.byref(size)):
                return None
            return buffer.value
        finally:
            kernel32.CloseHandle(token)
    except Exception:
        return None


def home(env=None) -> str:
    """`os.homedir()`."""
    env = os.environ if env is None else env
    if WINDOWS:
        value = _env(env, "USERPROFILE")
        if value:
            return value
        return _windows_profile_dir() or os.path.expanduser("~")
    value = _env(env, "HOME")
    if value:
        return value
    try:
        import pwd

        return pwd.getpwuid(os.getuid()).pw_dir
    except Exception:
        return "/"


def temp(env=None, windows: bool | None = None) -> str:
    """`os.tmpdir()`."""
    env = os.environ if env is None else env
    windows = WINDOWS if windows is None else windows
    if windows:
        path = _env(env, "TEMP") or _env(env, "TMP")
        if not path:
            # Node concatenates whatever it has, `undefined` included.
            root = _env(env, "SystemRoot") or _env(env, "windir") or "undefined"
            path = root + "\\temp"
        if len(path) > 1 and path.endswith("\\") and not path.endswith(":\\"):
            path = path[:-1]
        return path
    path = _env(env, "TMPDIR") or _env(env, "TMP") or _env(env, "TEMP") or "/tmp"
    if len(path) > 1 and path.endswith("/"):
        path = path[:-1]
    return path
