"""jev-update-check: one background update check (port of node/bin/jev-update-check.mjs, SPEC 14)."""

from __future__ import annotations

from .. import repo
from ..update import check_for_update, write_state


def main() -> int:
    """Runs one update check and records it (nothing outside a repository checkout)."""
    if not repo.ROOT:
        return 0
    write_state(check_for_update(repo.ROOT))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
