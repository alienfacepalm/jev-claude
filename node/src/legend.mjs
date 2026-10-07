import { icons } from "./icons.mjs";

/**
 * What each part of the status line means, drawn with the marks the status line itself is drawing
 * right now (glyphs, or the words in a console that cannot show them), so the key never describes
 * a symbol the person is not looking at.
 */
export function formatLegend(set = icons()) {
  const rows = [
    [set.model, "the model the last turn ran on, then Jev's confidence in that pick"],
    [set.effort, "the reasoning effort it ran at (Haiku takes none, so none is shown)"],
    [set.agents, "the models sub-agents are running on, with +N for any more"],
    [set.dir, "the directory; left out when the worktree has the same name"],
    [set.branch, "the git branch, cut with … when long; (detached) when none is checked out"],
    [set.worktree, "the linked git worktree you are working in"],
    [set.context, "how much of the context window is used"],
    ["☞", "you picked the model with /model, so Jev leaves it alone"],
    ["(why)", "a reason in brackets after the effort, only when the pick is not the obvious one"],
    ["new …", "a newer model than the router was tuned for: run /jev-calibrate"],
  ];
  const width = Math.max(...rows.map(([mark]) => [...mark].length));
  return rows.map(([mark, meaning]) => `${mark}${" ".repeat(width - [...mark].length)}  ${meaning}`).join("\n");
}
