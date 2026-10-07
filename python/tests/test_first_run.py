"""Port of node/test/first-run.test.mjs."""

import io
import os
import shutil
import tempfile
import unittest

from jev_router.first_run import ask_yes_no, mark_offered, shadows_skill, should_offer, was_offered


def answer(text: str) -> object:
    return ask_yes_no("? ", input=io.StringIO(text), output=io.StringIO())


class FirstRun(unittest.TestCase):
    def test_the_setup_check_is_offered_only_on_a_plain_interactive_first_launch(self) -> None:
        """the setup check is offered only on a plain interactive first launch"""
        self.assertTrue(should_offer(args=[], interactive=True, offered=False))
        self.assertFalse(should_offer(args=[], interactive=True, offered=True), "once per user")
        self.assertFalse(should_offer(args=[], interactive=False, offered=False), "nobody to ask")
        for args in [["-p", "fix it"], ["--resume"], ["explain this repo"]]:
            with self.subTest(args=args):
                self.assertFalse(should_offer(args=args, interactive=True, offered=False))

    def test_the_offer_is_not_made_where_a_repository_defines_its_own_jev_calibrate_skill(self) -> None:
        """the offer is not made where a repository defines its own jev-calibrate skill"""
        repo = tempfile.mkdtemp(prefix="jev-shadow-")
        self.addCleanup(shutil.rmtree, repo, True)
        router = os.path.join(repo, "router")
        os.makedirs(os.path.join(router, ".claude", "skills", "jev-calibrate"))
        other = os.path.join(repo, "other")
        os.makedirs(os.path.join(other, ".claude", "skills", "jev-calibrate"))

        self.assertTrue(shadows_skill(other, router), "someone else's skill of the same name")
        self.assertFalse(shadows_skill(router, router), "the router's own skill, in its own repository")
        self.assertFalse(shadows_skill(repo, router), "no such skill here")
        self.assertFalse(should_offer(args=[], interactive=True, offered=False, shadowed=True))

    def test_an_offer_is_remembered_whatever_the_answer(self) -> None:
        """an offer is remembered whatever the answer"""
        directory = tempfile.mkdtemp(prefix="jev-first-run-")
        self.addCleanup(shutil.rmtree, directory, True)
        file = os.path.join(directory, "nested", "first-run.json")
        self.assertFalse(was_offered(file))
        mark_offered(False, file)
        self.assertTrue(was_offered(file), "a no is remembered too, so it is never asked again")
        with open(file, encoding="utf-8") as handle:
            text = handle.read()
        self.assertRegex(text, r'^\{"offeredAt":"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z","accepted":false\}$')

    def test_a_closed_or_failing_input_settles_with_no_answer_instead_of_hanging(self) -> None:
        """a closed or failing input settles with no answer instead of hanging"""
        self.assertIsNone(ask_yes_no("? ", input=io.StringIO(""), output=io.StringIO()))

        class Broken(io.RawIOBase):
            def readline(self, size: int | None = -1) -> bytes:
                raise OSError(5, "EIO")

        self.assertIsNone(ask_yes_no("? ", input=Broken(), output=io.StringIO()))

        class Interrupted(io.RawIOBase):
            def readline(self, size: int | None = -1) -> bytes:
                raise KeyboardInterrupt

        self.assertEqual(ask_yes_no("? ", input=Interrupted(), output=io.StringIO()), "interrupt")

    def test_an_empty_answer_or_yes_accepts_and_no_declines(self) -> None:
        """an empty answer or yes accepts, and no declines"""
        self.assertIs(answer("\n"), True)
        self.assertIs(answer("y\n"), True)
        self.assertIs(answer("Yes\n"), True)
        self.assertIs(answer("n\n"), False)
        self.assertIs(answer("NO\n"), False)
        self.assertIs(answer(" no \r\n"), False)


if __name__ == "__main__":
    unittest.main()
