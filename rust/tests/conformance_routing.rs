//! Black-box harness (SPEC 16.3): routed, passthrough, auxiliary, continuation, and failure turns
//! through the built proxy, with the Jev request, the forwarded request, the status file, and the
//! status line checked for each (SPEC 7.2-7.7, 8.2, 12).

mod conformance;

use conformance::json::{self, Json, obj, stringify};
use conformance::*;
use std::path::PathBuf;
use std::sync::Arc;

/// The status line's stdin directory for a proxy: one that does not exist (so no git branch).
fn status_dir(proxy: &Proxy) -> PathBuf {
    proxy.base.join("no-such-dir").join("proj")
}

/// The status line's stdin for a session, with `extra` keys appended.
fn status_input(proxy: &Proxy, session: &str, extra: Vec<(&str, Json)>) -> String {
    let input = obj(vec![
        ("session_id", session.into()),
        ("workspace", obj(vec![("current_dir", status_dir(proxy).to_string_lossy().into_owned().into())])),
        ("context_window", obj(vec![("used_percentage", 12.5.into())])),
    ]);
    stringify(&input.with(extra))
}

/// The end of the line: the directory name, the context share, then the whole path, dimmed
/// (SPEC 12).
fn tail_of(proxy: &Proxy) -> String {
    let sep = sep();
    format!("{sep}{} proj{sep}{} 13% {DIM}· {}{RESET}\n", icon("❐"), icon("≡"), status_dir(proxy).display())
}

fn path() -> String {
    FIXTURE.at("url").str().to_string()
}

fn post(proxy: &Proxy, body: &Json) -> conformance::http::Response {
    send_json(proxy.port, &path(), CLIENT_HEADERS, body)
}

#[test]
fn a_routed_turn_the_jev_request_the_rewritten_request_the_status_file_and_the_status_line() {
    run(|ctx| {
        let session = "harness-routed";
        let answer = jev_answer(HAIKU, 0.92, [2, 3, 1]);
        let Setup { jev, upstream, proxy } =
            setup(ctx, Opts { answer: Some(always(answer.clone())), ..Opts::default() });
        let body = fixture_body(session);
        let from = now_ms();

        let res = post(&proxy, &body);
        let to = now_ms();

        assert_eq!(res.status, 200);
        assert_eq!(res.text(), UPSTREAM_REPLY);
        assert_relayed_headers(UPSTREAM_REPLY_HEADERS, &res.headers);

        let prompt = prompt_of(&body);
        assert_eq!(
            prompt, "rename the variable x to count in utils.js",
            "reminders stripped and the trailing system message skipped"
        );
        let request = jev_request(&prompt, SONNET, context_tokens_of(&body), &menu("static"));
        assert_eq!(jev.posts().len(), 1, "Jev is asked once");
        assert_jev_call(&jev.posts()[0], &request, None);

        assert_eq!(upstream.requests().len(), 1);
        let forwarded = upstream.request(0);
        assert_eq!(forwarded.method, "POST");
        assert_eq!(forwarded.url, path());
        let expected_body = rewritten(&body, "haiku", Some(HAIKU));
        assert_eq!(forwarded.text(), expected_body, "the body is the captured request routed to Haiku");
        let sent = json::parse_bytes(&forwarded.body).unwrap();
        assert_eq!(sent.at("model").str(), HAIKU);
        assert!(sent.get("thinking").is_none(), "Haiku takes no adaptive thinking");
        assert!(sent.get("context_management").is_none(), "nor the thinking-clearing edit");
        assert!(sent.get("output_config").is_none(), "nor an effort");
        assert_forwarded_headers(&res.sent, &forwarded, &upstream.host(), expected_body.as_bytes(), false);

        let key = key_of(&body);
        let label = label_of(&body);
        let decision = routed_status(&body, "haiku", HAIKU, Some(&answer), "jev", &request, Json::Null);
        let agent = agent_entry(&label, true, "haiku", HAIKU, 0.92.into(), Json::Null, "jev");
        let expected = decision.with(vec![
            ("agents", obj(vec![(key.as_str(), agent)])),
            ("history", Json::Arr(vec![history_item(&decision, &key, &label, true)])),
        ]);
        assert_status_file(proxy.file_of(session), &expected, from, to);

        let line = run_status_line(&proxy.env, &proxy.cwd, &status_input(&proxy, session, vec![]));
        assert_eq!(line, format!("{} {GREEN}Haiku 5.5{RESET} {DIM}(92%){RESET}{}", icon("✧✦"), tail_of(&proxy)));

        // The fire-and-forget prewarm (SPEC 5.4) reaches Jev's origin.
        let prewarmed = || jev.server.requests().iter().any(|r| r.method == "HEAD" && r.url == "/");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !prewarmed() && std::time::Instant::now() < deadline {
            pause(50);
        }
        assert!(prewarmed(), "a HEAD request warmed the Jev connection");
    });
}

#[test]
fn an_auxiliary_call_is_not_routed_a_tool_continuation_keeps_the_tier_and_a_sub_agent_gets_its_own() {
    const MAIN_TASK: &str = "rename the variable x to count in utils.js";
    const SUB_TASK: &str = "Search the codebase for callers of handler 7 and report them";
    run(|ctx| {
        let session = "harness-continuation";
        let main_answer = jev_answer(HAIKU, 0.92, [2, 3, 1]);
        let sub_answer = jev_answer(SONNET, 0.81, [4, 4, 5]);
        let (a, b) = (main_answer.clone(), sub_answer.clone());
        let answer: Answer = Arc::new(move |req: &Json| match req.at("state").at("request").as_str() {
            Some(MAIN_TASK) => JevReply::Ok(a.clone()),
            Some(SUB_TASK) => JevReply::Ok(b.clone()),
            _ => JevReply::Status(200, String::new()),
        });
        let Setup { jev, upstream, proxy } = setup(ctx, Opts { answer: Some(answer), ..Opts::default() });
        let body = fixture_body(session);
        let from = now_ms();

        // Claude Code's title call: the sentinel, no tools. It runs on the default tier, asks
        // nobody, records nothing, and does not become the session's main thread.
        let aux = obj(vec![
            ("model", "jev-router".into()),
            ("metadata", body.at("metadata").clone()),
            ("max_tokens", 32.0.into()),
            (
                "messages",
                json::parse(r#"[{"role":"user","content":"Summarize this conversation in five words"}]"#).unwrap(),
            ),
        ]);
        post(&proxy, &aux);
        assert_eq!(jev.posts().len(), 0, "no Jev call for a call without tools");
        assert_eq!(
            upstream.request(0).text(),
            rewritten(&aux, "sonnet", Some(SONNET)),
            "the default tier, with its effort"
        );
        assert!(proxy.file_of(session).is_none(), "nothing recorded");

        post(&proxy, &body);
        let after_turn = proxy.file_of(session);
        assert_eq!(jev.posts().len(), 1);

        let next = continuation(&body);
        post(&proxy, &next);
        assert_eq!(jev.posts().len(), 1, "Jev is asked once per turn, not per tool call");
        assert_eq!(
            upstream.request(2).text(),
            rewritten(&next, "haiku", Some(HAIKU)),
            "the continuation stays on Haiku"
        );
        assert_eq!(proxy.file_of(session), after_turn, "a continuation records nothing new");

        let sub = sub_agent_body(session, SUB_TASK);
        post(&proxy, &sub);
        let to = now_ms();
        assert_eq!(jev.posts().len(), 2);
        let sub_request = jev_request(SUB_TASK, SONNET, context_tokens_of(&sub), &menu("static"));
        assert_jev_call(&jev.posts()[1], &sub_request, None);
        assert_eq!(upstream.request(3).text(), rewritten(&sub, "sonnet", Some(SONNET)));

        let (main_key, sub_key) = (key_of(&body), key_of(&sub));
        let (main_label, sub_label) = (label_of(&body), label_of(&sub));
        let main_request = jev_request(&prompt_of(&body), SONNET, context_tokens_of(&body), &menu("static"));
        let first = routed_status(&body, "haiku", HAIKU, Some(&main_answer), "jev", &main_request, Json::Null);
        let second =
            routed_status(&sub, "sonnet", SONNET, Some(&sub_answer), "jev/no-change", &sub_request, "high".into());
        let expected = second.with(vec![
            (
                "agents",
                obj(vec![
                    (main_key.as_str(), agent_entry(&main_label, true, "haiku", HAIKU, 0.92.into(), Json::Null, "jev")),
                    (
                        sub_key.as_str(),
                        agent_entry(&sub_label, false, "sonnet", SONNET, 0.81.into(), "high".into(), "jev/no-change"),
                    ),
                ]),
            ),
            (
                "history",
                Json::Arr(vec![
                    history_item(&first, &main_key, &main_label, true),
                    history_item(&second, &sub_key, &sub_label, false),
                ]),
            ),
        ]);
        assert_status_file(proxy.file_of(session), &expected, from, to);

        let line = run_status_line(&proxy.env, &proxy.cwd, &status_input(&proxy, session, vec![]));
        let want = format!(
            "{} {GREEN}Haiku 5.5{RESET} {DIM}(92%){RESET}{}{} {CYAN}Sonnet 5.5{RESET}{}",
            icon("✧✦"),
            sep(),
            icon("✦"),
            tail_of(&proxy)
        );
        assert_eq!(line, want);
    });
}

#[test]
fn a_model_the_user_picked_passes_through_untouched_and_pauses_routing() {
    run(|ctx| {
        let session = "harness-manual";
        let Setup { jev, upstream, proxy } = setup(ctx, Opts::default());
        let body = fixture_body(session).with(vec![("model", "claude-opus-5-5".into())]);
        let raw = stringify(&body);
        let from = now_ms();

        let res = send(proxy.port, "POST", &path(), CLIENT_HEADERS, Some(raw.as_bytes()));
        let to = now_ms();
        assert_eq!(res.status, 200);
        assert_eq!(jev.posts().len(), 0, "the user's choice is not second-guessed");
        assert_eq!(upstream.request(0).text(), rewritten(&body, "nonsense", None));
        assert_eq!(upstream.request(0).text(), raw, "a compact body comes out byte for byte");
        assert_forwarded_headers(&res.sent, &upstream.request(0), &upstream.host(), raw.as_bytes(), false);

        let (key, label) = (key_of(&body), label_of(&body));
        let agent = obj(vec![
            ("label", label.into()),
            ("main", true.into()),
            ("model", "claude-opus-5-5".into()),
            ("manual", true.into()),
            ("at", 0.0.into()),
        ]);
        let manual =
            obj(vec![("agents", obj(vec![(key.as_str(), agent)])), ("manual", true.into()), ("at", 0.0.into())]);
        assert_status_file(proxy.file_of(session), &manual, from, to);

        // A continuation of that turn, and Claude Code's own Haiku title call, change nothing.
        let before = proxy.file_of(session);
        post(&proxy, &continuation(&body));
        let title = obj(vec![
            ("model", HAIKU.into()),
            ("metadata", body.at("metadata").clone()),
            ("max_tokens", 32.0.into()),
            ("messages", json::parse(r#"[{"role":"user","content":"title this"}]"#).unwrap()),
        ]);
        post(&proxy, &title);
        assert_eq!(upstream.request(2).text(), stringify(&title));
        assert_eq!(proxy.file_of(session), before);
        assert_eq!(jev.posts().len(), 0);

        let input = status_input(&proxy, session, vec![("model", obj(vec![("display_name", "Opus 5.5".into())]))]);
        let line = run_status_line(&proxy.env, &proxy.cwd, &input);
        assert_eq!(line, format!("{DIM}☞ manual{RESET} Opus 5.5{}", tail_of(&proxy)));
    });
}

#[test]
fn a_jev_answer_naming_a_model_that_is_not_on_the_menu_counts_as_no_answer() {
    run(|ctx| {
        let session = "harness-off-menu";
        let answer = jev_answer("claude-opus-4-1", 0.9, [3, 4, 2]);
        let Setup { jev, upstream, proxy } =
            setup(ctx, Opts { answer: Some(always(answer.clone())), ..Opts::default() });
        let body = fixture_body(session);
        let from = now_ms();
        post(&proxy, &body);
        let to = now_ms();

        let request = jev_request(&prompt_of(&body), SONNET, context_tokens_of(&body), &menu("static"));
        assert_jev_call(&jev.posts()[0], &request, None);
        assert_eq!(
            upstream.request(0).text(),
            rewritten(&body, "sonnet", Some(SONNET)),
            "the current tier, never the sentinel"
        );

        let (key, label) = (key_of(&body), label_of(&body));
        let reason = "jev-unavailable/no-change";
        let decision = routed_status(&body, "sonnet", SONNET, Some(&answer), reason, &request, "high".into());
        let expected = decision.with(vec![
            (
                "agents",
                obj(vec![(
                    key.as_str(),
                    agent_entry(&label, true, "sonnet", SONNET, 0.9.into(), "high".into(), reason),
                )]),
            ),
            ("history", Json::Arr(vec![history_item(&decision, &key, &label, true)])),
        ]);
        assert_status_file(proxy.file_of(session), &expected, from, to);
        let line = run_status_line(&proxy.env, &proxy.cwd, &status_input(&proxy, session, vec![]));
        let want = format!(
            "{} {CYAN}Sonnet 5.5{RESET} {DIM}(90%){RESET}{}{} high {DIM}(router offline){RESET}{}",
            icon("✧✦"),
            sep(),
            icon("◔"),
            tail_of(&proxy)
        );
        assert_eq!(line, want);
    });
}

#[test]
fn when_jev_fails_after_its_one_retry_the_turn_runs_on_the_default_tier() {
    run(|ctx| {
        let session = "harness-jev-down";
        let answer: Answer = Arc::new(|_| JevReply::Status(500, r#"{"error":"down"}"#.into()));
        let Setup { jev, upstream, proxy } = setup(ctx, Opts { answer: Some(answer), ..Opts::default() });
        let body = fixture_body(session);
        let from = now_ms();
        let res = post(&proxy, &body);
        let to = now_ms();

        assert_eq!(res.status, 200, "a Jev failure never reaches the user's request");
        let request = jev_request(&prompt_of(&body), SONNET, context_tokens_of(&body), &menu("static"));
        assert_eq!(jev.posts().len(), 2, "one attempt and one retry on a 500");
        assert_jev_call(&jev.posts()[0], &request, None);
        assert_jev_call(&jev.posts()[1], &request, Some(1));
        assert_eq!(upstream.request(0).text(), rewritten(&body, "sonnet", Some(SONNET)));

        let (key, label) = (key_of(&body), label_of(&body));
        let reason = "jev-unavailable/no-change";
        let decision = routed_status(&body, "sonnet", SONNET, None, reason, &request, "high".into());
        let expected = decision.with(vec![
            (
                "agents",
                obj(vec![(
                    key.as_str(),
                    agent_entry(&label, true, "sonnet", SONNET, Json::Null, "high".into(), reason),
                )]),
            ),
            ("history", Json::Arr(vec![history_item(&decision, &key, &label, true)])),
        ]);
        assert_status_file(proxy.file_of(session), &expected, from, to);
        let line = run_status_line(&proxy.env, &proxy.cwd, &status_input(&proxy, session, vec![]));
        let want = format!(
            "{} {CYAN}Sonnet 5.5{RESET}{}{} high {DIM}(router offline){RESET}{}",
            icon("✧✦"),
            sep(),
            icon("◔"),
            tail_of(&proxy)
        );
        assert_eq!(line, want);
    });
}

#[test]
fn a_token_count_request_is_processed_like_a_turn() {
    run(|ctx| {
        let Setup { jev, upstream, proxy } = setup(ctx, Opts::default());
        let body = fixture_body("harness-count-tokens");
        send_json(proxy.port, "/v1/messages/count_tokens?beta=true", CLIENT_HEADERS, &body);
        assert_eq!(jev.posts().len(), 1);
        assert_eq!(upstream.request(0).url, "/v1/messages/count_tokens?beta=true");
        assert_eq!(upstream.request(0).text(), rewritten(&body, "haiku", Some(HAIKU)));
        assert!(proxy.file_of("harness-count-tokens").is_some(), "and recorded like one");
    });
}

#[test]
fn malformed_and_non_object_bodies_are_forwarded_never_the_sentinel() {
    run(|ctx| {
        let Setup { jev, upstream, proxy } = setup(ctx, Opts::default());
        let cases = [
            ("not JSON is forwarded as sent", "/v1/messages", "{not json", "{not json"),
            ("a JSON null is forwarded as sent", "/v1/messages", "null ", "null "),
            ("a number is re-serialised", "/v1/messages", " 42 ", "42"),
            ("an array is re-serialised", "/v1/messages", "[1, 2,\n3]", "[1,2,3]"),
            ("a string is re-serialised", "/v1/messages", " \"x\" ", "\"x\""),
            (
                "other paths are not processed",
                "/v1/other",
                r#"{ "model" : "jev-router" }"#,
                r#"{ "model" : "jev-router" }"#,
            ),
        ];
        for (name, path, sent_body, want) in cases {
            let before = upstream.requests().len();
            let res = send(proxy.port, "POST", path, CLIENT_HEADERS, Some(sent_body.as_bytes()));
            assert_eq!(res.status, 200, "{name}");
            let forwarded = upstream.request(before);
            assert_eq!(forwarded.text(), want, "{name}");
            assert_forwarded_headers(&res.sent, &forwarded, &upstream.host(), want.as_bytes(), false);
        }

        // A routed body whose processing throws (a null content block) still leaves on a real
        // model: the conversation's tier, here the default, with the effort that tier is given.
        let broken = obj(vec![
            ("model", "jev-router".into()),
            ("metadata", fixture_body("harness-broken").at("metadata").clone()),
            ("tools", json::parse(r#"[{"name":"Bash"}]"#).unwrap()),
            ("messages", json::parse(r#"[{"role":"user","content":[null]}]"#).unwrap()),
        ]);
        send_json(proxy.port, "/v1/messages", CLIENT_HEADERS, &broken);
        let last = upstream.requests().last().cloned().expect("a forwarded request");
        assert_eq!(last.text(), rewritten(&broken, "sonnet", Some(SONNET)));
        assert_eq!(jev.posts().len(), 0, "nothing was asked");
        assert!(proxy.file_of("harness-broken").is_none(), "and nothing recorded");
    });
}
