"""Port of node/test/statusline.test.mjs: the real jev-statusline program, run as Claude Code runs it."""

import json
import os
import re
import subprocess
import sys
import tempfile
import unittest

from jev_router.status import now_ms, write_decision

from . import SRC, force_rmtree

MAIN = {"key": "main", "label": "main", "main": True}
PID = os.getpid()


def render(test, session_id, workspace=None, extra=None, icons="text"):
    """Runs the status line with stdin JSON and returns its text without colours."""
    payload = {
        "session_id": session_id,
        "workspace": {"current_dir": "/work/proj", **(workspace or {})},
        "context_window": {"used_percentage": 8},
        **(extra or {}),
    }
    env = {**os.environ, "JEV_ICONS": icons, "PYTHONPATH": SRC}
    out = subprocess.run([sys.executable, "-m", "jev_router.cli.statusline"], input=json.dumps(payload).encode("utf-8"),
                         capture_output=True, env=env, timeout=60)
    test.assertEqual(out.returncode, 0, out.stderr.decode("utf-8", "replace"))
    return re.sub(r"\x1b\[[0-9;]*m", "", out.stdout.decode("utf-8")).strip()


class StatusLine(unittest.TestCase):
    def test_symbols_replace_the_words_and_the_branch_uses_the_powerline_glyph(self):
        """symbols replace the words, and the branch uses the Powerline glyph"""
        sid = f"statusline-symbols-{PID}"
        write_decision(sid, {"tier": "sonnet", "model": "claude-sonnet-5-5", "confidence": 0.94, "effort": "high", "reason": "jev", "at": now_ms()}, MAIN)
        line = render(self, sid, {}, {"worktree": {"name": "login-fix", "branch": "fix/login"}}, "symbols")
        self.assertEqual(line, "◆ Sonnet 5.5 (94%) · ◔ high · ❐ proj ·  fix/login · ⌂ login-fix · ≡ 8%")

    def test_shows_the_effort_the_turn_ran_at_next_to_the_model_and_confidence(self):
        """shows the effort the turn ran at next to the model and confidence"""
        sid = f"statusline-effort-{PID}"
        write_decision(sid, {"tier": "sonnet", "model": "claude-sonnet-5-5", "confidence": 0.94, "effort": "high", "reason": "jev", "at": now_ms()}, MAIN)
        self.assertEqual(render(self, sid), "model Sonnet 5.5 (94%) · effort high · dir proj · ctx 8%")

    def test_shows_a_higher_effort_when_claude_code_asked_for_one(self):
        """shows a higher effort when Claude Code asked for one"""
        sid = f"statusline-xhigh-{PID}"
        write_decision(sid, {"tier": "opus", "model": "claude-opus-5-5", "confidence": 0.91, "effort": "xhigh", "reason": "jev", "at": now_ms()}, MAIN)
        self.assertRegex(render(self, sid), r"^model Opus 5.5 \(91%\) · effort xhigh · ")

    def test_says_nothing_about_effort_for_haiku_which_takes_none(self):
        """says nothing about effort for Haiku, which takes none"""
        sid = f"statusline-haiku-{PID}"
        write_decision(sid, {"tier": "haiku", "model": "claude-haiku-4-5-20251001", "confidence": 0.97, "effort": None, "reason": "jev", "at": now_ms()}, MAIN)
        line = render(self, sid)
        self.assertRegex(line, r"^model Haiku 4.5 \(97%\) · dir proj")
        self.assertNotIn("effort", line)

    def test_a_session_recorded_before_effort_was_tracked_still_renders(self):
        """a session recorded before effort was tracked still renders"""
        sid = f"statusline-old-{PID}"
        write_decision(sid, {"tier": "sonnet", "model": "claude-sonnet-5-5", "confidence": 0.8, "reason": "jev", "at": now_ms()}, MAIN)
        line = render(self, sid)
        self.assertRegex(line, r"^model Sonnet 5.5 \(80%\)")
        self.assertNotIn("effort", line)

    def test_inside_a_worktree_the_branch_and_the_worktree_are_each_named(self):
        """inside a worktree, the branch and the worktree are each named"""
        line = render(self, f"statusline-worktree-{PID}", {}, {"worktree": {"name": "login-fix", "branch": "fix/login"}})
        self.assertRegex(line, r" · dir proj · branch fix/login · worktree login-fix · ctx 8%$")

    def test_a_worktree_named_like_the_directory_is_not_said_twice(self):
        """a worktree named like the directory is not said twice"""
        line = render(self, f"statusline-samename-{PID}", {"current_dir": "/work/COR-1", "git_worktree": "COR-1"})
        self.assertRegex(line, r" · worktree COR-1 · ctx 8%$")
        self.assertNotIn("dir", line)

    def test_a_long_branch_name_is_cut_with_an_ellipsis(self):
        """a long branch name is cut with an ellipsis"""
        branch = "COR-1263/multi-edit-inspection-sync"
        line = render(self, f"statusline-longbranch-{PID}", {}, {"worktree": {"name": "wt", "branch": branch}})
        self.assertRegex(line, " · branch COR-1263/multi-edit-inspect… · ")
        self.assertNotIn("inspection-sync", line)

    def test_a_linked_worktree_whose_branch_cannot_be_read_still_names_the_worktree(self):
        """a linked worktree whose branch cannot be read still names the worktree"""
        line = render(self, f"statusline-linked-{PID}", {"git_worktree": "scratch"})
        self.assertRegex(line, r" · dir proj · worktree scratch · ")
        self.assertNotIn("branch", line)

    def test_the_main_working_tree_shows_its_branch_and_no_worktree(self):
        """the main working tree shows its branch and no worktree"""
        directory = os.path.realpath(tempfile.mkdtemp(prefix="jev-statusline-repo-"))
        self.addCleanup(force_rmtree, directory)
        subprocess.run(["git", "init", "-q", "-b", "main-line"], cwd=directory, check=True)
        line = render(self, f"statusline-main-{PID}", {"current_dir": directory})
        self.assertRegex(line, r" · branch main-line · ctx 8%$")
        self.assertNotIn("worktree", line)

    def test_a_directory_that_is_not_a_git_checkout_shows_neither_a_branch_nor_a_worktree(self):
        """a directory that is not a git checkout shows neither a branch nor a worktree"""
        self.assertNotRegex(render(self, f"statusline-nogit-{PID}"), r"branch|worktree")

    def test_a_space_separates_the_sub_agents_symbol_from_the_first_model_name(self):
        """a space separates the sub-agents symbol from the first model name"""
        sid = f"statusline-agents-{PID}"
        write_decision(sid, {"tier": "sonnet", "model": "claude-sonnet-5-5", "confidence": 0.94, "effort": "high", "reason": "jev", "at": now_ms()}, MAIN)
        write_decision(sid, {"tier": "haiku", "model": "claude-haiku-4-5-20251001", "confidence": 0.9, "reason": "jev", "at": now_ms()}, {"key": "a1", "label": "a1"})
        self.assertRegex(render(self, sid, {}, {}, "symbols"), "✦ Haiku 4\\.5")


if __name__ == "__main__":
    unittest.main()
