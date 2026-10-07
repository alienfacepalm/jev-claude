"""jev-proxy-host: starts the proxy alone and prints its port (port of node/scripts/proxy-host.mjs).

Used by the conformance harness (SPEC 16.3): loads the env files, forwards to
ANTHROPIC_BASE_URL when set, writes `PORT=<port>` and runs until killed.
"""

from __future__ import annotations

import os
import threading

from ..env import load_env
from ..proxy import start_proxy
from ._io import write_stdout


def main() -> int:
    """Starts the proxy, prints `PORT=<port>` and serves until interrupted."""
    load_env()
    inherited = os.environ.get("ANTHROPIC_BASE_URL")
    proxy = start_proxy(upstream_url=inherited) if inherited else start_proxy()
    write_stdout(f"PORT={proxy.port}\n")
    stop = threading.Event()
    try:
        while not stop.wait(1):
            pass
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
