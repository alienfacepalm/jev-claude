"""What each part of the status line means (port of node/src/legend.mjs, SPEC 13)."""

from __future__ import annotations

from collections.abc import Mapping

from .icons import icons
from .jsstr import code_points


def format_legend(marks: Mapping[str, str] | None = None) -> str:
    """The status line key: each label and what it means."""
    marks = icons() if marks is None else marks
    if marks["meterFull"]:
        five_hour = (
            "how much of your 5-hour usage limit is used, in quarters "
            f"({marks['meterFull']} used, {marks['meterEmpty']} left); bold from 70%"
        )
    else:
        five_hour = "how much of your 5-hour usage limit is used; bold from 70%"
    rows = [
        [marks["model"], "the model the last turn ran on, then Jev's confidence in that pick"],
        [marks["effort"], "the reasoning effort it ran at (Haiku takes none, so none is shown)"],
        [marks["agents"], "the models sub-agents are running on, with +N for any more"],
        [marks["dir"], "the directory; left out when the worktree has the same name"],
        [marks["branch"], "the git branch, cut with \u2026 when long; (detached) when none is checked out"],
        [marks["worktree"], "the linked git worktree you are working in"],
        [marks["context"], "how much of the context window is used"],
        [marks["fiveHour"], five_hour],
        [marks["sevenDay"], "the same for your 7-day usage limit"],
        ["\u261e", "you picked the model with /model, so Jev leaves it alone"],
        ["(why)", "a reason in brackets after the effort, only when the pick is not the obvious one"],
        ["new \u2026", "a newer model than the router was tuned for: run /jev-calibrate"],
    ]
    width = max(code_points(mark) for mark, _ in rows)
    return "\n".join(f"{mark}{' ' * (width - code_points(mark))}  {meaning}" for mark, meaning in rows)
