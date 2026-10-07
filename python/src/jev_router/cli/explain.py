"""jev-explain <sessionId>: the explanation panel (port of node/bin/jev-explain.mjs, SPEC 13)."""

from __future__ import annotations

import sys

from ..explain import format_agents, format_explanation
from ..jsstr import UNDEFINED, truthy
from ..status import main_decision, read_status
from ._io import write_stdout


def render(session_id: object) -> str:
    """The explanation panel and the agent table for a session."""
    status = read_status(session_id)
    main = main_decision(status)
    agents = format_agents(status)
    if truthy(main) and isinstance(status, dict) and truthy(status.get("manual", UNDEFINED)):
        shown = {**main, "manual": True} if isinstance(main, dict) else main
    else:
        shown = main
    return f"{format_explanation(shown)}\n{agents + chr(10) if agents else ''}"


def main() -> int:
    """Prints the explanation panel for the session id in argv[1]."""
    write_stdout(render(sys.argv[1] if len(sys.argv) > 1 else UNDEFINED))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
