"""Port of node/test/worktree.test.mjs."""

import os
import shutil
import subprocess
import tempfile
import unittest

from jev_router.jsstr import UNDEFINED
from jev_router.worktree import git_branch, location_info

from . import force_rmtree


def never(directory):
    raise AssertionError("the branch should not have been looked up")


class Worktree(unittest.TestCase):
    def test_outside_a_git_checkout_there_is_nothing_to_show(self):
        """outside a git checkout there is nothing to show"""
        self.assertIsNone(location_info({"workspace": {"current_dir": "/nowhere"}}, lambda d: None))
        self.assertIsNone(location_info({}, lambda d: None))
        self.assertIsNone(location_info(UNDEFINED, lambda d: None))

    def test_the_main_working_tree_has_a_branch_and_no_worktree(self):
        """the main working tree has a branch and no worktree"""
        info = {"workspace": {"current_dir": "/repo"}}
        self.assertEqual(location_info(info, lambda d: "master" if d == "/repo" else None), {"branch": "master", "worktree": None})

    def test_a_worktree_session_carries_its_own_name_and_branch_with_no_git_call(self):
        """a worktree session carries its own name and branch, with no git call"""
        info = {"worktree": {"name": "my-feature", "branch": "worktree-my-feature", "path": "/r/.claude/worktrees/my-feature"}}
        self.assertEqual(location_info(info, never), {"branch": "worktree-my-feature", "worktree": "my-feature"})

    def test_a_linked_worktree_has_only_a_name_so_the_branch_is_read_from_git(self):
        """a linked worktree has only a name, so the branch is read from git in the current directory"""
        asked = []
        info = {"workspace": {"current_dir": "/wt/feature-xyz", "git_worktree": "feature-xyz"}}
        result = location_info(info, lambda d: (asked.append(d), "feature/xyz")[1])
        self.assertEqual(result, {"branch": "feature/xyz", "worktree": "feature-xyz"})
        self.assertEqual(asked, ["/wt/feature-xyz"])

    def test_a_worktree_session_without_a_branch_hook_based_falls_back_to_git(self):
        """a worktree session without a branch (hook-based) falls back to git"""
        info = {"worktree": {"name": "scratch", "path": "/wt/scratch"}}
        self.assertEqual(location_info(info, lambda d: "main-2" if d == "/wt/scratch" else None), {"branch": "main-2", "worktree": "scratch"})

    def test_a_detached_head_in_a_worktree_still_names_the_worktree(self):
        """a detached HEAD in a worktree still names the worktree"""
        info = {"workspace": {"current_dir": "/wt/x", "git_worktree": "x"}}
        self.assertEqual(location_info(info, lambda d: ""), {"branch": "", "worktree": "x"})

    def test_git_branch_a_branch_a_detached_head_and_no_checkout(self):
        """gitBranch: a branch, a detached HEAD, and no checkout"""
        directory = os.path.realpath(tempfile.mkdtemp(prefix="jev-worktree-test-"))
        self.addCleanup(force_rmtree, directory)

        def git(*args):
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", *args], cwd=directory,
                           check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        self.assertIsNone(git_branch(directory), "not a repository")
        git("init", "-q", "-b", "topic/a")
        self.assertEqual(git_branch(directory), "topic/a", "works before the first commit")
        git("commit", "-q", "--allow-empty", "-m", "x")
        git("checkout", "-q", "--detach")
        self.assertEqual(git_branch(directory), "", "detached")
        self.assertIsNone(git_branch(UNDEFINED))
        self.assertIsNone(git_branch(os.path.join(directory, "missing")))


if __name__ == "__main__":
    unittest.main()
