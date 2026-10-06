# jev-router (Python port)

The Python implementation of jev-claude, ported from the Node.js reference in `../node` to the
specification in [`../SPEC.md`](../SPEC.md). It provides the seven programs of SPEC 1.1 and the
library behaviour behind them, using the Python standard library only.

| Program | Module |
| --- | --- |
| `jev-claude` | `jev_router.cli.claude` |
| `jev-statusline` | `jev_router.cli.statusline` |
| `jev-explain` | `jev_router.cli.explain` |
| `jev-legend` | `jev_router.cli.legend` |
| `jev-check` | `jev_router.cli.check` |
| `jev-update-check` | `jev_router.cli.update_check` |
| `jev-proxy-host` | `jev_router.cli.proxy_host` |

## Requirements

CPython 3.12 or newer. No third-party packages at run time or for the tests.

## Install and run

Without installing, put `python/src` on `PYTHONPATH` and run a module:

```sh
# from the repository root
PYTHONPATH=python/src python -m jev_router.cli.claude          # POSIX shells
$env:PYTHONPATH = "python\src"; python -m jev_router.cli.claude # PowerShell
```

Installing creates the seven commands from `[project.scripts]`:

```sh
pip install ./python        # or: pipx install ./python
jev-claude
```

`jev-claude` registers its status line with Claude Code as
`"<sys.executable>" -m jev_router.cli.statusline` (SPEC 10.1), so the interpreter that runs the
launcher must be able to import `jev_router` (installed, or with `PYTHONPATH` set; Claude Code
inherits the launcher's environment).

Settings are the same as the Node implementation's: `JEV_API_KEY` (or `TYPESAFE_API_KEY`) in the
environment, a project `.env` (allow-listed `JEV_*` keys only), `~/.jev-router.env`, or
`~/.jev-claude.env`. Status files are shared with the other implementations (SPEC 2.5).

The repository root (for `--add-dir`, the release version and `jev-check`'s mode) is found by
walking up from the package's own file to the directory holding
`.claude/skills/jev-calibrate/SKILL.md`; `JEV_ROOT` overrides that (SPEC 2.4). An installed copy
outside the repository has no root, which SPEC 2.4 describes.

## Test

From the repository root:

```sh
python -m unittest discover -s python/tests -t python
python -m compileall -q python/src
```

`python/tests/__init__.py` points `JEV_STATUS_DIR` at a throwaway directory before anything imports
the package, and puts `python/src` on `sys.path`. The suite needs `git` on `PATH` (the update and
worktree tests drive real repositories) and uses `node` where a Node test did (the npm shim test
runs a real `.cmd` shim's script).

| Test file | What it covers |
| --- | --- |
| `test_jsstr.py`, `test_jsjson.py`, `test_osdirs.py`, `test_repo.py` | the JavaScript-compatibility helpers (SPEC 3), with expected values taken from Node 24 |
| `test_<module>.py` for each `node/test/*.test.mjs` | the Node unit tests, ported one file per file with each Node title as the test docstring (`bump-version` is out of scope, SPEC 1.2) |
| `test_router.py` | the Jev client written in place of the SDK: headers, one retry, timeouts, the 3 s deadline |
| `test_golden.py` | every case in `conformance/cases/*.json` (SPEC 16.2), decoding the tagged encoding of `conformance/cases/README.md` |

## Conformance harness

The black-box harness (SPEC 16.3) runs this port's proxy host and status line as child processes.
On Windows, give the full path of the interpreter: a bare `python` can resolve to the Microsoft
Store alias inside Node's `spawn`.

```sh
# Git Bash, from the repository root
PY="$(python -c 'import sys; print(sys.executable)')"
export PYTHONPATH="$(cygpath -w "$PWD/python/src")"
export JEV_IMPL_CMD_PROXY="\"$PY\" -m jev_router.cli.proxy_host"
export JEV_IMPL_CMD_STATUSLINE="\"$PY\" -m jev_router.cli.statusline"
node --test conformance/harness
```

```powershell
# PowerShell, from the repository root
$py = (Get-Command python).Source  # or the interpreter's full path
$env:PYTHONPATH = "$PWD\python\src"
$env:JEV_IMPL_CMD_PROXY = "`"$py`" -m jev_router.cli.proxy_host"
$env:JEV_IMPL_CMD_STATUSLINE = "`"$py`" -m jev_router.cli.statusline"
node --test conformance/harness
```

`PYTHONPATH` passes through the harness's environment filter, which only removes `JEV_*`,
`TYPESAFE_*`, `ANTHROPIC_*` and `CLAUDE_*`. On macOS and Linux the same commands work with `python3`
and without `cygpath`.

## Layout

```
pyproject.toml            project jev-router 0.0.0, [project.scripts] for the seven programs
src/jev_router/           one module per Node module, plus jsjson, jsstr, osdirs, repo
  cli/                    one module per program, each with main()
tests/                    unittest suites (see above); support.py holds the loopback servers
```

## Divergences from Node, and why

Each is permitted or required by SPEC.md; none shows in the golden cases or the harness.

- **Concurrency.** Requests are served on threads. A process-wide lock guards every status-file
  read-modify-write and calibration write, and the proxy's `convos`/`mains`/`catalog` sit under
  one lock that is released while Jev is asked (SPEC 3.10). Temporary files are
  `<file>.<pid>.<seq>.tmp`.
- **Jev client.** Written against SPEC 5 with `http.client` instead of the TypeSafe SDK. It sends
  only the headers SPEC 5.1 lists (`User-Agent: jev-router-python/<version>`), not the SDK's
  `X-TypeSafe-SDK`/`X-TypeSafe-Runtime`. Each attempt runs on its own thread so the 1500 ms timeout
  covers connect, send and the whole body; the prewarm `HEAD` does not share its connection with
  the first call (SPEC 5.4 allows Python to skip that).
- **`claudeModels` tie-break.** Equal versions are ordered by `releasedAt` in plain UTF-16
  code-unit order (SPEC 7.5, 19.1); Node calls `localeCompare`, which agrees on the ISO dates the
  catalog carries.
- **Splitting a surrogate pair.** Where a UTF-16 cut would split a pair, the lone high surrogate is
  dropped (SPEC 3.2); Node keeps it. No real input reaches this.
- **Client disconnects.** Node learns of them from socket events; this port watches the client
  socket on a helper thread while a request is in flight, so a client that leaves during the Jev
  call or the upstream stream still aborts the upstream request (SPEC 7.2 step 7).
- **Response framing.** Python's HTTP server does not frame for us, so the proxy writes
  `content-length` when the upstream gave one and re-chunks otherwise (SPEC 7.7 allows framing
  headers). It never adds a `Server` header.
- **Rename on Windows.** Replacing a status file retries briefly when a reader holds it open
  without delete sharing (a Python reader does; libuv readers do not).
- **Launcher signals on Windows.** Console close, logoff and shutdown are caught with
  `SetConsoleCtrlHandler`; the child is ended with `TerminateProcess` (what Node's `child.kill`
  does on Windows), and cleanup runs before the process exits.
