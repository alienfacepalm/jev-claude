//! jev-claude in Rust: a launcher for Claude Code with a local routing proxy. The specification
//! is `SPEC.md` at the repository root; the Node implementation in `node/` is the reference.

pub mod config;
pub mod env;
pub mod envx;
pub mod explain;
pub mod firstrun;
pub mod fsx;
pub mod http;
pub mod icons;
pub mod jsjson;
pub mod jsstr;
pub mod launch;
pub mod legend;
pub mod log;
pub mod model_names;
pub mod osdirs;
pub mod policy;
pub mod process;
pub mod proxy;
pub mod reasons;
pub mod repo;
pub mod router;
pub mod settings;
pub mod status;
pub mod statusline;
pub mod timefmt;
pub mod update;
pub mod worktree;

/// Fixes the values Node computes when its modules load (SPEC 3.9): the status directory, the
/// log file, whether stdout is a terminal, and the files under the home directory. Programs call
/// this first, before reading any settings file.
pub fn init() {
    let _ = status::status_dir();
    let _ = &*log::LOG_FILE;
    let _ = *log::INTERACTIVE;
    let _ = &*settings::USER_SETTINGS;
    let _ = &*firstrun::FIRST_RUN_FILE;
    let _ = &*update::UPDATE_FILE;
}
