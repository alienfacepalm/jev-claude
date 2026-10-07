"""jev-legend: the status line key (port of node/bin/jev-legend.mjs, SPEC 13)."""

from __future__ import annotations

from ..legend import format_legend
from ._io import write_stdout


def main() -> int:
    """Prints the status line key."""
    write_stdout(f"Status line key\n\n{format_legend()}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
