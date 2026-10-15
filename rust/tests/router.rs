//! The Jev client's failure, retry, backoff and timeout rules (SPEC 5.3), driven through
//! `ask_jev` against a loopback Jev that scripts each reply.
//!
//! Every test sets the Jev address and keys through the process environment overlay, so they run
//! one at a time under [`LOCK`]. This is its own test binary, so nothing else shares the overlay.

// Each test's doc comment is its title as a plain sentence; Markdown backticks would change them.
#![allow(clippy::doc_markdown)]

mod common;

use common::*;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use jev_router::config::Model;
use jev_router::envx;
use jev_router::router::{RouteArgs, ask_jev};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// One scripted reply: status, extra headers, and how long to wait before answering.
#[derive(Clone)]
struct Script {
    status: u16,
    headers: Vec<(&'static str, &'static str)>,
    delay: Duration,
}

fn reply(status: u16) -> Script {
    Script { status, headers: vec![], delay: Duration::ZERO }
}

fn reply_with(status: u16, headers: &[(&'static str, &'static str)]) -> Script {
    Script { status, headers: headers.to_vec(), delay: Duration::ZERO }
}

/// What the fake Jev saw for one POST.
#[derive(Debug, Clone)]
struct Post {
    path: String,
    authorization: Option<String>,
    retry_count: Option<String>,
    at: Instant,
}

struct FakeJev {
    url: String,
    posts: Arc<Mutex<Vec<Post>>>,
}

impl FakeJev {
    fn posts(&self) -> Vec<Post> {
        self.posts.lock().unwrap().clone()
    }

    /// The time between the first and second POST.
    fn gap(&self) -> Duration {
        let posts = self.posts();
        assert!(posts.len() >= 2, "expected a retry, got {} POSTs", posts.len());
        posts[1].at - posts[0].at
    }
}

/// A 200 body `ask_jev` accepts: the first golden `jev-request` case's response.
fn ok_body() -> String {
    let case = load_cases("jev-request").into_iter().next().unwrap();
    case.input.get("response").to_js_string().to_string_lossy()
}

/// A loopback Jev answering each POST with the next scripted reply (the last one repeats).
async fn fake_jev(script: Vec<Script>) -> FakeJev {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let posts = Arc::new(Mutex::new(Vec::new()));
    let queue = Arc::new(Mutex::new(VecDeque::from(script)));
    let ok = ok_body();
    let log = posts.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { return };
            let (log, queue, ok) = (log.clone(), queue.clone(), ok.clone());
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let (log, queue, ok) = (log.clone(), queue.clone(), ok.clone());
                    async move {
                        let at = Instant::now();
                        let header = |name: &str| {
                            req.headers().get(name).map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
                        };
                        let is_post = req.method() == hyper::Method::POST;
                        let post = Post {
                            path: req.uri().path().to_string(),
                            authorization: header("authorization"),
                            retry_count: header("x-typesafe-retry-count"),
                            at,
                        };
                        let _ = req.into_body().collect().await;
                        if !is_post {
                            return Ok::<_, std::convert::Infallible>(hyper::Response::new(Full::new(Bytes::new())));
                        }
                        log.lock().unwrap().push(post);
                        let script = {
                            let mut q = queue.lock().unwrap();
                            if q.len() > 1 { q.pop_front().unwrap() } else { q.front().cloned().unwrap() }
                        };
                        tokio::time::sleep(script.delay).await;
                        let body = if script.status == 200 { ok } else { r#"{"error":"scripted"}"#.to_string() };
                        let mut res =
                            hyper::Response::builder().status(script.status).header("content-type", "application/json");
                        for (k, v) in script.headers {
                            res = res.header(k, v);
                        }
                        Ok(res.body(Full::new(Bytes::from(body))).unwrap())
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    FakeJev { url: format!("http://127.0.0.1:{port}"), posts }
}

/// Points the client at `base` with a key; the default model name is left alone.
fn use_jev(base: &str) {
    envx::set("TYPESAFE_BASE_URL", base);
    envx::set("JEV_API_KEY", "router-test-key");
    envx::unset_overlay("TYPESAFE_API_KEY");
    envx::unset_overlay("TYPESAFE_DEFAULT_MODEL");
}

fn args() -> RouteArgs {
    let model = Model::from_value(&json(r#"{"id":"claude-opus-5-5","tier":"opus","releasedAt":""}"#)).unwrap();
    RouteArgs { prompt: "rename x to count".into(), current: "opus".into(), context_tokens: 100.0, models: vec![model] }
}

/// "a non-retryable 4xx is sent once"
#[tokio::test(flavor = "multi_thread")]
async fn non_retryable_4xx_is_sent_once() {
    let _guard = LOCK.lock().await;
    for status in [401, 403, 404, 422] {
        let jev = fake_jev(vec![reply(status), reply(200)]).await;
        use_jev(&jev.url);
        assert!(ask_jev(args()).await.is_none(), "{status} fails the turn");
        assert_eq!(jev.posts().len(), 1, "{status} is not retried");
    }
}

/// "408, 429 and 5xx are retried once, saying so in x-typesafe-retry-count"
#[tokio::test(flavor = "multi_thread")]
async fn retryable_status_is_retried_once_with_retry_count() {
    let _guard = LOCK.lock().await;
    for status in [408, 429, 503] {
        let jev = fake_jev(vec![reply(status), reply(200)]).await;
        use_jev(&jev.url);
        assert!(ask_jev(args()).await.is_some(), "{status} then 200 succeeds");
        let posts = jev.posts();
        assert_eq!(posts.len(), 2, "{status} is retried once");
        assert_eq!(posts[0].retry_count, None, "the first attempt has no retry count");
        assert_eq!(posts[1].retry_count.as_deref(), Some("1"));
    }
    let jev = fake_jev(vec![reply(503)]).await;
    use_jev(&jev.url);
    assert!(ask_jev(args()).await.is_none(), "two 503s fail the turn");
    assert_eq!(jev.posts().len(), 2, "only one retry");
}

/// "retry-after-ms is honoured"
#[tokio::test(flavor = "multi_thread")]
async fn retry_after_ms_is_honoured() {
    let _guard = LOCK.lock().await;
    let jev = fake_jev(vec![reply_with(503, &[("retry-after-ms", "700")]), reply(200)]).await;
    use_jev(&jev.url);
    assert!(ask_jev(args()).await.is_some());
    let gap = jev.gap();
    assert!(gap >= Duration::from_millis(650) && gap < Duration::from_millis(1000), "gap {gap:?}");
}

/// "retry-after in seconds is honoured"
#[tokio::test(flavor = "multi_thread")]
async fn retry_after_seconds_is_honoured() {
    let _guard = LOCK.lock().await;
    let jev = fake_jev(vec![reply_with(503, &[("retry-after", "1")]), reply(200)]).await;
    use_jev(&jev.url);
    assert!(ask_jev(args()).await.is_some());
    let gap = jev.gap();
    assert!(gap >= Duration::from_millis(950) && gap < Duration::from_millis(1400), "gap {gap:?}");
}

/// "a retry-after over 60 s falls back to the backoff"
#[tokio::test(flavor = "multi_thread")]
async fn retry_after_over_60s_falls_back_to_backoff() {
    let _guard = LOCK.lock().await;
    let jev = fake_jev(vec![reply_with(503, &[("retry-after", "120")]), reply(200)]).await;
    use_jev(&jev.url);
    assert!(ask_jev(args()).await.is_some());
    let gap = jev.gap();
    assert!(gap < Duration::from_millis(600), "gap {gap:?}");
}

/// "a negative retry-after-ms is ignored in favour of retry-after"
#[tokio::test(flavor = "multi_thread")]
async fn negative_retry_after_ms_is_ignored() {
    let _guard = LOCK.lock().await;
    let jev = fake_jev(vec![reply_with(503, &[("retry-after-ms", "-5"), ("retry-after", "1")]), reply(200)]).await;
    use_jev(&jev.url);
    assert!(ask_jev(args()).await.is_some());
    let gap = jev.gap();
    assert!(gap >= Duration::from_millis(950) && gap < Duration::from_millis(1400), "gap {gap:?}");
}

/// "a retry-after HTTP date in the past retries at once"
#[tokio::test(flavor = "multi_thread")]
async fn past_http_date_retries_promptly() {
    let _guard = LOCK.lock().await;
    let jev = fake_jev(vec![reply_with(503, &[("retry-after", "Sun, 06 Nov 1994 08:49:37 GMT")]), reply(200)]).await;
    use_jev(&jev.url);
    assert!(ask_jev(args()).await.is_some());
    let gap = jev.gap();
    assert!(gap < Duration::from_millis(100), "gap {gap:?}: a past date means no wait, not the backoff");
}

/// "a retry-after HTTP date in the future is waited for"
#[tokio::test(flavor = "multi_thread")]
async fn future_http_date_is_waited_for() {
    let _guard = LOCK.lock().await;
    // An HTTP date has whole seconds, so wait for the next second to be most of a second away.
    let now_ms = jev_router::timefmt::now_ms();
    let next_whole = (now_ms / 1000.0).floor() * 1000.0 + 2000.0;
    let date = http_date(next_whole);
    let date: &'static str = Box::leak(date.into_boxed_str());
    let jev = fake_jev(vec![reply_with(503, &[("retry-after", date)]), reply(200)]).await;
    use_jev(&jev.url);
    assert!(ask_jev(args()).await.is_some());
    let gap = jev.gap();
    assert!(gap >= Duration::from_millis(900) && gap < Duration::from_millis(2100), "gap {gap:?} for {date}");
}

/// `Sun, 06 Nov 1994 08:49:37 GMT` for a time in milliseconds.
fn http_date(ms: f64) -> String {
    let iso = jev_router::timefmt::iso(ms); // 1994-11-06T08:49:37.000Z
    let (y, mo, d, t) = (&iso[0..4], &iso[5..7], &iso[8..10], &iso[11..19]);
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let month = months[mo.parse::<usize>().unwrap() - 1];
    // The weekday is not checked by the parser; any is accepted.
    format!("Mon, {d} {month} {y} {t} GMT")
}

/// "without retry-after the backoff waits 150 ms less up to a quarter"
#[tokio::test(flavor = "multi_thread")]
async fn default_backoff_window() {
    let _guard = LOCK.lock().await;
    let jev = fake_jev(vec![reply(503), reply(200)]).await;
    use_jev(&jev.url);
    assert!(ask_jev(args()).await.is_some());
    let gap = jev.gap();
    // 150 x (1 - r x 0.25) is 112-150 ms; the bounds allow for scheduler slack.
    assert!(gap >= Duration::from_millis(100) && gap <= Duration::from_millis(450), "gap {gap:?}");
}

/// "a slow Jev hits the 1500 ms attempt timeout and the 3000 ms deadline"
#[tokio::test(flavor = "multi_thread")]
async fn slow_jev_hits_attempt_timeout_and_deadline() {
    let _guard = LOCK.lock().await;
    let jev = fake_jev(vec![Script { status: 200, headers: vec![], delay: Duration::from_secs(5) }]).await;
    use_jev(&jev.url);
    let started = Instant::now();
    assert!(ask_jev(args()).await.is_none());
    let elapsed = started.elapsed();
    let posts = jev.posts();
    assert_eq!(posts.len(), 2, "one attempt, one retry");
    assert!(posts[1].at - posts[0].at >= Duration::from_millis(1500), "the first attempt ran its full 1500 ms");
    assert!(elapsed >= Duration::from_millis(2900) && elapsed < Duration::from_millis(3600), "elapsed {elapsed:?}");
}

/// "no key sends nothing"
#[tokio::test(flavor = "multi_thread")]
async fn no_key_sends_nothing() {
    let _guard = LOCK.lock().await;
    // The overlay cannot hide a key in the real environment (a developer's shell may have one).
    if std::env::var_os("JEV_API_KEY").is_some() || std::env::var_os("TYPESAFE_API_KEY").is_some() {
        eprintln!("note: no_key_sends_nothing skipped: a Jev key is set in the environment");
        return;
    }
    let jev = fake_jev(vec![reply(200)]).await;
    use_jev(&jev.url);
    envx::unset_overlay("JEV_API_KEY");
    assert!(ask_jev(args()).await.is_none());
    assert!(jev.posts().is_empty(), "nothing is sent without a key");
}

/// "an empty JEV_API_KEY still wins over TYPESAFE_API_KEY and sends `Bearer `"
#[tokio::test(flavor = "multi_thread")]
async fn empty_jev_api_key_wins_over_typesafe_key() {
    let _guard = LOCK.lock().await;
    let jev = fake_jev(vec![reply(200)]).await;
    use_jev(&jev.url);
    envx::set("JEV_API_KEY", "");
    envx::set("TYPESAFE_API_KEY", "typesafe-key");
    assert_eq!(envx::get("JEV_API_KEY").as_deref(), Some(""), "an empty variable is set, not missing");
    assert!(ask_jev(args()).await.is_some());
    let posts = jev.posts();
    assert_eq!(posts.len(), 1);
    // The client sends `Bearer ` (an empty key, not TYPESAFE_API_KEY); the HTTP parser drops a
    // header value's trailing space, so the fake sees `Bearer`.
    assert_eq!(posts[0].authorization.as_deref(), Some("Bearer"), "the empty JEV_API_KEY was used");
    envx::unset_overlay("TYPESAFE_API_KEY");
}

/// "a trailing slash on TYPESAFE_BASE_URL is dropped, and a path prefix is kept"
#[tokio::test(flavor = "multi_thread")]
async fn base_url_trailing_slash_and_path_prefix() {
    let _guard = LOCK.lock().await;
    let jev = fake_jev(vec![reply(200)]).await;
    for (base, path) in [(format!("{}/", jev.url), "/v1/systemone"), (format!("{}/api/", jev.url), "/api/v1/systemone")]
    {
        jev.posts.lock().unwrap().clear();
        use_jev(&base);
        assert!(ask_jev(args()).await.is_some(), "{base}");
        let posts = jev.posts();
        assert_eq!(posts.len(), 1, "{base}");
        assert_eq!(posts[0].path, path, "{base}");
        assert_eq!(posts[0].authorization.as_deref(), Some("Bearer router-test-key"));
    }
}

/// "an empty menu sends nothing"
#[tokio::test(flavor = "multi_thread")]
async fn an_empty_menu_sends_nothing() {
    let _guard = LOCK.lock().await;
    let jev = fake_jev(vec![reply(200)]).await;
    use_jev(&jev.url);
    let mut a = args();
    a.models.clear();
    assert!(ask_jev(a).await.is_none());
    assert!(jev.posts().is_empty());
}
