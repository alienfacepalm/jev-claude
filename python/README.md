# jev-claude in Python

A port of the Node.js implementation in [`../node`](../node) to CPython 3.12+, standard library
only. [`../SPEC.md`](../SPEC.md) is the specification; where it is silent, the Node source is.

**The full guide is [`../doc/PYTHON.md`](../doc/PYTHON.md)**: installing Python on Windows,
macOS and Linux, installing the programs side by side and on `PATH`, configuration, tests,
lint, format and type-check, the conformance harness, troubleshooting, and the known
divergences from Node.

## Quick start

Needs CPython 3.12+, git, Claude Code (`claude` on `PATH`), and Node.js 22+ with `pnpm install`
run at the repository root (the `/jev-*` skills and the harness are Node programs). From the
repository root:

```bash
python -m venv python/.venv
python/.venv/bin/python -m pip install -e "./python[dev]"   # Windows: python/.venv/Scripts/python
source python/.venv/bin/activate                            # Git Bash: python/.venv/Scripts/activate; PowerShell: python\.venv\Scripts\Activate.ps1
cp .env.example ~/.jev-router.env                           # then paste your key after JEV_API_KEY=
jev-claude                                                  # any claude arguments pass through
```

The editable install keeps the programs inside the clone, so they find it on their own. A copy
installed elsewhere (pipx, a regular `pip install`) needs `JEV_ROOT=<clone>`; see
[Install the programs and put them on PATH](../doc/PYTHON.md#install-the-programs-and-put-them-on-path).

## Check a change

From the repository root, with the virtual environment active:

```bash
python -m ruff format --check python
python -m ruff check python
python -m mypy --config-file python/pyproject.toml --platform win32
python -m mypy --config-file python/pyproject.toml --platform linux
python -m unittest discover -s python/tests -t python      # ported Node tests and every golden case
```

Then run the conformance harness against this port as in
[Conformance harness](../doc/PYTHON.md#conformance-harness).

## Layout

| Path | Contents |
| --- | --- |
| `pyproject.toml` | The project, the seven `[project.scripts]`, the `dev` extra (ruff, mypy), and the `[tool.ruff]` and `[tool.mypy]` policy |
| `src/jev_router/` | One module per Node module, plus `jsjson`, `jsstr`, `osdirs` and `repo` |
| `src/jev_router/cli/` | The seven programs: `claude`, `statusline`, `explain`, `legend`, `check`, `update_check`, `proxy_host` |
| `tests/` | unittest suites; `support.py` holds the loopback servers |
