//! The status line's item labels (SPEC 12.1; `node/src/icons.mjs`).

use crate::envx::Env;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Icons {
    pub model: &'static str,
    pub effort: &'static str,
    pub agents: &'static str,
    pub dir: &'static str,
    pub branch: &'static str,
    pub worktree: &'static str,
    pub context: &'static str,
}

impl Icons {
    /// `(name, mark)` pairs in the order Node's object lists them.
    pub fn entries(&self) -> [(&'static str, &'static str); 7] {
        [
            ("model", self.model),
            ("effort", self.effort),
            ("agents", self.agents),
            ("dir", self.dir),
            ("branch", self.branch),
            ("worktree", self.worktree),
            ("context", self.context),
        ]
    }
}

pub const SYMBOLS: Icons = Icons {
    model: "\u{25C6}",
    effort: "\u{25D4}",
    agents: "\u{2726}",
    dir: "\u{2750}",
    branch: "\u{E0A0}",
    worktree: "\u{2302}",
    context: "\u{2261}",
};

pub const TEXT: Icons = Icons {
    model: "model",
    effort: "effort",
    agents: "agents",
    dir: "dir",
    branch: "branch",
    worktree: "worktree",
    context: "ctx",
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
