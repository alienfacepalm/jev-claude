# Updates

jev-claude is a git clone (`~/jev-claude` by default). Updating means fast-forwarding that clone.

## The check

On every interactive start (stdin and stdout are terminals, `JEV_NO_UPDATE_CHECK` unset),
`jev-claude`:

1. reads `~/.jev-router/update.json` and, if the last check found a newer version than the one
   installed, prints ``[jev] Update available: X -> Y. Run `jev-claude --update`.``;
2. if the last check is older than 6 hours (or missing), stamps the file and starts
   `bin/jev-update-check.mjs` detached, which does the actual look and rewrites the file.

Nothing waits on the network, so a slow or absent connection costs the launch nothing. The notice
you see is the result of the previous check, so a release shows up one launch after the check that
finds it. A failed check is retried at the same 6-hour interval, not on every launch.

The look is `git fetch origin <branch>` (with `GIT_TERMINAL_PROMPT=0`, so a remote that wants
credentials fails instead of prompting), then a comparison of history and of `package.json`'s
version at the fetched commit.

## Applying

`jev-claude --update` fetches again, fast-forwards with `git merge --ff-only`, and runs
`pnpm install --frozen-lockfile` when `pnpm-lock.yaml` changed. Only the lockfile counts: every
release bumps the version in `package.json`, and a frozen install requires the two to agree.
Output is one line, and the exit code is 0 for "updated" or "already up to date", 1 otherwise.
It takes effect on the next start; the running process is never replaced.

## When nothing is changed

`--update` refuses, and the notice is never shown, for a folder that:

- is not a git clone of its own (including one copied inside another repository);
- is on a detached HEAD;
- has uncommitted changes to tracked files;
- has commits that `origin` does not have (a development copy). A copy that is only ahead of
  origin is reported as up to date.

The check never discards anything: it only fetches, and `--update` only fast-forwards.

## Settings and files

| | |
| --- | --- |
| `JEV_NO_UPDATE_CHECK=1` | No notice and no background check. Shell or `~/.jev-router.env` only. |
| `~/.jev-router/update.json` | `{ checkedAt, available, latest, remote }` from the last check. Safe to delete. |

There is no automatic update. Applying code that was just fetched, on every launch, would give
anyone with push access to the repository code execution on every user's machine without a
prompt, in the process that also holds the Jev key. `--update` keeps that a decision.

## Tests

`test/update.test.mjs` uses real git: a bare repository as origin, clones made as the installer
makes them (full and `--depth 1`), and real commits upstream. It checks the state of the checkout
after each step. It needs `git` but no network.
