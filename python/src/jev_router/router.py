"""Asking Jev which tier fits a prompt (port of node/src/router.mjs, SPEC 5 and 6).

The Jev client is written against the wire contract in SPEC 5: one POST per attempt, each
bounded at 1500 ms covering connect, send and the whole body, at most one retry, and a 3000 ms
deadline for the whole call that also cuts a backoff wait short.
"""

from __future__ import annotations

import email.utils
import http.client
import math
import os
import random
import threading
import time
import urllib.parse

from . import jsjson, repo
from .config import COMPLEXITY_MAX_SCORE, CONTEXT_WINDOW_TOKENS, QUESTIONS, THRESHOLDS, question_for_models
from .jsstr import UNDEFINED, js_trim, math_round, to_number, to_string
from .log import log

DEFAULT_BASE_URL = "https://api.typesafe.ai"
RETRY_STATUSES = {408, 429} | set(range(500, 600))
MAX_RETRY_AFTER_MS = 60000
BACKOFF_INITIAL_MS = 150
BACKOFF_MAX_MS = 400
BACKOFF_JITTER = 0.25


class JevError(Exception):
    pass


class _Retryable(JevError):
    """A failure the policy retries: a connection error, a timeout, or a retryable status."""

    def __init__(self, message, headers=None):
        super().__init__(message)
        self.headers = headers


def base_url(env=None) -> str:
    env = os.environ if env is None else env
    value = env.get("TYPESAFE_BASE_URL")
    value = js_trim(value) if isinstance(value, str) else ""
    return (value or DEFAULT_BASE_URL).rstrip("/")


def api_key(env=None):
    """`JEV_API_KEY ?? TYPESAFE_API_KEY` (an empty value counts); None when both are absent."""
    env = os.environ if env is None else env
    if "JEV_API_KEY" in env:
        return env["JEV_API_KEY"]
    if "TYPESAFE_API_KEY" in env:
        return env["TYPESAFE_API_KEY"]
    return None


def default_model(env=None) -> str:
    env = os.environ if env is None else env
    value = env.get("TYPESAFE_DEFAULT_MODEL")
    value = js_trim(value) if isinstance(value, str) else ""
    return value or "jev-latest"


def user_agent() -> str:
    return f"jev-router-python/{repo.release_version()}"


def retry_after_ms(headers, now_ms=None):
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
    if moment is None:
        return None
    now_ms = time.time() * 1000 if now_ms is None else now_ms
    return max(0, moment.timestamp() * 1000 - now_ms)


def retry_delay_ms(attempt, headers=None) -> float:
    if headers is not None:
        server = retry_after_ms(headers)
        if server is not None and server <= MAX_RETRY_AFTER_MS:
            return server
    exponential = min(BACKOFF_INITIAL_MS * 2**attempt, BACKOFF_MAX_MS)
    return math_round(exponential * (1 - random.random() * BACKOFF_JITTER))


class _Attempt:
    """One HTTP round trip on its own thread, so it can be abandoned at the timeout."""

    def __init__(self, url, headers, body):
        self.url = url
        self.headers = headers
        self.body = body
        self.connection = None
        self.result = None
        self.error = None
        self.done = threading.Event()
        self.cancelled = False

    def run(self):
        try:
            parts = urllib.parse.urlsplit(self.url)
            port = parts.port
            if parts.scheme == "http":
                self.connection = http.client.HTTPConnection(parts.hostname, port, timeout=5)
            elif parts.scheme == "https":
                self.connection = http.client.HTTPSConnection(parts.hostname, port, timeout=5)
            else:
                raise JevError(f"unsupported protocol {parts.scheme}:")
            path = parts.path or "/"
            if parts.query:
                path += "?" + parts.query
            connection = self.connection
            connection.putrequest("POST", path, skip_accept_encoding=True)
            for name, value in self.headers:
                connection.putheader(name, value)
            connection.putheader("Content-Length", str(len(self.body)))
            connection.endheaders(self.body)
            response = connection.getresponse()
            data = response.read()
            self.result = (response.status, {k.lower(): v for k, v in response.getheaders()}, data)
        except BaseException as error:  # noqa: BLE001 - reported to the waiting caller
            self.error = error
        finally:
            try:
                if self.connection is not None:
                    self.connection.close()
            except Exception:
                pass
            self.done.set()

    def abort(self):
        self.cancelled = True
        connection = self.connection
        if connection is not None and connection.sock is not None:
            try:
                connection.sock.shutdown(2)
            except Exception:
                pass
            try:
                connection.sock.close()
            except Exception:
                pass


def system_one(request: dict, env=None) -> object:
    """POST {base}/v1/systemone with retries; returns the parsed response body or raises."""
    env = os.environ if env is None else env
    started = time.monotonic()
    deadline = started + THRESHOLDS["jevDeadlineMs"] / 1000
    key = api_key(env)
    if key is None:
        raise JevError("The TYPESAFE_API_KEY environment variable is missing or empty")
    body = jsjson.dumps_bytes({**request, "model": default_model(env)})
    url = f"{base_url(env)}/v1/systemone"
    headers = [
        ("Authorization", f"Bearer {key}"),
        ("Accept", "application/json"),
        ("User-Agent", user_agent()),
        ("Content-Type", "application/json"),
    ]
    max_retries = THRESHOLDS["jevMaxRetries"]
    attempt = 0
    while True:
        retries_left = max_retries - attempt
        attempt_headers = headers if attempt == 0 else headers + [("X-TypeSafe-Retry-Count", str(attempt))]
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise JevError("Request was aborted.")
        timeout = THRESHOLDS["jevTimeoutMs"] / 1000
        job = _Attempt(url, attempt_headers, body)
        thread = threading.Thread(target=job.run, name="jev-attempt", daemon=True)
        thread.start()
        finished = job.done.wait(min(timeout, remaining))
        if not finished:
            job.abort()
            if deadline - time.monotonic() <= 0:
                raise JevError("Request was aborted.")
            failure = _Retryable(f"Request timed out after {THRESHOLDS['jevTimeoutMs']}ms.")
            headers_for_delay = None
        elif job.error is not None:
            if isinstance(job.error, JevError) and not isinstance(job.error, _Retryable):
                raise job.error
            failure = _Retryable(f"Connection error: {job.error}")
            headers_for_delay = None
        else:
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
        if retries_left <= 0:
            raise failure
        delay = retry_delay_ms(attempt, headers_for_delay) / 1000
        remaining = deadline - time.monotonic()
        if delay >= remaining:
            time.sleep(max(0, remaining))
            raise JevError("Request was aborted.")
        time.sleep(delay)
        attempt += 1


def _prop(obj, key):
    if obj is None or obj is UNDEFINED:
        raise TypeError(f"Cannot read properties of {to_string(obj)} (reading '{key}')")
    if isinstance(obj, dict):
        return obj.get(key, UNDEFINED)
    return UNDEFINED


def _score(answer) -> float:
    return to_number(_prop(answer, "score")) / COMPLEXITY_MAX_SCORE


def ask_jev(prompt=None, current=None, context_tokens=0, models=None, env=None):
    """Asks Jev which model fits; None on any failure (SPEC 6)."""
    if not models:
        return None
    started = time.monotonic()
    request = {
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
        metrics = {
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


def prewarm(env=None) -> None:
    """One fire-and-forget HEAD to the Jev origin, ignoring every outcome (SPEC 5.4)."""
    env = os.environ if env is None else env
    base = env.get("TYPESAFE_BASE_URL")
    base = DEFAULT_BASE_URL if base is None else base

    def run():
        try:
            parts = urllib.parse.urlsplit(base)
            if parts.scheme == "https":
                connection = http.client.HTTPSConnection(parts.hostname, parts.port, timeout=5)
            elif parts.scheme == "http":
                connection = http.client.HTTPConnection(parts.hostname, parts.port, timeout=5)
            else:
                return
            connection.request("HEAD", "/")
            connection.getresponse().read()
            connection.close()
        except Exception:
            pass

    threading.Thread(target=run, name="jev-prewarm", daemon=True).start()
