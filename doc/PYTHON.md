# The Python implementation

`python/` is a complete port of jev-claude to Python: the launcher, the local routing proxy, the
status line, and the helper programs. It follows [`SPEC.md`](../SPEC.md) and behaves the same as
the Node implementation in [`node/`](../node), which stays the reference. Every port must pass
the same golden cases and black-box harness, so you can swap one for another.

The programs need CPython 3.12 or newer and nothing else: the package uses the standard library
only, at run time and in its tests. Ruff and mypy are development tools, installed into a
virtual environment, never into your system Python. The installers (`install.sh`,
`install.ps1`), the shim, and the plugin still set up the Node implementation, so you install
this one by hand, as described below.

- [Programs](#programs)
- [Install the toolchain](#install-the-toolchain)
- [Build](#build)
- [Install the programs and put them on PATH](#install-the-programs-and-put-them-on-path)
- [Run jev-claude](#run-jev-claude)
- [Configuration](#configuration)
- [Tests](#tests)
- [Lint, format and type-check](#lint-format-and-type-check)
- [Code style](#code-style)
- [Conformance harness](#conformance-harness)
- [What CI runs](#what-ci-runs)
- [Troubleshooting](#troubleshooting)
- [Behaviour notes](#behaviour-notes)
- [Known divergences from Node](#known-divergences-from-node)
- [Source layout](#source-layout)

## Programs

The package `jev-router` (import name `jev_router`) provides seven programs. Installing it
creates a command for each from `[project.scripts]` in `python/pyproject.toml`; without
installing, run the module with `python -m`.

| Program | Module | What it does | SPEC |
| --- | --- | --- | --- |
| `jev-claude` | `jev_router.cli.claude` | The launcher. It loads your settings, starts the proxy, and runs the real `claude` through it | 10 |
| `jev-statusline` | `jev_router.cli.statusline` | The status line command that Claude Code runs | 12 |
| `jev-explain <session id>` | `jev_router.cli.explain` | The explanation panel for `/jev-explain` | 13 |
| `jev-legend` | `jev_router.cli.legend` | The status line key for `/jev-legend` | 13 |
| `jev-check` | `jev_router.cli.check` | The read-only setup report for `/jev-calibrate` | 13 |
| `jev-update-check` | `jev_router.cli.update_check` | The background update check. It writes `~/.jev-router/update.json` | 14 |
| `jev-proxy-host` | `jev_router.cli.proxy_host` | Starts the proxy on its own, prints `PORT=<port>`, and runs until killed. The conformance harness uses it | 16.3 |

The package version is fixed at `0.0.0`: the release version lives only in the root
`package.json`, which the programs read at run time (SPEC 2.3).

## Install the toolchain

You need CPython 3.12 or newer and `git`. Node.js 22+ is needed too, with `pnpm install` run at
the repository root: the conformance harness is a Node program, and the `/jev-*` skills inside
Claude Code run the Node helper programs (see [Run jev-claude](#run-jev-claude)).

**Windows** (PowerShell):

```powershell
winget install --id Python.Python.3.12
# Open a new terminal, then:
py -3.12 --version
```

The python.org installer works too; tick "Add python.exe to PATH". If `python` opens the
Microsoft Store instead, turn off the `python.exe` and `python3.exe` App execution aliases in
Settings, or use `py -3.12`. pyenv-win (`pyenv install 3.12.8`, `pyenv global 3.12.8`) also
works; see [Troubleshooting](#troubleshooting) for the one place its `.bat` shims matter.

**macOS:**

```bash
brew install python@3.12
python3.12 --version
```

**Linux** (Debian and Ubuntu shown; on other distributions install the 3.12 package and its
`venv` module the same way, or use pyenv):

```bash
sudo apt install python3.12 python3.12-venv      # Ubuntu 22.04: add ppa:deadsnakes/ppa first
python3.12 --version
```

The version must print 3.12 or later. The commands below write `python`; on macOS and Linux use
`python3.12` (or `python3` when that is 3.12+) until a virtual environment is active.

## Build

There is nothing to compile. To check that every module byte-compiles, from the repository
root:

```bash
python -m compileall -q python/src
```

To build a wheel (for example to install it elsewhere), from the repository root:

```bash
python -m pip wheel ./python --no-deps -w python/dist
```

`python/dist/`, `python/.venv/`, the tool caches and `*.egg-info` are ignored by git
(`python/.gitignore`).

## Install the programs and put them on PATH

Two rules apply, whichever way you install:

1. **The interpreter that runs `jev-claude` must import `jev_router`.** The launcher registers
   the status line with Claude Code as `"<that interpreter>" -m jev_router.cli.statusline`
   (SPEC 10.1), and Claude Code inherits the launcher's environment. A virtual environment or
   pipx satisfies this by itself.
2. **Tell programs outside the clone where the clone is.** The programs find the repository by
   walking up from the package's own files to the nearest directory holding
   `.claude/skills/jev-calibrate/SKILL.md` (SPEC 2.4). An editable install or `PYTHONPATH`
   points into the clone, so this always works. A regular install copies the package into
   `site-packages`; then set `JEV_ROOT` to the clone. Without it, the launcher cannot pass
   `--add-dir <clone>`, so the `/jev-*` skills are missing outside the clone; `jev-check` reports
   `Mode installed` instead of `repository`; `jev-update-check` has no clone to check and exits
   without writing; and the release version sent to Jev in the User-Agent falls back to `0.0.0`.

### Option A: an editable install in the repository's virtual environment (recommended)

The same environment holds the lint tools, and `git pull` updates the programs with no
reinstall. From the repository root:

```bash
python -m venv python/.venv
python/.venv/Scripts/python -m pip install -e "./python[dev]"   # Git Bash on Windows
python/.venv/bin/python -m pip install -e "./python[dev]"       # macOS, Linux
```

```powershell
python -m venv python\.venv
python\.venv\Scripts\python -m pip install -e ".\python[dev]"
```

The seven commands land in `python/.venv/Scripts` (Windows) or `python/.venv/bin` (macOS,
Linux). Put that directory on `PATH`; activating the environment does it for the current
terminal:

```bash
source python/.venv/Scripts/activate     # Git Bash on Windows
source python/.venv/bin/activate         # macOS, Linux
```

```powershell
python\.venv\Scripts\Activate.ps1        # if scripts are blocked: Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
```

To keep it on `PATH` in every new terminal without activating, add the directory itself.
Git Bash, macOS, or Linux, in `~/.bashrc` or `~/.zshrc`:

```bash
export PATH="/path/to/jev-claude/python/.venv/bin:$PATH"     # Git Bash: .../python/.venv/Scripts
```

PowerShell, your user `Path`:

```powershell
$dir = "C:\path\to\jev-claude\python\.venv\Scripts"
[Environment]::SetEnvironmentVariable("Path", "$dir;" + [Environment]::GetEnvironmentVariable("Path", "User"), "User")
```

`[dev]` adds only ruff and mypy; leave it out (`pip install -e ./python`) if you only want to run
the programs.

### Option B: pipx

pipx gives the programs an environment of their own and puts the commands on `PATH`:

```bash
pipx install ./python          # from the repository root
pipx ensurepath                # once, then open a new terminal
```

This is a copy, so set `JEV_ROOT` to the clone permanently and run
`pipx install --force ./python` after each `git pull`:

```bash
echo 'export JEV_ROOT="/path/to/jev-claude"' >> ~/.bashrc          # Git Bash / Linux; ~/.zshrc on macOS
```

```powershell
[Environment]::SetEnvironmentVariable("JEV_ROOT", "C:\path\to\jev-claude", "User")
```

### Option C: no install

Put `python/src` on `PYTHONPATH` and run the modules. From the repository root:

```bash
PYTHONPATH="$PWD/python/src" python -m jev_router.cli.claude        # Git Bash: PYTHONPATH="$(cygpath -w "$PWD/python/src")"
```

```powershell
$env:PYTHONPATH = "$PWD\python\src"; python -m jev_router.cli.claude
```

Keep `PYTHONPATH` set while Claude Code runs: the status line it starts imports the package the
same way.

### Next to the Node, Go and Rust installs

If you also ran the installer, Node's `jev-claude` is on `PATH` as well, and whichever directory
comes first on `PATH` wins. To see which one runs, use `command -v jev-claude` (Bash) or
`Get-Command jev-claude -All` (PowerShell). To keep Node as the default and still try Python,
leave the virtual environment off `PATH` and call it by path, or define a short name:

```bash
alias jev-claude-py='/path/to/jev-claude/python/.venv/bin/jev-claude'      # Git Bash: .../Scripts/jev-claude.exe
```

```powershell
function jev-claude-py { & "C:\path\to\jev-claude\python\.venv\Scripts\jev-claude.exe" @args }
```

Every implementation shares the same files: settings, the status directory, the decision log,
and the update state (SPEC 2.5). Use whichever you like, one session at a time or side by side.

## Run jev-claude

Give `jev-claude` the same arguments you would give `claude`:

```bash
jev-claude                         # interactive session
jev-claude -p "what is 2+2?"       # one-shot
jev-claude --resume                # any claude flag passes through
```

What it needs:

- **The real Claude Code CLI on `PATH`** as `claude`. On Windows the launcher finds npm's
  `claude.cmd` shim and runs the script behind it with `node` from `PATH`, so arguments are
  never re-parsed by `cmd.exe`; without `node` it falls back to `cmd.exe` with full quoting.
- **A Jev API key** in `~/.jev-router.env` (see [Configuration](#configuration)). Without one,
  Claude Code starts without routing and the launcher says how to turn it on.
- **Node.js and `pnpm install` at the repository root**, for the slash commands only.
  `/jev-calibrate`, `/jev-explain` and `/jev-legend` are skills in `.claude/skills/` that run
  `node node/bin/...`, whichever implementation launched the session. The status files they read
  have the same format in every port.

On the first interactive run, the launcher offers to check your setup with `/jev-calibrate`.
Then it starts the proxy on a loopback port and points Claude Code at it with
`ANTHROPIC_BASE_URL`. It adds **Jev Router** to `/model`, installs its status line (unless you
already have one or set `JEV_NO_STATUSLINE`), waits for Claude Code to exit, and exits with
Claude Code's exit code. Like the Node launcher, it does not print an update notice or accept
`--update` (SPEC 1.2); `jev-update-check` is there for when that wiring lands.

You can also run the helper programs directly, for example `jev-explain <session id>`,
`jev-legend` or `jev-check`.

## Configuration

The Python programs read the same files and variables as Node. A variable already set in the
environment always wins over the files:

| Source | What it may set |
| --- | --- |
| The process environment | Anything |
| `~/.jev-router.env` (and the older `~/.jev-claude.env`) | Anything; this is your own settings file |
| `.env` in the directory you start `jev-claude` from | Only `JEV_API_KEY`, `TYPESAFE_API_KEY`, `JEV_DEBUG`, `JEV_ALLOW_FABLE`, `JEV_NO_STATUSLINE`, `JEV_ICONS`, and the `JEV_*_EFFORT` / `JEV_*_FORCE_EFFORT` keys, so a cloned repository can never redirect your traffic |

Start from the commented template:

```bash
cp .env.example ~/.jev-router.env                      # Git Bash, macOS, Linux
```

```powershell
Copy-Item .env.example "$HOME\.jev-router.env"
```

Then paste your key after `JEV_API_KEY=`.

| Variable | Effect |
| --- | --- |
| `JEV_API_KEY` (or `TYPESAFE_API_KEY`) | Turns routing on. It is sent only to Jev and is removed from Claude Code's environment |
| `ANTHROPIC_API_KEY` | Runs Claude Code on an API key instead of your sign-in. Set it only in your own file or shell |
| `JEV_ALLOW_FABLE` | `0`, `false`, `no` or `off` stops routing to Fable |
| `JEV_SONNET_EFFORT`, `JEV_OPUS_EFFORT`, `JEV_FABLE_EFFORT` | The effort a tier gets when Claude Code sends none (`low`, `medium`, `high`, `xhigh`, `max`) |
| `JEV_FORCE_EFFORT`, `JEV_<TIER>_FORCE_EFFORT` | An effort that replaces the one Claude Code sends; the per-tier key wins |
| `JEV_DEBUG` | Logs each routing decision to `~/.jev-claude.log` |
| `JEV_NO_STATUSLINE` | Leaves Claude Code's status line alone |
| `JEV_ICONS` | `symbols` or `text` for the status line labels; the default is symbols, except in a plain Windows console |
| `ANTHROPIC_BASE_URL` | When set before launch, the proxy forwards to it instead of `https://api.anthropic.com` |
| `ANTHROPIC_MODEL` | Starts the session on that model instead of Jev Router |
| `TYPESAFE_BASE_URL`, `TYPESAFE_DEFAULT_MODEL` | Point the router at another Jev endpoint or model. Read once per process, when a key is first seen (SPEC 20.3) |
| `JEV_ROOT` | The repository clone, for programs installed outside it |
| `JEV_STATUS_DIR` | The status directory; the default is `<temp>/jev-claude` |
| `JEV_DUMP` | `1` saves each request body into the status directory; any other value is a path prefix. This is for debugging only, because bodies hold your conversation |

`PYTHONPATH` matters only for [Option C](#option-c-no-install). The root
[README's Configuration section](../README.md#configuration) explains each setting in more
depth.

## Tests

From the repository root, with any CPython 3.12+ (the tests need no third-party packages):

```bash
python -m unittest discover -s python/tests -t python
```

The run takes about 40 seconds. On Windows it ends with `OK (skipped=2)`: the POSIX home-directory
rule and the file-mode check do not apply there; on macOS and Linux the `cmd.exe` shim test is
the one skipped. The tests need `git` on
`PATH`; the launcher-shim and home-directory tests also run `node`. They run offline: every
server they talk to is a loopback fake. They cover:

| Test file | What it covers |
| --- | --- |
| `test_<module>.py` for each `node/test/*.test.mjs` | The Node unit tests, ported one file per file, with each Node title as the test docstring (`bump-version` is out of scope, SPEC 1.2). They use real temporary files, real git repositories (a bare repository stands in for GitHub), real child processes, and loopback HTTP servers |
| `test_jsstr.py`, `test_jsjson.py`, `test_osdirs.py`, `test_repo.py` | The JavaScript-compatibility helpers (SPEC 3), with expected values taken from Node 24 |
| `test_router.py` | The Jev client written in place of the SDK: headers, one retry, the per-attempt timeout (connect included), the 3 s deadline, and settings read once per process |
| `test_hardening.py` | Besides Node's hardening tests: a client leaving a connection-close framed stream ends the upstream exchange, and a client reset prints nothing on the terminal |
| `test_check.py` | `jev-check`'s report from a real calibration file |
| `test_golden.py` | Every case in `conformance/cases/*.json` (SPEC 16.2), each as its own `subTest`, including `jev-request` against a loopback fake Jev and `status-line` against the real `jev-statusline` process |

`python/tests/__init__.py` points `JEV_STATUS_DIR` at a throwaway directory before anything
imports the package, and puts `python/src` on `sys.path`, so the tests never touch your real
status files and need no install. Table-driven tests use `self.subTest`, so every failing row is
reported by name.

To run one file, class or test, from `python/`:

```bash
python -m unittest tests.test_router -v
python -m unittest tests.test_proxy.ApplyTier -v
python -m unittest tests.test_golden.Golden.test_decide -v
```

## Lint, format and type-check

The configuration is checked in, in [`python/pyproject.toml`](../python/pyproject.toml): ruff
(formatter and linter) under `[tool.ruff]`, mypy under `[tool.mypy]`, and the pinned versions
in the `dev` extra (`ruff~=0.16.10`, `mypy~=2.4`). Install them into the repository's virtual
environment as in [Option A](#option-a-an-editable-install-in-the-repositorys-virtual-environment-recommended),
activate it, then from the repository root:

```bash
python -m ruff format --check python                                    # "N files already formatted"; `ruff format python` fixes
python -m ruff check python                                             # "All checks passed!"
python -m mypy --config-file python/pyproject.toml --platform win32     # "Success: no issues found"
python -m mypy --config-file python/pyproject.toml --platform linux
```

Each must exit 0. `--platform` makes each run check the code as that platform sees it, so a
Windows machine also checks the POSIX branches and the reverse; macOS reads the same branches as
Linux. The tools are dev-only: the `dev` extra is an optional dependency (rather than a PEP 735
dependency group, because `pip install --group` needs pip 25.1 and Python 3.12 ships pip 24),
and the package itself declares no dependencies.

What is enforced:

- **ruff format**, line length 120 (the width the port was written to, as `rust/rustfmt.toml`).
- **ruff check** with pycodestyle (`E`, `W`), pyflakes (`F`), import sorting (`I`), pyupgrade for
  3.12 (`UP`), bugbear (`B`), shadowed builtins (`A`), blind excepts (`BLE`), comprehensions
  (`C4`), `PIE`, `SIM`, Ruff's own rules (`RUF`), pylint errors, warnings and conventions (`PLE`,
  `PLW`, `PLC`), and docstrings on the public API (`D`, pep257 convention). The few rules turned
  off are listed in `pyproject.toml`, each with its reason: `SIM105`/`SIM108` (the ported
  functions keep Node's try/catch and if/else shapes), `D401`/`D105`/`D107`, `E501` for the Jev
  prose in `config.py` (kept one string per line to diff against `node/src/config.mjs`), and `D`
  in tests (a test docstring is the Node test title, verbatim).
- **mypy `strict = true`** over `src/jev_router` and `tests`, plus `warn_unreachable` and the
  `ignore-without-code`, `redundant-expr`, `truthy-bool` and `possibly-undefined` error codes.

Fix a finding rather than suppressing it. A deliberate exception is a `# noqa: <rule> - <reason>`
on the line (or the reason on the line above it), and RUF100 rejects a `noqa` that no longer
applies. There is no `# type: ignore` in the code.

## Code style

The port reproduces JavaScript semantics in Python, and a few idioms follow from that:

- **`UNDEFINED` is JavaScript's `undefined`** (`jsstr.Undefined.UNDEFINED`, a one-member enum, so
  `value is UNDEFINED` narrows types the way `value is None` does). `None` is `null`. Object
  literals keep `UNDEFINED` values, and `jsjson.stringify` omits them, as `JSON.stringify` does.
- **Every JSON number is a `float`** (`jsjson.parse` reads integers as floats), so arithmetic and
  formatting follow IEEE doubles; `jsstr.number_to_string` prints them as Node does.
- **`JsValue` and `JsObject`** (`jsstr`) type JSON values. Functions that accept anything take
  `object` and narrow with `isinstance`; the `_get`/`_prop` helpers are `obj?.key` and `obj.key`,
  including the `TypeError` JavaScript throws for `null.key`. `coalesce` is `??`;
  `coalesce_js` is the same for operands that are all JSON values, typed as one. Typed records
  (`TierSpec`, `Thresholds`, `Decision`, `Agent`, `RouteRequest`, `Calibration`, `LaunchSpec`)
  are `TypedDict`s, so they stay plain dicts and serialise as JSON objects.
- **`os.path`, not `pathlib`**: Node's paths are strings, and the shared files must be named
  byte for byte the same.
- **Broad `except Exception` mirrors Node's `catch {}`** where SPEC.md says errors are swallowed
  (status files, the log, the update check, routing). Each one carries
  `# noqa: BLE001 - <reason>`, the reason taken from the matching Node catch comment. Narrow
  excepts (`OSError` around socket cleanup) are used where the failure set is known.
- **Regular expressions** written for JavaScript's `i` flag compile with `re.IGNORECASE |
  re.ASCII`, and `\s` is spelled out as `jsstr.JSWS` (SPEC 3.1).
- **Tests use `Any` only for decoded JSON** (as `json.loads` types it), after narrowing the top
  level with the `as_dict`/`as_list`/`present` helpers in `tests/__init__.py`, which fail the test
  on the wrong shape.

## Conformance harness

The harness in `conformance/harness/` treats the proxy and the status line as black boxes,
started as child processes. Install the Node dependencies once (`pnpm install` at the repository
root), then run it from the repository root. Give the interpreter's real path
(`sys.executable`): Node's `spawn` runs it without a shell.

Git Bash (Windows):

```bash
PY="$(python -c 'import sys; print(sys.executable)')"
export PYTHONPATH="$(cygpath -w "$PWD/python/src")"
export JEV_IMPL_CMD_PROXY="\"$PY\" -m jev_router.cli.proxy_host"
export JEV_IMPL_CMD_STATUSLINE="\"$PY\" -m jev_router.cli.statusline"
node --test conformance/harness
unset PYTHONPATH JEV_IMPL_CMD_PROXY JEV_IMPL_CMD_STATUSLINE
```

macOS and Linux: the same commands with `python3` (or the virtual environment's `python`) and
`export PYTHONPATH="$PWD/python/src"`, without `cygpath`.

PowerShell:

```powershell
$py = python -c "import sys; print(sys.executable)"
$env:PYTHONPATH = "$PWD\python\src"
$env:JEV_IMPL_CMD_PROXY = "`"$py`" -m jev_router.cli.proxy_host"
$env:JEV_IMPL_CMD_STATUSLINE = "`"$py`" -m jev_router.cli.statusline"
node --test conformance/harness
Remove-Item Env:JEV_IMPL_CMD_PROXY, Env:JEV_IMPL_CMD_STATUSLINE, Env:PYTHONPATH
```

All 17 harness tests must pass. `PYTHONPATH` passes through the harness's environment filter,
which removes only `JEV_*`, `TYPESAFE_*`, `ANTHROPIC_*` and `CLAUDE_*`. With an editable install
(Option A) you can point both commands at the virtual environment's interpreter instead and
leave `PYTHONPATH` unset. The golden cases need no separate step, because the unittest run
already checks them.

## What CI runs

From the repository root, on Windows (the gating platform, SPEC 16) and on Linux. The first two
lines create the tool environment; on Windows the interpreter is
`python/.venv/Scripts/python`, elsewhere `python/.venv/bin/python` (shown as `$PY` below).

```bash
python -m venv python/.venv
$PY -m pip install -e "./python[dev]"
$PY -m ruff format --check python
$PY -m ruff check python
$PY -m mypy --config-file python/pyproject.toml --platform win32
$PY -m mypy --config-file python/pyproject.toml --platform linux
$PY -m compileall -q python/src
$PY -m unittest discover -s python/tests -t python
# then the conformance harness, as in the previous section
```

## Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| `[jev] Claude Code is not installed, or claude is not on your PATH` | Install Claude Code (https://code.claude.com/docs/en/setup), then check with `claude --version` |
| `[jev] no JEV_API_KEY found - starting Claude Code without routing` | Put `JEV_API_KEY=...` in `~/.jev-router.env`. A project `.env` works too, but only in that directory |
| The status line shows `No module named jev_router` or nothing | The interpreter that started `jev-claude` cannot import the package. Use an install (Option A or B), or keep `PYTHONPATH` set (Option C) |
| `/jev-explain`, `/jev-legend` or `/jev-calibrate` is missing | The package is installed outside the clone (pipx or a regular `pip install`) and `JEV_ROOT` is unset, so `--add-dir` was not passed. Set `JEV_ROOT`, or use an editable install |
| A `/jev-*` skill fails with a Node error | The skills run Node: install Node.js 22+ and run `pnpm install` at the repository root |
| `jev-check` says `Mode installed` although you run from a clone | Same cause: set `JEV_ROOT` to the clone |
| `python` opens the Microsoft Store, or the harness cannot start `python` | Windows' App execution aliases shadow the real interpreter. Turn them off, use `py -3.12`, and give the harness the full path from `sys.executable` |
| The harness fails with `spawn EINVAL` (PowerShell, pyenv-win) | `(Get-Command python).Source` is pyenv-win's `python.bat` shim, which Node's shell-less `spawn` rejects. Use `python -c "import sys; print(sys.executable)"`, as in [Conformance harness](#conformance-harness) |
| `pip install --group dev` fails | Groups need pip 25.1+. The dev tools are an extra: `pip install -e "./python[dev]"` |
| `error: externally-managed-environment` | Your distribution protects the system Python. Use the virtual environment (Option A) or pipx |
| `Activate.ps1 cannot be loaded because running scripts is disabled` | `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`, or call `python\.venv\Scripts\python.exe` directly |
| mypy reports errors only with `--platform linux` (or `win32`) | A platform-specific branch: guard it with a literal `if sys.platform == "win32":` (mypy reads only the matching branch) |
| A test fails with `git` or `node` not found | Put both on `PATH`; the tests start them as real processes |
| You want to see routing decisions | Set `JEV_DEBUG=1`, then read `~/.jev-claude.log`. In a terminal, the launcher prints the log's path at start |

## Behaviour notes

These are places where the Python code differs in mechanism but matches Node's behaviour.

- **Threads, not an event loop (SPEC 3.10).** Requests are served on threads. A process-wide
  lock guards every status-file read-modify-write and calibration write, and the proxy's
  `convos`, `mains` and `catalog` sit under one lock that is released while Jev is asked.
- **The Jev client (SPEC 5).** It is written against the wire contract with `http.client`,
  instead of the TypeSafe SDK. Each attempt runs on its own thread with a socket timeout of the
  remaining per-attempt budget (never more than 1500 ms), and it connects explicitly before
  sending, so an attempt abandoned at the timeout or the 3 s deadline while still connecting
  sees that it was cancelled and never sends its POST ("aborts whatever is in flight"). The key,
  base URL and default model are read from the environment once per process, when a key is
  first present (SPEC 20.3), as Node's lazily built client keeps them.
- **Client disconnects (SPEC 7.2 step 7).** Node learns of them from socket events; this port
  watches the client socket on a helper thread while a request is in flight. When the client
  leaves, the watcher shuts down the upstream's raw socket, which also ends a reply framed by
  connection close (one that `http.client` hands over to the response object).
- **No tracebacks on the terminal.** The proxy server's `handle_error` logs a client that
  resets or abandons its connection only under `JEV_DEBUG` (Node is silent on `ECONNRESET`),
  and logs any other failure as one `[jev]` line, never a traceback: in the launcher, stderr is
  Claude Code's terminal.
- **Response framing.** Python's HTTP server does not frame for us, so the proxy writes
  `content-length` when the upstream gave one and re-chunks otherwise (SPEC 7.7 allows framing
  headers). It never adds a `Server` header.
- **Rename on Windows.** Replacing a status file retries briefly when a reader holds it open
  without delete sharing (a Python reader does; libuv readers do not). A failed write removes
  its temporary file.
- **Signals (SPEC 10 step 8).** On Unix, SIGHUP and SIGTERM are forwarded to Claude Code. On
  Windows, console close, logoff and shutdown are caught with `SetConsoleCtrlHandler`, and the
  child is ended with `TerminateProcess` (what Node's `child.kill` does there); cleanup runs
  before the process exits. Ctrl+C is left to Claude Code.

## Known divergences from Node

Each is permitted or required by SPEC.md; none shows in the golden cases or the harness. This
list is the only copy; `python/README.md` links here.

- **Temporary file names** are `<file>.<pid>.<seq>.tmp` (SPEC 3.10); Node uses
  `<file>.<pid>.tmp`, which is safe only because Node is single-threaded.
- **Jev request headers** are exactly SPEC 5.1's (`User-Agent: jev-router-python/<version>`),
  without the SDK's `X-TypeSafe-SDK`/`X-TypeSafe-Runtime`. The prewarm `HEAD` does not share its
  connection with the first call (SPEC 5.4 allows Python to skip that).
- **`claudeModels` ties** on equal versions are broken by `releasedAt` in plain UTF-16 code-unit
  order (SPEC 7.5, 19.1); Node calls `localeCompare`, which agrees on the ISO dates the catalog
  carries.
- **Splitting a surrogate pair**: where a UTF-16 cut would split a pair, the lone high surrogate
  is dropped (SPEC 3.2); Node keeps it. No real input reaches this.
- **`jev-check`'s "as of" time** is printed with the platform's local date and time format
  (`%x, %X`), not `toLocaleString()`; SPEC 13 excludes it from comparison.
- **A `.cmd` shim's script** runs with `node` from `PATH`, while Node uses its own executable.
  When there is no `node`, the `cmd.exe` route is used (SPEC 10.2).

## Source layout

| Path | What it holds |
| --- | --- |
| `python/pyproject.toml` | Project `jev-router` 0.0.0, `[project.scripts]` for the seven programs, the `dev` extra, and the `[tool.ruff]` and `[tool.mypy]` policy |
| `python/src/jev_router/` | One module per Node module in `node/src/` (`config`, `env`, `explain`, `first_run`, `icons`, `launch`, `legend`, `log`, `model_names`, `policy`, `proxy`, `reasons`, `router`, `settings`, `status`, `update`, `worktree`) |
| `python/src/jev_router/jsjson.py`, `jsstr.py` | JSON and string, number and regular-expression semantics as JavaScript has them (SPEC 3) |
| `python/src/jev_router/osdirs.py`, `repo.py` | Node's home and temp rules; the repository root and release version (SPEC 2.4, 3.8) |
| `python/src/jev_router/cli/` | One module per program, each with `main()` |
| `python/tests/` | The unittest suites; `support.py` holds the loopback servers, `__init__.py` the isolation and narrowing helpers |

Where SPEC.md is silent, the Node source is the reference; module docstrings name the Node file
each one ports, and comments name the SPEC section.
