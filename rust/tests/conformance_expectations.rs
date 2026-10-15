//! The harness holds no routing logic: what a request should become is precomputed data in
//! `conformance/fixtures/harness-expectations.json`. These tests do not start a program; they
//! check that data against the frozen golden cases (SPEC 16.2), which are authoritative, so a hand
//! edit to the fixture that disagrees with them fails here and not as a confusing routing failure.
//!
//! Every entry of the file is checked. Most equal a golden case outright. The rest are derived
//! from golden cases by a rule the goldens themselves show, and each derivation says which: the
//! conversation keys under the harness's own session ids (SPEC 7.5's formula, first checked
//! against the `conversation-key` cases), the sub-agent turn's prompt and label (the captured
//! turn's golden values with the task swapped, and the `agent-label` cut), the account menu's
//! Sonnet description, and the forwarded bodies for inputs no golden case holds (a golden rewrite
//! with the input's own messages, or a golden rule applied to the input). The last test fails when
//! the file gains an entry none of these checks reaches.

mod conformance;

use conformance::json::{Json, stringify};
use conformance::*;
use sha1::{Digest, Sha1};
use std::fmt::Write as _;

const SUB_TASK: &str = "Search the codebase for callers of handler 7 and report them";

/// The sessions the routing tests look conversation keys up under, and the capture's own.
const SESSIONS: [&str; 5] =
    ["harness-routed", "harness-continuation", "harness-manual", "harness-off-menu", "harness-jev-down"];

fn same(actual: &Json, expected: &Json, message: &str) {
    assert_eq!(stringify(actual), stringify(expected), "{message}");
}

fn captured() -> Json {
    FIXTURE.at("body").clone()
}

fn captured_session() -> String {
    session_of(&captured())
}

/// The captured request as the routing tests' sub-agent turn sends it.
fn sub_agent() -> Json {
    sub_agent_body(&captured_session(), SUB_TASK)
}

/// The routing tests' user-picked turn.
fn manual() -> Json {
    captured().with(vec![("model", "claude-opus-5-5".into())])
}

/// The routing tests' auxiliary (title) call.
fn auxiliary() -> Json {
    let messages = r#"[{"role":"user","content":"Summarize this conversation in five words"}]"#;
    conformance::json::obj(vec![
        ("model", "jev-router".into()),
        ("metadata", captured().at("metadata").clone()),
        ("max_tokens", 32.0.into()),
        ("messages", conformance::json::parse(messages).unwrap()),
    ])
}

/// The routing tests' body whose processing throws.
fn throwing() -> Json {
    conformance::json::obj(vec![
        ("model", "jev-router".into()),
        ("metadata", captured().at("metadata").clone()),
        ("tools", conformance::json::parse(r#"[{"name":"Bash"}]"#).unwrap()),
        ("messages", conformance::json::parse(r#"[{"role":"user","content":[null]}]"#).unwrap()),
    ])
}

/// Compares a forwarded body with the expected one. The harness writes `metadata.user_id` with
/// `JSON.stringify`, which may space it differently from the capture, so that string is compared
/// parsed.
fn same_forwarded(got: &str, want: &Json, message: &str) {
    let got = conformance::json::parse(got).unwrap();
    let user_id = |body: &Json| conformance::json::parse(body.at("metadata").at("user_id").str()).unwrap();
    same(&user_id(&got), &user_id(want), &format!("{message}: same metadata"));
    let blank = |body: &Json| body.with(vec![("metadata", 0.0.into())]);
    same(&blank(&got), &blank(want), message);
}

/// A golden `apply-tier` result with the captured tools replaced by the golden `sanitize-schema`
/// results for them (the proxy sanitises tool schemas, SPEC 7.3, then rewrites for the tier).
fn golden_rewrite(case: &str) -> Json {
    let tier_case = golden_case("apply-tier", case);
    same(tier_case.at("input").at("body"), &captured(), &format!("apply-tier {case} is the captured request"));
    let mut want = tier_case.at("expected").clone();
    for (i, name) in ["captured tool 0 Bash", "captured tool 1 Edit", "captured tool 2 Read"].iter().enumerate() {
        let sanitize = golden_case("sanitize-schema", name);
        same(
            sanitize.at("input").at("node"),
            captured().at("tools").idx(i).at("input_schema"),
            &format!("sanitize case {i} is the captured tool"),
        );
        want.at_mut("tools").arr_mut()[i].set("input_schema", sanitize.at("expected").clone());
    }
    want
}

/// `golden_rewrite` for a body that differs from the capture only in its messages: `apply-tier`
/// leaves `messages` alone (its golden result for the capture has the capture's messages), so the
/// expected body is the golden one with these messages.
fn golden_rewrite_with_messages(case: &str, body: &Json) -> Json {
    let want = golden_rewrite(case);
    same(want.at("messages"), captured().at("messages"), &format!("apply-tier {case} leaves the messages alone"));
    let mut rest = body.clone();
    rest.set("messages", captured().at("messages").clone());
    same(&rest, &captured(), "the body differs from the capture only in its messages");
    want.with(vec![("messages", body.at("messages").clone())])
}

/// A golden `apply-tier` rule applied to a body it does not hold: each key of the golden result
/// set on the body, in place or appended, as the golden case did to its own input.
fn golden_rule(case: &str, body: &Json) -> Json {
    let rule = golden_case("apply-tier", case);
    let input = rule.at("input").at("body");
    let mut want = body.clone();
    for key in rule.at("expected").keys() {
        want.set(&key, rule.at("expected").at(&key).clone());
    }
    let mut applied = input.clone();
    for key in rule.at("expected").keys() {
        applied.set(&key, rule.at("expected").at(&key).clone());
    }
    same(&applied, rule.at("expected"), &format!("apply-tier {case} only sets keys"));
    want
}

/// SPEC 7.5 `conversationKey`: the first 12 hex digits of SHA-1 over `<session>|<text>`, the text
/// being the first message's string content or its `text` blocks' text with no separator. For the
/// string sessions and text the harness uses; `conversation_key_formula_matches_the_golden_cases`
/// checks it first.
fn conversation_key(session: &str, body: &Json) -> String {
    let first = body.at("messages").idx(0).at("content");
    let text: String = match first {
        Json::Str(s) => s.clone(),
        Json::Arr(blocks) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(Json::as_str) == Some("text"))
            .map(|b| b.at("text").str().to_string())
            .collect(),
        other => panic!("unsupported content {}", stringify(other)),
    };
    let digest = Sha1::digest(format!("{session}|{text}").as_bytes());
    let hex: String = digest.iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    });
    hex[..12].to_string()
}

/// SPEC 7.5 `sessionOf` for the string sessions of the golden cases used here; "" without one.
fn session_or_empty(body: &Json) -> String {
    let user_id = body.get("metadata").and_then(|m| m.get("user_id")).and_then(Json::as_str);
    user_id.map_or_else(String::new, |id| conformance::json::parse(id).unwrap().at("session_id").str().to_string())
}

/// SPEC 7.5 `agentLabel`'s cut for ASCII text: longer than `max` units becomes the first `max - 1`
/// plus `…`. `agent_label_cut_matches_the_golden_cases` checks it first.
fn cut(text: &str, max: usize) -> String {
    if text.len() > max { format!("{}…", &text[..max - 1]) } else { text.to_string() }
}

#[test]
fn the_static_menu_is_what_the_golden_cases_give_for_an_empty_account_catalog() {
    let menu = menu("static");
    same(&menu, golden_case("claude-models", "empty").at("expected"), "claude-models empty");
    same(&menu, golden_case("newest-per-tier", "empty").at("expected"), "newest-per-tier empty");
    assert_eq!(ids(&menu), ["claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5", "claude-fable-5-1"]);
}

#[test]
fn the_jev_request_for_the_static_menu_is_the_golden_request_byte_for_byte() {
    let case = golden_case("jev-request", "captured prompt, static menu");
    let input = case.at("input");
    let request = jev_request(
        input.at("prompt").str(),
        input.at("current").str(),
        input.at("contextTokens").as_f64().unwrap(),
        &menu("static"),
    );
    let body = stringify(&request.with(vec![("model", "jev-latest".into())]));
    assert_eq!(body, case.at("expected").at("body").str());
}

#[test]
fn the_catalog_menu_is_the_golden_account_menu_with_this_catalogs_sonnet_description() {
    // The golden account menu came from a catalog whose Sonnet lists `max_input_tokens`; the
    // harness catalog's Sonnet does not, so its description lacks that clause (SPEC 7.5
    // `claudeModels`: only the truthy items are joined). Everything else is the golden menu.
    let golden_menu = golden_case("jev-request", "captured prompt, account menu").at("input").at("models").clone();
    let menu = menu("catalog");
    assert_eq!(ids(&menu), ids(&golden_menu));
    for (ours, theirs) in menu.arr().iter().zip(golden_menu.arr()) {
        let description = theirs.at("description").str();
        let want = if ours.at("id").str() == SONNET {
            description.replace("; 1000000 input tokens", "")
        } else {
            description.to_string()
        };
        assert_ne!(ours.at("id").str() == SONNET, want == description, "only Sonnet's description differs");
        same(ours, &theirs.with(vec![("description", want.into())]), ours.at("id").str());
    }
}

#[test]
fn the_questions_for_the_account_menu_are_the_golden_ones_with_this_catalogs_descriptions() {
    let case = golden_case("jev-request", "captured prompt, account menu");
    let golden_menu = case.at("input").at("models");
    let golden_body = conformance::json::parse(case.at("expected").at("body").str()).unwrap();
    let mut want = golden_body.at("questions").clone();
    // The golden case shows the rule: each criterion's `model` is that menu item's description.
    let menu = menu("catalog");
    for (theirs, ours) in golden_menu.arr().iter().zip(menu.arr()) {
        let criterion = want.at_mut("model").at_mut("criteria").at_mut(theirs.at("id").str());
        assert_eq!(criterion.at("model"), theirs.at("description"), "the golden criterion names the menu item");
        criterion.set("model", ours.at("description").clone());
    }
    let request = jev_request("x", SONNET, 1.0, &menu);
    same(request.at("questions"), &want, "every question, the account menu's included");
}

#[test]
fn the_prompt_conversation_key_and_label_of_the_captured_request_are_the_golden_ones() {
    let prompt = golden_case("new-turn-prompt", "captured print request");
    let key = golden_case("conversation-key", "captured request");
    let label = golden_case("agent-label", "captured print request");
    for case in [&prompt, &key, &label] {
        same(case.at("input").at("body"), &captured(), "the golden case is the captured request");
    }
    let body = captured();
    assert_eq!(prompt_of(&body), prompt.at("expected").str());
    assert_eq!(key_of(&body), key.at("expected").str());
    assert_eq!(label_of(&body), label.at("expected").str());
}

#[test]
fn conversation_key_formula_matches_the_golden_cases() {
    let names = [
        "captured request",
        "captured request without metadata",
        "main task",
        "sub-agent task",
        "session a",
        "session b",
        "blocks joined with nothing",
        "non-ascii text",
    ];
    for name in names {
        let case = golden_case("conversation-key", name);
        let body = case.at("input").at("body");
        assert_eq!(conversation_key(&session_or_empty(body), body), case.at("expected").str(), "{name}");
    }
}

#[test]
fn every_conversation_key_in_the_file_follows_the_golden_checked_formula() {
    let captured_session = captured_session();
    let mut checked = 0;
    for (body, what) in [(captured(), "captured turn"), (sub_agent(), "sub-agent turn"), (manual(), "user-picked turn")]
    {
        let turn = turn_of(&body);
        let mut sessions: Vec<&str> = vec![&captured_session];
        sessions.extend(SESSIONS);
        assert_eq!(
            turn.at("keys").keys(),
            sessions,
            "{what}: a key for the capture's session and each one the tests use"
        );
        for session in sessions {
            let want = conversation_key(session, &body);
            assert_eq!(turn.at("keys").at(session).str(), want, "{what}, session {session}");
            checked += 1;
        }
    }
    assert_eq!(checked, 18);
}

#[test]
fn agent_label_cut_matches_the_golden_cases() {
    for name in ["80 x", "exactly 48", "exactly 49"] {
        let case = golden_case("agent-label", name);
        let text = case.at("input").at("body").at("messages").idx(0).at("content").str();
        assert_eq!(cut(text, 48), case.at("expected").str(), "{name}");
    }
}

#[test]
fn the_sub_agent_turns_prompt_and_label_are_the_captured_turns_with_the_task_swapped() {
    // The golden prompt and label of the capture are its first message's last text block, so the
    // other blocks (system reminders) contribute nothing; the sub-agent body differs from the
    // capture only in that block.
    let captured = captured();
    let task = captured.at("messages").idx(0).at("content").arr().last().unwrap().at("text").str().to_string();
    assert_eq!(golden_case("new-turn-prompt", "captured print request").at("expected").str(), task);
    assert_eq!(golden_case("agent-label", "captured print request").at("expected").str(), task);
    let turn = turn_of(&sub_agent());
    assert_eq!(turn.at("prompt").str(), SUB_TASK);
    assert_eq!(turn.at("label").str(), cut(SUB_TASK, 48));
}

#[test]
fn the_user_picked_turn_has_the_captured_turns_prompt_keys_and_label() {
    // SPEC 7.5: the prompt, key and label read the messages and the session, never `model`.
    let blank = |turn: Json| turn.with(vec![("name", Json::Null), ("sha256", Json::Null)]);
    same(&blank(turn_of(&manual())), &blank(turn_of(&captured())), "same turn");
}

#[test]
fn the_forwarded_body_of_the_captured_request_is_the_golden_tier_rewrite_with_the_golden_sanitised_tools() {
    for (tier, id) in [("haiku", HAIKU), ("sonnet", SONNET)] {
        let want = golden_rewrite(&format!("captured request to {tier}, no env"));
        same_forwarded(
            &rewritten(&captured(), tier, Some(id)),
            &want,
            &format!("the captured request routed to {tier}"),
        );
    }
}

#[test]
fn the_forwarded_body_for_the_catalogs_opus_is_the_golden_explicit_model_rewrite() {
    let case = golden_case("apply-tier", "explicit model");
    assert_eq!(case.at("input").at("tier").str(), "opus");
    assert_eq!(case.at("input").at("model").str(), "claude-opus-6");
    // `golden_rewrite` checks the case's input is the capture.
    same_forwarded(
        &rewritten(&captured(), "opus", Some("claude-opus-6")),
        &golden_rewrite("explicit model"),
        "routed to claude-opus-6",
    );
}

#[test]
fn the_continuation_and_sub_agent_bodies_are_the_golden_rewrites_with_their_own_messages() {
    let next = continuation(&captured());
    let want = golden_rewrite_with_messages("captured request to haiku, no env", &next);
    same_forwarded(&rewritten(&next, "haiku", Some(HAIKU)), &want, "the continuation on Haiku");
    let sub = sub_agent();
    let want = golden_rewrite_with_messages("captured request to sonnet, no env", &sub);
    same_forwarded(&rewritten(&sub, "sonnet", Some(SONNET)), &want, "the sub-agent turn on Sonnet");
}

#[test]
fn the_auxiliary_and_throwing_bodies_get_the_golden_sonnet_floor() {
    // Neither has a tool schema to sanitise (the throwing body's one tool has none) nor thinking
    // to strip, so the default tier's rewrite is the golden `sonnet floor named` rule alone.
    for (body, what) in [(auxiliary(), "the auxiliary call"), (throwing(), "the throwing body")] {
        let want = golden_rule("sonnet floor named", &body);
        assert_eq!(rewritten(&body, "sonnet", Some(SONNET)), stringify(&want), "{what}");
    }
}

#[test]
fn the_user_picked_body_is_forwarded_as_sent() {
    let case = golden_case("apply-tier", "unknown tier untouched");
    same(case.at("expected"), case.at("input").at("body"), "an unknown tier leaves the body alone");
    let body = manual();
    assert_eq!(rewritten(&body, "nonsense", None), stringify(&body));
}

#[test]
fn context_tokens_and_metrics_are_the_golden_ones() {
    // The two formulas the harness computes itself (SPEC 6.1, 7.3), anchored to a golden case.
    let case = golden_case("jev-request", "captured prompt, static menu");
    same(&context_tokens_of(&captured()).into(), case.at("input").at("contextTokens"), "context tokens");
    let response = conformance::json::parse(case.at("input").at("response").str()).unwrap();
    let tokens = case.at("input").at("contextTokens").as_f64().unwrap();
    same(&metrics_of(response.at("answers"), tokens), case.at("expected").at("result").at("metrics"), "metrics");
}

#[test]
fn every_entry_in_the_file_has_a_cross_check() {
    // The inputs the tests above check. An entry the file gains beyond these has no cross-check
    // yet: add one here before relying on it.
    let menus = EXPECTED.at("menus").keys();
    assert_eq!(menus, ["static", "catalog"]);
    let questions: Vec<String> = EXPECTED.at("questions").arr().iter().map(|q| stringify(q.at("models"))).collect();
    assert_eq!(questions, [stringify(&menu("static")), stringify(&menu("catalog"))]);

    let mut turns: Vec<String> =
        [captured(), sub_agent(), manual()].iter().map(|b| sha256::sha256_hex(sessionless(b).as_bytes())).collect();
    let mut listed: Vec<String> = EXPECTED.at("turns").arr().iter().map(|t| t.at("sha256").str().to_string()).collect();
    turns.sort();
    listed.sort();
    assert_eq!(listed, turns, "turns");

    let next = continuation(&captured());
    let checked = [
        forwarded_sha(&captured(), "haiku", Some(HAIKU)),
        forwarded_sha(&captured(), "sonnet", Some(SONNET)),
        forwarded_sha(&captured(), "opus", Some("claude-opus-6")),
        forwarded_sha(&next, "haiku", Some(HAIKU)),
        forwarded_sha(&sub_agent(), "sonnet", Some(SONNET)),
        forwarded_sha(&auxiliary(), "sonnet", Some(SONNET)),
        forwarded_sha(&throwing(), "sonnet", Some(SONNET)),
        forwarded_sha(&manual(), "nonsense", None),
    ];
    let mut checked: Vec<String> = checked.into_iter().collect();
    let mut listed: Vec<String> =
        EXPECTED.at("forwarded").arr().iter().map(|f| f.at("sha256").str().to_string()).collect();
    checked.sort();
    listed.sort();
    assert_eq!(listed, checked, "forwarded");
}

/// Why a frozen case no longer matches the file it was copied from.
fn drifted(file: &str, case: &str, source: &str) -> String {
    format!(
        "conformance/cases/{file}.json case '{case}' no longer matches {source}; update the case by hand (conformance/README.md)"
    )
}

fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn read_source(path: &str) -> Vec<u8> {
    let path = root().join(path);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// The golden cases copy their inputs from files people edit (`.env.example`, the calibration
/// prompts, the captured fixtures). Nothing regenerates them, so this is the guard that the copy
/// and the source still agree; a failure means editing the case by hand.
#[test]
fn golden_inputs_still_match_their_sources() {
    let env_source = ".env.example";
    let env = String::from_utf8(read_source(env_source)).unwrap();
    assert_eq!(
        golden_case("parse-env", ".env.example").at("input").at("text").str(),
        lf(&env),
        "{}",
        drifted("parse-env", ".env.example", env_source)
    );

    let handback_source = "conformance/fixtures/subagent-handback-prompt.txt";
    let handback = lf(&String::from_utf8(read_source(handback_source)).unwrap());
    for (file, name) in [("detect-override", "handback fixture"), ("decide", "handback quote is not an override")] {
        assert_eq!(
            golden_case(file, name).at("input").at("prompt").str(),
            handback,
            "{}",
            drifted(file, name, handback_source)
        );
    }

    let calibration_source = "rust/examples/calibration-cases.json";
    let calibration = conformance::json::parse_bytes(&read_source(calibration_source)).unwrap();
    let mut pairs: Vec<(String, String)> = Vec::new();
    for group in calibration.at("groups").arr() {
        for prompt in group.at("prompts").arr() {
            pairs.push((group.at("want").str().to_string(), prompt.str().to_string()));
        }
    }
    assert!(!pairs.is_empty(), "{calibration_source} lists no prompts");
    for (i, (want, prompt)) in pairs.iter().enumerate() {
        for (file, name) in
            [("detect-override", format!("calibration {i} ({want})")), ("jev-request", format!("calibration {i}"))]
        {
            let cases = golden(file);
            let case = cases.arr().iter().find(|c| c.at("name").as_str() == Some(name.as_str())).unwrap_or_else(|| {
                panic!("{}", drifted(file, &name, &format!("{calibration_source} (the case is missing)")))
            });
            assert_eq!(case.at("input").at("prompt").str(), prompt, "{}", drifted(file, &name, calibration_source));
        }
    }
    for file in ["detect-override", "jev-request"] {
        let held = golden(file).arr().iter().filter(|c| c.at("name").str().starts_with("calibration ")).count();
        assert_eq!(
            held,
            pairs.len(),
            "{}",
            drifted(file, "calibration *", &format!("{calibration_source} (a case outlives its prompt)"))
        );
    }

    let print_source = "conformance/fixtures/claude-code-print-request.json";
    assert_eq!(
        golden_case("parse", "captured request file").at("input").at("bytes").at("$hex").str(),
        hex(&read_source(print_source)),
        "{}",
        drifted("parse", "captured request file", print_source)
    );
    same(
        golden_case("stringify", "captured body").at("input").at("value"),
        &captured(),
        &drifted("stringify", "captured body", print_source),
    );
    same(
        golden_case("stringify", "captured messages").at("input").at("value"),
        captured().at("messages"),
        &drifted("stringify", "captured messages", print_source),
    );
    same(
        golden_case("session-of", "captured request").at("input").at("body"),
        &captured(),
        &drifted("session-of", "captured request", print_source),
    );
}
