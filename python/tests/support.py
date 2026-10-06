"""Real loopback HTTP servers and a client for the proxy tests (no mocks: real sockets)."""

import http.client
import http.server
import json
import threading


class Upstream:
    """A loopback HTTP server that records every request in full, then lets `handle` answer.

    `handle(record, handler)` writes the response with the handler's own methods; the default
    answers a small JSON message.
    """

    def __init__(self, handle=None):
        self.requests = []
        recorder = self

        class Handler(http.server.BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"
            disable_nagle_algorithm = True

            def log_message(self, *args):
                pass

            def _any(self):
                length = int(self.headers.get("content-length") or 0)
                body = self.rfile.read(length) if length else b""
                record = {"method": self.command, "url": self.path, "headers": {k.lower(): v for k, v in self.headers.items()},
                          "body": body}
                recorder.requests.append(record)
                (handle or Upstream.json_message)(record, self)

            do_GET = do_POST = do_PUT = do_DELETE = _any

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        # Tests that abandon connections on purpose (timeouts, aborts) are not server errors.
        self.server.handle_error = lambda request, address: None
        self.port = self.server.server_address[1]
        self.url = f"http://127.0.0.1:{self.port}"
        threading.Thread(target=self.server.serve_forever, kwargs={"poll_interval": 0.1}, daemon=True).start()

    @staticmethod
    def reply(handler, status, body: bytes, content_type="application/json"):
        handler.send_response(status)
        handler.send_header("content-type", content_type)
        handler.send_header("content-length", str(len(body)))
        handler.end_headers()
        handler.wfile.write(body)

    @staticmethod
    def json_message(record, handler):
        Upstream.reply(handler, 200, b'{"id":"msg_1","type":"message"}')

    def bodies(self):
        return [json.loads(r["body"]) if r["body"] else None for r in self.requests]

    def close(self):
        self.server.shutdown()
        self.server.server_close()


def request(port, method, path, body=None, headers=None, timeout=30):
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


def post(port, body, path="/v1/messages", headers=None):
    return request(port, "POST", path, body, headers)
