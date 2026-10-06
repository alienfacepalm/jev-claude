"""The routing proxy (port of node/src/proxy.mjs, SPEC 7).

Listens on 127.0.0.1, rewrites requests that name the sentinel model to the tier Jev and the
policy pick, and relays everything to the upstream byte for byte, streaming responses as they
arrive. Requests are served on their own threads; `convos`, `mains` and `catalog` sit under one
lock that is released while the router is awaited (SPEC 3.10).
"""

from __future__ import annotations

import functools
import hashlib
import http.client
import http.server
import math
import os
import re
import select
import socket
import socketserver
import threading
import urllib.parse

from . import jsjson
from .config import (
    THRESHOLDS,
    TIERS,
    available_tiers,
    effort_floor,
    forced_effort,
    id_of,
    is_auto,
    should_use_exact_model,
    tier_of,
    tier_spec,
)
from .jsjson import js_keys, js_values, spread
from .jsstr import (
    JSWS,
    UNDEFINED,
    array_join,
    coalesce,
    is_nullish,
    is_number,
    js_trim,
    math_round,
    to_fixed2,
    to_number,
    to_string,
    truthy,
    u16_len,
    u16_slice,
    utf8,
)
from .log import debug
from .policy import decide
from .router import ask_jev, prewarm
from .status import mark_manual, now_ms, write_calibration, write_decision
from .status import dump_body

ANTHROPIC_BASE_URL = "https://api.anthropic.com"

HOP_BY_HOP = ["content-length", "transfer-encoding", "connection", "keep-alive"]

MAX_CONVERSATIONS = 50

_REMINDER = re.compile(r"<system-reminder>.*?</system-reminder>", re.DOTALL)
_SPACES = re.compile(JSWS + "+")
_THINKING = re.compile(r"thinking", re.IGNORECASE | re.ASCII)
_MODELS_PATH = re.compile(r"/v1/models(?:\?|\Z)")
_SERVED_BY = re.compile(r'"model"' + JSWS + "*:" + JSWS + r'*"([^"]+)"')


# --- JavaScript property access over JSON values -----------------------------------------------


def _get(obj, key):
    """`obj?.key`."""
    if isinstance(obj, dict):
        return obj.get(key, UNDEFINED)
    if isinstance(obj, (str, list)) and key == "length":
        return float(len(obj))
    return UNDEFINED


def _prop(obj, key):
    """`obj.key`, which throws for null and undefined."""
    if is_nullish(obj):
        raise TypeError(f"Cannot read properties of {to_string(obj)} (reading '{key}')")
    return _get(obj, key)


def _first(obj):
    """`obj?.[0]`."""
    if isinstance(obj, list):
        return obj[0] if obj else UNDEFINED
    if isinstance(obj, str):
        return obj[0] if obj else UNDEFINED
    if isinstance(obj, dict):
        return obj.get("0", UNDEFINED)
    return UNDEFINED


def _iterate(value) -> list:
    """`for...of` / `[...value]`: arrays and strings iterate, anything else throws."""
    if isinstance(value, list):
        return list(value)
    if isinstance(value, str):
        return list(value)
    raise TypeError(f"{to_string(value)} is not iterable")


def _is_str(value, text) -> bool:
    """`value === text` for a string literal."""
    return isinstance(value, str) and value == text


def _has_tools(body) -> bool:
    """`Array.isArray(body?.tools) && body.tools.length > 0`."""
    tools = _get(body, "tools")
    return isinstance(tools, list) and len(tools) > 0


# --- Pure functions (SPEC 7.5) -------------------------------------------------------------------


def sanitize_schema(node) -> None:
    """Converts draft-04 boolean exclusive bounds into draft 2020-12 numbers, in place."""
    if isinstance(node, list):
        for item in node:
            sanitize_schema(item)
        return
    if not isinstance(node, dict):
        return
    for key, bound in (("exclusiveMinimum", "minimum"), ("exclusiveMaximum", "maximum")):
        if isinstance(node.get(key, UNDEFINED), bool):
            if node[key] and is_number(node.get(bound, UNDEFINED)):
                node[key] = node[bound]
                del node[bound]
            else:
                del node[key]
    for value in js_values(node):
        sanitize_schema(value)


def _block_text(block):
    return _prop(block, "text")


def new_turn_prompt(body):
    """The text of a genuinely new user turn, or None."""
    if not _has_tools(body):
        return None
    messages = coalesce(_get(body, "messages"), [])
    last = UNDEFINED
    for message in reversed(_iterate(messages)):
        if not _is_str(_get(message, "role"), "system"):
            last = message
            break
    if not truthy(last) or not _is_str(_prop(last, "role"), "user"):
        return None
    content = _prop(last, "content")
    if isinstance(content, str):
        text = content
    elif isinstance(content, list):
        for block in content:
            if _is_str(_prop(block, "type"), "tool_result"):
                return None
        text = array_join([_block_text(b) for b in content if _is_str(_prop(b, "type"), "text")], "\n")
    else:
        return None
    return js_trim(_REMINDER.sub("", text)) or None


def apply_tier(body, tier_name, model=UNDEFINED, env=None):
    """Points a request at a tier, removing request fields that tier cannot accept."""
    tier = tier_spec(tier_name)
    if not tier:
        return body
    if model is UNDEFINED:
        model = id_of(tier_name)
    body["model"] = model
    if not tier["thinking"]:
        body.pop("thinking", None)
        management = _get(body, "context_management")
        edits = _get(management, "edits")
        if isinstance(edits, list):
            kept = [e for e in edits if not _THINKING.search(to_string(coalesce(_get(e, "type"), "")))]
            management["edits"] = kept
            if len(kept) == 0:
                body.pop("context_management", None)
    output_config = _get(body, "output_config")
    if not tier["effort"] and truthy(output_config):
        if isinstance(output_config, dict):
            output_config.pop("effort", None)
            remaining = len(output_config)
        elif isinstance(output_config, (str, list)):
            remaining = len(output_config)
        else:
            remaining = 0
        if remaining == 0:
            body.pop("output_config", None)
    elif tier["effort"]:
        effort = forced_effort(tier_name, env)
        if effort is None:
            effort = None if truthy(_get(output_config, "effort")) else effort_floor(tier_name, env)
        if truthy(effort):
            body["output_config"] = {**spread(output_config), "effort": effort}
    return body


def version_of(model) -> list:
    """`[major, minor]` read from a model id, `[0, 0]` when unreadable."""
    model_id = _get(model, "id")
    if model_id is UNDEFINED:
        model_id = ""
    tier = tier_spec(_get(model, "tier"))
    family = tier["family"] if tier else None
    if not family:
        return [0, 0]
    pattern = re.compile(rf"^(?:[\w-]+\.)?claude-{family}-(\d+)(?:-(\d{{1,2}})(?!\d))?", re.ASCII)
    match = pattern.match(to_string(model_id))
    if not match:
        return [0, 0]
    return [float(match.group(1)), float(match.group(2) or 0)]


def _compare_versions(a, b) -> float:
    return (a[0] - b[0]) or (a[1] - b[1])


def _code_units(text: str) -> bytes:
    return text.encode("utf-16-be", "surrogatepass")


def _released_order(a, b) -> int:
    # SPEC 7.5: plain UTF-16 code-unit comparison (Node calls localeCompare; see the README).
    left, right = a["releasedAt"], b["releasedAt"]
    if not isinstance(left, str) or not isinstance(right, str):
        raise TypeError("releasedAt.localeCompare is not a function")
    x, y = _code_units(left), _code_units(right)
    return (x > y) - (x < y)


def _date_part(created_at):
    if isinstance(created_at, str):
        return u16_slice(created_at, 0, 10)
    if isinstance(created_at, list):
        return to_string(created_at[:10])
    raise TypeError("created_at.slice is not a function")


def claude_models(catalog=()) -> list:
    """Exact Claude models from the catalog, newest first; the static ids when there are none."""
    models = []
    for model in catalog:
        model_id = _get(model, "id")
        if not tier_of(model_id):
            continue
        created_at = model.get("created_at", UNDEFINED)
        parts = [
            model.get("display_name", UNDEFINED),
            f"released {_date_part(created_at)}" if truthy(created_at) else created_at,
            (
                f"{to_string(model['max_input_tokens'])} input tokens"
                if truthy(model.get("max_input_tokens", UNDEFINED))
                else model.get("max_input_tokens", UNDEFINED)
            ),
        ]
        models.append(
            {
                "id": model_id,
                "tier": tier_of(model_id),
                "releasedAt": coalesce(created_at, ""),
                "description": array_join([p for p in parts if truthy(p)], "; "),
            }
        )

    def order(a, b):
        by_version = _compare_versions(version_of(b), version_of(a))
        if by_version:
            return -1 if by_version < 0 else 1
        return -_released_order(a, b)

    models.sort(key=functools.cmp_to_key(order))
    if models:
        return models
    return [{"id": t["id"], "tier": t["name"], "releasedAt": "", "description": t["id"]} for t in TIERS]


def newest_per_tier(models) -> list:
    newest: dict = {}
    for model in models:
        tier = model["tier"]
        if tier not in newest:
            newest[tier] = model
    return list(newest.values())


def model_for_tier(models, tier):
    for model in models:
        if model["tier"] == tier:
            return model["id"]
    return id_of(tier)


def newer_than_calibrated(catalog=()) -> list:
    return [
        model["id"]
        for model in newest_per_tier(claude_models(catalog))
        if _compare_versions(version_of(model), version_of({"id": id_of(model["tier"]), "tier": model["tier"]})) > 0
    ]


def session_of(body):
    """The session id Claude Code embeds in request metadata, or ""."""
    try:
        user_id = coalesce(_get(_get(body, "metadata"), "user_id"), "{}")
        parsed = jsjson.parse(to_string(user_id))
        return coalesce(_prop(parsed, "session_id"), "")
    except Exception:
        return ""


def _first_text(body, separator: str) -> str:
    content = _get(_first(_get(body, "messages")), "content")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return array_join([_block_text(b) for b in content if _is_str(_prop(b, "type"), "text")], separator)
    return ""


def conversation_key(body) -> str:
    """First 12 hex digits of SHA-1 over `<session>|<first message text>`."""
    session = session_of(body)
    text = _first_text(body, "")
    return hashlib.sha1(utf8(f"{to_string(session)}|{text}")).hexdigest()[:12]


def agent_label(body, max_units=48) -> str:
    """A short human-readable name for a conversation, from its first message."""
    text = _first_text(body, " ")
    clean = js_trim(_SPACES.sub(" ", _REMINDER.sub("", text)))
    limit = to_number(max_units)
    if u16_len(clean) > limit:
        end = limit - 1
        return u16_slice(clean, 0, int(end) if math.isfinite(end) else (0 if end < 0 else u16_len(clean))) + "…"
    return clean


class JsMap:
    """A JavaScript `Map`: insertion order and SameValueZero keys (strings, numbers, objects)."""

    def __init__(self):
        self._items: dict = {}

    @staticmethod
    def _key(value):
        if isinstance(value, str):
            return value
        if isinstance(value, bool):
            return ("bool", value)
        if is_number(value):
            x = float(value)
            return ("nan",) if x != x else ("number", x)
        if value is None:
            return ("null",)
        if value is UNDEFINED:
            return ("undefined",)
        return ("object", id(value))

    def has(self, key) -> bool:
        return self._key(key) in self._items

    def get(self, key):
        entry = self._items.get(self._key(key))
        return entry[1] if entry else UNDEFINED

    def set(self, key, value) -> None:
        k = self._key(key)
        if k in self._items:
            self._items[k] = (self._items[k][0], value)
        else:
            self._items[k] = (key, value)

    def delete_first(self) -> None:
        if self._items:
            del self._items[next(iter(self._items))]

    def values(self) -> list:
        return [v for _, v in self._items.values()]

    def entries(self) -> list:
        return [[k, v] for k, v in self._items.values()]

    def __len__(self) -> int:
        return len(self._items)


def agent_of(body, mains: JsMap) -> dict:
    """Which agent inside a session a request belongs to, and whether it is the main thread."""
    key = conversation_key(body)
    session = session_of(body)
    real = _has_tools(body)
    if truthy(session) and real and not mains.has(session):
        if len(mains) > 50:
            mains.delete_first()
        mains.set(session, key)
    main = not truthy(session) or mains.get(session) == key
    return {"key": key, "label": agent_label(body) or ("main" if main else key), "main": main}


# --- HTTP plumbing -------------------------------------------------------------------------------

_SINGLE = {
    "content-type", "content-length", "user-agent", "referer", "host", "authorization",
    "proxy-authorization", "if-modified-since", "if-unmodified-since", "from", "location",
    "max-forwards", "retry-after", "etag", "last-modified", "server", "age", "expires",
}


def node_headers(pairs) -> dict:
    """Header pairs as Node's `IncomingMessage.headers`: lower-case names, duplicates merged."""
    out: dict = {}
    for name, value in pairs:
        key = name.lower()
        if key not in out:
            out[key] = [value] if key == "set-cookie" else value
        elif key == "set-cookie":
            out[key].append(value)
        elif key in _SINGLE:
            continue
        elif key == "cookie":
            out[key] = f"{out[key]}; {value}"
        else:
            out[key] = f"{out[key]}, {value}"
    return out


def upstream_headers(incoming: dict, host: str, body: bytes) -> dict:
    headers = {**incoming, "host": host}
    for name in HOP_BY_HOP:
        headers.pop(name, None)
    if body:
        headers["content-length"] = str(len(body))
    return headers


def url_host(parts) -> str:
    """`new URL(...).host`: the host, with the port only when it is not the scheme's default."""
    hostname = parts.hostname or ""
    if ":" in hostname:
        hostname = f"[{hostname}]"
    port = parts.port
    default = 443 if parts.scheme == "https" else 80
    if port is not None and port != default:
        return f"{hostname}:{port}"
    return hostname


class _ClientWatcher:
    """Notices a client that disconnects while the proxy is still working on its request."""

    def __init__(self, sock):
        self.sock = sock
        self.gone = False
        self.upstream = None
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._run, name="jev-client-watch", daemon=True)
        self._thread.start()

    def _run(self):
        while not self._stop.is_set():
            try:
                readable, _, _ = select.select([self.sock], [], [], 0.05)
            except (OSError, ValueError):
                self._lost()
                return
            if not readable or self._stop.is_set():
                continue
            try:
                data = self.sock.recv(1, socket.MSG_PEEK)
            except BlockingIOError:
                continue
            except OSError:
                self._lost()
                return
            if not data:
                self._lost()
            return

    def _lost(self):
        self.gone = True
        self.abort_upstream()

    def abort_upstream(self):
        connection = self.upstream
        if connection is not None and connection.sock is not None:
            try:
                connection.sock.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            try:
                connection.sock.close()
            except OSError:
                pass

    def stop(self):
        self._stop.set()
        self._thread.join(1)


class ProxyState:
    """The per-proxy maps of SPEC 7.4 and the lock that guards them."""

    def __init__(self):
        self.lock = threading.Lock()
        self.convos: dict = {}
        self.catalog: dict = {}
        self.mains = JsMap()

    def state_for(self, key, fallback=None) -> dict:
        state = self.convos.get(key)
        if state is None and fallback and fallback in self.convos:
            state = self.convos.pop(fallback)
        if state is not None:
            self.convos.pop(key, None)
        elif len(self.convos) >= MAX_CONVERSATIONS:
            main_keys = {v for v in self.mains.values() if isinstance(v, str)}
            oldest = list(self.convos.keys())
            victim = next((k for k in oldest if k not in main_keys), oldest[0])
            del self.convos[victim]
        if state is None:
            state = {"tier": None}
        self.convos[key] = state
        return state


class Proxy:
    def __init__(self, upstream_url=ANTHROPIC_BASE_URL, route=None, calibration_file=None):
        self.upstream_url = upstream_url
        self.route = route or _default_route
        self.calibration_file = calibration_file
        self.state = ProxyState()

    # --- body processing (SPEC 7.3) ---

    def process(self, raw: bytes) -> bytes:
        body = UNDEFINED
        state = None
        try:
            body = jsjson.parse(raw)
            dump_body(body)
            tools = _prop(body, "tools")
            if not is_nullish(tools):
                if not isinstance(tools, list):
                    raise TypeError("body.tools?.forEach is not a function")
                for tool in tools:
                    sanitize_schema(_prop(tool, "input_schema"))

            if not is_auto(_get(body, "model")):
                debug(f"passthrough, user selected {to_string(_get(body, 'model'))}")
                if truthy(_get(_get(body, "tools"), "length")):
                    with self.state.lock:
                        agent = agent_of(body, self.state.mains)
                    debug(f"{agent['key']} passthrough {'main' if agent['main'] else 'sub'} {to_string(_get(body, 'model'))}")
                    if truthy(new_turn_prompt(body)):
                        mark_manual(session_of(body), _get(body, "model"), agent)
            else:
                with self.state.lock:
                    agent = agent_of(body, self.state.mains)
                    key = agent["key"]
                    fallback = None
                    if truthy(session_of(body)):
                        fallback = conversation_key({**body, "metadata": UNDEFINED})
                    state = self.state.state_for(key, fallback)
                    current = coalesce(state["tier"], THRESHOLDS["uncertainDefault"])
                    state_model = state.get("model", UNDEFINED)
                    catalog = list(self.state.catalog.values())
                prompt = new_turn_prompt(body)
                explaining = isinstance(prompt, str) and "<jev-explain>" in prompt
                fresh = None
                if truthy(prompt) and not explaining:
                    allowed = available_tiers()
                    models = newest_per_tier([m for m in claude_models(catalog) if m["tier"] in allowed])
                    available = list(dict.fromkeys(m["tier"] for m in models))
                    current_model = coalesce(state_model, model_for_tier(models, current))
                    context_tokens = math_round(u16_len(jsjson.stringify(body["messages"])) / 4)
                    jev = self.route(
                        {"prompt": prompt, "current": current_model, "contextTokens": context_tokens, "models": models}
                    )
                    choice = _get(jev, "choice")
                    chosen = next((m for m in models if isinstance(choice, str) and m["id"] == choice), None)
                    tier_answer = {**spread(jev), "choice": chosen["tier"]} if chosen else None
                    with self.state.lock:
                        routed_before = truthy(state["tier"])
                    decision = decide(
                        prompt=prompt,
                        jev=tier_answer,
                        current=current,
                        available=available,
                        context_tokens=context_tokens if routed_before else 0,
                    )
                    tier, reason = decision["tier"], decision["reason"]
                    if should_use_exact_model(reason, chosen["tier"] if chosen else UNDEFINED, tier):
                        model = chosen["id"]
                    elif tier == current:
                        model = current_model
                    else:
                        model = model_for_tier(models, tier)
                    with self.state.lock:
                        state["tier"] = tier
                        state["model"] = model
                    has_jev = truthy(jev)
                    fresh = {
                        "prompt": prompt,
                        "model": model,
                        "confidence": coalesce(_get(jev, "confidence"), None),
                        "metrics": coalesce(_get(jev, "metrics"), None),
                        "reason": reason,
                        "jev": {"request": _get(jev, "request"), "response": _get(jev, "response")} if has_jev else None,
                    }
                    timing = (
                        f"{to_string(_get(jev, 'ms'))}ms p={to_fixed2(to_number(_get(jev, 'confidence')))}"
                        if has_jev
                        else "no-jev"
                    )
                    role = "main" if agent["main"] else f"sub[{agent['label']}]"
                    debug(
                        f"{key} {role} {timing} {current} -> {tier} ({reason}) "
                        f"ctx~{to_string(context_tokens)} | {u16_slice(prompt, 0, 60)}"
                    )
                with self.state.lock:
                    tier = coalesce(state["tier"], current)
                    model = coalesce(state.get("model", UNDEFINED), id_of(tier))
                debug(f"{key} rewrite {to_string(body['model'])} -> {to_string(model)}")
                apply_tier(body, tier, model)
                if fresh is not None and not explaining:
                    effort = coalesce(_get(_get(body, "output_config"), "effort"), None)
                    session = session_of(body)
                    write_decision(
                        session if truthy(session) else key,
                        {"tier": tier, **fresh, "effort": effort, "at": now_ms()},
                        agent,
                    )
            return jsjson.dumps_bytes(body)
        except Exception as error:  # noqa: BLE001 - mirrors Node's catch-all around the body
            debug(f"could not process body: {error}")
            try:
                if is_auto(_get(body, "model")):
                    with self.state.lock:
                        tier = coalesce(state["tier"] if state else UNDEFINED, THRESHOLDS["uncertainDefault"])
                        model = coalesce(state.get("model", UNDEFINED) if state else UNDEFINED, id_of(tier))
                    apply_tier(body, tier, model)
                    return jsjson.dumps_bytes(body)
            except Exception as again:  # noqa: BLE001
                debug(f"could not process body: {again}")
            return raw

    # --- /v1/models (SPEC 7.2 step 5) ---

    def record_catalog(self, data: bytes) -> None:
        try:
            listed = coalesce(_prop(jsjson.parse(data), "data"), [])
            with self.state.lock:
                for model in _iterate(listed):
                    model_id = _get(model, "id")
                    if tier_of(model_id):
                        self.state.catalog[model_id] = model
                catalog = list(self.state.catalog.values())
            write_calibration(
                newer=newer_than_calibrated(catalog),
                models=[m["id"] for m in newest_per_tier(claude_models(catalog))],
                file=self.calibration_file,
            )
        except Exception as error:  # noqa: BLE001
            debug(f"could not read Claude model catalog: {error}")


def _default_route(args):
    return ask_jev(
        prompt=args["prompt"], current=args["current"], context_tokens=args["contextTokens"], models=args["models"]
    )


_NO_BODY = {204, 304}


class _Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    # Headers and body go out in separate writes; with Nagle on, the second waits for a delayed ACK.
    disable_nagle_algorithm = True
    proxy: Proxy  # set on the subclass made per server

    def log_message(self, format, *args):  # noqa: A002 - signature fixed by the base class
        pass

    def __getattr__(self, name):
        if name.startswith("do_"):
            return self._serve
        raise AttributeError(name)

    def _send_head(self, status: int, headers: dict, framing: dict) -> None:
        self.send_response_only(status)
        names = set()
        for name, value in headers.items():
            for item in value if isinstance(value, list) else [value]:
                self.send_header(name, item)
            names.add(name.lower())
        if "date" not in names:
            self.send_header("date", self.date_time_string())
        for name, value in framing.items():
            self.send_header(name, value)
        self.end_headers()

    def _read_body(self) -> bytes:
        encoding = (self.headers.get("transfer-encoding") or "").lower()
        if "chunked" in encoding:
            chunks = []
            while True:
                line = self.rfile.readline(65537)
                if not line:
                    raise ConnectionError("request body ended early")
                size = int(line.split(b";", 1)[0].strip() or b"0", 16)
                if size == 0:
                    while True:
                        trailer = self.rfile.readline(65537)
                        if trailer in (b"\r\n", b"\n", b""):
                            break
                    return b"".join(chunks)
                data = self.rfile.read(size)
                if len(data) < size:
                    raise ConnectionError("request body ended early")
                chunks.append(data)
                self.rfile.readline(65537)
        length = self.headers.get("content-length")
        if length:
            size = int(length)
            data = self.rfile.read(size)
            if len(data) < size:
                raise ConnectionError("request body ended early")
            return data
        return b""

    def _abandon(self) -> None:
        """End the client connection without completing the response."""
        self.close_connection = True
        try:
            self.connection.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass

    def _serve(self) -> None:
        proxy = self.proxy
        if self.command == "HEAD":
            self._send_head(200, {}, {"content-length": "0"})
            return
        try:
            raw = self._read_body()
        except (ConnectionError, ValueError, OSError):
            self.close_connection = True
            return
        watcher = _ClientWatcher(self.connection)
        try:
            self._forward(proxy, raw, watcher)
        finally:
            watcher.stop()

    def _forward(self, proxy: Proxy, raw: bytes, watcher: _ClientWatcher) -> None:
        target_path = self.path
        out = raw
        if target_path.startswith("/v1/messages"):
            out = proxy.process(raw)

        if watcher.gone:
            self.close_connection = True
            return

        target = urllib.parse.urlsplit(proxy.upstream_url)
        incoming = node_headers(self.headers.items())
        headers = upstream_headers(incoming, url_host(target), out)
        is_models = self.command == "GET" and bool(_MODELS_PATH.match(target_path))
        if is_models:
            headers.pop("accept-encoding", None)
        if truthy(os.environ.get("JEV_DEBUG")):
            headers.pop("accept-encoding", None)
        base_path = target.path[:-1] if target.path.endswith("/") else target.path

        try:
            if target.scheme == "http":
                connection = http.client.HTTPConnection(target.hostname, target.port)
            else:
                connection = http.client.HTTPSConnection(target.hostname, target.port)
            watcher.upstream = connection
            connection.connect()
            connection.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, True)
            if watcher.gone:
                watcher.abort_upstream()
                self.close_connection = True
                return
            connection.putrequest(self.command, base_path + target_path, skip_host=True, skip_accept_encoding=True)
            for name, value in headers.items():
                for item in value if isinstance(value, list) else [value]:
                    connection.putheader(name, item)
            connection.endheaders(out if out else None)
            response = connection.getresponse()
        except Exception as error:  # noqa: BLE001 - every upstream failure before headers
            if watcher.gone:
                self.close_connection = True
                return
            debug(f"upstream error: {error}")
            payload = jsjson.dumps_bytes({"type": "error", "error": {"message": str(error)}})
            self._send_head(502, {"content-type": "application/json"}, {"content-length": str(len(payload))})
            self.wfile.write(payload)
            return

        try:
            self._relay(proxy, response, is_models, watcher)
        finally:
            try:
                connection.close()
            except Exception:
                pass

    def _relay(self, proxy: Proxy, response, is_models: bool, watcher: _ClientWatcher) -> None:
        received = node_headers(response.getheaders())
        status = response.status

        if is_models:
            try:
                data = response.read()
            except Exception:
                self._abandon()
                return
            proxy.record_catalog(data)
            headers = {k: v for k, v in received.items() if k not in ("content-length", "transfer-encoding", "connection", "keep-alive")}
            self._send_head(status, headers, {"content-length": str(len(data))})
            self.wfile.write(data)
            return

        has_length = "content-length" in received
        headers = {k: v for k, v in received.items() if k not in ("transfer-encoding", "connection", "keep-alive")}
        bodiless = status in _NO_BODY or 100 <= status < 200
        chunked = not has_length and not bodiless
        try:
            self._send_head(status, headers, {"transfer-encoding": "chunked"} if chunked else {})
        except OSError:
            watcher.abort_upstream()
            self.close_connection = True
            return

        watching_model = truthy(os.environ.get("JEV_DEBUG"))
        while True:
            try:
                chunk = response.read1(65536)
            except Exception as error:  # noqa: BLE001 - upstream dropped mid-stream
                if not watcher.gone:
                    debug(f"stream ended early: {error}")
                self._abandon()
                return
            if not chunk:
                break
            if watching_model:
                found = _SERVED_BY.search(chunk.decode("utf-8", "replace"))
                if found:
                    watching_model = False
                    debug(f"{status} served by {found.group(1)}")
            try:
                if chunked:
                    self.wfile.write(b"%x\r\n%s\r\n" % (len(chunk), chunk))
                else:
                    self.wfile.write(chunk)
            except OSError:
                watcher.abort_upstream()
                self.close_connection = True
                return
        if has_length and response.length:
            # The upstream closed before its declared length: fail the client, never end cleanly.
            debug("stream ended early: upstream closed before the declared length")
            self._abandon()
            return
        if chunked:
            try:
                self.wfile.write(b"0\r\n\r\n")
            except OSError:
                self.close_connection = True


class _Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = False


class RunningProxy:
    def __init__(self, server: _Server, thread: threading.Thread):
        self._server = server
        self._thread = thread
        self.port = server.server_address[1]

    def close(self) -> None:
        self._server.shutdown()
        self._server.server_close()


def start_proxy(upstream_url=ANTHROPIC_BASE_URL, route=None, calibration_file=None) -> RunningProxy:
    """Starts the proxy on 127.0.0.1 and an ephemeral port; returns `.port` and `.close()`."""
    proxy = Proxy(upstream_url=upstream_url, route=route, calibration_file=calibration_file)
    handler = type("ProxyHandler", (_Handler,), {"proxy": proxy})
    server = _Server(("127.0.0.1", 0), handler)
    if route is None:
        prewarm()
    thread = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.2}, name="jev-proxy", daemon=True)
    thread.start()
    return RunningProxy(server, thread)


__all__ = [
    "sanitize_schema", "new_turn_prompt", "apply_tier", "version_of", "claude_models", "newest_per_tier",
    "newer_than_calibrated", "session_of", "conversation_key", "agent_label", "agent_of", "JsMap",
    "start_proxy", "node_headers", "math", "js_keys",
]
