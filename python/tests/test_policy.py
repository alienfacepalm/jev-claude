"""Port of node/test/policy.test.mjs."""

import os
import unittest
from collections.abc import Sequence

from jev_router.config import QUESTIONS, available_tiers, should_use_exact_model
from jev_router.jsstr import JsObject, JsValue
from jev_router.policy import Decision, decide, detect_override

from . import FIXTURES, as_dict, as_list

ALL = ["haiku", "sonnet", "opus", "fable"]


def sure(choice: str) -> JsObject:
    return {"choice": choice, "confidence": 0.95}


def unsure(choice: str) -> JsObject:
    return {"choice": choice, "confidence": 0.2}


def run(
    prompt: object = "refactor the parser",
    jev: object = None,
    current: JsValue = "sonnet",
    available: Sequence[str] = ALL,
    context_tokens: object = 0,
) -> Decision:
    """`decide` with the Node test's defaults for whatever is not given."""
    return decide(prompt=prompt, jev=jev, current=current, available=available, context_tokens=context_tokens)


class Policy(unittest.TestCase):
    def test_score_rubrics_contain_only_api_valid_descriptions(self) -> None:
        """score rubrics contain only API-valid descriptions"""
        scores = [as_dict(q) for q in QUESTIONS.values() if as_dict(q)["type"] == "score"]
        self.assertEqual(len(scores), 3)
        for question in scores:
            with self.subTest(instructions=question["instructions"]):
                criteria = as_list(question["criteria"])
                self.assertTrue(all(isinstance(d, str) for d in criteria))
                self.assertLessEqual(len(criteria), 10)

    def test_follows_a_confident_jev_answer(self) -> None:
        """follows a confident Jev answer"""
        self.assertEqual(run(jev=sure("opus")), {"tier": "opus", "reason": "jev", "changed": True})

    def test_an_explicit_user_override_beats_jev(self) -> None:
        """an explicit user override beats Jev"""
        out = run(prompt="use haiku to fix this typo", jev=sure("opus"))
        self.assertEqual(out["tier"], "haiku")
        self.assertEqual(out["reason"], "override")

    def test_a_sub_agent_report_quoting_an_override_phrase_does_not_force_a_model(self) -> None:
        """a sub-agent report quoting an override phrase does not force a model"""
        with open(os.path.join(FIXTURES, "subagent-handback-prompt.txt"), encoding="utf-8", newline="") as handle:
            prompt = handle.read()
        self.assertIsNone(detect_override(prompt))
        out = run(prompt=prompt, current="opus", jev=sure("sonnet"))
        self.assertEqual(out["tier"], "sonnet", "Jev's confident answer is acted on, not the quoted phrase")
        self.assertEqual(out["reason"], "jev")

    def test_detect_override_only_fires_on_a_real_instruction(self) -> None:
        """detectOverride only fires on a real instruction"""
        self.assertEqual(detect_override("switch to opus"), "opus")
        self.assertEqual(detect_override("use haiku"), "haiku")
        self.assertEqual(detect_override("use the strong model"), "opus")
        self.assertEqual(detect_override("Use Claude Haiku for this one"), "haiku")
        self.assertIsNone(detect_override("the opus of his career"))

    def test_detect_override_ignores_ordinary_prose_that_mentions_a_tier_word(self) -> None:
        """detectOverride ignores ordinary prose that mentions a tier word"""
        for prompt in [
            "help me with fast fourier transform code",
            "replace the polling loop with long polling",
            "the test only fails on fast CI runners",
            "write tests with long input strings",
            "turn on fast refresh in vite",
            "refactor this to rely on strong typing",
            "use haiku-style commit messages",
            "use long variable names",
        ]:
            with self.subTest(prompt=prompt):
                self.assertIsNone(detect_override(prompt))

    def test_keeps_the_current_model_when_jev_is_unreachable(self) -> None:
        """keeps the current model when Jev is unreachable"""
        out = run(jev=None)
        self.assertEqual(out["tier"], "sonnet")
        self.assertFalse(out["changed"])
        self.assertIn("jev-unavailable", out["reason"])

    def test_ignores_a_tier_name_jev_invented(self) -> None:
        """ignores a tier name Jev invented"""
        self.assertEqual(run(jev=sure("mystery-9"))["tier"], "sonnet")

    def test_an_unsure_pick_of_opus_runs_one_tier_lower_on_the_default(self) -> None:
        """an unsure pick of Opus runs one tier lower, on the default"""
        out = run(current="haiku", jev=unsure("opus"))
        self.assertEqual(out["tier"], "sonnet")
        self.assertEqual(out["reason"], "low-confidence-default")

    def test_never_downgrades_on_a_low_confidence_answer(self) -> None:
        """never downgrades on a low-confidence answer"""
        out = run(jev=unsure("haiku"))
        self.assertEqual(out["tier"], "sonnet", "an unsure downgrade is not a reason to leave the default")
        self.assertIn("low-confidence-default", out["reason"])

    def test_a_middling_answer_is_not_followed_down_to_a_weaker_model(self) -> None:
        """a middling answer is not followed down to a weaker model"""
        middling = run(jev={"choice": "haiku", "confidence": 0.45})
        self.assertEqual(middling["tier"], "sonnet")
        self.assertIn("low-confidence-default", middling["reason"])
        self.assertEqual(run(jev={"choice": "haiku", "confidence": 0.78})["tier"], "haiku")

    def test_an_unsure_answer_keeps_opus_when_opus_is_already_in_use(self) -> None:
        """an unsure answer keeps Opus when Opus is already in use"""
        out = run(current="opus", jev=unsure("haiku"))
        self.assertEqual(out["tier"], "opus")
        self.assertIn("no-change", out["reason"])

    def test_keeps_a_tier_stronger_than_the_default_on_a_low_confidence_answer(self) -> None:
        """keeps a tier stronger than the default on a low-confidence answer"""
        out = run(current="fable", jev=unsure("haiku"))
        self.assertEqual(out["tier"], "fable")
        self.assertIn("no-change", out["reason"])

    def test_an_answer_without_a_confidence_is_treated_as_unsure(self) -> None:
        """an answer without a confidence is treated as unsure"""
        out = run(current="haiku", jev={"choice": "haiku"})
        self.assertEqual(out["tier"], "sonnet")
        self.assertEqual(out["reason"], "low-confidence-default")

    def test_an_unsure_answer_runs_one_tier_below_its_pick(self) -> None:
        """an unsure answer runs one tier below its pick"""
        self.assertEqual(run(current="sonnet", jev=unsure("opus"))["tier"], "sonnet")
        self.assertEqual(run(current="sonnet", jev=unsure("fable"))["tier"], "opus")
        self.assertEqual(run(current="haiku", jev=unsure("sonnet"))["tier"], "sonnet", "never below the default")

    def test_a_low_confidence_answer_cannot_reach_fable(self) -> None:
        """a low-confidence answer cannot reach fable"""
        out = run(current="haiku", jev=unsure("fable"))
        self.assertEqual(out["tier"], "opus", "one step below fable, never fable itself")
        self.assertEqual(out["reason"], "low-confidence-default")

    def test_still_allows_a_confident_upgrade_to_fable(self) -> None:
        """still allows a confident upgrade to fable"""
        self.assertEqual(run(jev=sure("fable"))["tier"], "fable")

    def test_refuses_a_downgrade_once_the_cache_rebuild_costs_more_than_it_saves(self) -> None:
        """refuses a downgrade once the cache rebuild costs more than it saves"""
        out = run(current="opus", jev=sure("haiku"), context_tokens=80000)
        self.assertEqual(out["tier"], "opus")
        self.assertIn("cache-rebuild", out["reason"])

    def test_allows_the_same_downgrade_early_in_a_conversation(self) -> None:
        """allows the same downgrade early in a conversation"""
        self.assertEqual(run(current="opus", jev=sure("haiku"))["tier"], "haiku")

    def test_substitutes_upward_when_the_chosen_tier_is_unavailable(self) -> None:
        """substitutes upward when the chosen tier is unavailable"""
        out = run(current="haiku", available=["haiku", "opus"], jev=sure("sonnet"))
        self.assertEqual(out["tier"], "opus")
        self.assertIn("unavailable", out["reason"])

    def test_never_substitutes_upward_into_paid_fable(self) -> None:
        """never substitutes upward into paid fable"""
        self.assertEqual(run(current="haiku", available=["haiku", "fable"], jev=sure("opus"))["tier"], "haiku")

    def test_accepts_exact_model_changes_within_the_same_tier(self) -> None:
        """accepts exact model changes within the same tier"""
        self.assertTrue(should_use_exact_model("jev/no-change", "opus", "opus"))
        self.assertFalse(should_use_exact_model("low-confidence-default/no-change", "opus", "opus"))

    def test_fable_is_on_offer_by_default_and_can_be_switched_off(self) -> None:
        """fable is on offer by default and can be switched off"""
        self.assertIn("fable", available_tiers({}))
        self.assertIn("fable", available_tiers({"JEV_ALLOW_FABLE": "1"}))
        for off in ["0", "false", "No", " off "]:
            with self.subTest(off=off):
                self.assertNotIn("fable", available_tiers({"JEV_ALLOW_FABLE": off}))
        self.assertEqual(available_tiers({"JEV_ALLOW_FABLE": "0"}), ["haiku", "sonnet", "opus"])


if __name__ == "__main__":
    unittest.main()
