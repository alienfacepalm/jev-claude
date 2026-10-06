import { execFileSync } from "node:child_process";

/**
 * The branch checked out in `dir`: its name, "" for a detached HEAD, or null when `dir` is not
 * a git checkout at all. `git branch --show-current` succeeds with no output when detached and
 * fails outside a repository, which is what tells those two apart; it also works before the
 * first commit, where `rev-parse HEAD` does not.
 */
export function gitBranch(dir) {
  if (!dir) return null;
  try {
    return execFileSync("git", ["branch", "--show-current"], {
      cwd: dir,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
      timeout: 1000,
    }).trim();
  } catch {
    return null;
  }
}

/**
 * Where a session is working, as `{ branch, worktree }`, or null outside a git checkout. `branch`
 * is a name, "" when detached, or null when unknown; `worktree` is the linked worktree's name, or
 * null in the main working tree.
 *
 * Claude Code names the worktree in two places: `worktree` for a session it started in one
 * (`--worktree`), which also carries the branch, and `workspace.git_worktree` for any directory
 * inside a linked worktree made with `git worktree add`, which does not. Whatever the input
 * lacks is read from git, in the session's current directory.
 *
 * `branchOf` is injectable so the lookup can be tested without a repository.
 */
export function locationInfo(input, branchOf = gitBranch) {
  const worktree = input?.worktree?.name ?? input?.workspace?.git_worktree ?? null;
  const dir = input?.workspace?.current_dir ?? input?.cwd ?? input?.worktree?.path;
  const branch = input?.worktree?.branch ?? branchOf(dir);
  return worktree === null && branch === null ? null : { branch, worktree };
}
