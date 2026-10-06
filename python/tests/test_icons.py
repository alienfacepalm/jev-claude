"""Port of node/test/icons.test.mjs."""

import json
import unittest

from jev_router.icons import icons


def is_text(marks):
    return marks["dir"] == "dir"


class Icons(unittest.TestCase):
    def test_symbols_everywhere_but_the_legacy_windows_console(self):
        """symbols everywhere but the legacy Windows console"""
        self.assertFalse(is_text(icons({}, "darwin")))
        self.assertFalse(is_text(icons({}, "linux")))
        self.assertTrue(is_text(icons({}, "win32")), "conhost fonts lack the glyphs")

    def test_a_windows_terminal_that_announces_itself_gets_symbols(self):
        """a Windows terminal that announces itself gets symbols"""
        for env in [{"WT_SESSION": "1"}, {"TERM_PROGRAM": "vscode"}, {"TERM_PROGRAM": "mintty"}, {"ConEmuPID": "42"}]:
            self.assertFalse(is_text(icons(env, "win32")), json.dumps(env))

    def test_jev_icons_overrides_the_guess_in_both_directions(self):
        """JEV_ICONS overrides the guess in both directions"""
        self.assertTrue(is_text(icons({"JEV_ICONS": "text"}, "darwin")))
        self.assertTrue(is_text(icons({"JEV_ICONS": "ASCII"}, "darwin")))
        self.assertFalse(is_text(icons({"JEV_ICONS": "symbols"}, "win32")))

    def test_every_item_has_a_symbol_and_a_word(self):
        """every item has a symbol and a word"""
        keys = list(icons({"JEV_ICONS": "text"}, "darwin").keys())
        self.assertEqual(list(icons({"JEV_ICONS": "symbols"}, "darwin").keys()), keys)
        for choice in ["text", "symbols"]:
            for name, label in icons({"JEV_ICONS": choice}, "darwin").items():
                self.assertTrue(label, f"{choice}.{name}")


if __name__ == "__main__":
    unittest.main()
