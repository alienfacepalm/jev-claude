# Updates

jev-claude is a git clone (`~/jev-claude` by default). Updating means fast-forwarding that clone.

## Status

The update library (`node/src/update.mjs`, and its Go, Rust and Python ports; SPEC.md section 14)
and the `jev-update-check` program (`node/bin/jev-update-check.mjs`) exist and are tested by
`node/test/update.test.mjs`. The `jev-claude` launcher does not call them yet (SPEC.md 1.2). So
today the launcher:

- prints no update notice and starts no background check;
- does not accept `--update`: `jev-claude --update` passes `--update` straight through to Claude
  Code, and leaves jev-claude as it was;
- ignores `JEV_NO_UPDATE_CHECK`.

Nothing updates by itself. The two sections marked (planned) describe the launcher wiring as
designed, for when it ships; the rest of this page describes the library as it works today.

## How to update today

Run the installer again (see the [README](../README.md#install)): it fast-forwards the clone with
`git pull --ff-only`, runs `pnpm install --frozen-lockfile`, and re-links the `jev-claude`
command. Or update by hand, as in [doc/NODE.md](NODE.md#updating):

```bash
git -C ~/jev-claude pull --ff-only
cd ~/jev-claude && pnpm install --frozen-lockfile
```

The `jev-claude` command itself is a pnpm shim that records the path of the entry point at install
time, and nothing here rewrites it. An install made before the code moved into `node/` (any install older
than the move, whatever its version) therefore fails after a fast-forward with `Cannot find module .../bin/jev-claude.mjs`; the
fix is to run the installer again (or `pnpm add --global "link:<clone>"`), which regenerates the shim
from the current `bin` map.

## The check (planned)

On every interactive start (stdin and stdout are terminals, `JEV_NO_UPDATE_CHECK` unset),
`jev-claude` will:

1. read `~/.jev-router/update.json` and, if the last check found a newer version than the one
   installed, print ``[jev] Update available: X -> Y. Run `jev-claude --update`.``;
2. if the last check is older than 6 hours (or missing), stamp the file and start
   `node/bin/jev-update-check.mjs` detached, which does the actual look and rewrites the file.

Nothing will wait on the network, so a slow or absent connection costs the launch nothing. The
notice shown will be the result of the previous check, so a release shows up one launch after the
check that finds it. A failed check is retried at the same 6-hour interval, not on every launch.

The look (implemented today in `jev-update-check`) is `git fetch origin <branch>` (with
`GIT_TERMINAL_PROMPT=0`, so a remote that wants credentials fails instead of prompting), then a
comparison of history and of the root `package.json`'s version at the fetched commit.

## Applying (planned)

`jev-claude --update` will fetch again, fast-forward with `git merge --ff-only`, and run
`pnpm install --frozen-lockfile` when `pnpm-lock.yaml` changed. Only the lockfile counts: every
release bumps the version in the root `package.json`, and a frozen install requires the two to agree.
Output will be one line, and the exit code 0 for "updated" or "already up to date", 1 otherwise.
It will take effect on the next start; the running process is never replaced.

## When nothing is changed

The update library refuses to apply, and reports no update available, for a folder that:

- is not a git clone of its own (including one copied inside another repository);
- is on a detached HEAD;
- has uncommitted changes to tracked files;
- has commits that `origin` does not have (a development copy). A copy that is only ahead of
  origin is reported as up to date.

The check never discards anything: it only fetches, and applying only fast-forwards.

## Settings and files

| | |
| --- | --- |
| `JEV_NO_UPDATE_CHECK=1` | Planned; read by nothing yet. Will turn off the notice and the background check. Shell or `~/.jev-router.env` only. |
| `~/.jev-router/update.json` | `{ checkedAt, available, latest, remote }` from the last check, written by `jev-update-check`. Safe to delete. |

There will be no automatic update. Applying code that was just fetched, on every launch, would
give anyone with push access to the repository code execution on every user's machine without a
prompt, in the process that also holds the Jev key. `--update` keeps that a decision.

## Tests

`node/test/update.test.mjs` uses real git: a bare repository as origin, clones made as the installer
makes them (full and `--depth 1`), and real commits upstream. It checks the state of the checkout
after each step. It needs `git` but no network.
