"""Real loopback HTTP servers and a client for the proxy tests (no mocks: real sockets)."""

import http.client
import http.server
import json
import threading
from collections.abc import Callable, Mapping
from typing import Any, TypedDict


class Recorded(TypedDict):
    """One request as the fake upstream received it."""

    method: str
    url: str
    headers: dict[str, str]
    body: bytes


# `handle(record, handler)` answers a recorded request with the handler's own methods.
type Handle = Callable[[Recorded, http.server.BaseHTTPRequestHandler], None]


class QuietServer(http.server.ThreadingHTTPServer):
    daemon_threads = True

    def handle_error(self, request: object, client_address: object) -> None:
        """Tests that abandon connections on purpose (timeouts, aborts) are not server errors."""


class Upstream:
    """A loopback HTTP server that records every request in full, then lets `handle` answer.

    `handle(record, handler)` writes the response with the handler's own methods; the default
    answers a small JSON message.
    """

    def __init__(self, handle: Handle | None = None) -> None:
        self.requests: list[Recorded] = []
        recorder = self
        answer: Handle = handle or Upstream.json_message

        class Handler(http.server.BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"
            disable_nagle_algorithm = True

            def log_message(self, format: str, *args: object) -> None:  # noqa: A002 - the base class's name
                pass

            def _any(self) -> None:
                length = int(self.headers.get("content-length") or 0)
                body = self.rfile.read(length) if length else b""
                record: Recorded = {
                    "method": self.command,
                    "url": self.path,
                    "headers": {k.lower(): v for k, v in self.headers.items()},
                    "body": body,
                }
                recorder.requests.append(record)
                answer(record, self)

            do_GET = do_POST = do_PUT = do_DELETE = _any

        self.server = QuietServer(("127.0.0.1", 0), Handler)
        self.port: int = self.server.server_address[1]
        self.url = f"http://127.0.0.1:{self.port}"
        threading.Thread(target=self.server.serve_forever, kwargs={"poll_interval": 0.1}, daemon=True).start()

    @staticmethod
    def reply(
        handler: http.server.BaseHTTPRequestHandler, status: int, body: bytes, content_type: str = "application/json"
    ) -> None:
        """Answers with `status`, a content type and a content-length body."""
        handler.send_response(status)
        handler.send_header("content-type", content_type)
        handler.send_header("content-length", str(len(body)))
        handler.end_headers()
        handler.wfile.write(body)

    @staticmethod
    def json_message(record: Recorded, handler: http.server.BaseHTTPRequestHandler) -> None:
        """The default answer: a small JSON message."""
        Upstream.reply(handler, 200, b'{"id":"msg_1","type":"message"}')

    def bodies(self) -> list[Any]:
        """Each recorded request body parsed as JSON (None for an empty one)."""
        return [json.loads(r["body"]) if r["body"] else None for r in self.requests]

    def close(self) -> None:
        """Stops the server and closes its socket."""
        self.server.shutdown()
        self.server.server_close()


def request(
    port: int,
    method: str,
    path: str,
    body: object = None,
    headers: Mapping[str, str] | None = None,
    timeout: float = 30,
) -> tuple[int, dict[str, str], bytes]:
    """One request to the proxy; returns (status, headers, body bytes)."""
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=timeout)
    try:
        data = body if isinstance(body, (bytes, type(None))) else json.dumps(body).encode("utf-8")
        all_headers = {"content-type": "application/json", **(headers or {})}
        connection.request(method, path, body=data, headers=all_headers)
        response = connection.getresponse()
        return response.status, {k.lower(): v for k, v in response.getheaders()}, response.read()
    finally:
        connection.close()


def post(
    port: int, body: object, path: str = "/v1/messages", headers: Mapping[str, str] | None = None
) -> tuple[int, dict[str, str], bytes]:
    """A POST to the proxy; returns (status, headers, body bytes)."""
    return request(port, "POST", path, body, headers)
