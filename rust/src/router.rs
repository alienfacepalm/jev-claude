//! Asking Jev which tier fits (SPEC 5, 6; `node/src/router.mjs`), with this port's own client
//! for the one endpoint Node's SDK is used for.

use crate::config::{
    COMPLEXITY_MAX_SCORE, CONTEXT_WINDOW_TOKENS, JEV_DEADLINE_MS, JEV_MAX_RETRIES, JEV_TIMEOUT_MS, Model,
    question_for_models, questions,
};
use crate::envx;
use crate::http::{ReqBody, Url, bytes_compat::Bytes, collect, connect};
use crate::jsjson::{self, Object, Value};
use crate::jsstr::{JsStr, math_round, trim_str};
use crate::log::log;
use crate::timefmt::{now_ms, parse_iso};
use hyper::client::conn::http1::SendRequest;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub const TYPESAFE_BASE_URL: &str = "https://api.typesafe.ai";

/// What the proxy hands the router.
#[derive(Debug, Clone)]
pub struct RouteArgs {
    pub prompt: JsStr,
    pub current: JsStr,
    pub context_tokens: f64,
    pub models: Vec<Model>,
}

/// The router request (6.1), without `model`.
pub fn build_request(args: &RouteArgs) -> Value {
    let mut session = Object::new();
    session.insert("current_model", Value::String(args.current.clone()));
    session.insert("context_tokens", Value::Number(args.context_tokens));
    let mut environment = Object::new();
    environment
        .insert("available_models", Value::Array(args.models.iter().map(|m| Value::String(m.id.clone())).collect()));
    let mut state = Object::new();
    state.insert("request", Value::String(args.prompt.clone()));
    state.insert("session", Value::Object(session));
    state.insert("environment", Value::Object(environment));
    let mut qs = Object::new();
    for (name, q) in questions() {
        qs.insert(name, q);
    }
    qs.insert("model", question_for_models(&args.models));
    let mut request = Object::new();
    request.insert("state", Value::Object(state));
    request.insert("questions", Value::Object(qs));
    Value::Object(request)
}

/// The body sent to `/v1/systemone`: the request with `model` appended.
pub fn request_body(request: &Value) -> Vec<u8> {
    let mut body = request.spread_of();
    let model = envx::get("TYPESAFE_DEFAULT_MODEL")
        .map(|m| trim_str(&m).to_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "jev-latest".to_string());
    body.insert("model", model.into());
    jsjson::to_bytes(&Value::Object(body))
}

fn base_url() -> String {
    let base = envx::get("TYPESAFE_BASE_URL").map(|b| trim_str(&b).to_string()).filter(|b| !b.is_empty());
    base.unwrap_or_else(|| TYPESAFE_BASE_URL.to_string()).trim_end_matches('/').to_string()
}

fn api_key() -> Option<String> {
    envx::get("JEV_API_KEY").or_else(|| envx::get("TYPESAFE_API_KEY"))
}

/// One idle connection to the Jev origin, warmed by the prewarm and reused by the next call.
static IDLE: Mutex<Option<(String, SendRequest<ReqBody>)>> = Mutex::new(None);

fn take_idle(origin: &str) -> Option<SendRequest<ReqBody>> {
    let mut idle = IDLE.lock().unwrap();
    match idle.take() {
        Some((o, s)) if o == origin && !s.is_closed() => Some(s),
        _ => None,
    }
}

fn put_idle(origin: String, send: SendRequest<ReqBody>) {
    if !send.is_closed() {
        *IDLE.lock().unwrap() = Some((origin, send));
    }
}

/// The fire-and-forget `HEAD` that warms the TLS connection (5.4).
pub fn prewarm() {
    let base = envx::get("TYPESAFE_BASE_URL").unwrap_or_else(|| TYPESAFE_BASE_URL.to_string());
    let Ok(url) = Url::parse(&base) else { return };
    tokio::spawn(async move {
        let origin = url.origin();
        let Ok(mut send) = connect(&url).await else { return };
        let req = hyper::Request::builder()
            .method("HEAD")
            .uri("/")
            .header("host", url.host())
            .body(ReqBody::new(Bytes::new()));
        let Ok(req) = req else { return };
        let Ok(Ok(res)) = tokio::time::timeout(Duration::from_secs(10), send.send_request(req)).await else { return };
        let _ = collect(res.into_body()).await;
        put_idle(origin, send);
    });
}

enum AttemptError {
    /// Connection error or attempt timeout: retried.
    Connection(String),
}

struct Reply {
    status: u16,
    retry_after: Option<f64>,
    body: Bytes,
}

static SEED: AtomicU64 = AtomicU64::new(0);

/// `Math.random()` for backoff jitter: a xorshift seeded from the clock.
fn random() -> f64 {
    let mut x = SEED.load(Ordering::Relaxed);
    if x == 0 {
        x = (now_ms() as u64) ^ 0x9E37_79B9_7F4A_7C15 ^ (std::process::id() as u64) << 32;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    SEED.store(x, Ordering::Relaxed);
    (x >> 11) as f64 / (1u64 << 53) as f64
}

fn parse_retry_after(headers: &hyper::HeaderMap) -> Option<f64> {
    let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    if let Some(ms) = text("retry-after-ms") {
        let n = Value::from(ms.as_str()).to_number();
        if n.is_finite() && n >= 0.0 {
            return Some(n);
        }
    }
    let raw = text("retry-after")?;
    let seconds = Value::from(raw.as_str()).to_number();
    if seconds.is_finite() {
        return (seconds >= 0.0).then_some(seconds * 1000.0);
    }
    http_date(&raw).map(|date| (date - now_ms()).max(0.0))
}

/// An HTTP date (`Sun, 06 Nov 1994 08:49:37 GMT`) or an ISO date, in milliseconds.
fn http_date(raw: &str) -> Option<f64> {
    if let Some(ms) = parse_iso(raw.trim()) {
        return Some(ms);
    }
    let parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.len() != 6 || !parts[5].eq_ignore_ascii_case("GMT") {
        return None;
    }
    let months = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let day: u32 = parts[1].parse().ok()?;
    let month = months.iter().position(|m| parts[2].eq_ignore_ascii_case(m))? + 1;
    let year: u32 = parts[3].parse().ok()?;
    let iso = format!("{year:04}-{month:02}-{day:02}T{}.000Z", parts[4]);
    parse_iso(&iso)
}

async fn attempt(
    url: &Url,
    origin: &str,
    path: &str,
    key: &str,
    body: &[u8],
    retry: u32,
) -> Result<Reply, AttemptError> {
    let work = async {
        let mut send = match take_idle(origin) {
            Some(s) => s,
            None => connect(url).await.map_err(|e| AttemptError::Connection(format!("Connection error: {e}")))?,
        };
        send.ready().await.map_err(|e| AttemptError::Connection(format!("Connection error: {e}")))?;
        let mut req = hyper::Request::builder()
            .method("POST")
            .uri(path)
            .header("host", url.host())
            .header("authorization", format!("Bearer {key}"))
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .header("user-agent", format!("jev-router-rust/{}", crate::repo::release_version()))
            .header("content-length", body.len().to_string());
        if retry > 0 {
            req = req.header("x-typesafe-retry-count", retry.to_string());
        }
        let req = req
            .body(ReqBody::new(Bytes::copy_from_slice(body)))
            .map_err(|e| AttemptError::Connection(format!("Connection error: {e}")))?;
        let res =
            send.send_request(req).await.map_err(|e| AttemptError::Connection(format!("Connection error: {e}")))?;
        let status = res.status().as_u16();
        let retry_after = parse_retry_after(res.headers());
        let body =
            collect(res.into_body()).await.map_err(|e| AttemptError::Connection(format!("Connection error: {e}")))?;
        put_idle(origin.to_string(), send);
        Ok(Reply { status, retry_after, body })
    };
    match tokio::time::timeout(Duration::from_millis(JEV_TIMEOUT_MS), work).await {
        Ok(r) => r,
        Err(_) => Err(AttemptError::Connection(format!("Request timed out after {JEV_TIMEOUT_MS}ms."))),
    }
}

/// `POST /v1/systemone` with retries; the parsed response or an error message.
async fn system_one(request: &Value) -> Result<Value, String> {
    let Some(key) = api_key() else {
        return Err("No API key: set JEV_API_KEY or TYPESAFE_API_KEY.".to_string());
    };
    let base = base_url();
    let url = Url::parse(&base)?;
    let origin = url.origin();
    let path = format!("{}/v1/systemone", url.pathname.trim_end_matches('/'));
    let body = request_body(request);
    let mut n = 0u32;
    loop {
        let retries_left = JEV_MAX_RETRIES - n;
        let delay = match attempt(&url, &origin, &path, &key, &body, n).await {
            Err(AttemptError::Connection(message)) => {
                if retries_left == 0 {
                    return Err(message);
                }
                None
            }
            Ok(reply) if (200..300).contains(&reply.status) => {
                let parsed = jsjson::parse_bytes(&reply.body).map_err(|e| format!("invalid response body: {e}"))?;
                return Ok(parsed);
            }
            Ok(reply) => {
                let retryable = matches!(reply.status, 408 | 429 | 500..=599);
                if retries_left == 0 || !retryable {
                    return Err(format!("{} status code", reply.status));
                }
                reply.retry_after.filter(|ms| *ms <= 60_000.0)
            }
        };
        let wait = delay.unwrap_or_else(|| {
            let exponential = (150.0 * 2f64.powi(n as i32)).min(400.0);
            math_round(exponential * (1.0 - random() * 0.25))
        });
        tokio::time::sleep(Duration::from_millis(wait as u64)).await;
        n += 1;
    }
}

fn is_nullish_answer(answers: &Value, name: &str) -> bool {
    answers.get(name).is_nullish()
}

/// `askJev(args)`: the answer with request, response, metrics and timing; None on any failure.
pub async fn ask_jev(args: RouteArgs) -> Option<Value> {
    if args.models.is_empty() {
        return None;
    }
    let started = Instant::now();
    let request = build_request(&args);
    let outcome = tokio::time::timeout(Duration::from_millis(JEV_DEADLINE_MS), system_one(&request)).await;
    let result = match outcome {
        Err(_) => Err("Request was aborted.".to_string()),
        Ok(Err(e)) => Err(e),
        Ok(Ok(v)) => {
            if !matches!(v, Value::Object(_)) {
                Err("Unexpected response: not a JSON object".to_string())
            } else if !matches!(v.get("answers"), Value::Object(_)) {
                Err("Unexpected response: answers missing".to_string())
            } else if ["task_complexity", "reasoning_required", "tool_complexity"]
                .iter()
                .any(|n| is_nullish_answer(v.get("answers"), n))
            {
                Err("Cannot read properties of undefined (reading 'score')".to_string())
            } else {
                Ok(v)
            }
        }
    };
    match result {
        Err(message) => {
            log(&format!("routing failed, keeping {}: {message}", args.current));
            None
        }
        Ok(response) => {
            let answers = response.get("answers");
            let mut out = answers.get("model").spread_of();
            let score = |n: &str| Value::Number(answers.get(n).get("score").to_number() / COMPLEXITY_MAX_SCORE);
            let mut metrics = Object::new();
            metrics.insert("taskComplexity", score("task_complexity"));
            metrics.insert("reasoningRequired", score("reasoning_required"));
            metrics.insert("toolComplexity", score("tool_complexity"));
            metrics.insert("contextSize", Value::Number((args.context_tokens / CONTEXT_WINDOW_TOKENS).min(1.0)));
            out.insert("request", request);
            out.insert("response", response.clone());
            out.insert("metrics", Value::Object(metrics));
            out.insert("ms", Value::Number(started.elapsed().as_millis() as f64));
            Some(Value::Object(out))
        }
    }
}
