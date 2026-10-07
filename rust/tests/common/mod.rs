//! Shared test support: an isolated status directory (Node's `isolate-status.mjs`), temporary
//! directories, the golden-case tagged encoding, and loopback HTTP servers.
#![allow(dead_code)]

use jev_router::jsjson::{self, Object, Value};
use jev_router::jsstr::JsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

static STATUS: OnceLock<PathBuf> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);

/// The repository root (the worktree holding `rust/`).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// A fresh, empty directory under the system temp directory, removed with everything in it when
/// the guard drops. Bind it (`let dir = temp_dir(..);`) for as long as the directory is used.
pub struct TempDir(PathBuf);

impl TempDir {
    /// The directory's path.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<std::ffi::OsStr> for TempDir {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.0.as_os_str()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A path inside a [`TempDir`] that keeps the directory alive while the path is in use.
pub struct TempFile {
    _dir: TempDir,
    path: PathBuf,
}

impl TempFile {
    /// `name` inside a fresh temporary directory named after `prefix`.
    pub fn new(prefix: &str, name: &str) -> TempFile {
        let dir = temp_dir(prefix);
        let path = dir.join(name);
        TempFile { _dir: dir, path }
    }
}

impl std::ops::Deref for TempFile {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for TempFile {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

/// Creates a [`TempDir`] named `<prefix><pid>-<nanos>-<seq>`.
pub fn temp_dir(prefix: &str) -> TempDir {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "{prefix}{}-{}-{n}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(dir)
}

/// Points this test binary's status directory at a throwaway one, once. The directory has a fixed
/// name per test binary (the guard of a `static` never drops), emptied when the binary starts, so
/// repeated runs reuse it instead of leaving a new one behind each time.
pub fn isolate_status() -> &'static Path {
    let dir = STATUS.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("jev-status-test-rust-{}", env!("CARGO_CRATE_NAME")));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    });
    jev_router::status::set_status_dir(dir.clone())
}

/// Parses JSON text written by these tests.
pub fn json(text: &str) -> Value {
    jsjson::parse(text).unwrap_or_else(|e| panic!("bad JSON in test: {e}: {text}"))
}

pub fn s(v: &str) -> Value {
    Value::from(v)
}

// ---------------------------------------------------------------------------------------------
// Golden cases (conformance/cases/README.md)

pub struct Case {
    pub file: String,
    pub name: String,
    pub input: Value,
    pub expected: Value,
}

/// Loads `conformance/cases/<file>.json` with its tags decoded.
pub fn load_cases(file: &str) -> Vec<Case> {
    let path = repo_root().join("conformance").join("cases").join(format!("{file}.json"));
    let text = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let Value::Array(items) = jsjson::parse_bytes(&text).unwrap() else { panic!("{file}: not an array") };
    items
        .into_iter()
        .map(|c| Case {
            file: file.to_string(),
            name: c.get("name").as_str().unwrap().to_string_lossy(),
            input: decode(c.get("input")),
            expected: decode(c.get("expected")),
        })
        .collect()
}

fn single_tag(o: &Object) -> Option<(&str, &Value)> {
    if o.len() != 1 {
        return None;
    }
    let (k, v) = o.iter().next()?;
    let k = k.as_str()?;
    ["$undefined", "$number", "$utf16", "$hex", "$throws", "$clock"].contains(&k).then_some((k, v))
}

/// Raw bytes from a `$hex` tag.
pub fn hex_bytes(v: &Value) -> Vec<u8> {
    let Value::Object(o) = v else { panic!("not a $hex tag") };
    let hex = o.get("$hex").and_then(Value::as_str).expect("$hex").to_string_lossy();
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
}

/// Decodes `$undefined`, `$number`, and `$utf16`; leaves `$hex`, `$throws`, `$clock` in place.
pub fn decode(v: &Value) -> Value {
    match v {
        Value::Object(o) => {
            if let Some((tag, inner)) = single_tag(o) {
                return match tag {
                    "$undefined" => Value::Undefined,
                    "$number" => Value::Number(match inner.as_str().unwrap().to_string_lossy().as_str() {
                        "NaN" => f64::NAN,
                        "Infinity" => f64::INFINITY,
                        "-Infinity" => f64::NEG_INFINITY,
                        "-0" => -0.0,
                        other => panic!("unknown $number {other}"),
                    }),
                    "$utf16" => {
                        let units: Vec<u16> =
                            inner.as_array().unwrap().iter().map(|u| u.as_number().unwrap() as u16).collect();
                        Value::String(JsStr::from_utf16(&units))
                    }
                    _ => v.clone(),
                };
            }
            let mut out = Object::new();
            for (k, item) in o.iter() {
                out.insert(k.clone(), decode(item));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(decode).collect()),
        other => other.clone(),
    }
}

pub fn is_throws(v: &Value) -> bool {
    matches!(v, Value::Object(o) if single_tag(o).is_some_and(|(k, _)| k == "$throws"))
}

fn is_clock(v: &Value) -> bool {
    matches!(v, Value::Object(o) if single_tag(o).is_some_and(|(k, _)| k == "$clock"))
}

/// Deep comparison with key order, the sign of zero, NaN equal to NaN, and `$clock` matching
/// any number. Returns the path of the first difference.
pub fn diff(expected: &Value, actual: &Value, path: &str) -> Option<String> {
    if is_clock(expected) {
        return (!matches!(actual, Value::Number(_)))
            .then(|| format!("{path}: expected a clock number, got {actual:?}"));
    }
    let mismatch = || Some(format!("{path}: expected {expected:?}, got {actual:?}"));
    match (expected, actual) {
        (Value::Number(a), Value::Number(b)) => {
            // Golden values are exact: the same bits, NaN equal to NaN, and -0 apart from 0.
            #[allow(clippy::float_cmp)]
            let same = (a.is_nan() && b.is_nan()) || (a == b && a.is_sign_negative() == b.is_sign_negative());
            if same { None } else { mismatch() }
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return mismatch();
            }
            a.iter().zip(b).enumerate().find_map(|(i, (x, y))| diff(x, y, &format!("{path}[{i}]")))
        }
        (Value::Object(a), Value::Object(b)) => {
            let ka: Vec<&JsStr> = a.keys().collect();
            let kb: Vec<&JsStr> = b.keys().collect();
            if ka != kb {
                return Some(format!("{path}: keys {ka:?} != {kb:?}"));
            }
            a.iter().zip(b.iter()).find_map(|((k, x), (_, y))| diff(x, y, &format!("{path}.{k}")))
        }
        _ => {
            if expected == actual {
                None
            } else {
                mismatch()
            }
        }
    }
}

/// Runs every case of a file through `run`, collecting failures; panics listing all of them.
pub fn check_cases(file: &str, run: &mut dyn FnMut(&Value) -> Result<Value, String>) -> usize {
    let cases = load_cases(file);
    let mut failures = Vec::new();
    for case in &cases {
        let got = run(&case.input);
        let problem = match (&got, is_throws(&case.expected)) {
            (Err(_), true) => None,
            (Ok(v), true) => Some(format!("expected a throw, got {v:?}")),
            (Err(e), false) => Some(format!("failed: {e}")),
            (Ok(v), false) => diff(&case.expected, v, "$"),
        };
        if let Some(p) = problem {
            failures.push(format!("{file} / {}: {p}", case.name));
        }
    }
    assert!(failures.is_empty(), "{} of {} cases failed:\n{}", failures.len(), cases.len(), failures.join("\n"));
    cases.len()
}

/// An object from a `Value` (panics otherwise).
pub fn obj(v: &Value) -> Object {
    v.as_object().cloned().unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// Loopback HTTP

use http_body_util::BodyExt;
use hyper::body::Bytes;
use jev_router::http::{ChannelBody, ProxyBody, ReqBody, Url, connect};
use std::sync::{Arc, Mutex};

/// What a fake upstream saw for one request.
#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub url: String,
    pub headers: hyper::HeaderMap,
    pub body: Vec<u8>,
}

impl Seen {
    pub fn json(&self) -> Value {
        jsjson::parse_bytes(&self.body).unwrap_or(Value::Null)
    }
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers.get(name).map(|v| v.to_str().unwrap().to_string())
    }
}

/// A reply from a fake upstream.
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: ProxyBody,
}

impl Reply {
    pub fn json(status: u16, body: &str) -> Reply {
        Reply {
            status,
            headers: vec![("content-type".into(), "application/json".into())],
            body: ProxyBody::Full(Some(Bytes::from(body.to_string()))),
        }
    }
}

pub type Handler = Arc<dyn Fn(&Seen) -> Reply + Send + Sync>;

/// A loopback HTTP/1.1 server recording every request; returns its URL and the log.
pub async fn serve(handler: Handler) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { return };
            let (handler, log) = (handler.clone(), log.clone());
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let (handler, log) = (handler.clone(), log.clone());
                    async move {
                        let (parts, body) = req.into_parts();
                        let body = body.collect().await.map(|b| b.to_bytes().to_vec()).unwrap_or_default();
                        let seen = Seen {
                            method: parts.method.to_string(),
                            url: parts.uri.to_string(),
                            headers: parts.headers,
                            body,
                        };
                        log.lock().unwrap().push(seen.clone());
                        let reply = handler(&seen);
                        let mut res = hyper::Response::builder().status(reply.status);
                        for (k, v) in reply.headers {
                            res = res.header(k, v);
                        }
                        Ok::<_, std::convert::Infallible>(res.body(reply.body).unwrap())
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    (format!("http://127.0.0.1:{port}"), seen)
}

/// A streamed reply body and the sender that feeds it.
pub fn stream_reply() -> (tokio::sync::mpsc::Sender<Result<Bytes, jev_router::http::BoxError>>, ProxyBody) {
    let (tx, body) = ChannelBody::new(4);
    (tx, ProxyBody::Stream(body))
}

/// A client response: status, headers, body.
pub struct Got {
    pub status: u16,
    pub headers: hyper::HeaderMap,
    pub body: Vec<u8>,
}

impl Got {
    pub fn json(&self) -> Value {
        jsjson::parse_bytes(&self.body).unwrap_or(Value::Null)
    }
}

/// Sends one request to `127.0.0.1:<port>` and reads the whole response.
pub async fn request(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Got {
    let res = send(port, method, path, headers, body).await;
    let status = res.status().as_u16();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.map(|b| b.to_bytes().to_vec()).unwrap_or_default();
    Got { status, headers, body }
}

/// Sends one request and returns the response with its body unread.
pub async fn send(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> hyper::Response<hyper::body::Incoming> {
    let url = Url::parse(&format!("http://127.0.0.1:{port}")).unwrap();
    let mut client = connect(&url).await.unwrap();
    let mut req = hyper::Request::builder().method(method).uri(path).header("host", format!("127.0.0.1:{port}"));
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    if !body.is_empty() {
        req = req.header("content-length", body.len().to_string());
    }
    client.send_request(req.body(ReqBody::new(Bytes::copy_from_slice(body))).unwrap()).await.unwrap()
}

/// POSTs a JSON body to the proxy's `/v1/messages`.
pub async fn post_json(port: u16, path: &str, body: &Value) -> Got {
    request(port, "POST", path, &[("content-type", "application/json")], &jsjson::to_bytes(body)).await
}

/// A route that answers with a fixed choice and confidence.
pub fn answer(choice: &str, confidence: f64) -> jev_router::proxy::RouteFn {
    let choice = choice.to_string();
    Arc::new(move |_args| {
        let choice = choice.clone();
        Box::pin(async move {
            let mut o = Object::new();
            o.insert("choice", Value::from(choice));
            o.insert("confidence", Value::Number(confidence));
            o.insert("ms", Value::Number(1.0));
            Ok(Some(Value::Object(o)))
        })
    })
}
