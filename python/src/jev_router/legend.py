"""What each part of the status line means (port of node/src/legend.mjs, SPEC 13)."""

from __future__ import annotations

from .icons import icons
from .jsstr import code_points


def format_legend(marks=None) -> str:
    marks = icons() if marks is None else marks
    rows = [
        [marks["model"], "the model the last turn ran on, then Jev's confidence in that pick"],
        [marks["effort"], "the reasoning effort it ran at (Haiku takes none, so none is shown)"],
        [marks["agents"], "the models sub-agents are running on, with +N for any more"],
        [marks["dir"], "the directory; left out when the worktree has the same name"],
        [marks["branch"], "the git branch, cut with \u2026 when long; (detached) when none is checked out"],
        [marks["worktree"], "the linked git worktree you are working in"],
        [marks["context"], "how much of the context window is used"],
        ["\u23f8", "you picked the model with /model, so Jev leaves it alone"],
        ["(why)", "a reason in brackets after the effort, only when the pick is not the obvious one"],
        ["new \u2026", "a newer model than the router was tuned for: run /jev-calibrate"],
    ]
    width = max(code_points(mark) for mark, _ in rows)
    return "\n".join(f"{mark}{' ' * (width - code_points(mark))}  {meaning}" for mark, meaning in rows)
