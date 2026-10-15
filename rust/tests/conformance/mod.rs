//! Shared machinery for the black-box harness (SPEC.md 16.3): fake Jev and Anthropic servers on
//! loopback, the built `jev-proxy-host` and `jev-statusline` as child processes, and the header
//! rules of SPEC 7.7. Every `tests/conformance_*.rs` file includes this module; nothing here is a
//! test.
//!
//! The harness talks to the programs only through their processes, sockets and files. It shares
//! no code with the crate under test: its JSON (`json`), hashing (`sha256`) and HTTP (`http`) are
//! its own.
#![allow(dead_code)]

pub mod http;
pub mod json;
pub mod sha256;

use http::{Conn, Handler, Record, Resp, Response, Server};
use json::{Json, obj, stringify};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Every harness test runs under one time limit, so a program that accepts a request and never
/// answers fails that test instead of hanging `cargo test` forever (the test runner sets no limit).
/// It sits well above the slowest legitimate test: a cold start of the proxy may take up to 20 s on
/// its own, and two transport tests then race 5 s timers.
pub const TEST_TIMEOUT: Duration = Duration::from_secs(60);

/// How long `send` waits for a response: below `TEST_TIMEOUT`, so the failure names the cause.
pub const RESPONSE_TIMEOUT: Duration = Duration::from_secs(20);

/// How long the proxy may take to print its port, and the status line to finish.
pub const START_TIMEOUT: Duration = Duration::from_secs(20);

pub const JEV_KEY: &str = "jev-harness-key";

pub const DIM: &str = "\x1b[2m";
pub const BOLD: &str = "\x1b[1m";
pub const RESET: &str = "\x1b[0m";
pub const GREEN: &str = "\x1b[32m";
pub const CYAN: &str = "\x1b[36m";

/// The separator between status line segments (SPEC 12).
pub fn sep() -> String {
    format!(" {DIM}·{RESET} ")
}

/// A status line icon, as `JEV_ICONS=symbols` prints it.
pub fn icon(mark: &str) -> String {
    format!("{BOLD}{mark}{RESET}")
}

/// The repository root (the directory holding `rust/` and `conformance/`).
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("rust/ has a parent").to_path_buf()
}

fn read_json(path: &Path) -> Json {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    json::parse_bytes(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// `conformance/fixtures/claude-code-print-request.json`: a request captured from `claude -p`.
pub static FIXTURE: LazyLock<Json> =
    LazyLock::new(|| read_json(&root().join("conformance").join("fixtures").join("claude-code-print-request.json")));

// What a request should become is not computed here: the harness holds no routing logic, so there
// is no second implementation to disagree with the one under test. The expectations are data,
// precomputed for the exact inputs the tests send (see the lookups below and conformance/README.md).
pub static EXPECTED: LazyLock<Json> =
    LazyLock::new(|| read_json(&root().join("conformance").join("fixtures").join("harness-expectations.json")));

/// A frozen golden case file, `conformance/cases/<file>.json` (SPEC 16.2), undecoded.
pub fn golden(file: &str) -> Json {
    read_json(&root().join("conformance").join("cases").join(format!("{file}.json")))
}

/// The golden case `name` of `file`, which must exist.
pub fn golden_case(file: &str, name: &str) -> Json {
    golden(file)
        .arr()
        .iter()
        .find(|c| c.at("name").as_str() == Some(name))
        .cloned()
        .unwrap_or_else(|| panic!("no golden case {file}: {name}"))
}

/// The menus the proxy offers Jev: `static`, with no account catalog, and `catalog`, the one the
/// test catalog in `conformance_models.rs` yields.
pub fn menu(name: &str) -> Json {
    EXPECTED.at("menus").at(name).clone()
}

/// The ids of a menu.
pub fn ids(models: &Json) -> Vec<String> {
    models.arr().iter().map(|m| m.at("id").str().to_string()).collect()
}

/// Milliseconds since the epoch, the clock the programs stamp `at` with.
pub fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as f64
}

/// Waits `ms` milliseconds.
pub fn pause(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

// -------------------------------------------------------------------------------------------------
// Running a test

type Kids = Arc<Mutex<Vec<Arc<Mutex<Child>>>>>;

/// One test's context: the child processes it started, killed if the test runs out of time.
pub struct Ctx {
    kids: Kids,
}

/// Runs one harness test under `TEST_TIMEOUT`. The test runs on its own thread; when it overruns,
/// its children are killed and the test fails, naming the limit, instead of hanging the run.
pub fn run(body: impl FnOnce(&Ctx) + Send + 'static) {
    let kids: Kids = Arc::new(Mutex::new(Vec::new()));
    let ctx = Ctx { kids: kids.clone() };
    let (done, finished) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        body(&ctx);
        let _ = done.send(());
    });
    match finished.recv_timeout(TEST_TIMEOUT) {
        Ok(()) => {
            let _ = worker.join();
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            // The test panicked: report its own failure.
            if let Err(panic) = worker.join() {
                std::panic::resume_unwind(panic);
            }
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            for kid in kids.lock().unwrap().iter() {
                let _ = kid.lock().unwrap().kill();
            }
            panic!(
                "the test did not finish within {} s (a request the program never answered?)",
                TEST_TIMEOUT.as_secs()
            );
        }
    }
}

// -------------------------------------------------------------------------------------------------
// Fake servers

/// What the fake Jev answers.
pub enum JevReply {
    /// 200 with this JSON.
    Ok(Json),
    /// This status and body.
    Status(u16, String),
}

/// The fake Jev's answer to a request body.
pub type Answer = Arc<dyn Fn(&Json) -> JevReply + Send + Sync>;

/// An answer that is always `reply`.
pub fn always(reply: Json) -> Answer {
    Arc::new(move |_| JevReply::Ok(reply.clone()))
}

/// A Jev response as SPEC 5.2 describes it.
pub fn jev_answer(choice: &str, confidence: f64, scores: [u32; 3]) -> Json {
    let score = |s: u32| {
        let key = s.to_string();
        obj(vec![
            ("type", "score".into()),
            ("score", f64::from(s).into()),
            ("confidence", 0.8.into()),
            ("legend", obj(vec![("0", "None".into())])),
            ("probabilities", obj(vec![(key.as_str(), 0.8.into())])),
        ])
    };
    obj(vec![
        ("model", "jev-1".into()),
        ("usage", obj(vec![("input_tokens", 812.0.into()), ("output_tokens", 9.0.into())])),
        (
            "answers",
            obj(vec![
                (
                    "model",
                    obj(vec![
                        ("type", "choice".into()),
                        ("choice", choice.into()),
                        ("confidence", confidence.into()),
                        ("probabilities", obj(vec![(choice, confidence.into())])),
                    ]),
                ),
                ("task_complexity", score(scores[0])),
                ("reasoning_required", score(scores[1])),
                ("tool_complexity", score(scores[2])),
            ]),
        ),
    ])
}

/// The default answer: Haiku at 92%.
pub fn default_answer() -> Answer {
    always(jev_answer(HAIKU, 0.92, [3, 4, 2]))
}

pub const HAIKU: &str = "claude-haiku-5-5";
pub const SONNET: &str = "claude-sonnet-5-5";

/// A fake Jev. The prewarm `HEAD` is recorded with everything else; `posts` is the real calls.
pub struct Jev {
    pub server: Server,
}

impl Jev {
    pub fn start(answer: Answer) -> Jev {
        let handler: Handler = Arc::new(move |record: &Record, res: &mut Resp| {
            if record.method != "POST" {
                return res.reply(200, &[], b"");
            }
            let reply = match json::parse_bytes(&record.body) {
                Ok(body) => answer(&body),
                Err(e) => JevReply::Status(500, stringify(&obj(vec![("error", e.into())]))),
            };
            let (status, body) = match reply {
                JevReply::Ok(value) => (200, stringify(&value)),
                JevReply::Status(status, body) => (status, body),
            };
            res.reply(status, &[("content-type", "application/json")], body.as_bytes());
        });
        Jev { server: Server::start(handler) }
    }

    pub fn url(&self) -> String {
        self.server.url.clone()
    }

    /// The `POST`s, in order.
    pub fn posts(&self) -> Vec<Record> {
        self.server.requests().into_iter().filter(|r| r.method == "POST").collect()
    }
}

pub const UPSTREAM_REPLY_HEADERS: &[(&str, &str)] = &[
    ("content-type", "application/json"),
    ("request-id", "req_harness_1"),
    ("anthropic-ratelimit-tokens-remaining", "123456"),
    ("x-should-retry", "false"),
    ("x-harness-trace", "abc"),
    ("server", "fake-anthropic"),
];

pub const UPSTREAM_REPLY: &str = r#"{"id":"msg_1","type":"message","model":"claude-haiku-5-5","content":[]}"#;

/// A fake Anthropic API; without a handler it answers every request with a small JSON message.
pub fn fake_upstream(handle: Option<Handler>) -> Server {
    Server::start(handle.unwrap_or_else(|| {
        Arc::new(|_: &Record, res: &mut Resp| res.reply(200, UPSTREAM_REPLY_HEADERS, UPSTREAM_REPLY.as_bytes()))
    }))
}

// -------------------------------------------------------------------------------------------------
// The programs under test

static SEQ: AtomicU64 = AtomicU64::new(0);

/// The parent environment minus everything that would change a child's behaviour, plus `extra`.
pub fn clean_env(extra: &[(String, String)]) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(key, _)| {
            let key = key.to_string_lossy().to_ascii_uppercase();
            !["JEV_", "TYPESAFE_", "ANTHROPIC_", "CLAUDE_"].iter().any(|prefix| key.starts_with(prefix))
        })
        .collect();
    for (key, value) in extra {
        // Windows treats variable names without regard to case.
        env.retain(|(k, _)| {
            let k = k.to_string_lossy();
            if cfg!(windows) { !k.eq_ignore_ascii_case(key) } else { k != key.as_str() }
        });
        env.push((key.into(), value.into()));
    }
    env
}

/// The proxy under test, killed and its directories removed when dropped.
pub struct Proxy {
    pub port: u16,
    /// The temporary directory holding the others.
    pub base: PathBuf,
    /// `JEV_STATUS_DIR`.
    pub status: PathBuf,
    /// The proxy's working directory.
    pub cwd: PathBuf,
    /// The environment it runs with, which the status line reuses.
    pub env: Vec<(OsString, OsString)>,
    child: Arc<Mutex<Child>>,
    stderr: Arc<Mutex<String>>,
}

impl Proxy {
    /// A file in the status directory, parsed, or `None` when it does not exist.
    pub fn status_file(&self, name: &str) -> Option<Json> {
        read_json_file(&self.status.join(name))
    }

    /// `<session>.json` in the status directory.
    pub fn file_of(&self, session: &str) -> Option<Json> {
        self.status_file(&format!("{session}.json"))
    }

    pub fn stderr(&self) -> String {
        self.stderr.lock().unwrap().clone()
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        {
            let mut child = self.child.lock().unwrap();
            let _ = child.kill();
            let _ = child.wait();
        }
        for _ in 0..5 {
            if std::fs::remove_dir_all(&self.base).is_ok() || !self.base.exists() {
                break;
            }
            pause(100);
        }
    }
}

/// Reads a file as JSON, or `None` when it does not exist.
pub fn read_json_file(path: &Path) -> Option<Json> {
    path.exists().then(|| read_json(path))
}

/// Starts the proxy under test with the environment SPEC 16.3 lists and returns once it has
/// printed its port.
pub fn start_proxy(ctx: &Ctx, jev_url: &str, upstream_url: &str, extra: &[(&str, &str)]) -> Proxy {
    let base = std::env::temp_dir().join(format!(
        "jev-harness-{}-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let (status, home, temp, cwd) = (base.join("status"), base.join("home"), base.join("temp"), base.join("cwd"));
    for dir in [&home, &temp, &cwd] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let path = |p: &Path| p.to_string_lossy().into_owned();
    let mut vars: Vec<(String, String)> = vec![
        ("JEV_STATUS_DIR".into(), path(&status)),
        ("HOME".into(), path(&home)),
        ("USERPROFILE".into(), path(&home)),
        ("TEMP".into(), path(&temp)),
        ("TMP".into(), path(&temp)),
        ("TMPDIR".into(), path(&temp)),
        ("JEV_ICONS".into(), "symbols".into()),
        ("TYPESAFE_BASE_URL".into(), jev_url.into()),
        ("JEV_API_KEY".into(), JEV_KEY.into()),
        ("ANTHROPIC_BASE_URL".into(), upstream_url.into()),
    ];
    vars.extend(extra.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())));
    let env = clean_env(&vars);
    let mut child = Command::new(env!("CARGO_BIN_EXE_jev-proxy-host"))
        .current_dir(&cwd)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start jev-proxy-host");
    let stdout = child.stdout.take().unwrap();
    let mut stderr_pipe = child.stderr.take().unwrap();
    let child = Arc::new(Mutex::new(child));
    ctx.kids.lock().unwrap().push(child.clone());
    let stderr = Arc::new(Mutex::new(String::new()));
    let sink = stderr.clone();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        while let Ok(n) = stderr_pipe.read(&mut buffer) {
            if n == 0 {
                break;
            }
            sink.lock().unwrap().push_str(&String::from_utf8_lossy(&buffer[..n]));
        }
    });
    let (lines, incoming) = mpsc::channel();
    std::thread::spawn(move || {
        // Keeps draining stdout after the port line, so the proxy never blocks on a full pipe.
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let _ = lines.send(line);
        }
    });
    let mut proxy = Proxy { port: 0, base, status, cwd, env, child, stderr };
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match incoming.recv_timeout(left) {
            Ok(line) => {
                let port = line.trim_end_matches('\r').strip_prefix("PORT=").and_then(|p| p.parse::<u16>().ok());
                if let Some(port) = port {
                    proxy.port = port;
                    return proxy;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!("proxy printed no PORT= line within {} s; stderr:\n{}", START_TIMEOUT.as_secs(), proxy.stderr())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let code = proxy.child.lock().unwrap().wait().map(|s| s.to_string()).unwrap_or_default();
                pause(100);
                panic!("proxy exited ({code}) before printing its port; stderr:\n{}", proxy.stderr())
            }
        }
    }
}

/// What a test sets up beyond the defaults.
#[derive(Default)]
pub struct Opts {
    pub answer: Option<Answer>,
    pub upstream: Option<Handler>,
    pub env: Vec<(&'static str, &'static str)>,
}

/// One fake Jev, one fake upstream, and the proxy between them, all closed when dropped.
pub struct Setup {
    pub jev: Jev,
    pub upstream: Server,
    pub proxy: Proxy,
}

pub fn setup(ctx: &Ctx, opts: Opts) -> Setup {
    let jev = Jev::start(opts.answer.unwrap_or_else(default_answer));
    let upstream = fake_upstream(opts.upstream);
    let proxy = start_proxy(ctx, &jev.url(), &upstream.url, &opts.env);
    Setup { jev, upstream, proxy }
}

/// Runs the status line under test with `env` in `cwd`, writing `stdin`; returns its stdout.
pub fn run_status_line(env: &[(OsString, OsString)], cwd: &Path, stdin: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_jev-statusline"))
        .current_dir(cwd)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start jev-statusline");
    let mut input = child.stdin.take().unwrap();
    let text = stdin.to_string();
    std::thread::spawn(move || {
        let _ = input.write_all(text.as_bytes());
    });
    let collect = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut out = Vec::new();
            let _ = pipe.read_to_end(&mut out);
            String::from_utf8_lossy(&out).into_owned()
        })
    };
    let stdout = collect(Box::new(child.stdout.take().unwrap()));
    let stderr = collect(Box::new(child.stderr.take().unwrap()));
    let deadline = Instant::now() + START_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("the status line did not finish within {} s", START_TIMEOUT.as_secs());
        }
        pause(10);
    };
    let (stdout, stderr) = (stdout.join().unwrap(), stderr.join().unwrap());
    assert!(status.success(), "status line exited {status}: {stderr}");
    stdout
}

// -------------------------------------------------------------------------------------------------
// The client side

/// Headers a real Claude Code request carries, as far as the proxy is concerned.
pub const CLIENT_HEADERS: &[(&str, &str)] = &[
    ("content-type", "application/json"),
    ("accept", "application/json"),
    ("anthropic-version", "2023-06-01"),
    ("anthropic-beta", "claude-code-20250219,interleaved-thinking-2025-05-14"),
    ("x-api-key", "sk-ant-api03-harness"),
    ("user-agent", "claude-cli/2.1.287 (external, cli)"),
    ("x-app", "cli"),
    ("x-stainless-retry-count", "0"),
    ("accept-encoding", "gzip, deflate, br"),
];

/// One request with exactly the headers given (plus `host` and `connection: close`), read to the
/// end; fails when no response arrives within `RESPONSE_TIMEOUT`.
pub fn send(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Response {
    let response = open(port, method, path, headers, body).finish().unwrap_or_else(|e| {
        assert!(!e.timed_out, "the proxy under test sent no response within {} s", RESPONSE_TIMEOUT.as_secs());
        panic!("{method} {path}: {}", e.message)
    });
    assert!(response.complete, "{method} {path}: the response ended early");
    response
}

/// `send` of a JSON body.
pub fn send_json(port: u16, path: &str, headers: &[(&str, &str)], body: &Json) -> Response {
    send(port, "POST", path, headers, Some(stringify(body).as_bytes()))
}

/// Opens a request whose response the test reads itself.
pub fn open(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Conn {
    http::open(port, method, path, headers, body, RESPONSE_TIMEOUT)
}

const HOP: &[&str] = &["connection", "keep-alive", "transfer-encoding"];

/// SPEC 7.7, upstream side: `host` names the upstream, `content-length` is the forwarded body's
/// length (absent for an empty body), every other client header arrives unchanged except the
/// hop-by-hop ones and `accept-encoding`, and the proxy's HTTP client adds nothing else.
pub fn assert_forwarded_headers(
    sent: &[(String, String)],
    received: &Record,
    host: &str,
    body: &[u8],
    accept_encoding_removed: bool,
) {
    assert_eq!(received.header("host").as_deref(), Some(host), "host names the upstream");
    if body.is_empty() {
        assert_eq!(received.header("content-length"), None, "no content-length for an empty body");
    } else {
        assert_eq!(
            received.header("content-length"),
            Some(body.len().to_string()),
            "content-length is the forwarded body's length"
        );
    }
    for (name, value) in sent {
        let key = name.to_ascii_lowercase();
        if HOP.contains(&key.as_str()) || ["host", "content-length", "accept-encoding"].contains(&key.as_str()) {
            continue;
        }
        assert_eq!(
            received.header(&key).as_deref(),
            Some(value.as_str()),
            "client header {key} reaches upstream unchanged"
        );
    }
    if accept_encoding_removed {
        assert_eq!(received.header("accept-encoding"), None, "accept-encoding removed");
    }
    let mut allowed: BTreeSet<String> = sent.iter().map(|(k, _)| k.to_ascii_lowercase()).collect();
    allowed.extend(["host", "content-length"].iter().chain(HOP).map(|s| (*s).to_string()));
    for key in received.header_names() {
        assert!(allowed.contains(&key), "the proxy added a header the client did not send: {key}");
    }
}

fn compared_on_client(name: &str) -> bool {
    ["content-type", "content-encoding", "request-id"].contains(&name)
        || name.starts_with("anthropic-")
        || name.starts_with("x-")
}

/// SPEC 7.7, client side: the compared headers are exactly the upstream's.
pub fn assert_relayed_headers(upstream: &[(&str, &str)], client: &BTreeMap<String, String>) {
    let want: BTreeMap<String, String> = upstream
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), (*v).to_string()))
        .filter(|(k, _)| compared_on_client(k))
        .collect();
    let got: BTreeMap<String, String> =
        client.iter().filter(|(k, _)| compared_on_client(k)).map(|(k, v)| (k.clone(), v.clone())).collect();
    assert_eq!(got, want, "content-type, content-encoding, request-id, anthropic-* and x-* are relayed as sent");
}

// -------------------------------------------------------------------------------------------------
// Requests and what they should become

/// The captured request's body with its session id replaced, so each scenario writes its own
/// status file.
pub fn fixture_body(session: &str) -> Json {
    let mut body = FIXTURE.at("body").clone();
    let mut meta = json::parse(body.at("metadata").at("user_id").str()).unwrap();
    meta.set("session_id", session.into());
    body.at_mut("metadata").set("user_id", stringify(&meta).into());
    body
}

/// The same conversation one step later: a tool ran and its result is being sent back.
pub fn continuation(body: &Json) -> Json {
    let messages = body.at("messages").arr();
    let mut next = vec![messages[0].clone()];
    next.push(json::parse(r#"{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"ls"}}]}"#).unwrap());
    next.push(
        json::parse(
            r#"{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"utils.js"}]}"#,
        )
        .unwrap(),
    );
    next.extend(messages[1..].iter().cloned());
    body.with(vec![("messages", Json::Arr(next))])
}

/// The captured request as a sub-agent's turn: the first message's last text block is the task.
pub fn sub_agent_body(session: &str, task: &str) -> Json {
    let mut sub = fixture_body(session);
    let content = sub.at_mut("messages").arr_mut()[0].at_mut("content").arr_mut();
    content.last_mut().expect("a text block").set("text", task.into());
    sub
}

pub const SESSION_PLACEHOLDER: &str = "@@SESSION@@";

/// The body's text with its session id replaced, so the same conversation under another session
/// id is one input.
pub fn sessionless(body: &Json) -> String {
    let mut copy = body.clone();
    if let Some(Json::Str(user_id)) = body.get("metadata").and_then(|m| m.get("user_id")) {
        let meta = json::parse(user_id).unwrap().with(vec![("session_id", SESSION_PLACEHOLDER.into())]);
        copy.at_mut("metadata").set("user_id", stringify(&meta).into());
    }
    stringify(&copy)
}

pub fn session_of(body: &Json) -> String {
    json::parse(body.at("metadata").at("user_id").str()).unwrap().at("session_id").str().to_string()
}

fn missing(what: &str, name: &str) -> String {
    format!(
        "conformance/fixtures/harness-expectations.json has no {what} for this input ({name}); the harness changed an input without updating the file (conformance/README.md says how to add one)"
    )
}

/// The `turns` entry for this body.
pub fn turn_of(body: &Json) -> Json {
    let sha = sha256::sha256_hex(sessionless(body).as_bytes());
    EXPECTED
        .at("turns")
        .arr()
        .iter()
        .find(|t| t.at("sha256").as_str() == Some(sha.as_str()))
        .cloned()
        .unwrap_or_else(|| panic!("{}", missing("turn", "prompt, conversation key and label")))
}

/// The text of the turn's last user message, as SPEC 7.1 extracts it.
pub fn prompt_of(body: &Json) -> String {
    turn_of(body).at("prompt").str().to_string()
}

/// The conversation key of SPEC 8.2: it hashes the session id too, so each session the tests use
/// has its own entry.
pub fn key_of(body: &Json) -> String {
    let session = session_of(body);
    let turn = turn_of(body);
    let key = turn.at("keys").get(&session).and_then(Json::as_str);
    key.unwrap_or_else(|| panic!("{}", missing("conversation key", &format!("session {session}")))).to_string()
}

/// The agent label of SPEC 8.2.
pub fn label_of(body: &Json) -> String {
    turn_of(body).at("label").str().to_string()
}

/// The `forwarded` lookup hash of a body, tier and model.
pub fn forwarded_sha(body: &Json, tier: &str, model: Option<&str>) -> String {
    let key = Json::Arr(vec![sessionless(body).into(), tier.into(), model.map_or(Json::Null, Json::from)]);
    sha256::sha256_hex(stringify(&key).as_bytes())
}

/// What the proxy should forward for a routed or defaulted body: the precomputed result of the
/// schema sanitising and tier rewrite (SPEC 7.3) for exactly this input, under this session id.
pub fn rewritten(body: &Json, tier: &str, model: Option<&str>) -> String {
    let sha = forwarded_sha(body, tier, model);
    let entry = EXPECTED.at("forwarded").arr().iter().find(|f| f.at("sha256").as_str() == Some(sha.as_str())).cloned();
    let entry = entry.unwrap_or_else(|| {
        panic!("{}", missing("forwarded body", &format!("tier {tier}, model {}", model.unwrap_or("none"))))
    });
    entry.at("body").str().replace(SESSION_PLACEHOLDER, &session_of(body))
}

/// The exact request the proxy's Jev client should send for this menu (SPEC 5.1, 6.1), before the
/// SDK adds `model`.
pub fn jev_request(prompt: &str, current: &str, context_tokens: f64, models: &Json) -> Json {
    let wanted = stringify(models);
    let entry = EXPECTED.at("questions").arr().iter().find(|q| stringify(q.at("models")) == wanted).cloned();
    let entry =
        entry.unwrap_or_else(|| panic!("{}", missing("Jev questions", &format!("menu {}", ids(models).join(", ")))));
    let available: Vec<Json> = ids(models).into_iter().map(Json::from).collect();
    obj(vec![
        (
            "state",
            obj(vec![
                ("request", prompt.into()),
                ("session", obj(vec![("current_model", current.into()), ("context_tokens", context_tokens.into())])),
                ("environment", obj(vec![("available_models", Json::Arr(available))])),
            ]),
        ),
        ("questions", entry.at("questions").clone()),
    ])
}

/// SPEC 7.3: `MathRound(utf16Length(stringify(body.messages)) / 4)`. Anchored to the golden
/// `jev-request` case's `contextTokens` for the captured request in `conformance_expectations.rs`.
pub fn context_tokens_of(body: &Json) -> f64 {
    let units = stringify(body.at("messages")).encode_utf16().count() as f64;
    (units / 4.0 + 0.5).floor()
}

/// Checks the Jev call: path, the headers SPEC 5.1 fixes, and the body byte for byte.
pub fn assert_jev_call(call: &Record, request: &Json, retry: Option<u32>) {
    assert_eq!(call.method, "POST");
    assert_eq!(call.url, "/v1/systemone");
    assert_eq!(call.header("authorization"), Some(format!("Bearer {JEV_KEY}")));
    assert_eq!(call.header("accept").as_deref(), Some("application/json"));
    assert_eq!(call.header("content-type").as_deref(), Some("application/json"));
    // SPEC 5.1: jev-router-<lang>/<version>, and none of the SDK's own X-TypeSafe-* headers.
    let agent = call.header("user-agent").unwrap_or_default();
    let version = agent.strip_prefix("jev-router-rust/").unwrap_or("");
    assert!(!version.is_empty() && !version.chars().any(char::is_whitespace), "user-agent {agent:?}");
    assert_eq!(call.header("x-typesafe-retry-count"), retry.map(|r| r.to_string()));
    let want = request.with(vec![("model", "jev-latest".into())]);
    assert_eq!(call.text(), stringify(&want), "the Jev request body, byte for byte");
}

/// Compares a status file with what it should hold, key order included. Every `at` must be a
/// clock reading taken during the test; they are then zeroed on both sides.
pub fn assert_status_file(actual: Option<Json>, expected: &Json, from: f64, to: f64) {
    fn zero(value: &Json, check: Option<(f64, f64)>) -> Json {
        match value {
            Json::Arr(items) => Json::Arr(items.iter().map(|v| zero(v, check)).collect()),
            Json::Obj(pairs) => Json::Obj(
                pairs
                    .iter()
                    .map(|(k, v)| {
                        if k != "at" {
                            return (k.clone(), zero(v, check));
                        }
                        if let Some((from, to)) = check {
                            let at = v.as_f64();
                            assert!(
                                at.is_some_and(|at| at >= from && at <= to),
                                "at {v:?} is a clock reading within the test ({from}..{to})"
                            );
                        }
                        (k.clone(), Json::Num(0.0))
                    })
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    let actual = actual.expect("the status file exists");
    assert_eq!(stringify(&zero(&actual, Some((from, to)))), stringify(&zero(expected, None)));
}

/// The metrics a routed turn records for a Jev answer (SPEC 6.1): each score / 9, and
/// `contextSize = min(contextTokens / 200000, 1)`. Anchored to the golden `jev-request` case's
/// `metrics` in `conformance_expectations.rs`.
pub fn metrics_of(answers: &Json, context_tokens: f64) -> Json {
    let score = |name: &str| (answers.at(name).at("score").as_f64().unwrap() / 9.0).into();
    obj(vec![
        ("taskComplexity", score("task_complexity")),
        ("reasoningRequired", score("reasoning_required")),
        ("toolComplexity", score("tool_complexity")),
        ("contextSize", (context_tokens / 200_000.0).min(1.0).into()),
    ])
}

/// The decision a routed turn records (SPEC 7.3 step 5, 8.2), with `at` to be zeroed.
pub fn routed_status(
    body: &Json,
    tier: &str,
    model: &str,
    jev: Option<&Json>,
    reason: &str,
    request: &Json,
    effort: Json,
) -> Json {
    let answers = jev.map(|j| j.at("answers"));
    let confidence = answers.and_then(|a| a.at("model").get("confidence")).cloned().unwrap_or(Json::Null);
    obj(vec![
        ("tier", tier.into()),
        ("prompt", prompt_of(body).into()),
        ("model", model.into()),
        ("confidence", confidence),
        ("metrics", answers.map_or(Json::Null, |a| metrics_of(a, context_tokens_of(body)))),
        ("reason", reason.into()),
        ("jev", jev.map_or(Json::Null, |j| obj(vec![("request", request.clone()), ("response", j.clone())]))),
        ("effort", effort),
        ("at", 0.0.into()),
    ])
}

/// The agent entry a routed turn records under its conversation key.
pub fn agent_entry(
    label: &str,
    main: bool,
    tier: &str,
    model: &str,
    confidence: Json,
    effort: Json,
    reason: &str,
) -> Json {
    obj(vec![
        ("label", label.into()),
        ("main", main.into()),
        ("tier", tier.into()),
        ("model", model.into()),
        ("confidence", confidence),
        ("effort", effort),
        ("reason", reason.into()),
        ("at", 0.0.into()),
    ])
}

/// A history item: the decision plus the agent it was for.
pub fn history_item(decision: &Json, key: &str, label: &str, main: bool) -> Json {
    decision.with(vec![("agent", obj(vec![("key", key.into()), ("label", label.into()), ("main", main.into())]))])
}
