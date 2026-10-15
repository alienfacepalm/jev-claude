//! Black-box harness (SPEC 16.3): how the built proxy carries bytes. Live streaming, aborts in
//! both directions, the 502 path, `HEAD` probes, redirects, compressed bodies, and an upstream
//! base path (SPEC 7.1, 7.2, 7.7).

mod conformance;

use conformance::http::{Handler, Record, Resp};
use conformance::json::{self, Json, stringify};
use conformance::*;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// A model the user picked, so these tests exercise transport without asking Jev anything.
fn passthrough() -> Vec<u8> {
    let body =
        fixture_body("harness-transport").with(vec![("model", "claude-opus-5-5".into()), ("stream", true.into())]);
    stringify(&body).into_bytes()
}

const SSE_HEADERS: &[(&str, &str)] =
    &[("content-type", "text/event-stream; charset=utf-8"), ("request-id", "req_stream"), ("x-harness", "sse")];

fn handler(f: impl Fn(&Record, &mut Resp) + Send + Sync + 'static) -> Handler {
    Arc::new(f)
}

/// The CRC-32 of gzip (ISO 3309), for a gzip member the test builds by hand.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// `data` as a gzip member (RFC 1952) holding one stored deflate block (RFC 1951 3.2.4).
fn gzip(data: &[u8]) -> Vec<u8> {
    let len = u16::try_from(data.len()).expect("one stored block");
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff, 1];
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&(!len).to_le_bytes());
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

#[test]
fn a_streamed_response_reaches_the_client_chunk_by_chunk_before_the_upstream_finishes() {
    const EVENTS: [&str; 3] = [
        "event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"hi\"}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    ];
    run(|ctx| {
        let writes: Arc<Mutex<Vec<Instant>>> = Arc::new(Mutex::new(Vec::new()));
        let log = writes.clone();
        let upstream = handler(move |_, res| {
            res.start(200, SSE_HEADERS);
            for (i, event) in EVENTS.iter().enumerate() {
                if i > 0 {
                    pause(400);
                }
                res.write(event.as_bytes());
                log.lock().unwrap().push(Instant::now());
            }
            res.end();
        });
        let Setup { jev: _jev, upstream: _upstream, proxy } =
            setup(ctx, Opts { upstream: Some(upstream), ..Opts::default() });
        let res = send(proxy.port, "POST", "/v1/messages", CLIENT_HEADERS, Some(&passthrough()));

        assert_eq!(res.status, 200);
        assert_eq!(res.text(), EVENTS.concat(), "every byte arrives, in order");
        assert_relayed_headers(SSE_HEADERS, &res.headers);
        let second_write = writes.lock().unwrap()[1];
        assert!(res.chunks[0].0 < second_write, "the first event arrived before the upstream sent the second");
        assert!(res.chunks.len() >= 3, "events were not held back and delivered together");
    });
}

#[test]
fn a_client_that_disconnects_stops_the_upstream_response() {
    run(|ctx| {
        let (closed, sent) = mpsc::channel::<u32>();
        let upstream = handler(move |_, res| {
            res.start(200, SSE_HEADERS);
            let mut count = 0;
            while count < 200 {
                if !res.write(format!("data: {{\"n\":{count}}}\n\n").as_bytes()) {
                    break;
                }
                count += 1;
                pause(20);
            }
            if count == 200 {
                res.end();
            }
            let _ = closed.send(count);
        });
        let Setup { jev: _jev, upstream: _upstream, proxy } =
            setup(ctx, Opts { upstream: Some(upstream), ..Opts::default() });
        let mut client = open(proxy.port, "POST", "/v1/messages", CLIENT_HEADERS, Some(&passthrough()));
        client.read_head().expect("a response head");
        let first = client.read_some().expect("the first event");
        assert!(!first.is_empty(), "the first event arrives");
        client.abort();

        let count = sent.recv_timeout(Duration::from_secs(5));
        assert!(count.is_ok(), "the upstream connection was closed after the client left (still streaming after 5 s)");
        let count = count.unwrap();
        assert!(count < 200, "the upstream stopped early (after {count} of 200 events)");
    });
}

#[test]
fn an_upstream_that_drops_mid_stream_ends_the_clients_response_instead_of_hanging_it() {
    run(|ctx| {
        let upstream = handler(|_, res| {
            res.start(200, SSE_HEADERS);
            res.write(b"data: {\"n\":0}\n\n");
            pause(100);
            res.destroy();
        });
        let Setup { jev: _jev, upstream: _upstream, proxy } =
            setup(ctx, Opts { upstream: Some(upstream), ..Opts::default() });
        let client = conformance::http::open(
            proxy.port,
            "POST",
            "/v1/messages",
            CLIENT_HEADERS,
            Some(&passthrough()),
            Duration::from_secs(5),
        );
        let outcome = match client.finish() {
            Ok(res) if res.complete => "complete",
            Err(e) if e.timed_out => "still waiting after 5s",
            Ok(_) | Err(_) => "incomplete",
        };
        assert_eq!(outcome, "incomplete");
    });
}

#[test]
fn an_unreachable_upstream_answers_502_with_a_json_error() {
    run(|ctx| {
        let jev = Jev::start(always(Json::Obj(Vec::new())));
        // A port that was just free and is now closed: connections are refused.
        let gone_url = fake_upstream(None).url.clone();
        let proxy = start_proxy(ctx, &jev.url(), &gone_url, &[]);

        let res = send(proxy.port, "POST", "/v1/messages", CLIENT_HEADERS, Some(&passthrough()));
        assert_eq!(res.status, 502);
        assert_eq!(res.header("content-type"), Some("application/json"));
        let error = json::parse_bytes(&res.body).unwrap();
        assert_eq!(error.keys(), ["type", "error"]);
        assert_eq!(error.at("type").str(), "error");
        assert_eq!(error.at("error").keys(), ["message"]);
        assert!(error.at("error").at("message").as_str().is_some(), "the message is a string");
    });
}

#[test]
fn head_is_answered_by_the_proxy_itself() {
    run(|ctx| {
        let Setup { jev: _jev, upstream, proxy } = setup(ctx, Opts::default());
        let res = send(proxy.port, "HEAD", "/", &[], None);
        assert_eq!(res.status, 200);
        assert_eq!(res.body, b"");
        let deep = send(proxy.port, "HEAD", "/v1/messages?beta=true", &[], None);
        assert_eq!(deep.status, 200);
        assert_eq!(upstream.requests().len(), 0, "nothing reached the upstream");
    });
}

#[test]
fn redirects_are_relayed_not_followed_and_compressed_bodies_pass_through_undecoded() {
    run(|ctx| {
        let gzipped = gzip(br#"{"id":"msg_gz","type":"message"}"#);
        let zipped_reply = gzipped.clone();
        let upstream = handler(move |record, res| match record.url.as_str() {
            "/v1/messages" => res.reply(
                307,
                &[
                    ("location", "https://elsewhere.example/v1/messages"),
                    ("content-type", "text/plain"),
                    ("x-redirect", "yes"),
                ],
                b"moved",
            ),
            "/v1/messages/count_tokens" => res.reply(
                200,
                &[("content-type", "application/json"), ("content-encoding", "gzip"), ("request-id", "req_gz")],
                &zipped_reply,
            ),
            _ => res.reply(404, &[], b""),
        });
        let Setup { jev: _jev, upstream, proxy } = setup(ctx, Opts { upstream: Some(upstream), ..Opts::default() });

        let moved = send(proxy.port, "POST", "/v1/messages", CLIENT_HEADERS, Some(&passthrough()));
        assert_eq!(moved.status, 307);
        assert_eq!(moved.header("location"), Some("https://elsewhere.example/v1/messages"));
        assert_eq!(moved.text(), "moved");
        assert_relayed_headers(&[("content-type", "text/plain"), ("x-redirect", "yes")], &moved.headers);
        assert_eq!(upstream.requests().len(), 1, "the redirect was not followed");

        let zipped = send(proxy.port, "POST", "/v1/messages/count_tokens", CLIENT_HEADERS, Some(&passthrough()));
        assert_eq!(zipped.status, 200);
        assert_eq!(zipped.body, gzipped, "the gzip bytes arrive exactly as the upstream sent them");
        assert_relayed_headers(
            &[("content-type", "application/json"), ("content-encoding", "gzip"), ("request-id", "req_gz")],
            &zipped.headers,
        );
        let accept_encoding =
            CLIENT_HEADERS.iter().find(|(k, _)| *k == "accept-encoding").map(|(_, v)| (*v).to_string());
        assert_eq!(
            upstream.request(1).header("accept-encoding"),
            accept_encoding,
            "a message request keeps the client's accept-encoding"
        );
    });
}

#[test]
fn an_upstream_base_path_is_kept_minus_one_trailing_slash_and_the_host_carries_its_port() {
    run(|ctx| {
        let jev = Jev::start(always(Json::Obj(Vec::new())));
        let upstream = fake_upstream(None);
        let proxy = start_proxy(ctx, &jev.url(), &format!("{}/gateway/anthropic/", upstream.url), &[]);

        let body = passthrough();
        let res = send(proxy.port, "POST", "/v1/messages?beta=true", CLIENT_HEADERS, Some(&body));
        assert_eq!(res.status, 200);
        assert_eq!(upstream.request(0).url, "/gateway/anthropic/v1/messages?beta=true");
        assert_eq!(upstream.request(0).body, body);
        assert_forwarded_headers(
            &res.sent,
            &upstream.request(0),
            &format!("127.0.0.1:{}", upstream.port),
            &body,
            false,
        );
    });
}
