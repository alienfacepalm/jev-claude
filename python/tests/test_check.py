"""jev-check's setup report (port of node/bin/jev-check.mjs), from a real calibration file."""

import json
import os
import unittest

from jev_router.cli.check import report
from jev_router.status import CALIBRATION_FILE, ensure_dir


class Report(unittest.TestCase):
    def write_calibration(self, data: object) -> None:
        ensure_dir()
        with open(CALIBRATION_FILE, "w", encoding="utf-8") as handle:
            json.dump(data, handle)
        self.addCleanup(os.unlink, CALIBRATION_FILE)

    def rows(self, label: str) -> list[str]:
        return [line for line in report(env={}, root=None).split("\n") if line.startswith(label)]

    def test_the_account_rows_join_the_lists_as_array_join_does(self) -> None:
        """Array.prototype.join prints null as nothing, not "null" (SPEC 3.6)."""
        self.write_calibration({"models": [None, "claude-opus-4-1"], "newer": [None, "claude-opus-6"], "at": 0})
        [account] = self.rows("Your account")
        self.assertIn(", claude-opus-4-1 (as of ", account)
        self.assertNotIn("null", account)
        [newer] = self.rows("Newer models")
        self.assertIn(", claude-opus-6 - routing already uses them", newer)
        self.assertNotIn("null", newer)

    def test_no_calibration_yet_says_so(self) -> None:
        [account] = self.rows("Your account")
        self.assertIn("not read yet", account)


if __name__ == "__main__":
    unittest.main()
