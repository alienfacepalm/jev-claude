"""Port of node/test/explain.test.mjs."""

import os
import re
import unittest

from jev_router.explain import format_explanation
from jev_router.reasons import long_reason, short_reason

from . import REPO_ROOT


class Explain(unittest.TestCase):
    def test_formats_the_last_routing_decision(self) -> None:
        """formats the last routing decision"""
        output = format_explanation(
            {
                "prompt": "Explain the router architecture",
                "tier": "sonnet",
                "confidence": 0.94,
                "reason": "jev",
                "jev": {
                    "request": {"state": {"session": {"current_model": "haiku", "context_tokens": 6200.0}}},
                    "response": {"answers": {"model": {"choice": "claude-sonnet-5-5", "confidence": 0.94}}},
                },
                "metrics": {
                    "taskComplexity": 0.82,
                    "reasoningRequired": 0.91,
                    "toolComplexity": 0.64,
                    "contextSize": 0.31,
                },
            }
        )
        self.assertRegex(output, r"Task complexity     0\.82")
        self.assertRegex(output, r"Prompt: Explain the router")
        self.assertRegex(output, r"Current model: HAIKU")
        self.assertRegex(output, r"Context tokens: 6200 ")
        self.assertRegex(output, r"Recommended tier: SONNET")
        self.assertRegex(output, r"Selected model: SONNET")
        self.assertRegex(output, r"Confidence: 94%")
        self.assertRegex(output, re.compile("Decision: the router's\\s*│\n│ recommendation"))

    def test_shows_jevs_own_pick_when_policy_overruled_it(self) -> None:
        """shows Jev's own pick when policy overruled it"""
        output = format_explanation(
            {
                "tier": "opus",
                "model": "claude-opus-5-5",
                "confidence": 0.97,
                "reason": "downgrade-not-worth-cache-rebuild/no-change",
                "jev": {"response": {"answers": {"model": {"choice": "claude-haiku-4-5-20251001"}}}},
            }
        )
        self.assertRegex(output, r"Recommended tier: HAIKU")
        self.assertRegex(output, r"Selected model: CLAUDE-OPUS-5-5")

    def test_reads_the_recommendation_from_sessions_recorded_before_the_rename(self) -> None:
        """reads the recommendation from sessions recorded before the rename"""
        old = {"tier": "opus", "jev": {"response": {"answers": {"model_tier": {"choice": "sonnet"}}}}}
        self.assertRegex(format_explanation(old), r"Recommended tier: SONNET")

    def test_claude_skill_pre_approves_its_read_only_explanation_command(self) -> None:
        """Claude skill pre-approves its read-only explanation command"""
        with open(os.path.join(REPO_ROOT, ".claude", "skills", "jev-explain", "SKILL.md"), encoding="utf-8") as handle:
            skill = handle.read()
        self.assertRegex(skill, re.compile(r"^allowed-tools: Bash\(node \*\)$", re.MULTILINE))

    def test_says_a_held_decision_in_words_a_person_reads_not_the_reason_code(self) -> None:
        """says a held decision in words a person reads, not the reason code"""
        self.assertEqual(short_reason("downgrade-not-worth-cache-rebuild/no-change"), "keeping the cache")
        self.assertRegex(long_reason("downgrade-not-worth-cache-rebuild"), r"re-read the whole conversation")
        self.assertEqual(short_reason("jev-unavailable"), "router offline")
        self.assertEqual(short_reason("jev+unavailable"), "nearest available")

    def test_the_status_line_stays_quiet_where_the_reason_is_obvious_or_not_actionable(self) -> None:
        """the status line stays quiet where the reason is obvious or not actionable"""
        self.assertIsNone(short_reason("low-confidence-default"))
        self.assertIsNone(short_reason("override"))
        self.assertRegex(long_reason("low-confidence-default"), r"unsure")
        self.assertRegex(long_reason("override"), r"named this model")

    def test_an_ordinary_recommendation_adds_nothing_to_the_status_line(self) -> None:
        """an ordinary recommendation adds nothing to the status line"""
        self.assertIsNone(short_reason("jev"))
        self.assertIsNone(short_reason("jev/no-change"))
        self.assertEqual(long_reason("jev"), "the router's recommendation")


if __name__ == "__main__":
    unittest.main()
