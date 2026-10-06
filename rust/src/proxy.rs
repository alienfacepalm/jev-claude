//! The local routing proxy (SPEC 7; `node/src/proxy.mjs`).

use crate::config::{
    self, Model, TIERS, UNCERTAIN_DEFAULT, available_tiers, effort_floor, forced_effort, id_of, is_auto,
    should_use_exact_model, tier_of, tier_of_str, tier_spec,
};
use crate::envx::{self, Env, ProcessEnv};
use crate::http::{BoxError, ChannelBody, ProxyBody, ReqBody, Url, bytes_compat::Bytes, collect, connect};
use crate::jsjson::{self, Object, Value, join_array};
use crate::jsstr::{JSWS_RUN, JsStr, SYSTEM_REMINDER, math_round, minor_after, to_fixed2, utf16_len_bytes};
use crate::log::debug;
use crate::policy::decide;
use crate::router::{RouteArgs, ask_jev, prewarm};
use crate::status::{self, Agent, dump_body, iterate, mark_manual, write_calibration, write_decision};
use crate::timefmt::now_ms;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::header::{HeaderMap, HeaderName, HeaderValue};
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use regex::bytes::Regex;
use sha1::{Digest, Sha1};
use std::convert::Infallible;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use tokio::net::TcpListener;

pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const MAX_CONVERSATIONS: usize = 50;
const HOP_BY_HOP: [&str; 4] = ["content-length", "transfer-encoding", "connection", "keep-alive"];

// ---------------------------------------------------------------------------------------------
// Pure functions (7.5)

/// Draft-04 boolean `exclusiveMinimum`/`exclusiveMaximum` become draft 2020-12 numbers.
pub fn sanitize_schema(node: &mut Value) {
    match node {
        Value::Array(items) => items.iter_mut().for_each(sanitize_schema),
        Value::Object(o) => {
            for (key, bound) in [("exclusiveMinimum", "minimum"), ("exclusiveMaximum", "maximum")] {
                if let Some(Value::Bool(flag)) = o.get(key) {
                    let flag = *flag;
                    match o.get(bound) {
                        Some(Value::Number(n)) if flag => {
                            let n = *n;
                            o.insert(key, Value::Number(n));
                            o.remove(bound);
                        }
                        _ => {
                            o.remove(key);
                        }
                    }
                }
            }
            for (_, v) in o.iter_mut() {
                sanitize_schema(v);
            }
        }
        _ => {}
    }
}

fn has_tools(body: &Value) -> bool {
    body.get("tools").as_array().is_some_and(|t| !t.is_empty())
}

/// `b.type`, which throws for a `null` or `undefined` block.
fn block_type(b: &Value) -> Result<&Value, String> {
    if b.is_nullish() {
        return Err("Cannot read properties of null (reading 'type')".into());
    }
    Ok(b.get("type"))
}

/// The text of a genuinely new user turn, or None (`newTurnPrompt`).
pub fn new_turn_prompt(body: &Value) -> Result<Option<JsStr>, String> {
    if !has_tools(body) {
        return Ok(None);
    }
    let messages = iterate(body.get("messages")).map_err(|_| "body.messages is not iterable".to_string())?;
    let Some(last) = messages.iter().rev().find(|m| !m.get("role").is_str("system")) else {
        return Ok(None);
    };
    if !last.get("role").is_str("user") {
        return Ok(None);
    }
    let text = match last.get("content") {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => {
            for b in blocks {
                if block_type(b)?.is_str("tool_result") {
                    return Ok(None);
                }
            }
            let mut texts = Vec::new();
            for b in blocks {
                if block_type(b)?.is_str("text") {
                    texts.push(b.get("text").clone());
                }
            }
            join_array(&texts, "\n")
        }
        _ => return Ok(None),
    };
    let clean = text.replace_all(&SYSTEM_REMINDER, "").trim();
    Ok(if clean.is_empty() { None } else { Some(clean) })
}

/// Points a request at a tier, removing fields that tier cannot accept (`applyTier`).
pub fn apply_tier(body: &mut Value, tier_name: &str, model: Option<&JsStr>, env: &dyn Env) {
    let Some(tier) = tier_spec(tier_name) else { return };
    let Some(o) = body.as_object_mut() else { return };
    let model = model.cloned().unwrap_or_else(|| tier.id.into());
    o.insert("model", Value::String(model));
    if !tier.thinking {
        o.remove("thinking");
        let edits = o.get("context_management").map(|c| c.get("edits").clone());
        if let Some(Value::Array(edits)) = edits {
            let thinking = Regex::new("(?i-u)thinking").unwrap();
            let kept: Vec<Value> = edits
                .into_iter()
                .filter(|e| {
                    let t = e.get("type").or(&Value::from("")).to_js_string();
                    !thinking.is_match(t.as_bytes())
                })
                .collect();
            let empty = kept.is_empty();
            if let Some(Value::Object(cm)) = o.get_mut("context_management") {
                cm.insert("edits", Value::Array(kept));
            }
            if empty {
                o.remove("context_management");
            }
        }
    }
    let output_config = o.get("output_config").cloned().unwrap_or_default();
    if !tier.effort && output_config.truthy() {
        let mut remaining = output_config.own_key_count();
        if let Some(Value::Object(oc)) = o.get_mut("output_config") {
            oc.remove("effort");
            remaining = oc.key_count();
        }
        if remaining == 0 {
            o.remove("output_config");
        }
    } else if tier.effort {
        let effort = forced_effort(tier_name, env)
            .or_else(|| if output_config.get("effort").truthy() { None } else { effort_floor(tier_name, env) });
        if let Some(effort) = effort {
            let mut oc = output_config.spread_of();
            oc.insert("effort", effort.into());
            o.insert("output_config", Value::Object(oc));
        }
    }
}

static VERSION_RES: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    TIERS
        .iter()
        .map(|t| {
            let re = format!(r"^(?:(?-u:[A-Za-z0-9_-])+\.)?claude-{}-([0-9]+)", t.family);
            (t.name, Regex::new(&re).unwrap())
        })
        .collect()
});

/// `[major, minor]` read from a model id (`versionOf`).
pub fn version_of(id: &[u8], tier: &str) -> (f64, f64) {
    let Some((_, re)) = VERSION_RES.iter().find(|(n, _)| *n == tier) else { return (0.0, 0.0) };
    let Some(c) = re.captures(id) else { return (0.0, 0.0) };
    let major_m = c.get(1).unwrap();
    let num = |b: &[u8]| std::str::from_utf8(b).unwrap().parse::<f64>().unwrap_or(0.0);
    let minor = minor_after(id, major_m.end()).map_or(0.0, num);
    (num(major_m.as_bytes()), minor)
}

fn compare_version(a: (f64, f64), b: (f64, f64)) -> f64 {
    let d = a.0 - b.0;
    if d != 0.0 && !d.is_nan() { d } else { a.1 - b.1 }
}

/// Stable merge sort with a comparator that may fail (as a JavaScript comparator may throw).
fn try_sort<T: Clone>(items: Vec<T>, cmp: &dyn Fn(&T, &T) -> Result<f64, String>) -> Result<Vec<T>, String> {
    if items.len() <= 1 {
        return Ok(items);
    }
    let mid = items.len() / 2;
    let right = try_sort(items[mid..].to_vec(), cmp)?;
    let left = try_sort(items[..mid].to_vec(), cmp)?;
    let mut out = Vec::with_capacity(left.len() + right.len());
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        // Take from the right only when it must come strictly before the left.
        if cmp(&left[i], &right[j])? > 0.0 {
            out.push(right[j].clone());
            j += 1;
        } else {
            out.push(left[i].clone());
            i += 1;
        }
    }
    out.extend_from_slice(&left[i..]);
    out.extend_from_slice(&right[j..]);
    Ok(out)
}

/// `value.slice(0, 10)` in `released ${created_at.slice(0, 10)}`.
fn slice10(v: &Value) -> Result<JsStr, String> {
    match v {
        Value::String(s) => Ok(s.slice_units(10)),
        Value::Array(a) => Ok(join_array(&a[..a.len().min(10)], ",")),
        _ => Err("model.created_at.slice is not a function".into()),
    }
}

/// Exact Claude models in the catalog, newest first; the four static ids when there are none.
pub fn claude_models(catalog: &[Value]) -> Result<Vec<Model>, String> {
    let mut models = Vec::new();
    for model in catalog {
        let id = model.get("id");
        let Some(tier) = tier_of(id) else { continue };
        let created = model.get("created_at");
        let mut parts: Vec<Value> = vec![model.get("display_name").clone()];
        parts.push(if created.truthy() {
            let mut s = JsStr::from("released ");
            s.push_js(&slice10(created)?);
            Value::String(s)
        } else {
            created.clone()
        });
        let max = model.get("max_input_tokens");
        parts.push(if max.truthy() {
            let mut s = max.to_js_string();
            s.push_str(" input tokens");
            Value::String(s)
        } else {
            max.clone()
        });
        let kept: Vec<Value> = parts.into_iter().filter(Value::truthy).collect();
        models.push(Model {
            id: id.as_str().unwrap().clone(),
            tier: tier.to_string(),
            released_at: created.or(&Value::from("")).clone(),
            description: Some(join_array(&kept, "; ")),
        });
    }
    let sorted = try_sort(models, &|a: &Model, b: &Model| {
        let v = compare_version(version_of(b.id.as_bytes(), &b.tier), version_of(a.id.as_bytes(), &a.tier));
        if v != 0.0 && !v.is_nan() {
            return Ok(v);
        }
        // `b.releasedAt.localeCompare(a.releasedAt)`, compared by UTF-16 code units (7.5).
        let Value::String(bs) = &b.released_at else {
            return Err("b.releasedAt.localeCompare is not a function".into());
        };
        Ok(match bs.cmp_utf16(&a.released_at.to_js_string()) {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        })
    })?;
    if sorted.is_empty() {
        return Ok(TIERS
            .iter()
            .map(|t| Model {
                id: t.id.into(),
                tier: t.name.to_string(),
                released_at: "".into(),
                description: Some(t.id.into()),
            })
            .collect());
    }
    Ok(sorted)
}

/// The first model of each tier, in first-seen order.
pub fn newest_per_tier(models: &[Model]) -> Vec<Model> {
    let mut out: Vec<Model> = Vec::new();
    for m in models {
        if !out.iter().any(|o| o.tier == m.tier) {
            out.push(m.clone());
        }
    }
    out
}

fn model_for_tier(models: &[Model], tier: &str) -> JsStr {
    models.iter().find(|m| m.tier == tier).map_or_else(|| id_of(tier).unwrap_or("").into(), |m| m.id.clone())
}

/// Models newer than the version their tier was calibrated for.
pub fn newer_than_calibrated(catalog: &[Value]) -> Result<Vec<JsStr>, String> {
    Ok(newest_per_tier(&claude_models(catalog)?)
        .into_iter()
        .filter(|m| {
            let calibrated = id_of(&m.tier).unwrap_or("");
            compare_version(version_of(m.id.as_bytes(), &m.tier), version_of(calibrated.as_bytes(), &m.tier)) > 0.0
        })
        .map(|m| m.id)
        .collect())
}

/// The session id in request metadata, or "" (`sessionOf`); any JSON value it holds.
pub fn session_of(body: &Value) -> Value {
    let user_id = body.get("metadata").get("user_id");
    let text = if user_id.is_nullish() { JsStr::from("{}") } else { user_id.to_js_string() };
    match jsjson::parse(&text.to_string_lossy()) {
        Ok(parsed) if !parsed.is_nullish() => parsed.get("session_id").or(&Value::from("")).clone(),
        _ => Value::from(""),
    }
}

/// The first message's string content, or its `text` blocks' text joined with `sep`.
fn first_text(body: &Value, sep: &str) -> Result<JsStr, String> {
    match body.get("messages").get("0").get("content") {
        Value::String(s) => Ok(s.clone()),
        Value::Array(blocks) => {
            let mut texts = Vec::new();
            for b in blocks {
                if block_type(b)?.is_str("text") {
                    texts.push(b.get("text").clone());
                }
            }
            Ok(join_array(&texts, sep))
        }
        _ => Ok(JsStr::new()),
    }
}

fn key_for(session: &Value, text: &JsStr) -> JsStr {
    let mut input = session.to_js_string();
    input.push_str("|");
    input.push_js(text);
    let digest = Sha1::digest(input.to_string_lossy().as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    hex[..12].into()
}

/// The first 12 hex digits of SHA-1 over `<session>|<first message text>` (`conversationKey`).
pub fn conversation_key(body: &Value) -> Result<JsStr, String> {
    Ok(key_for(&session_of(body), &first_text(body, "")?))
}

/// A short human-readable name for a conversation (`agentLabel`).
pub fn agent_label(body: &Value, max: usize) -> Result<JsStr, String> {
    let text = first_text(body, " ")?;
    let clean = text.replace_all(&SYSTEM_REMINDER, "").replace_all(&JSWS_RUN, " ").trim();
    if clean.utf16_len() > max {
        let mut cut = clean.slice_units(max - 1);
        cut.push_str("\u{2026}");
        Ok(cut)
    } else {
        Ok(clean)
    }
}

/// A `Map` key with SameValueZero equality.
#[derive(Debug, Clone, PartialEq)]
enum MapKey {
    Str(JsStr),
    Num(u64),
    Bool(bool),
    /// Objects and arrays: each parsed value is a distinct object.
    Unique(u64),
}

static UNIQUE: AtomicU64 = AtomicU64::new(0);

fn map_key(v: &Value) -> MapKey {
    match v {
        Value::String(s) => MapKey::Str(s.clone()),
        Value::Number(n) => {
            let n = if *n == 0.0 {
                0.0
            } else if n.is_nan() {
                f64::NAN
            } else {
                *n
            };
            MapKey::Num(n.to_bits())
        }
        Value::Bool(b) => MapKey::Bool(*b),
        _ => MapKey::Unique(UNIQUE.fetch_add(1, Ordering::SeqCst)),
    }
}

/// Session id -> the first conversation key seen with tools in it (`mains`).
#[derive(Default)]
pub struct Mains {
    entries: Vec<(MapKey, Value, JsStr)>,
}

impl Mains {
    pub fn new() -> Self {
        Self::default()
    }

    fn get(&self, k: &MapKey) -> Option<&JsStr> {
        self.entries.iter().find(|(key, _, _)| key == k).map(|(_, _, v)| v)
    }

    fn values(&self) -> impl Iterator<Item = &JsStr> {
        self.entries.iter().map(|(_, _, v)| v)
    }

    /// `[session, key]` pairs in insertion order.
    pub fn entries(&self) -> impl Iterator<Item = (&Value, &JsStr)> {
        self.entries.iter().map(|(_, s, k)| (s, k))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The main key recorded for a session id, if any.
    pub fn main_of(&self, session: &Value) -> Option<&JsStr> {
        self.get(&map_key(session))
    }
}

/// Which agent inside a session a request belongs to (`agentOf`).
pub fn agent_of(body: &Value, mains: &mut Mains) -> Result<Agent, String> {
    let key = conversation_key(body)?;
    let session = session_of(body);
    let real = has_tools(body);
    let mk = map_key(&session);
    if session.truthy() && real && mains.get(&mk).is_none() {
        if mains.entries.len() > 50 {
            mains.entries.remove(0);
        }
        mains.entries.push((mk.clone(), session.clone(), key.clone()));
    }
    let main = !session.truthy() || mains.get(&mk) == Some(&key);
    let label = agent_label(body, 48)?;
    let label = if !label.is_empty() {
        label
    } else if main {
        "main".into()
    } else {
        key.clone()
    };
    Ok(Agent { key, label, main })
}

// ---------------------------------------------------------------------------------------------
// The server (7.1-7.4)

/// The router a proxy asks; tests inject their own.
pub type RouteFuture = Pin<Box<dyn Future<Output = Result<Option<Value>, String>> + Send>>;
pub type RouteFn = Arc<dyn Fn(RouteArgs) -> RouteFuture + Send + Sync>;

/// Options for [`start_proxy`].
#[derive(Clone, Default)]
pub struct ProxyOptions {
    pub upstream_url: Option<String>,
    /// None means the real router (and the prewarm).
    pub route: Option<RouteFn>,
    pub calibration_file: Option<PathBuf>,
}

#[derive(Debug, Default)]
struct ConvState {
    tier: Option<String>,
    model: Option<JsStr>,
}

type StateRef = Arc<Mutex<ConvState>>;

#[derive(Default)]
struct Shared {
    convos: Vec<(JsStr, StateRef)>,
    mains: Mains,
    catalog: Vec<(JsStr, Value)>,
}

impl Shared {
    fn state_for(&mut self, key: &JsStr, fallback: Option<&JsStr>) -> StateRef {
        let take = |convos: &mut Vec<(JsStr, StateRef)>, k: &JsStr| {
            convos.iter().position(|(c, _)| c == k).map(|i| convos.remove(i).1)
        };
        let mut s = take(&mut self.convos, key);
        if s.is_none()
            && let Some(f) = fallback
        {
            s = take(&mut self.convos, f);
        }
        if s.is_none() && self.convos.len() >= MAX_CONVERSATIONS {
            let mains: Vec<&JsStr> = self.mains.values().collect();
            let victim = self.convos.iter().position(|(k, _)| !mains.contains(&k)).unwrap_or(0);
            self.convos.remove(victim);
        }
        let s = s.unwrap_or_default();
        self.convos.push((key.clone(), s.clone()));
        s
    }

    fn catalog_values(&self) -> Vec<Value> {
        self.catalog.iter().map(|(_, v)| v.clone()).collect()
    }
}

struct Ctx {
    upstream: String,
    route: RouteFn,
    calibration_file: PathBuf,
    shared: Mutex<Shared>,
}

/// A running proxy.
pub struct ProxyHandle {
    pub port: u16,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

impl ProxyHandle {
    /// Stops accepting connections.
    pub fn close(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

impl Drop for ProxyHandle {
    fn drop(&mut self) {
        self.close();
    }
}

fn real_route() -> RouteFn {
    Arc::new(|args| Box::pin(async move { Ok(ask_jev(args).await) }))
}

/// Starts the proxy on `127.0.0.1`, port 0 (`startProxy`). Must run inside a Tokio runtime.
pub async fn start_proxy(opts: ProxyOptions) -> std::io::Result<ProxyHandle> {
    let is_real = opts.route.is_none();
    let ctx = Arc::new(Ctx {
        upstream: opts.upstream_url.unwrap_or_else(|| ANTHROPIC_BASE_URL.to_string()),
        route: opts.route.unwrap_or_else(real_route),
        calibration_file: opts.calibration_file.unwrap_or_else(status::calibration_file),
        shared: Mutex::new(Shared::default()),
    });
    if is_real {
        prewarm();
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut stop_rx => break,
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { continue };
                    let _ = stream.set_nodelay(true);
                    let ctx = ctx.clone();
                    tokio::spawn(async move {
                        let service = hyper::service::service_fn(move |req| handle(req, ctx.clone()));
                        let _ = hyper::server::conn::http1::Builder::new()
                            .serve_connection(TokioIo::new(stream), service)
                            .await;
                    });
                }
            }
        }
    });
    Ok(ProxyHandle { port, stop: Some(stop_tx) })
}

fn is_models_get(method: &hyper::Method, target: &str) -> bool {
    method == hyper::Method::GET
        && target.strip_prefix("/v1/models").is_some_and(|rest| rest.is_empty() || rest.starts_with('?'))
}

fn error_502(message: &str) -> Response<ProxyBody> {
    let mut error = Object::new();
    error.insert("message", message.into());
    let mut body = Object::new();
    body.insert("type", "error".into());
    body.insert("error", Value::Object(error));
    Response::builder()
        .status(502)
        .header("content-type", "application/json")
        .body(ProxyBody::Full(Some(Bytes::from(jsjson::to_bytes(&Value::Object(body))))))
        .unwrap()
}

/// Headers whose duplicates Node drops, keeping the first.
const SINGLE: [&str; 17] = [
    "age",
    "authorization",
    "content-length",
    "content-type",
    "etag",
    "expires",
    "from",
    "host",
    "if-modified-since",
    "if-unmodified-since",
    "last-modified",
    "location",
    "max-forwards",
    "proxy-authorization",
    "referer",
    "retry-after",
    "server",
    // user-agent handled below with the same rule
];

/// Node's `message.headers`: one value per name, duplicates joined or dropped as Node does.
fn node_headers(headers: &HeaderMap) -> Vec<(HeaderName, Vec<HeaderValue>)> {
    let mut out: Vec<(HeaderName, Vec<HeaderValue>)> = Vec::new();
    for name in headers.keys() {
        let values: Vec<&HeaderValue> = headers.get_all(name).iter().collect();
        let n = name.as_str();
        let merged = if values.len() <= 1 || n == "set-cookie" {
            values.into_iter().cloned().collect()
        } else if SINGLE.contains(&n) || n == "user-agent" {
            vec![values[0].clone()]
        } else {
            let sep: &[u8] = if n == "cookie" { b"; " } else { b", " };
            let mut joined = Vec::new();
            for (i, v) in values.iter().enumerate() {
                if i > 0 {
                    joined.extend_from_slice(sep);
                }
                joined.extend_from_slice(v.as_bytes());
            }
            vec![HeaderValue::from_bytes(&joined).unwrap_or_else(|_| values[0].clone())]
        };
        out.push((name.clone(), merged));
    }
    out
}

/// Methods for which Node sends `Content-Length: 0` on an empty request body.
fn sends_zero_length(method: &hyper::Method) -> bool {
    !matches!(method.as_str(), "GET" | "HEAD" | "DELETE" | "OPTIONS" | "TRACE" | "CONNECT")
}

async fn handle(req: Request<Incoming>, ctx: Arc<Ctx>) -> Result<Response<ProxyBody>, Infallible> {
    if req.method() == hyper::Method::HEAD {
        return Ok(Response::builder().status(200).body(ProxyBody::Full(None)).unwrap());
    }
    let (parts, body) = req.into_parts();
    let target = parts.uri.to_string();
    let raw = match body.collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => return Ok(error_502("request body could not be read")),
    };

    // Processing runs to completion even if the client leaves meanwhile, as Node's does; only
    // the upstream request is skipped then, because this future is dropped with the connection.
    let out = if target.starts_with("/v1/messages") {
        let ctx2 = ctx.clone();
        let raw2 = raw.clone();
        match tokio::spawn(async move { process(&ctx2, &raw2).await }).await {
            Ok(Some(bytes)) => Bytes::from(bytes),
            _ => raw,
        }
    } else {
        raw
    };

    Ok(forward(&ctx, &parts, &target, out).await)
}

/// Body processing (7.3) with the error path of 7.2 step 2. None means: forward the original.
async fn process(ctx: &Ctx, raw: &[u8]) -> Option<Vec<u8>> {
    let mut body: Option<Value> = None;
    let mut state: Option<StateRef> = None;
    match process_inner(ctx, raw, &mut body, &mut state).await {
        Ok(out) => Some(out),
        Err(message) => {
            debug(|| format!("could not process body: {message}"));
            let mut body = body?;
            if !is_auto(body.get("model")) {
                return None;
            }
            let (tier, model) = match &state {
                Some(s) => {
                    let s = s.lock().unwrap();
                    (s.tier.clone(), s.model.clone())
                }
                None => (None, None),
            };
            let tier = tier.unwrap_or_else(|| UNCERTAIN_DEFAULT.to_string());
            let model = model.unwrap_or_else(|| id_of(&tier).unwrap_or("").into());
            apply_tier(&mut body, &tier, Some(&model), &ProcessEnv);
            Some(jsjson::to_bytes(&body))
        }
    }
}

async fn process_inner(
    ctx: &Ctx,
    raw: &[u8],
    body_slot: &mut Option<Value>,
    state_slot: &mut Option<StateRef>,
) -> Result<Vec<u8>, String> {
    let parsed = jsjson::parse_bytes(raw)?;
    *body_slot = Some(parsed);
    let body = body_slot.as_mut().unwrap();
    dump_body(body, envx::get("JEV_DUMP").as_deref());
    if matches!(body, Value::Null) {
        return Err("Cannot read properties of null (reading 'tools')".into());
    }
    if let Some(o) = body.as_object_mut()
        && let Some(tools) = o.get_mut("tools")
    {
        match tools {
            Value::Undefined | Value::Null => {}
            Value::Array(items) => {
                for t in items.iter_mut() {
                    if t.is_nullish() {
                        return Err("Cannot read properties of null (reading 'input_schema')".into());
                    }
                    if let Some(schema) = t.as_object_mut().and_then(|o| o.get_mut("input_schema")) {
                        sanitize_schema(schema);
                    }
                }
            }
            _ => return Err("body.tools?.forEach is not a function".into()),
        }
    }

    if !is_auto(body.get("model")) {
        let model = body.get("model").to_js_string();
        debug(|| format!("passthrough, user selected {model}"));
        if has_tools(body) {
            let agent = {
                let mut shared = ctx.shared.lock().unwrap();
                agent_of(body, &mut shared.mains)?
            };
            let role = if agent.main { "main" } else { "sub" };
            debug(|| format!("{} passthrough {role} {model}", agent.key));
            if new_turn_prompt(body)?.is_some() {
                mark_manual(&session_of(body), body.get("model"), Some(&agent));
            }
        }
        return Ok(jsjson::to_bytes(body));
    }

    let (agent, state) = {
        let mut shared = ctx.shared.lock().unwrap();
        let agent = agent_of(body, &mut shared.mains)?;
        let fallback =
            if session_of(body).truthy() { Some(key_for(&Value::from(""), &first_text(body, "")?)) } else { None };
        let state = shared.state_for(&agent.key, fallback.as_ref());
        (agent, state)
    };
    *state_slot = Some(state.clone());
    let key = agent.key.clone();
    let (state_tier, state_model) = {
        let s = state.lock().unwrap();
        (s.tier.clone(), s.model.clone())
    };
    let current = state_tier.clone().unwrap_or_else(|| UNCERTAIN_DEFAULT.to_string());
    let prompt = new_turn_prompt(body)?;
    let explaining = prompt.as_ref().is_some_and(|p| p.contains("<jev-explain>"));
    let mut fresh: Option<Object> = None;
    if let Some(prompt) = prompt.as_ref().filter(|_| !explaining) {
        let catalog = ctx.shared.lock().unwrap().catalog_values();
        let available = available_tiers(&ProcessEnv);
        let all = claude_models(&catalog)?;
        let filtered: Vec<Model> = all.into_iter().filter(|m| available.contains(&m.tier.as_str())).collect();
        let models = newest_per_tier(&filtered);
        let mut avail: Vec<&str> = Vec::new();
        for m in &models {
            if !avail.contains(&m.tier.as_str()) {
                avail.push(m.tier.as_str());
            }
        }
        let current_model = state_model.clone().unwrap_or_else(|| model_for_tier(&models, &current));
        let messages = jsjson::stringify(body.get("messages")).map_or(0, |s| utf16_len_bytes(s.as_bytes()));
        let context_tokens = math_round(messages as f64 / 4.0);
        let args = RouteArgs {
            prompt: prompt.clone(),
            current: current_model.clone(),
            context_tokens,
            models: models.clone(),
        };
        let jev = (ctx.route)(args).await?;
        let jev = jev.filter(Value::truthy);
        let choice = jev.as_ref().map(|j| j.get("choice").clone()).unwrap_or_default();
        let chosen = models.iter().find(|m| matches!(&choice, Value::String(c) if *c == m.id)).cloned();
        let tier_answer = chosen.as_ref().map(|c| {
            let mut a = jev.as_ref().unwrap().spread_of();
            a.insert("choice", c.tier.as_str().into());
            Value::Object(a)
        });
        let routed_before = state.lock().unwrap().tier.is_some();
        let decision = decide(
            &Value::String(prompt.clone()),
            tier_answer.as_ref(),
            &current,
            &avail,
            if routed_before { context_tokens } else { 0.0 },
        );
        let model =
            if should_use_exact_model(&decision.reason, chosen.as_ref().map(|c| c.tier.as_str()), &decision.tier) {
                chosen.as_ref().unwrap().id.clone()
            } else if decision.tier == current {
                current_model.clone()
            } else {
                model_for_tier(&models, &decision.tier)
            };
        {
            let mut s = state.lock().unwrap();
            s.tier = Some(decision.tier.clone());
            s.model = Some(model.clone());
        }
        let mut f = Object::new();
        f.insert("prompt", Value::String(prompt.clone()));
        f.insert("model", Value::String(model.clone()));
        f.insert("confidence", jev.as_ref().map_or(Value::Null, |j| j.get("confidence").or(&Value::Null).clone()));
        f.insert("metrics", jev.as_ref().map_or(Value::Null, |j| j.get("metrics").or(&Value::Null).clone()));
        f.insert("reason", decision.reason.clone().into());
        f.insert(
            "jev",
            match &jev {
                Some(j) => {
                    let mut x = Object::new();
                    x.insert("request", j.get("request").clone());
                    x.insert("response", j.get("response").clone());
                    Value::Object(x)
                }
                None => Value::Null,
            },
        );
        fresh = Some(f);
        debug(|| {
            let role = if agent.main { "main".to_string() } else { format!("sub[{}]", agent.label) };
            let p = match &jev {
                Some(j) => format!("{}ms p={}", j.get("ms").to_js_string(), to_fixed2(j.get("confidence").to_number())),
                None => "no-jev".to_string(),
            };
            format!(
                "{key} {role} {p} {current} -> {} ({}) ctx~{} | {}",
                decision.tier,
                decision.reason,
                crate::jsstr::number_to_string(context_tokens),
                prompt.slice_units(60)
            )
        });
    }
    let (tier, model) = {
        let s = state.lock().unwrap();
        let tier = s.tier.clone().unwrap_or_else(|| current.clone());
        let model = s.model.clone().unwrap_or_else(|| id_of(&tier).unwrap_or("").into());
        (tier, model)
    };
    let from = body.get("model").to_js_string();
    debug(|| format!("{key} rewrite {from} -> {model}"));
    apply_tier(body, &tier, Some(&model), &ProcessEnv);
    if let Some(fresh) = fresh.filter(|_| !explaining) {
        let effort = body.get("output_config").get("effort").or(&Value::Null).clone();
        let mut decision = Object::new();
        decision.insert("tier", tier.as_str().into());
        decision.spread(&fresh);
        decision.insert("effort", effort);
        decision.insert("at", Value::Number(now_ms()));
        let session = session_of(body);
        let id = if session.truthy() { session } else { Value::String(key.clone()) };
        write_decision(&id, &decision, Some(&agent))?;
    }
    Ok(jsjson::to_bytes(body))
}

static SERVED_BY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r#""model"{}*:{}*"((?-u:[^"])+)""#, crate::jsstr::JSWS, crate::jsstr::JSWS)).unwrap()
});

/// Sends the request upstream and relays the answer (7.2 steps 4-7).
async fn forward(ctx: &Ctx, parts: &hyper::http::request::Parts, target: &str, out: Bytes) -> Response<ProxyBody> {
    let url = match Url::parse(&ctx.upstream) {
        Ok(u) => u,
        Err(e) => return error_502(&e),
    };
    let models = is_models_get(&parts.method, target);
    let mut builder = Request::builder().method(parts.method.clone());
    let path = format!("{}{target}", url.pathname.strip_suffix('/').unwrap_or(&url.pathname));
    builder = builder.uri(path);
    let drop_encoding = models || envx::truthy("JEV_DEBUG");
    for (name, values) in node_headers(&parts.headers) {
        let n = name.as_str();
        if n == "host" || HOP_BY_HOP.contains(&n) || (drop_encoding && n == "accept-encoding") {
            continue;
        }
        for v in values {
            builder = builder.header(name.clone(), v);
        }
    }
    builder = builder.header("host", url.host());
    if !out.is_empty() {
        builder = builder.header("content-length", out.len().to_string());
    } else if sends_zero_length(&parts.method) {
        builder = builder.header("content-length", "0");
    }
    let request = match builder.body(ReqBody::new(out)) {
        Ok(r) => r,
        Err(e) => return error_502(&e.to_string()),
    };

    let sent: Result<Response<Incoming>, BoxError> = async {
        let mut send = connect(&url).await?;
        send.ready().await?;
        Ok(send.send_request(request).await?)
    }
    .await;
    let upstream = match sent {
        Ok(r) => r,
        Err(e) => {
            debug(|| format!("upstream error: {e}"));
            return error_502(&e.to_string());
        }
    };
    let (up_parts, up_body) = upstream.into_parts();
    let status = up_parts.status;

    if models {
        let data = match collect(up_body).await {
            Ok(d) => d,
            Err(e) => {
                debug(|| format!("upstream error: {e}"));
                // Node destroys the client connection here; an aborted body does the same.
                let (tx, body) = ChannelBody::new(1);
                let _ = tx.try_send(Err(e));
                return Response::builder().status(status).body(ProxyBody::Stream(body)).unwrap();
            }
        };
        read_catalog(ctx, &data);
        let mut res = Response::builder().status(status);
        for (name, values) in node_headers(&up_parts.headers) {
            if name.as_str() == "content-length" {
                continue;
            }
            for v in values {
                res = res.header(name.clone(), v);
            }
        }
        return res.body(ProxyBody::Full(Some(data))).unwrap();
    }

    let mut res = Response::builder().status(status);
    for (name, values) in node_headers(&up_parts.headers) {
        for v in values {
            res = res.header(name.clone(), v);
        }
    }
    let (tx, body) = ChannelBody::new(16);
    let debugging = envx::truthy("JEV_DEBUG");
    let code = status.as_u16();
    tokio::spawn(async move {
        let mut up_body = up_body;
        let mut seen = !debugging;
        loop {
            tokio::select! {
                _ = tx.closed() => break,
                frame = up_body.frame() => match frame {
                    None => break,
                    Some(Ok(frame)) => {
                        let Ok(data) = frame.into_data() else { continue };
                        if !seen {
                            let text = String::from_utf8_lossy(&data);
                            if let Some(c) = SERVED_BY.captures(text.as_bytes()) {
                                seen = true;
                                let model = String::from_utf8_lossy(&c[1]).into_owned();
                                debug(|| format!("{code} served by {model}"));
                            }
                        }
                        if tx.send(Ok(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        debug(|| format!("stream ended early: {e}"));
                        let _ = tx.send(Err(Box::new(e))).await;
                        break;
                    }
                }
            }
        }
    });
    res.body(ProxyBody::Stream(body)).unwrap()
}

/// `/v1/models`: fills the catalog and writes the calibration file (7.2 step 5).
fn read_catalog(ctx: &Ctx, data: &[u8]) {
    let result = (|| -> Result<(), String> {
        let parsed = jsjson::parse_bytes(data)?;
        if parsed.is_nullish() {
            return Err("Cannot read properties of null (reading 'data')".into());
        }
        let entries = iterate(parsed.get("data")).map_err(|_| "data is not iterable".to_string())?;
        let catalog = {
            let mut shared = ctx.shared.lock().unwrap();
            for model in entries {
                let id = model.get("id");
                if let Some(id) = id.as_str().filter(|id| tier_of_str(id.as_bytes()).is_some()) {
                    let id = id.clone();
                    match shared.catalog.iter_mut().find(|(k, _)| *k == id) {
                        Some(slot) => slot.1 = model.clone(),
                        None => shared.catalog.push((id, model.clone())),
                    }
                }
            }
            shared.catalog_values()
        };
        let newer = newer_than_calibrated(&catalog)?;
        let models: Vec<JsStr> = newest_per_tier(&claude_models(&catalog)?).into_iter().map(|m| m.id).collect();
        write_calibration(&newer, &models, &ctx.calibration_file);
        Ok(())
    })();
    if let Err(e) = result {
        debug(|| format!("could not read Claude model catalog: {e}"));
    }
}

/// For callers that need the list of tier names.
pub fn tier_names() -> [&'static str; 4] {
    config::TIER_NAMES
}
