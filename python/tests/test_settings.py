"""Port of node/test/settings.test.mjs."""

import json
import os
import tempfile
import unittest
from typing import Any

from jev_router.jsstr import UNDEFINED
from jev_router.settings import read_saved_model, restore_saved_model


def file_with(settings: object) -> str:
    file = os.path.join(tempfile.mkdtemp(prefix="jev-settings-"), "settings.json")
    with open(file, "w", encoding="utf-8") as handle:
        json.dump(settings, handle, indent=2)
    return file


def model_in(file: str) -> Any:
    with open(file, encoding="utf-8") as handle:
        return json.load(handle).get("model", UNDEFINED)


def memo_file() -> str:
    return os.path.join(tempfile.mkdtemp(prefix="jev-memo-"), "saved-model.json")


class Settings(unittest.TestCase):
    def test_reads_the_saved_model_ignoring_a_leftover_sentinel(self) -> None:
        """reads the saved model, ignoring a leftover sentinel"""
        self.assertEqual(read_saved_model(file_with({"model": "opus"}), memo_file()), "opus")
        self.assertIs(read_saved_model(file_with({"model": "jev-router"}), memo_file()), UNDEFINED)
        self.assertIs(read_saved_model(file_with({}), memo_file()), UNDEFINED)
        missing = os.path.join(tempfile.mkdtemp(prefix="jev-settings-"), "does-not-exist.json")
        self.assertIs(read_saved_model(missing, memo_file()), UNDEFINED)

    def test_a_sentinel_left_by_a_killed_session_resolves_to_the_model_from_before_it(self) -> None:
        """a sentinel left by a killed session resolves to the model from before it"""
        memo = memo_file()
        self.assertEqual(read_saved_model(file_with({"model": "claude-opus-4-6"}), memo), "claude-opus-4-6")
        file = file_with({"model": "jev-router"})
        previous = read_saved_model(file, memo)
        self.assertEqual(previous, "claude-opus-4-6")
        self.assertTrue(restore_saved_model(previous, file))
        self.assertEqual(model_in(file), "claude-opus-4-6")

    def test_restores_the_previous_model_when_the_sentinel_was_saved(self) -> None:
        """restores the previous model when the sentinel was saved"""
        file = file_with({"model": "jev-router", "permissions": {"deny": ["Bash(rm*)"]}})
        self.assertTrue(restore_saved_model("opus", file))
        self.assertEqual(model_in(file), "opus")
        with open(file, encoding="utf-8") as handle:
            text = handle.read()
        self.assertEqual(json.loads(text)["permissions"], {"deny": ["Bash(rm*)"]})
        self.assertEqual(
            text, '{\n  "model": "opus",\n  "permissions": {\n    "deny": [\n      "Bash(rm*)"\n    ]\n  }\n}\n'
        )

    def test_removes_the_sentinel_when_there_was_no_previous_model(self) -> None:
        """removes the sentinel when there was no previous model"""
        file = file_with({"model": "jev-router"})
        self.assertTrue(restore_saved_model(UNDEFINED, file))
        self.assertIs(model_in(file), UNDEFINED)

    def test_leaves_a_real_model_the_user_chose_during_the_session_alone(self) -> None:
        """leaves a real model the user chose during the session alone"""
        file = file_with({"model": "claude-opus-4-6"})
        self.assertFalse(restore_saved_model("sonnet", file))
        self.assertEqual(model_in(file), "claude-opus-4-6")

    def test_a_missing_or_unreadable_settings_file_is_not_an_error(self) -> None:
        """a missing or unreadable settings file is not an error"""
        self.assertFalse(restore_saved_model("opus", os.path.join(tempfile.mkdtemp(), "nope", "settings.json")))


if __name__ == "__main__":
    unittest.main()
