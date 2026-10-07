"""Port of node/test/launcher.test.mjs: the real jev-claude launcher, run against a stand-in `claude`."""

import json
import os
import re
import stat
import subprocess
import sys
import tempfile
import unittest
from collections.abc import Sequence

from jev_router.launch import is_claude_subcommand

from . import REPO_ROOT, SRC, as_dict, as_list, as_str, force_rmtree

# The launcher finds the repository root from its own location, as the Node one does.
ROOT = os.path.realpath(REPO_ROOT)

# What the stand-in prints: the arguments and the variables that say whether a session was set up.
FAKE_CLAUDE = """process.stdout.write(JSON.stringify({
  args: process.argv.slice(2),
  env: Object.fromEntries(
    ["ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL", "JEV_API_KEY"].map((k) => [k, process.env[k] ?? null]),
  ),
}));
"""


def fake_claude(directory: str) -> None:
    """Puts a `claude` the launcher finds on PATH into `directory`.

    An npm-style `.cmd` shim on Windows (the form the launcher runs directly through node), an
    executable script elsewhere.
    """
    script = os.path.join(directory, "claude-fake.mjs")
    with open(script, "w", encoding="utf-8", newline="") as handle:
        handle.write(FAKE_CLAUDE)
    if sys.platform == "win32":
        with open(os.path.join(directory, "claude.cmd"), "w", encoding="utf-8", newline="") as handle:
            handle.write('@ECHO off\r\n"node"  "%~dp0\\claude-fake.mjs" %*\r\n')
    else:
        file = os.path.join(directory, "claude")
        with open(file, "w", encoding="utf-8", newline="") as handle:
            handle.write(f"#!/usr/bin/env node\n{FAKE_CLAUDE}")
        os.chmod(file, os.stat(file).st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def run_launcher(
    args: Sequence[str], base: str, home: str, cwd: str, remove: Sequence[str], extra: dict[str, str]
) -> subprocess.CompletedProcess[str]:
    """Runs the real launcher on `args` with the stand-in `claude` first on PATH."""
    env = {
        **os.environ,
        "PATH": f"{os.path.join(base, 'bin')}{os.pathsep}{os.environ.get('PATH', '')}",
        "HOME": home,
        "USERPROFILE": home,
        "PYTHONPATH": SRC,
        **extra,
    }
    for name in remove:
        env.pop(name, None)
    return subprocess.run(
        [sys.executable, "-m", "jev_router.cli.claude", *args],
        cwd=cwd,
        env=env,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=60,
        check=False,  # the exit code is asserted by the callers, with the output as the message
    )


def launch(test: unittest.TestCase, args: Sequence[str]) -> tuple[list[str], dict[str, str | None], str]:
    """Runs the real launcher in an empty home and working directory, with a Jev key set."""
    base = os.path.realpath(tempfile.mkdtemp(prefix="jev-launcher-"))
    try:
        home = os.path.join(base, "home")
        cwd = os.path.join(base, "cwd")
        for directory in (os.path.join(base, "bin"), home, cwd):
            os.mkdir(directory)
        fake_claude(os.path.join(base, "bin"))
        out = run_launcher(
            args,
            base,
            home,
            cwd,
            ["ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL", "TYPESAFE_API_KEY", "JEV_NO_STATUSLINE", "JEV_ROOT"],
            {"JEV_API_KEY": "test-key", "JEV_STATUS_DIR": os.path.join(base, "status")},
        )
        test.assertEqual(out.returncode, 0, f"{out.stdout}\n{out.stderr}")
        seen = as_dict(json.loads(out.stdout))
        env = as_dict(seen["env"])
        return [as_str(a) for a in as_list(seen["args"])], dict(env), out.stderr
    finally:
        force_rmtree(base)


class Launcher(unittest.TestCase):
    def test_only_the_first_argument_spelled_exactly_makes_a_claude_subcommand(self) -> None:
        """only the first argument, spelled exactly, makes a claude subcommand"""
        for name in ["mcp", "plugin", "plugins", "doctor", "update", "upgrade", "auth", "agents", "kill", "stop"]:
            self.assertTrue(is_claude_subcommand([name, "x"]), name)
        self.assertFalse(is_claude_subcommand([]))
        self.assertFalse(is_claude_subcommand(["update the docs"]), "a prompt that starts with the word")
        self.assertFalse(is_claude_subcommand(["-p", "mcp"]), "the word later on")
        self.assertFalse(is_claude_subcommand(["MCP"]))
        self.assertFalse(is_claude_subcommand(["--model", "opus"]))

    def test_a_session_launch_gets_add_dir_the_settings_file_and_the_proxy(self) -> None:
        """a session launch gets --add-dir, the settings file and the proxy"""
        args, env, _ = launch(self, [])
        self.assertEqual(args[-4], "--add-dir")
        self.assertEqual(args[-3], ROOT)
        self.assertEqual(args[-2], "--settings")
        self.assertRegex(as_str(env["ANTHROPIC_BASE_URL"]), r"^http://127\.0\.0\.1:\d+$")
        self.assertEqual(env["ANTHROPIC_MODEL"], "jev-router")
        self.assertIsNone(env["JEV_API_KEY"], "the key is the launcher's alone")

    def test_a_prompt_that_starts_with_a_subcommands_name_is_still_a_session(self) -> None:
        """a prompt that starts with a subcommand's name is still a session"""
        args, env, _ = launch(self, ["update the docs"])
        self.assertEqual(args[0], "update the docs")
        self.assertIn("--add-dir", args)
        self.assertRegex(as_str(env["ANTHROPIC_BASE_URL"]), r"^http:")

    def test_claude_subcommands_run_untouched_no_add_dir_no_proxy_no_routing_model_no_key(self) -> None:
        """claude subcommands run untouched: no --add-dir, no proxy, no routing model, no key"""
        for want in [["mcp", "list"], ["plugin", "install", "x", "--scope", "user"], ["doctor"], ["update"]]:
            args, env, _ = launch(self, want)
            label = " ".join(want)
            self.assertEqual(args, want, label)
            self.assertIsNone(env["ANTHROPIC_BASE_URL"], label)
            self.assertIsNone(env["ANTHROPIC_MODEL"], label)
            self.assertIsNone(env["JEV_API_KEY"], "the key is stripped from the subcommand's environment too")

    def test_a_subcommand_without_a_key_does_not_announce_that_routing_is_off(self) -> None:
        """a subcommand without a key does not announce that routing is off"""
        # No key: a subcommand is not a session, so there is nothing to say about routing.
        base = os.path.realpath(tempfile.mkdtemp(prefix="jev-launcher-nokey-"))
        try:
            os.mkdir(os.path.join(base, "bin"))
            fake_claude(os.path.join(base, "bin"))
            out = run_launcher(
                ["mcp", "list"],
                base,
                base,
                base,
                ["JEV_API_KEY", "TYPESAFE_API_KEY", "ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL", "JEV_ROOT"],
                {"JEV_STATUS_DIR": os.path.join(base, "status")},
            )
            self.assertEqual(out.returncode, 0, out.stderr)
            self.assertIsNone(re.search(r"no JEV_API_KEY", out.stderr), out.stderr)
            self.assertEqual(as_dict(json.loads(out.stdout))["args"], ["mcp", "list"])
        finally:
            force_rmtree(base)


if __name__ == "__main__":
    unittest.main()
