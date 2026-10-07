"""Port of node/test/model-names.test.mjs."""

import unittest

from jev_router.jsstr import UNDEFINED
from jev_router.model_names import short_name


class ModelNames(unittest.TestCase):
    def test_a_model_id_reads_as_its_family_and_version(self) -> None:
        """a model id reads as its family and version"""
        self.assertEqual(short_name("claude-opus-5-5"), "Opus 5.5")
        self.assertEqual(short_name("claude-sonnet-5-5"), "Sonnet 5.5")
        self.assertEqual(short_name("claude-fable-5-1"), "Fable 5.1")
        self.assertEqual(short_name("claude-opus-6"), "Opus 6", "a whole-number release")
        self.assertEqual(short_name("claude-sonnet-5-10"), "Sonnet 5.10")

    def test_a_date_suffix_or_a_context_tag_is_not_part_of_the_version(self) -> None:
        """a date suffix or a context tag is not part of the version"""
        self.assertEqual(short_name("claude-haiku-4-5-20251001"), "Haiku 4.5")
        self.assertEqual(short_name("claude-opus-4-6[1m]"), "Opus 4.6")

    def test_anything_that_is_not_a_claude_model_id_has_no_short_name(self) -> None:
        """anything that is not a Claude model id has no short name"""
        self.assertIsNone(short_name("mystery-9"))
        self.assertIsNone(short_name("jev-router"))
        self.assertIsNone(short_name(UNDEFINED))


if __name__ == "__main__":
    unittest.main()
