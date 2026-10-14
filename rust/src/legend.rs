//! What each part of the status line means (SPEC 13; `node/src/legend.mjs`).

use crate::icons::Icons;

/// `formatLegend(set)`, aligned by code points.
pub fn format_legend(set: &Icons) -> String {
    let five_hour = if set.meter_full.is_empty() {
        "how much of your 5-hour usage limit is used; bold from 70%".to_string()
    } else {
        format!(
            "how much of your 5-hour usage limit is used, in quarters ({} used, {} left); bold from 70%",
            set.meter_full, set.meter_empty
        )
    };
    let rows: [(&str, &str); 12] = [
        (set.model, "the model the last turn ran on, then Jev's confidence in that pick"),
        (set.effort, "the reasoning effort it ran at (Haiku takes none, so none is shown)"),
        (set.agents, "the models sub-agents are running on, with +N for any more"),
        (set.dir, "the directory; left out when the worktree has the same name"),
        (set.branch, "the git branch, cut with \u{2026} when long; (detached) when none is checked out"),
        (set.worktree, "the linked git worktree you are working in"),
        (set.context, "how much of the context window is used"),
        (set.five_hour, &five_hour),
        (set.seven_day, "the same for your 7-day usage limit"),
        ("\u{261E}", "you picked the model with /model, so Jev leaves it alone"),
        ("(why)", "a reason in brackets after the effort, only when the pick is not the obvious one"),
        ("new \u{2026}", "a newer model than the router was tuned for: run /jev-calibrate"),
    ];
    let width = rows.iter().map(|(m, _)| m.chars().count()).max().unwrap_or(0);
    rows.iter()
        .map(|(mark, meaning)| format!("{mark}{}  {meaning}", " ".repeat(width - mark.chars().count())))
        .collect::<Vec<_>>()
        .join("\n")
}
