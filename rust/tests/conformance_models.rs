//! Black-box harness (SPEC 16.3): `GET /v1/models` through the built proxy. The reply is relayed
//! whatever its status, the account's catalog feeds the Jev menu, and `calibration.json` records it
//! (SPEC 7.2 step 5, 8.3).

mod conformance;

use conformance::http::{self, Handler, Record, Resp};
use conformance::json::{Json, obj};
use conformance::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const CATALOG: &str = r#"{"data":[{"type":"model","id":"claude-opus-4-8","display_name":"Claude Opus 4.8","created_at":"2026-01-05T00:00:00Z"},{"type":"model","id":"claude-opus-5-5","display_name":"Claude Opus 5.5","created_at":"2026-09-02T00:00:00Z","max_input_tokens":1000000},{"type":"model","id":"claude-opus-6","display_name":"Claude Opus 6","created_at":"2026-11-01T00:00:00Z","max_input_tokens":1000000},{"type":"model","id":"claude-sonnet-5-5","display_name":"Claude Sonnet 5.5","created_at":"2026-08-11T00:00:00Z"},{"type":"model","id":"claude-haiku-5-5","display_name":"Claude Haiku 5.5","created_at":"2026-08-11T00:00:00Z","max_input_tokens":200000},{"type":"model","id":"claude-3-7-sonnet-20250219","display_name":"Claude Sonnet 3.7","created_at":"2025-02-19T00:00:00Z"}],"has_more":false,"first_id":"claude-opus-4-8"}"#;

const MODELS_HEADERS: &[(&str, &str)] = &[
    ("content-type", "application/json"),
    ("request-id", "req_models"),
    ("anthropic-organization-id", "org_harness"),
    ("x-cache", "miss"),
];

const REQUEST_HEADERS: &[(&str, &str)] = &[
    ("anthropic-version", "2023-06-01"),
    ("x-api-key", "sk-ant-api03-harness"),
    ("accept-encoding", "gzip, br"),
    ("user-agent", "claude-cli/2.1.287"),
];

/// The configured models, which calibration falls back to.
const STATIC_IDS: [&str; 4] = ["claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5", "claude-fable-5-1"];

/// One reply of the catalog endpoint.
#[derive(Clone)]
struct CatalogReply {
    status: u16,
    body: &'static str,
    headers: &'static [(&'static str, &'static str)],
}

const fn reply(status: u16, body: &'static str) -> CatalogReply {
    CatalogReply { status, body, headers: MODELS_HEADERS }
}

/// An upstream whose `/v1/models` answers with `replies` in turn (the last one repeating) and whose
/// messages endpoint answers simply.
fn upstream_with(replies: Vec<CatalogReply>) -> Handler {
    let next = AtomicUsize::new(0);
    Arc::new(move |record: &Record, res: &mut Resp| {
        if record.url.starts_with("/v1/models") {
            let i = next.fetch_add(1, Ordering::SeqCst).min(replies.len() - 1);
            let r = &replies[i];
            return res.reply(r.status, r.headers, r.body.as_bytes());
        }
        res.reply(200, &[("content-type", "application/json")], br#"{"id":"msg_1","type":"message"}"#);
    })
}

fn calibration(ids: &[&str], newer: &[&str]) -> Json {
    let list = |items: &[&str]| Json::Arr(items.iter().map(|s| Json::from(*s)).collect());
    obj(vec![("newer", list(newer)), ("models", list(ids)), ("at", 0.0.into())])
}

fn models(port: u16, path: &str) -> http::Response {
    send(port, "GET", path, REQUEST_HEADERS, None)
}

#[test]
fn a_200_catalog_is_relayed_recorded_for_calibration_and_becomes_the_jev_menu() {
    run(|ctx| {
        let Setup { jev, upstream, proxy } = setup(
            ctx,
            Opts {
                answer: Some(always(jev_answer("claude-opus-6", 0.88, [6, 7, 5]))),
                upstream: Some(upstream_with(vec![reply(200, CATALOG)])),
                ..Opts::default()
            },
        );
        let from = now_ms();
        let res = models(proxy.port, "/v1/models?limit=1000");
        let after_models = now_ms();

        assert_eq!(res.status, 200);
        assert_eq!(res.text(), CATALOG, "relayed byte for byte");
        assert_relayed_headers(MODELS_HEADERS, &res.headers);
        let forwarded = upstream.request(0);
        assert_eq!(forwarded.method, "GET");
        assert_eq!(forwarded.url, "/v1/models?limit=1000");
        assert_eq!(forwarded.body, b"");
        assert_forwarded_headers(&res.sent, &forwarded, &upstream.host(), b"", true);

        let menu = menu("catalog");
        assert_eq!(ids(&menu), ["claude-opus-6", "claude-sonnet-5-5", "claude-haiku-5-5"]);
        let menu_ids = ids(&menu);
        let menu_ids: Vec<&str> = menu_ids.iter().map(String::as_str).collect();
        assert_status_file(
            proxy.status_file("calibration.json"),
            &calibration(&menu_ids, &["claude-opus-6"]),
            from,
            after_models,
        );

        let body = fixture_body("harness-catalog");
        send_json(proxy.port, FIXTURE.at("url").str(), CLIENT_HEADERS, &body);
        let request = jev_request(&prompt_of(&body), SONNET, context_tokens_of(&body), &menu);
        assert_jev_call(&jev.posts()[0], &request, None);
        assert_eq!(
            upstream.request(1).text(),
            rewritten(&body, "opus", Some("claude-opus-6")),
            "Jev's exact model is used"
        );
        let status = proxy.file_of("harness-catalog").expect("the status file exists");
        assert_eq!(status.at("model").str(), "claude-opus-6");
        assert_eq!(status.at("tier").str(), "opus");
    });
}

#[test]
fn a_401_is_relayed_as_sent_and_calibration_falls_back_to_the_configured_models() {
    const ERROR: &str = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
    run(|ctx| {
        let Setup { jev: _jev, upstream, proxy } =
            setup(ctx, Opts { upstream: Some(upstream_with(vec![reply(401, ERROR)])), ..Opts::default() });
        let from = now_ms();
        let res = models(proxy.port, "/v1/models");
        let to = now_ms();

        assert_eq!(res.status, 401);
        assert_eq!(res.text(), ERROR);
        assert_relayed_headers(MODELS_HEADERS, &res.headers);
        assert_forwarded_headers(&res.sent, &upstream.request(0), &upstream.host(), b"", true);
        assert_status_file(proxy.status_file("calibration.json"), &calibration(&STATIC_IDS, &[]), from, to);
    });
}

#[test]
fn a_catalog_whose_data_cannot_be_iterated_or_that_is_not_json_writes_no_calibration_file() {
    let replies = vec![
        reply(200, r#"{"data":{"id":"claude-opus-6"}}"#),
        reply(200, r#"{"data":5}"#),
        CatalogReply { status: 502, body: "<html>bad gateway</html>", headers: &[("content-type", "text/html")] },
        // A string iterates its characters, none of which is a model, so the static ids are written.
        reply(200, r#"{"data":"claude-opus-6"}"#),
    ];
    run(move |ctx| {
        let Setup { jev: _jev, upstream: _upstream, proxy } =
            setup(ctx, Opts { upstream: Some(upstream_with(replies.clone())), ..Opts::default() });
        for reply in &replies[..3] {
            let res = models(proxy.port, "/v1/models");
            assert_eq!(res.status, reply.status);
            assert_eq!(res.text(), reply.body, "relayed whatever happened to the catalog");
            assert!(proxy.status_file("calibration.json").is_none(), "no calibration file after {}", reply.body);
        }
        let from = now_ms();
        models(proxy.port, "/v1/models");
        assert_status_file(proxy.status_file("calibration.json"), &calibration(&STATIC_IDS, &[]), from, now_ms());
    });
}
