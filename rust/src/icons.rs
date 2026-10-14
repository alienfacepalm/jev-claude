//! The status line's item labels (SPEC 12.1; `node/src/icons.mjs`).

use crate::envx::Env;

#[derive(Debug, Clone, Copy, PartialEq)]
/// The label for each status line item: symbols or plain words.
pub struct Icons {
    /// The model item.
    pub model: &'static str,
    /// The effort item.
    pub effort: &'static str,
    /// The sub-agent count item.
    pub agents: &'static str,
    /// The working directory item.
    pub dir: &'static str,
    /// The git branch item.
    pub branch: &'static str,
    /// The git worktree item.
    pub worktree: &'static str,
    /// The context usage item.
    pub context: &'static str,
    /// The label of the 5-hour usage meter.
    pub five_hour: &'static str,
    /// The label of the 7-day usage meter.
    pub seven_day: &'static str,
    /// A used cell of a usage meter's bar; empty when the set draws no bar.
    pub meter_full: &'static str,
    /// An unused cell of a usage meter's bar.
    pub meter_empty: &'static str,
}

impl Icons {
    /// `(name, mark)` pairs in the order Node's object lists them.
    pub fn entries(&self) -> [(&'static str, &'static str); 11] {
        [
            ("model", self.model),
            ("effort", self.effort),
            ("agents", self.agents),
            ("dir", self.dir),
            ("branch", self.branch),
            ("worktree", self.worktree),
            ("context", self.context),
            ("fiveHour", self.five_hour),
            ("sevenDay", self.seven_day),
            ("meterFull", self.meter_full),
            ("meterEmpty", self.meter_empty),
        ]
    }
}

/// Symbol labels (the default).
pub const SYMBOLS: Icons = Icons {
    model: "\u{2727}\u{2726}",
    effort: "\u{25D4}",
    agents: "\u{2726}",
    dir: "\u{2750}",
    branch: "\u{E0A0}",
    worktree: "\u{2302}",
    context: "\u{2261}",
    five_hour: "5h",
    seven_day: "7d",
    meter_full: "\u{25B0}",
    meter_empty: "\u{25B1}",
};

/// Word labels: `JEV_ICONS=text`, and the default in a Windows console that is not Windows
/// Terminal, `ConEmu`, or another terminal that sets `TERM_PROGRAM`.
pub const TEXT: Icons = Icons {
    model: "model",
    effort: "effort",
    agents: "agents",
    dir: "dir",
    branch: "branch",
    worktree: "worktree",
    context: "ctx",
    five_hour: "5h",
    seven_day: "7d",
    meter_full: "",
    meter_empty: "",
};

/// `icons(env, platform)`; `windows` stands for `platform === "win32"`.
pub fn icons_for(env: &dyn Env, windows: bool) -> Icons {
    let choice = env.get("JEV_ICONS").unwrap_or_default().to_lowercase();
    if choice == "symbols" {
        return SYMBOLS;
    }
    if choice == "text" || choice == "ascii" {
        return TEXT;
    }
    let set = |k: &str| env.get(k).is_some_and(|v| !v.is_empty());
    let modern = set("WT_SESSION") || set("TERM_PROGRAM") || set("ConEmuPID");
    if windows && !modern { TEXT } else { SYMBOLS }
}

/// `icons()` for this process.
pub fn icons() -> Icons {
    icons_for(&crate::envx::ProcessEnv, cfg!(windows))
}
