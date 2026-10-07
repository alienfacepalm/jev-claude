"""Home and temp directories follow Node's os.homedir() and os.tmpdir() rules (SPEC 3.8)."""

import os
import subprocess
import sys
import unittest

from jev_router import osdirs


class Temp(unittest.TestCase):
    def test_windows_order_and_trailing_backslash(self) -> None:
        self.assertEqual(osdirs.temp({"TEMP": "C:\\T\\", "TMP": "D:\\x"}, windows=True), "C:\\T")
        self.assertEqual(osdirs.temp({"TEMP": "", "TMP": "D:\\x"}, windows=True), "D:\\x")
        self.assertEqual(osdirs.temp({"TEMP": "C:\\"}, windows=True), "C:\\", "a drive root keeps its backslash")
        self.assertEqual(osdirs.temp({"SystemRoot": "C:\\Windows"}, windows=True), "C:\\Windows\\temp")
        self.assertEqual(osdirs.temp({"windir": "C:\\W"}, windows=True), "C:\\W\\temp")

    def test_posix_order_and_trailing_slash(self) -> None:
        self.assertEqual(osdirs.temp({"TMPDIR": "/var/t/", "TMP": "/x"}, windows=False), "/var/t")
        self.assertEqual(osdirs.temp({"TMPDIR": "", "TMP": "", "TEMP": "/e"}, windows=False), "/e")
        self.assertEqual(osdirs.temp({}, windows=False), "/tmp")
        self.assertEqual(osdirs.temp({"TMPDIR": "/"}, windows=False), "/")

    def test_matches_node_on_this_machine(self) -> None:
        try:
            node = subprocess.run(
                ["node", "-p", "require('os').tmpdir() + '\\n' + require('os').homedir()"],
                capture_output=True,
                text=True,
                timeout=20,
                check=False,  # node's exit status is not under test; its output is compared below
            )
        except OSError:
            self.skipTest("node is not installed")
        tmp, home = node.stdout.strip().split("\n")
        self.assertEqual(osdirs.temp(), tmp)
        self.assertEqual(osdirs.home(), home)


class Home(unittest.TestCase):
    @unittest.skipUnless(sys.platform == "win32", "Windows rule")
    def test_userprofile_wins_and_blank_falls_back_to_the_profile(self) -> None:
        self.assertEqual(osdirs.home({"USERPROFILE": "C:\\Users\\x"}), "C:\\Users\\x")
        self.assertTrue(os.path.isdir(osdirs.home({"USERPROFILE": ""})))

    @unittest.skipIf(sys.platform == "win32", "POSIX rule")
    def test_home_wins_and_blank_falls_back_to_passwd(self) -> None:
        self.assertEqual(osdirs.home({"HOME": "/h"}), "/h")
        self.assertTrue(os.path.isdir(osdirs.home({"HOME": ""})))


if __name__ == "__main__":
    unittest.main()
