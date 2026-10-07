//! Where a session is working (SPEC 12.2; `node/src/worktree.mjs`).

use crate::jsjson::Value;
use crate::jsstr::JsStr;
use crate::process::{RunOptions, run_with_timeout};
use std::time::Duration;

/// The branch checked out in `dir`: its name, "" for a detached HEAD, None outside a checkout.
pub fn git_branch(dir: &Value) -> Option<JsStr> {
    let dir = dir.as_str().filter(|d| !d.is_empty())?;
    let out = run_with_timeout(
        "git",
        &["branch", "--show-current"],
        &RunOptions {
            cwd: Some(dir.to_string_lossy().into()),
            timeout: Duration::from_millis(1000),
            ..Default::default()
        },
    )
    .ok()?;
    if !out.success {
        return None;
    }
    Some(JsStr::from(String::from_utf8_lossy(&out.stdout).into_owned()).trim())
}

/// `{ branch, worktree }`; `branch` is None when unknown (Node's null).
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    /// The branch name, `null` when it could not be read.
    pub branch: Value,
    /// The worktree name, or `null` outside a linked worktree.
    pub worktree: Value,
}

/// `locationInfo(input, branchOf)`.
pub fn location_info(input: &Value, branch_of: &dyn Fn(&Value) -> Option<JsStr>) -> Option<Location> {
    let worktree = input.get("worktree").get("name").or(input.get("workspace").get("git_worktree")).clone();
    let worktree = if worktree.is_undefined() { Value::Null } else { worktree };
    let dir =
        input.get("workspace").get("current_dir").or(input.get("cwd")).or(input.get("worktree").get("path")).clone();
    let given = input.get("worktree").get("branch");
    let branch = if given.is_nullish() { branch_of(&dir).map_or(Value::Null, Value::String) } else { given.clone() };
    if matches!(worktree, Value::Null) && matches!(branch, Value::Null) {
        return None;
    }
    Some(Location { branch, worktree })
}
