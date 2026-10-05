import { execFileSync } from "node:child_process";

/** The branch checked out in `dir`, or null when detached or when `dir` is not a git checkout. */
export function gitBranch(dir) {
  if (!dir) return null;
  try {
    const out = execFileSync("git", ["branch", "--show-current"], {
      cwd: dir,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
      timeout: 1000,
    });
    return out.trim() || null;
  } catch {
    return null;
  }
}

/**
 * The worktree a session is working in, as `{ name, branch }`, or null in the main working tree.
 * Claude Code names the worktree in two places: `worktree` for a session it started in one
 * (`--worktree`), which also carries the branch, and `workspace.git_worktree` for any directory
 * inside a linked worktree made with `git worktree add`, which does not. The branch is read from
 * git when the input lacks it; it stays null for a detached HEAD.
 *
 * `branchOf` is injectable so the lookup can be tested without a repository, and is only called
 * when there is a worktree to show: the status line redraws often and a plain checkout pays nothing.
 */
export function worktreeInfo(input, branchOf = gitBranch) {
  const name = input?.worktree?.name ?? input?.workspace?.git_worktree;
  if (!name) return null;
  const dir = input.workspace?.current_dir ?? input.cwd ?? input.worktree?.path;
  return { name, branch: input.worktree?.branch ?? branchOf(dir) };
}
