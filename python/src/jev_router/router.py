"""Asking Jev which tier fits a prompt (port of node/src/router.mjs, SPEC 5 and 6).

The Jev client is written against the wire contract in SPEC 5: one POST per attempt, each
bounded at 1500 ms covering connect, send and the whole body, at most one retry, and a 3000 ms
deadline for the whole call that also cuts a backoff wait short.
"""

from __future__ import annotations

import dataclasses
import email.utils
import http.client
import math
import os
import random
import socket
import threading
import time
import urllib.parse
from collections.abc import Mapping, Sequence
from typing import Final

from . import jsjson, repo
from .config import COMPLEXITY_MAX_SCORE, CONTEXT_WINDOW_TOKENS, QUESTIONS, THRESHOLDS, question_for_models
from .jsstr import UNDEFINED, JsObject, JsValue, js_trim, math_round, to_number, to_string
from .log import log

DEFAULT_BASE_URL: Final = "https://api.typesafe.ai"
RETRY_STATUSES: Final = {408, 429} | set(range(500, 600))
MAX_RETRY_AFTER_MS: Final = 60000
BACKOFF_INITIAL_MS: Final = 150
BACKOFF_MAX_MS: Final = 400
BACKOFF_JITTER: Final = 0.25


class JevError(Exception):
    """A Jev call that failed; `ask_jev` turns every one into "no answer"."""


class _Retryable(JevError):
    """A failure the policy retries: a connection error, a timeout, or a retryable status."""

    def __init__(self, message: str, headers: Mapping[str, str] | None = None) -> None:
        super().__init__(message)
        self.headers = headers


def base_url(env: Mapping[str, str] | None = None) -> str:
    """`TYPESAFE_BASE_URL`, trimmed, without trailing slashes; the public endpoint when unset."""
    env = os.environ if env is None else env
    value = env.get("TYPESAFE_BASE_URL")
    value = js_trim(value) if isinstance(value, str) else ""
    return (value or DEFAULT_BASE_URL).rstrip("/")


def api_key(env: Mapping[str, str] | None = None) -> str | None:
    """`JEV_API_KEY ?? TYPESAFE_API_KEY` (an empty value counts); None when both are absent."""
    env = os.environ if env is None else env
    if "JEV_API_KEY" in env:
        return env["JEV_API_KEY"]
    if "TYPESAFE_API_KEY" in env:
        return env["TYPESAFE_API_KEY"]
    return None


def default_model(env: Mapping[str, str] | None = None) -> str:
    """`TYPESAFE_DEFAULT_MODEL`, trimmed; `jev-latest` when unset or blank."""
    env = os.environ if env is None else env
    value = env.get("TYPESAFE_DEFAULT_MODEL")
    value = js_trim(value) if isinstance(value, str) else ""
    return value or "jev-latest"


def user_agent() -> str:
    """The User-Agent every Jev request carries (SPEC 5.1)."""
    return f"jev-router-python/{repo.release_version()}"


def retry_after_ms(headers: Mapping[str, str] | None, now_ms: float | None = None) -> float | None:
    """A server-requested delay, or None (SPEC 5.3)."""
    if headers is None:
        return None
    if "retry-after-ms" in headers:
        ms = to_number(headers["retry-after-ms"])
        if math.isfinite(ms) and ms >= 0:
            return ms
    raw = headers.get("retry-after")
    if raw is None:
        return None
    seconds = to_number(raw)
    if math.isfinite(seconds):
        return seconds * 1000 if seconds >= 0 else None
    try:
        moment = email.utils.parsedate_to_datetime(raw)
    except (TypeError, ValueError, IndexError):
        return None
    now_ms = time.time() * 1000 if now_ms is None else now_ms
    return max(0, moment.timestamp() * 1000 - now_ms)


def retry_delay_ms(attempt: int, headers: Mapping[str, str] | None = None) -> float:
    """The wait before a retry: the server's Retry-After when sane, else jittered backoff (SPEC 5.3)."""
    if headers is not None:
        server = retry_after_ms(headers)
        if server is not None and server <= MAX_RETRY_AFTER_MS:
            return server
    exponential = min(BACKOFF_INITIAL_MS * 2**attempt, BACKOFF_MAX_MS)
    return math_round(exponential * (1 - random.random() * BACKOFF_JITTER))


class _Attempt:
    """One HTTP round trip on its own thread, so it can be abandoned at the timeout.

    `timeout` bounds every blocking socket operation, connect included. An attempt abandoned
    while it is still connecting sees `cancelled` once the connection is up and never sends its
    POST (SPEC 5.3: the deadline "aborts whatever is in flight").
    """

    def __init__(self, url: str, headers: list[tuple[str, str]], body: bytes, timeout: float) -> None:
        self.url = url
        self.headers = headers
        self.body = body
        self.timeout = timeout
        self.connection: http.client.HTTPConnection | None = None
        self.result: tuple[int, dict[str, str], bytes] | None = None
        self.error: Exception | None = None
        self.done = threading.Event()
        self.cancelled = False

    def run(self) -> None:
        try:
            parts = urllib.parse.urlsplit(self.url)
            port = parts.port
            host = parts.hostname
            if host is None:
                raise OSError(f"Invalid URL: {self.url}")  # reported as a connection error, and retried
            if parts.scheme == "http":
                self.connection = http.client.HTTPConnection(host, port, timeout=self.timeout)
            elif parts.scheme == "https":
                self.connection = http.client.HTTPSConnection(host, port, timeout=self.timeout)
            else:
                raise JevError(f"unsupported protocol {parts.scheme}:")
            path = parts.path or "/"
            if parts.query:
                path += "?" + parts.query
            connection = self.connection
            # Connect explicitly: until it returns there is no socket for abort() to shut down.
            connection.connect()
            if self.cancelled:
                return
            connection.putrequest("POST", path, skip_accept_encoding=True)
            for name, value in self.headers:
                connection.putheader(name, value)
            connection.putheader("Content-Length", str(len(self.body)))
            connection.endheaders(self.body)
            response = connection.getresponse()
            data = response.read()
            self.result = (response.status, {k.lower(): v for k, v in response.getheaders()}, data)
        except Exception as error:  # noqa: BLE001 - every failure is reported to the waiting caller
            self.error = error
        finally:
            if self.connection is not None:
                try:
                    self.connection.close()
                except OSError:
                    pass
            self.done.set()

    def abort(self) -> None:
        self.cancelled = True
        connection = self.connection
        # Read the socket once: the worker's close() sets `connection.sock` to None at any moment.
        sock = connection.sock if connection is not None else None
        if sock is None:
            return
        try:
            sock.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass
        try:
            sock.close()
        except OSError:
            pass


@dataclasses.dataclass(frozen=True)
class JevSettings:
    """The Jev key, base URL and default model (SPEC 5.1)."""

    key: str
    base_url: str
    model: str


_fixed_settings: JevSettings | None = None
_settings_lock = threading.Lock()


def _read_settings(env: Mapping[str, str]) -> JevSettings | None:
    key = api_key(env)
    if key is None:
        return None
    return JevSettings(key, base_url(env), default_model(env))


def settings(env: Mapping[str, str] | None = None) -> JevSettings | None:
    """The Jev settings, or None without a key.

    With the process environment they are read once, when a key is first present, and kept for
    the process, as Node's lazily built client keeps them (SPEC 20.3). An explicit `env` (tests,
    golden cases) is read afresh every time.
    """
    global _fixed_settings  # noqa: PLW0603 - read once per process, as Node's lazily built client (SPEC 20.3)
    if env is not None:
        return _read_settings(env)
    with _settings_lock:
        if _fixed_settings is None:
            _fixed_settings = _read_settings(os.environ)
        return _fixed_settings


def reset_settings() -> None:
    """Forgets the settings kept for the process (tests)."""
    global _fixed_settings  # noqa: PLW0603 - the module-level cache `settings` keeps
    with _settings_lock:
        _fixed_settings = None


def system_one(request: Mapping[str, JsValue], env: Mapping[str, str] | None = None) -> JsValue:
    """POST {base}/v1/systemone with retries; returns the parsed response body or raises."""
    started = time.monotonic()
    deadline = started + THRESHOLDS["jevDeadlineMs"] / 1000
    fixed = settings(env)
    if fixed is None:
        raise JevError("The TYPESAFE_API_KEY environment variable is missing or empty")
    body = jsjson.dumps_bytes({**request, "model": fixed.model})
    url = f"{fixed.base_url}/v1/systemone"
    headers = [
        ("Authorization", f"Bearer {fixed.key}"),
        ("Accept", "application/json"),
        ("User-Agent", user_agent()),
        ("Content-Type", "application/json"),
    ]
    max_retries = THRESHOLDS["jevMaxRetries"]
    attempt = 0
    while True:
        retries_left = max_retries - attempt
        attempt_headers = headers if attempt == 0 else [*headers, ("X-TypeSafe-Retry-Count", str(attempt))]
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise JevError("Request was aborted.")
        timeout = THRESHOLDS["jevTimeoutMs"] / 1000
        budget = min(timeout, remaining)
        job = _Attempt(url, attempt_headers, body, budget)
        thread = threading.Thread(target=job.run, name="jev-attempt", daemon=True)
        thread.start()
        finished = job.done.wait(budget)
        if not finished:
            job.abort()
            if deadline - time.monotonic() <= 0:
                raise JevError("Request was aborted.")
            failure = _Retryable(f"Request timed out after {THRESHOLDS['jevTimeoutMs']}ms.")
            headers_for_delay: Mapping[str, str] | None = None
        elif job.error is not None:
            if isinstance(job.error, JevError) and not isinstance(job.error, _Retryable):
                raise job.error
            failure = _Retryable(f"Connection error: {job.error}")
            headers_for_delay = None
        elif job.result is not None:
            status, response_headers, data = job.result
            if 200 <= status < 300:
                text = jsjson.decode_bytes(data)
                if text == "":
                    return UNDEFINED
                try:
                    return jsjson.parse(text)
                except jsjson.ParseError:
                    return text
            if retries_left <= 0 or status not in RETRY_STATUSES:
                raise JevError(f"{status} status code")
            failure = _Retryable(f"{status} status code", response_headers)
            headers_for_delay = response_headers
        else:  # done without a result or an error: cannot happen, but never loop on it
            raise JevError("Jev attempt ended without a result")
        if retries_left <= 0:
            raise failure
        delay = retry_delay_ms(attempt, headers_for_delay) / 1000
        remaining = deadline - time.monotonic()
        if delay >= remaining:
            time.sleep(max(0, remaining))
            raise JevError("Request was aborted.")
        time.sleep(delay)
        attempt += 1


def _prop(obj: object, key: str) -> JsValue:
    if obj is None or obj is UNDEFINED:
        raise TypeError(f"Cannot read properties of {to_string(obj)} (reading '{key}')")
    if isinstance(obj, dict):
        value: JsValue = obj.get(key, UNDEFINED)
        return value
    return UNDEFINED


def _score(answer: object) -> float:
    return to_number(_prop(answer, "score")) / COMPLEXITY_MAX_SCORE


def ask_jev(
    prompt: JsValue = None,
    current: JsValue = None,
    context_tokens: JsValue = 0,
    models: Sequence[Mapping[str, JsValue]] | None = None,
    env: Mapping[str, str] | None = None,
) -> JsObject | None:
    """Asks Jev which model fits; None on any failure (SPEC 6)."""
    if not models:
        return None
    started = time.monotonic()
    request: JsObject = {
        "state": {
            "request": prompt,
            "session": {"current_model": current, "context_tokens": context_tokens},
            "environment": {"available_models": [m.get("id", UNDEFINED) for m in models]},
        },
        "questions": {**QUESTIONS, "model": question_for_models(models)},
    }
    try:
        result = system_one(request, env)
        answers = _prop(result, "answers")
        if answers is None or answers is UNDEFINED:
            raise TypeError("Cannot destructure 'result.answers' as it is undefined.")
        answer = _prop(answers, "model")
        task = _prop(answers, "task_complexity")
        reasoning = _prop(answers, "reasoning_required")
        tool = _prop(answers, "tool_complexity")
        metrics: JsObject = {
            "taskComplexity": _score(task),
            "reasoningRequired": _score(reasoning),
            "toolComplexity": _score(tool),
            "contextSize": min(to_number(context_tokens) / CONTEXT_WINDOW_TOKENS, 1),
        }
        out = jsjson.spread(answer)
        out["request"] = request
        out["response"] = result
        out["metrics"] = metrics
        out["ms"] = int((time.monotonic() - started) * 1000)
        return out
    except Exception as error:  # noqa: BLE001 - routing must never block a prompt
        log(f"routing failed, keeping {to_string(current)}: {error}")
        return None


def prewarm(env: Mapping[str, str] | None = None) -> None:
    """One fire-and-forget HEAD to the Jev origin, ignoring every outcome (SPEC 5.4)."""
    env = os.environ if env is None else env
    base = env.get("TYPESAFE_BASE_URL")
    base = DEFAULT_BASE_URL if base is None else base

    def run() -> None:
        try:
            parts = urllib.parse.urlsplit(base)
            host = parts.hostname
            connection: http.client.HTTPConnection
            if host is None:
                return  # a malformed base URL fails properly on the first real call instead
            if parts.scheme == "https":
                connection = http.client.HTTPSConnection(host, parts.port, timeout=5)
            elif parts.scheme == "http":
                connection = http.client.HTTPConnection(host, parts.port, timeout=5)
            else:
                return
            connection.request("HEAD", "/")
            connection.getresponse().read()
            connection.close()
        except Exception:  # noqa: BLE001 - the prewarm only opens a connection early; every outcome is ignored
            pass

    threading.Thread(target=run, name="jev-prewarm", daemon=True).start()
