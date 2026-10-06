"""jev-claude: the launcher (port of node/bin/jev-claude.mjs, SPEC 10)."""

from __future__ import annotations

import atexit
import os
import signal
import sys
import threading

from .. import jsjson, osdirs, repo
from ..config import AUTO_MODEL
from ..env import child_env, load_env
from ..first_run import ask_yes_no, mark_offered, shadows_skill, should_offer, was_offered
from ..jsstr import truthy
from ..launch import launch_spec, resolve_command, spawn_spec
from ..log import INTERACTIVE, LOG_FILE, write_stderr
from ..settings import read_saved_model, restore_saved_model
from ..status import SETTINGS_FILE, write_private

QUESTION = (
    "[jev] First run: check your Jev Router setup now with /jev-calibrate?\n"
    "[jev] It only reads your setup and changes nothing, using a little of your Claude usage. [Y/n] "
)


def auto_model_env() -> dict:
    env = {
        "ANTHROPIC_CUSTOM_MODEL_OPTION": AUTO_MODEL,
        "ANTHROPIC_CUSTOM_MODEL_OPTION_NAME": "Jev Router",
        "ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION": "Route each turn to the cheapest model that can do it",
        "ANTHROPIC_CUSTOM_MODEL_OPTION_SUPPORTED_CAPABILITIES": "thinking,adaptive_thinking,interleaved_thinking,effort,max_effort",
        "CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT": "1",
    }
    if not os.environ.get("ANTHROPIC_MODEL"):
        env["ANTHROPIC_MODEL"] = AUTO_MODEL
    return env


def status_line_command() -> str:
    """How Claude Code runs this port's status line (SPEC 10.1)."""
    return f'"{sys.executable}" -m jev_router.cli.statusline'


def status_line_args() -> list:
    if os.environ.get("JEV_NO_STATUSLINE"):
        return []
    for directory in (os.path.join(os.getcwd(), ".claude"), os.path.join(osdirs.home(), ".claude")):
        try:
            with open(os.path.join(directory, "settings.json"), "rb") as handle:
                settings = jsjson.parse(handle.read())
            if isinstance(settings, dict) and truthy(settings.get("statusLine")):
                return []
        except Exception:
            pass
    command = status_line_command()
    try:
        write_private(SETTINGS_FILE, jsjson.dumps_bytes({"statusLine": {"type": "command", "command": command}}))
    except Exception:
        return []
    return ["--settings", SETTINGS_FILE]


def _isatty(stream) -> bool:
    try:
        return stream is not None and stream.isatty()
    except Exception:
        return False


def main() -> int:
    saved_model_before = read_saved_model()
    load_env()

    args = sys.argv[1:]
    if repo.ROOT:
        args = [*args, "--add-dir", repo.ROOT]
    env = child_env()

    claude = resolve_command("claude")
    if not claude:
        write_stderr(
            "[jev] Claude Code is not installed, or `claude` is not on your PATH.\n"
            "[jev] jev-claude runs the real Claude Code CLI; install it first:\n"
            "[jev]   https://code.claude.com/docs/en/setup\n"
        )
        return 1

    if should_offer(
        args=sys.argv[1:],
        interactive=_isatty(sys.stdin) and _isatty(sys.stdout),
        offered=was_offered(),
        shadowed=shadows_skill(os.getcwd(), repo.ROOT) if repo.ROOT else False,
    ):
        answer = ask_yes_no(QUESTION)
        if answer == "interrupt":
            return 130
        if answer is not None:
            mark_offered(answer)
        if answer is True:
            args.insert(0, "/jev-calibrate check")

    cleanups = []
    if os.environ.get("JEV_API_KEY") or os.environ.get("TYPESAFE_API_KEY"):
        from ..proxy import start_proxy

        inherited = os.environ.get("ANTHROPIC_BASE_URL")
        proxy = start_proxy(upstream_url=inherited) if inherited else start_proxy()
        if inherited and os.environ.get("JEV_DEBUG"):
            write_stderr(f"[jev] upstream {inherited}\n")
        env["ANTHROPIC_BASE_URL"] = f"http://127.0.0.1:{proxy.port}"
        env["CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY"] = "1"
        env.update(auto_model_env())

        def cleanup():
            try:
                proxy.close()
            finally:
                restore_saved_model(saved_model_before)

        cleanups.append(cleanup)
        args.extend(status_line_args())
        if os.environ.get("JEV_DEBUG") and INTERACTIVE:
            write_stderr(f"[jev] routing decisions -> {LOG_FILE}\n")
    else:
        settings_file = os.path.join(osdirs.home(), ".jev-router.env")
        write_stderr(
            "[jev] no JEV_API_KEY found - starting Claude Code without routing\n"
            f"[jev] set it in {settings_file} to enable routing\n"
        )

    ran = threading.Lock()

    def run_cleanups():
        if not ran.acquire(blocking=False):
            return
        for fn in cleanups:
            try:
                fn()
            except Exception:
                pass

    atexit.register(run_cleanups)

    try:
        child = spawn_spec(launch_spec(claude), args, env=env)
    except OSError as error:
        write_stderr(f"[jev] could not start Claude Code: {error.strerror or error}\n")
        run_cleanups()
        return 1

    _install_signal_handlers(child, run_cleanups)

    while True:
        try:
            code = child.wait()
            break
        except KeyboardInterrupt:
            # Claude Code decides what Ctrl+C means; the launcher carries on.
            continue
    run_cleanups()
    # A negative code is a POSIX signal death.
    return 1 if code < 0 else code


def _install_signal_handlers(child, run_cleanups) -> None:
    def forward(signum=None, frame=None):
        try:
            if sys.platform == "win32" or signum is None:
                child.terminate()
            else:
                child.send_signal(signum)
        except Exception:
            pass

        def give_up():
            run_cleanups()
            os._exit(1)

        timer = threading.Timer(5, give_up)
        timer.daemon = True
        timer.start()

    signal.signal(signal.SIGINT, signal.SIG_IGN)
    if sys.platform == "win32":
        if hasattr(signal, "SIGBREAK"):
            signal.signal(signal.SIGBREAK, signal.SIG_IGN)
        signal.signal(signal.SIGTERM, forward)
        _install_console_handler(forward, run_cleanups)
    else:
        for name in ("SIGHUP", "SIGTERM"):
            signal.signal(getattr(signal, name), forward)


_console_handler = None


def _install_console_handler(forward, run_cleanups) -> None:
    """Console close, logoff and shutdown count as SIGHUP/SIGTERM; Ctrl+C and Ctrl+Break as SIGINT."""
    global _console_handler
    try:
        import ctypes
        from ctypes import wintypes

        handler_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.DWORD)

        def handler(event):
            if event in (0, 1):  # CTRL_C_EVENT, CTRL_BREAK_EVENT: Claude Code decides
                return True
            if event in (2, 5, 6):  # CLOSE, LOGOFF, SHUTDOWN
                forward()
                run_cleanups()
                return True
            return False

        _console_handler = handler_type(handler)
        ctypes.windll.kernel32.SetConsoleCtrlHandler(_console_handler, True)
    except Exception:
        pass


if __name__ == "__main__":
    raise SystemExit(main())
