"""Port of node/test/legend.test.mjs."""

import re
import unittest

from jev_router.icons import icons
from jev_router.legend import format_legend


class Legend(unittest.TestCase):
    def test_the_key_explains_every_mark_the_status_line_draws_in_the_set_it_is_drawing(self) -> None:
        """the key explains every mark the status line draws, in the set it is drawing"""
        for choice in ["symbols", "text"]:
            marks = icons({"JEV_ICONS": choice}, "darwin")
            legend = format_legend(marks)
            for name, mark in marks.items():
                with self.subTest(item=f"{choice}: {name}"):
                    self.assertIn(mark, legend)

    def test_the_symbols_are_the_same_ones_the_status_line_prints(self) -> None:
        """the symbols are the same ones the status line prints"""
        legend = format_legend(icons({"JEV_ICONS": "symbols"}, "darwin"))
        self.assertRegex(legend, re.compile("^✧✦ +the model"))
        self.assertRegex(legend, re.compile(" +the git branch"))


if __name__ == "__main__":
    unittest.main()
