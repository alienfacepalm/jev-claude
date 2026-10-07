"""The repository root is found from the package's own location (SPEC 2.4)."""

import json
import os
import shutil
import tempfile
import unittest

from jev_router import repo

from . import REPO_ROOT, present


class Root(unittest.TestCase):
    def test_found_from_the_package_file(self) -> None:
        self.assertEqual(
            os.path.normcase(present(repo.find_root(env={}))), os.path.normcase(os.path.realpath(REPO_ROOT))
        )

    def test_jev_root_wins_when_set(self) -> None:
        self.assertEqual(repo.find_root(env={"JEV_ROOT": "X:\\elsewhere"}), "X:\\elsewhere")
        self.assertEqual(
            os.path.normcase(present(repo.find_root(env={"JEV_ROOT": ""}))),
            os.path.normcase(os.path.realpath(REPO_ROOT)),
        )

    def test_no_marker_means_no_root(self) -> None:
        base = tempfile.mkdtemp(prefix="jev-repo-")
        self.addCleanup(shutil.rmtree, base, True)
        nested = os.path.join(base, "a", "b")
        os.makedirs(nested)
        start = os.path.join(nested, "module.py")
        open(start, "w").close()
        self.assertIsNone(repo.find_root(start, env={}))
        os.makedirs(os.path.join(base, "a", ".claude", "skills", "jev-calibrate"))
        open(os.path.join(base, "a", ".claude", "skills", "jev-calibrate", "SKILL.md"), "w").close()
        self.assertEqual(
            os.path.normcase(present(repo.find_root(start, env={}))),
            os.path.normcase(os.path.realpath(os.path.join(base, "a"))),
        )

    def test_release_version_reads_the_root_package_json(self) -> None:
        with open(os.path.join(REPO_ROOT, "package.json"), encoding="utf-8") as handle:
            expected = json.load(handle)["version"]
        self.assertEqual(repo.release_version(REPO_ROOT), expected)
        self.assertEqual(repo.release_version(None), "0.0.0")


if __name__ == "__main__":
    unittest.main()
