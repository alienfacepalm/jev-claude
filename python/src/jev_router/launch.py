"""Finding and starting the real `claude` (port of node/src/launch.mjs, SPEC 10.2)."""

from __future__ import annotations

import os
import re
import subprocess
import sys
from collections.abc import Iterable, Sequence
from typing import Any, NotRequired, TypedDict

from .jsstr import to_string

WIN = sys.platform == "win32"

_QUOTES = re.compile(r'^"|"\Z')


class LaunchSpec(TypedDict):
    """How to start an executable: `command` and leading arguments, plus `shim` for the cmd route."""

    command: str
    prefix: list[str]
    shim: NotRequired[str]


def resolve_command(
    name: str, exts: Sequence[str] | None = None, path: str | None = None, win: bool = WIN
) -> str | None:
    """Finds an executable on PATH, or None."""
    path = os.environ.get("PATH", "") if path is None else path
    if win:
        pathext = os.environ.get("PATHEXT")
        suffixes = exts if exts is not None else (".COM;.EXE;.BAT;.CMD" if pathext is None else pathext).split(";")
    else:
        suffixes = [""]
    for directory in path.split(";" if win else ":"):
        if not directory:
            continue
        for ext in suffixes:
            file = os.path.normpath(os.path.join(_QUOTES.sub("", directory), f"{name}{ext}"))
            if win:
                if os.path.exists(file):
                    return file
            elif os.access(file, os.X_OK):
                return file
    return None


_SHIM = re.compile(r'"%~?dp0%?\\([^"]+?\.[cm]?js)"', re.IGNORECASE | re.ASCII)


def shim_script(file: str) -> str | None:
    """The Node script an npm `.cmd` shim launches, or None."""
    try:
        with open(file, "rb") as handle:
            text = handle.read().decode("utf-8", "replace")
        match = _SHIM.search(text)
        if not match:
            return None
        script = os.path.normpath(os.path.join(os.path.dirname(file), *match.group(1).split("\\")))
        if not os.path.exists(script):
            return None
        return script
    except Exception:  # noqa: BLE001 - an unreadable shim is simply not one this launcher can see through
        return None


_META = re.compile(r'([()\][%!^"`<>&|;, *?])')


def _caret(text: str) -> str:
    return _META.sub(lambda m: "^" + m.group(1), text)


def quote_for_cmd(arg: object) -> str:
    """Quotes one argument for a command line cmd.exe parses and a batch file re-parses."""
    quoted = re.sub(r'(\\*)"', lambda m: m.group(1) * 2 + '\\"', to_string(arg))
    quoted = re.sub(r"(\\*)\Z", lambda m: m.group(1) * 2, quoted, count=1)
    return _caret(_caret(f'"{quoted}"'))


def launch_spec(file: str) -> LaunchSpec:
    """How to start a resolved executable: `{command, prefix}` plus `shim` for the cmd route."""
    if re.search(r"\.ps1\Z", file, re.IGNORECASE | re.ASCII):
        return {"command": "powershell.exe", "prefix": ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", file]}
    if re.search(r"\.(cmd|bat)\Z", file, re.IGNORECASE | re.ASCII):
        script = shim_script(file)
        node = resolve_command("node") if script else None
        if script and node:
            return {"command": node, "prefix": [script]}
        # Node reads `process.env.ComSpec`; Windows environment names are case-insensitive.
        return {"command": os.environ.get("ComSpec") or "cmd.exe", "prefix": [], "shim": file}  # noqa: SIM112
    return {"command": file, "prefix": []}


def command_line(spec: LaunchSpec, args: Iterable[object]) -> str:
    """The verbatim command line of the cmd.exe route."""
    line = " ".join([_caret(spec.get("shim", "")), *(quote_for_cmd(a) for a in args)])
    return f'{spec["command"]} /d /s /c "{line}"'


# `options` are subprocess.Popen's own keyword arguments, passed through untouched.
def spawn_spec(spec: LaunchSpec, args: Sequence[str], **options: Any) -> subprocess.Popen[bytes]:
    """Starts a launch spec with `args`, never through an implicit shell."""
    if not spec.get("shim"):
        return subprocess.Popen([spec["command"], *spec["prefix"], *args], shell=False, **options)
    # A string command with shell=False reaches CreateProcess verbatim (SPEC 10.2).
    return subprocess.Popen(command_line(spec, args), shell=False, **options)
