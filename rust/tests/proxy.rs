//! Port of node/test/proxy.test.mjs.

// Each test's doc comment is the Node test title, quoted verbatim so the two suites can be
// compared line by line; Markdown backticks would change the titles.
#![allow(clippy::doc_markdown)]

mod common;

use common::*;
use jev_router::config::{effort_floor, forced_effort, is_auto, tier_of};
use jev_router::envx::{EnvMap, map};
use jev_router::jsjson::{self, Value};
use jev_router::jsstr::JsStr;
use jev_router::proxy::{
    Mains, ProxyOptions, RouteFn, agent_label, agent_of, apply_tier, claude_models, conversation_key, new_turn_prompt,
    newer_than_calibrated, newest_per_tier, sanitize_schema, session_of, start_proxy,
};
use jev_router::status::{
    Agent, STALE_AFTER_MS, agent_view, agent_view_now, mark_manual, prune_stale, read_calibration, read_status,
    write_calibration, write_decision, write_status,
};
use jev_router::timefmt::now_ms;
use std::sync::{Arc, Mutex};

fn pid() -> u32 {
    std::process::id()
}

fn ids(models: &[jev_router::config::Model]) -> Vec<String> {
    models.iter().map(|m| m.id.to_string_lossy()).collect()
}

fn catalog(text: &str) -> Vec<Value> {
    json(text).as_array().unwrap().clone()
}

fn prompt(body: &str) -> Option<String> {
    new_turn_prompt(&json(body)).unwrap().map(|p| p.to_string_lossy())
}

fn with_tools(messages: &str) -> String {
    format!(r#"{{"tools":[{{"name":"Bash"}}],"messages":{messages}}}"#)
}

fn apply(body: &str, tier: &str, env: &EnvMap) -> Value {
    let mut b = json(body);
    apply_tier(&mut b, tier, None, env);
    b
}

/// "only the sentinel model is routed"
#[test]
fn only_the_sentinel_model_is_routed() {
    assert!(is_auto(&s("jev-router")));
    assert!(!is_auto(&s("claude-opus-4-6")), "a model the user picked is theirs");
    assert!(!is_auto(&s("claude-haiku-4-5-20251001")), "internal Haiku calls pass through");
    assert!(!is_auto(&Value::Undefined));
}

/// "the sentinel is not mistaken for a real tier"
#[test]
fn the_sentinel_is_not_mistaken_for_a_real_tier() {
    assert_eq!(tier_of(&s("jev-router")), None);
}

/// "reads the session id out of Claude Code's metadata"
#[test]
fn reads_the_session_id_out_of_claude_codes_metadata() {
    let sid = "11111111-2222-4333-8444-555555555555";
    let body = json(&format!(r#"{{"metadata":{{"user_id":"{{\"session_id\":\"{sid}\"}}"}}}}"#));
    assert_eq!(session_of(&body), s(sid));
    assert_eq!(session_of(&json(r#"{"metadata":{"user_id":"not-json"}}"#)), s(""));
    assert_eq!(session_of(&json("{}")), s(""));
}

/// "status round-trips per session and misses cleanly"
#[test]
fn status_round_trips_per_session_and_misses_cleanly() {
    isolate_status();
    let sid = s(&format!("test-{}", pid()));
    let status = json(r#"{"tier":"opus","confidence":0.87,"reason":"jev"}"#);
    write_status(&sid, &status);
    assert_eq!(read_status(&sid), status);
    assert_eq!(read_status(&s("no-such-session")), Value::Null);
    write_status(&s(""), &json(r#"{"tier":"opus"}"#));
}

/// "status files are private to their owner" (skipped on Windows, as in Node)
#[cfg(unix)]
#[test]
fn status_files_are_private_to_their_owner() {
    use std::os::unix::fs::PermissionsExt;
    let dir = isolate_status();
    let sid = format!("perm-{}", pid());
    write_status(&s(&sid), &json(r#"{"tier":"opus"}"#));
    assert_eq!(std::fs::metadata(dir).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(std::fs::metadata(dir.join(format!("{sid}.json"))).unwrap().permissions().mode() & 0o777, 0o600);
}

/// "stale status files are pruned and fresh ones kept"
#[test]
fn stale_status_files_are_pruned_and_fresh_ones_kept() {
    let dir = isolate_status();
    std::fs::create_dir_all(dir).unwrap();
    let stale = dir.join(format!("stale-{}.json", pid()));
    let fresh = dir.join(format!("fresh-{}.json", pid()));
    std::fs::write(&stale, "{}").unwrap();
    std::fs::write(&fresh, "{}").unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(8 * 24 * 60 * 60);
    std::fs::File::options().write(true).open(&stale).unwrap().set_modified(old).unwrap();
    // The count is not asserted: the first `write_status` in this process also prunes this shared
    // directory (once, from whichever test writes first), and when that runs before this call it
    // has already removed the stale file, so this call may legitimately remove none.
    prune_stale(STALE_AFTER_MS, now_ms());
    assert!(!stale.exists());
    assert!(fresh.exists());
}

/// "routing status retains the exact recent Jev exchanges"
#[test]
fn routing_status_retains_the_exact_recent_jev_exchanges() {
    isolate_status();
    let sid = s(&format!("history-{}", pid()));
    write_decision(
        &sid,
        &obj(&json(r#"{"prompt":"first","jev":{"request":{"id":1},"response":{"confidence":0.6}}}"#)),
        None,
    )
    .unwrap();
    write_decision(
        &sid,
        &obj(&json(r#"{"prompt":"second","jev":{"request":{"id":2},"response":{"confidence":0.8}}}"#)),
        None,
    )
    .unwrap();
    let status = read_status(&sid);
    assert_eq!(status.get("prompt"), &s("second"));
    let prompts: Vec<&Value> = status.get("history").as_array().unwrap().iter().map(|h| h.get("prompt")).collect();
    assert_eq!(prompts, vec![&s("first"), &s("second")]);
    assert_eq!(status.get("history").idx(0).get("jev").get("response").get("confidence"), &Value::Number(0.6));
}

/// "recognises older model versions within a tier"
#[test]
fn recognises_older_model_versions_within_a_tier() {
    assert_eq!(tier_of(&s("claude-sonnet-4-6")), Some("sonnet"));
    assert_eq!(tier_of(&s("claude-sonnet-5")), Some("sonnet"));
    assert_eq!(tier_of(&s("claude-haiku-4-5-20251001")), Some("haiku"));
    assert_eq!(tier_of(&s("claude-opus-4-1")), Some("opus"));
    assert_eq!(tier_of(&s("claude-fable-5-1[1m]")), Some("fable"));
    assert_eq!(tier_of(&s("mystery-9")), None);
    assert_eq!(tier_of(&Value::Undefined), None);
}

/// "keeps available Claude model versions as separate Jev choices"
#[test]
fn keeps_available_claude_model_versions_as_separate_jev_choices() {
    let models = claude_models(&catalog(
        r#"[{"id":"claude-opus-5-5","display_name":"Claude Opus 5"},{"id":"claude-opus-4-8","display_name":"Claude Opus 4.8"}]"#,
    ))
    .unwrap();
    let pairs: Vec<(String, String)> = models.iter().map(|m| (m.id.to_string_lossy(), m.tier.clone())).collect();
    assert_eq!(pairs, vec![("claude-opus-5-5".into(), "opus".into()), ("claude-opus-4-8".into(), "opus".into())]);
}

fn calibration_file(name: &str) -> std::path::PathBuf {
    isolate_status().join(format!("{name}-{}.json", pid()))
}

/// "a Claude API key reaches Anthropic untouched, on routed and manual requests alike"
#[tokio::test]
async fn a_claude_api_key_reaches_anthropic_untouched() {
    let (url, seen) = serve(Arc::new(|r: &Seen| {
        Reply::json(
            200,
            if r.url.starts_with("/v1/models") { r#"{"data":[]}"# } else { r#"{"id":"msg_1","type":"message"}"# },
        )
    }))
    .await;
    let calibration = calibration_file("calibration-key-test");
    let proxy = start_proxy(ProxyOptions {
        upstream_url: Some(url),
        route: Some(answer("claude-sonnet-5-5", 0.9)),
        calibration_file: Some(calibration.clone()),
    })
    .await
    .unwrap();

    let headers = [("content-type", "application/json"), ("x-api-key", "sk-ant-api03-test")];
    request(proxy.port, "GET", "/v1/models", &headers, b"").await;
    for model in ["jev-router", "claude-opus-5-5"] {
        let body = format!(
            r#"{{"model":"{model}","tools":[{{"name":"Bash"}}],"messages":[{{"role":"user","content":"hi"}}]}}"#
        );
        request(proxy.port, "POST", "/v1/messages", &headers, body.as_bytes()).await;
    }
    // An auth token, as ANTHROPIC_AUTH_TOKEN or a gateway sends it, passes through the same way.
    request(
        proxy.port,
        "POST",
        "/v1/messages",
        &[("content-type", "application/json"), ("authorization", "Bearer gateway-token")],
        br#"{"model":"claude-opus-5-5","messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;

    let seen = seen.lock().unwrap();
    let keys: Vec<Option<String>> = seen.iter().map(|s| s.header("x-api-key")).collect();
    let key = Some("sk-ant-api03-test".to_string());
    assert_eq!(keys, vec![key.clone(), key.clone(), key, None]);
    assert_eq!(seen[3].header("authorization").as_deref(), Some("Bearer gateway-token"));
    let _ = std::fs::remove_file(calibration);
}

/// "Claude proxy sends exact account models to Jev and routes the chosen version"
#[tokio::test]
async fn sends_exact_account_models_to_jev_and_routes_the_chosen_version() {
    let (url, seen) = serve(Arc::new(|r: &Seen| {
        if r.url.starts_with("/v1/models") {
            return Reply::json(
                200,
                r#"{"data":[{"id":"claude-opus-4-8","display_name":"Claude Opus 4.8","created_at":"2026-01-05"},{"id":"claude-opus-5-5","display_name":"Claude Opus 5.5","created_at":"2026-09-02"},{"id":"claude-sonnet-5-5","display_name":"Claude Sonnet 5.5","created_at":"2026-08-11"}]}"#,
            );
        }
        Reply::json(200, r#"{"id":"msg_1","type":"message","model":"claude-opus-5-5"}"#)
    }))
    .await;
    let calibration = calibration_file("calibration-proxy-test");
    let offered: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let log = offered.clone();
    let route: RouteFn = Arc::new(move |args| {
        log.lock().unwrap().push(ids(&args.models));
        Box::pin(async { Ok(Some(json(r#"{"choice":"claude-opus-5-5","confidence":0.91,"ms":1}"#))) })
    });
    let proxy = start_proxy(ProxyOptions {
        upstream_url: Some(url),
        route: Some(route),
        calibration_file: Some(calibration.clone()),
    })
    .await
    .unwrap();

    let models = request(proxy.port, "GET", "/v1/models", &[], b"").await;
    assert!(models.json().get("data").as_array().is_some());
    let recorded = read_calibration(&calibration);
    assert_eq!(recorded.models, vec![s("claude-opus-5-5"), s("claude-sonnet-5-5")], "the account's newest per tier");
    assert!(recorded.newer.is_empty(), "nothing newer than the router was tuned for");
    post_json(
        proxy.port,
        "/v1/messages",
        &json(r#"{"model":"jev-router","tools":[{"name":"Bash"}],"messages":[{"role":"user","content":"debug this race"}]}"#),
    )
    .await;

    // Only the newest of each tier is on the menu, newest first.
    assert_eq!(*offered.lock().unwrap(), vec![vec!["claude-opus-5-5".to_string(), "claude-sonnet-5-5".to_string()]]);
    let seen = seen.lock().unwrap();
    let message = seen.iter().find(|s| s.url.starts_with("/v1/messages")).unwrap();
    assert_eq!(message.json().get("model"), &s("claude-opus-5-5"));
    let _ = std::fs::remove_file(calibration);
}

/// "a routed request without metadata is recorded under the conversation key"
#[tokio::test]
async fn a_routed_request_without_metadata_is_recorded_under_the_conversation_key() {
    isolate_status();
    let (url, _) =
        serve(Arc::new(|_: &Seen| Reply::json(200, r#"{"id":"msg_1","type":"message","model":"claude-sonnet-5-5"}"#)))
            .await;
    let proxy = start_proxy(ProxyOptions {
        upstream_url: Some(url),
        route: Some(answer("claude-sonnet-5-5", 0.77)),
        ..Default::default()
    })
    .await
    .unwrap();
    // Exactly what `claude -p` sends first: no metadata, so no session id.
    let body = json(&format!(
        r#"{{"model":"jev-router","tools":[{{"name":"Bash"}}],"messages":[{{"role":"user","content":"rename this variable {}"}}]}}"#,
        pid()
    ));
    post_json(proxy.port, "/v1/messages", &body).await;

    assert_eq!(session_of(&body), s(""), "the request carries no session id");
    let status = read_status(&Value::String(conversation_key(&body).unwrap()));
    assert!(status.truthy(), "the decision is filed under the conversation key instead of being dropped");
    assert_eq!(status.get("tier"), &s("sonnet"));
    assert_eq!(status.get("confidence"), &Value::Number(0.77));
    assert_eq!(status.get("effort"), &s("high"), "records the effort that went out");
    assert_eq!(agent_view_now(&status).main.get("effort"), &s("high"), "and carries it to the per-agent entry");
}

/// "converts a draft-04 boolean exclusiveMinimum into a draft 2020-12 number"
#[test]
fn converts_a_draft_04_boolean_exclusive_minimum() {
    let mut schema = json(r#"{"type":"object","properties":{"topN":{"minimum":0,"exclusiveMinimum":true}}}"#);
    sanitize_schema(&mut schema);
    assert_eq!(schema.get("properties").get("topN"), &json(r#"{"exclusiveMinimum":0}"#));
}

/// "drops a false exclusiveMaximum and keeps the bound"
#[test]
fn drops_a_false_exclusive_maximum_and_keeps_the_bound() {
    let mut schema = json(r#"{"properties":{"n":{"maximum":10,"exclusiveMaximum":false}}}"#);
    sanitize_schema(&mut schema);
    assert_eq!(schema.get("properties").get("n"), &json(r#"{"maximum":10}"#));
}

/// "leaves an already-valid numeric bound alone"
#[test]
fn leaves_an_already_valid_numeric_bound_alone() {
    let mut schema = json(r#"{"properties":{"n":{"exclusiveMinimum":5}}}"#);
    sanitize_schema(&mut schema);
    assert_eq!(schema.get("properties").get("n").get("exclusiveMinimum"), &Value::Number(5.0));
}

/// "reaches schemas nested in arrays and sub-objects"
#[test]
fn reaches_schemas_nested_in_arrays_and_sub_objects() {
    let mut schema = json(r#"{"anyOf":[{"items":{"minimum":1,"exclusiveMinimum":true}}]}"#);
    sanitize_schema(&mut schema);
    assert_eq!(schema.get("anyOf").idx(0).get("items"), &json(r#"{"exclusiveMinimum":1}"#));
}

/// "survives null and primitive nodes"
#[test]
fn survives_null_and_primitive_nodes() {
    let mut null = Value::Null;
    sanitize_schema(&mut null);
    let mut mixed = json(r#"{"a":null,"b":3,"c":"x"}"#);
    sanitize_schema(&mut mixed);
    assert_eq!(mixed, json(r#"{"a":null,"b":3,"c":"x"}"#));
}

/// "reads a plain string prompt as a new turn"
#[test]
fn reads_a_plain_string_prompt_as_a_new_turn() {
    assert_eq!(prompt(&with_tools(r#"[{"role":"user","content":"fix the bug"}]"#)).as_deref(), Some("fix the bug"));
}

/// "reads a text block prompt as a new turn"
#[test]
fn reads_a_text_block_prompt_as_a_new_turn() {
    let body = with_tools(r#"[{"role":"user","content":[{"type":"text","text":"fix the bug"}]}]"#);
    assert_eq!(prompt(&body).as_deref(), Some("fix the bug"));
}

/// "hook context after the prompt does not hide the turn"
#[test]
fn hook_context_after_the_prompt_does_not_hide_the_turn() {
    let body = with_tools(
        r#"[{"role":"user","content":"refactor the parser"},{"role":"system","content":[{"type":"text","text":"SessionStart hook additional context: ..."}]}]"#,
    );
    assert_eq!(prompt(&body).as_deref(), Some("refactor the parser"));
}

/// "ignores a tool_result continuation mid-turn"
#[test]
fn ignores_a_tool_result_continuation_mid_turn() {
    let body = with_tools(
        r#"[{"role":"user","content":"fix the bug"},{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"done"}]}]"#,
    );
    assert_eq!(prompt(&body), None);
}

/// "ignores auxiliary calls that carry no tools"
#[test]
fn ignores_auxiliary_calls_that_carry_no_tools() {
    assert_eq!(prompt(r#"{"messages":[{"role":"user","content":"summarise this"}]}"#), None);
}

/// "ignores a request whose last message is from the assistant"
#[test]
fn ignores_a_request_whose_last_message_is_from_the_assistant() {
    assert_eq!(prompt(&with_tools(r#"[{"role":"assistant","content":"thinking"}]"#)), None);
}

/// "ignores an empty prompt"
#[test]
fn ignores_an_empty_prompt() {
    assert_eq!(prompt(&with_tools(r#"[{"role":"user","content":"   "}]"#)), None);
}

/// "survives a malformed body"
#[test]
fn survives_a_malformed_body() {
    assert_eq!(new_turn_prompt(&Value::Undefined).unwrap(), None);
    assert_eq!(prompt("{}"), None);
    assert_eq!(prompt(r#"{"tools":[],"messages":[]}"#), None);
}

/// "strips system reminders Claude Code injects into the prompt"
#[test]
fn strips_system_reminders_claude_code_injects() {
    let body = with_tools(
        r#"[{"role":"user","content":"fix the bug\n<system-reminder>be careful\nabout things</system-reminder>"}]"#,
    );
    assert_eq!(prompt(&body).as_deref(), Some("fix the bug"));
}

/// "a prompt that is only a system reminder is not a turn"
#[test]
fn a_prompt_that_is_only_a_system_reminder_is_not_a_turn() {
    assert_eq!(prompt(&with_tools(r#"[{"role":"user","content":"<system-reminder>noise</system-reminder>"}]"#)), None);
}

/// "routing to haiku strips fields haiku cannot accept"
#[test]
fn routing_to_haiku_strips_fields_haiku_cannot_accept() {
    let body = apply(
        r#"{"model":"claude-sonnet-4-6","thinking":{"type":"adaptive"},"output_config":{"effort":"medium"},"context_management":{"edits":[{"type":"clear_thinking_20251015","keep":"all"}]}}"#,
        "haiku",
        &EnvMap::new(),
    );
    assert_eq!(body, json(r#"{"model":"claude-haiku-4-5-20251001"}"#));
}

/// "routing to haiku keeps context-management strategies unrelated to thinking"
#[test]
fn routing_to_haiku_keeps_unrelated_context_management() {
    let body = apply(
        r#"{"model":"claude-sonnet-4-6","context_management":{"edits":[{"type":"clear_tool_uses_20250919"},{"type":"clear_thinking_20251015"}]}}"#,
        "haiku",
        &EnvMap::new(),
    );
    assert_eq!(body.get("context_management"), &json(r#"{"edits":[{"type":"clear_tool_uses_20250919"}]}"#));
}

/// "routing to opus leaves thinking and effort intact"
#[test]
fn routing_to_opus_leaves_thinking_and_effort_intact() {
    let body = apply(
        r#"{"model":"claude-sonnet-4-6","thinking":{"type":"adaptive"},"output_config":{"effort":"medium"}}"#,
        "opus",
        &EnvMap::new(),
    );
    assert_eq!(body.get("model"), &s("claude-opus-5-5"));
    assert_eq!(body.get("thinking"), &json(r#"{"type":"adaptive"}"#));
    assert_eq!(body.get("output_config"), &json(r#"{"effort":"medium"}"#));
}

/// "an unknown tier leaves the request untouched"
#[test]
fn an_unknown_tier_leaves_the_request_untouched() {
    let body = apply(r#"{"model":"claude-sonnet-4-6","thinking":{"type":"adaptive"}}"#, "nonsense", &EnvMap::new());
    assert_eq!(body.get("model"), &s("claude-sonnet-4-6"));
}

/// "names each tier's own effort when the request does not"
#[test]
fn names_each_tiers_own_effort_when_the_request_does_not() {
    let opus = apply(r#"{"model":"jev-router","thinking":{"type":"adaptive"}}"#, "opus", &EnvMap::new());
    assert_eq!(opus.get("output_config"), &json(r#"{"effort":"medium"}"#));
    let sonnet = apply(r#"{"model":"jev-router","thinking":{"type":"adaptive"}}"#, "sonnet", &EnvMap::new());
    assert_eq!(sonnet.get("output_config"), &json(r#"{"effort":"high"}"#));
}

/// "JEV_<TIER>_EFFORT overrides a tier's effort, and a bad value is ignored"
#[test]
fn jev_tier_effort_overrides_a_tiers_effort() {
    assert_eq!(effort_floor("opus", &map(&[("JEV_OPUS_EFFORT", "High")])), Some("high"));
    assert_eq!(effort_floor("sonnet", &map(&[("JEV_SONNET_EFFORT", "low")])), Some("low"));
    assert_eq!(effort_floor("opus", &map(&[("JEV_OPUS_EFFORT", "turbo")])), Some("medium"), "unknown value falls back");
    assert_eq!(effort_floor("opus", &EnvMap::new()), Some("medium"));
    assert_eq!(effort_floor("haiku", &map(&[("JEV_HAIKU_EFFORT", "high")])), None, "haiku takes no effort");
}

/// "a new major version is picked as the newest of its tier, dated or not"
#[test]
fn a_new_major_version_is_picked_as_the_newest_of_its_tier() {
    let newest = |text: &str| ids(&newest_per_tier(&claude_models(&catalog(text)).unwrap()));
    assert_eq!(
        newest(r#"[{"id":"claude-opus-5-5"},{"id":"claude-opus-6"},{"id":"claude-opus-4-8"}]"#),
        ["claude-opus-6"]
    );
    assert_eq!(newest(r#"[{"id":"claude-sonnet-5-5"},{"id":"claude-sonnet-5-10"}]"#), ["claude-sonnet-5-10"]);
    assert_eq!(
        newest(r#"[{"id":"claude-haiku-4-5-20251001"},{"id":"claude-haiku-4-6"}]"#),
        ["claude-haiku-4-6"],
        "a date suffix is not a minor version"
    );
}

/// "keeps an effort the request already carries"
#[test]
fn keeps_an_effort_the_request_already_carries() {
    let body = apply(
        r#"{"model":"jev-router","thinking":{"type":"adaptive"},"output_config":{"effort":"low"}}"#,
        "opus",
        &EnvMap::new(),
    );
    assert_eq!(body.get("output_config"), &json(r#"{"effort":"low"}"#), "the user's own choice outranks the floor");
}

/// "never names an effort for a tier that cannot take one"
#[test]
fn never_names_an_effort_for_a_tier_that_cannot_take_one() {
    let body = apply(r#"{"model":"jev-router","output_config":{"effort":"high"}}"#, "haiku", &EnvMap::new());
    assert_eq!(body.get("output_config"), &Value::Undefined);
}

fn captured_body() -> Value {
    let text = std::fs::read(repo_root().join("conformance/fixtures/claude-code-print-request.json")).unwrap();
    jsjson::parse_bytes(&text).unwrap().get("body").clone()
}

fn apply_captured(tier: &str, env: &EnvMap) -> Value {
    let mut body = captured_body();
    apply_tier(&mut body, tier, None, env);
    body
}

/// "JEV_FORCE_EFFORT replaces the effort Claude Code sent, and a per-tier one wins over it"
#[test]
fn jev_force_effort_replaces_the_effort_claude_code_sent() {
    assert_eq!(captured_body().get("output_config").get("effort"), &s("high"), "the capture really carries an effort");
    let forced = apply_captured("opus", &map(&[("JEV_FORCE_EFFORT", "low")]));
    assert_eq!(forced.get("output_config").get("effort"), &s("low"), "outranks the effort Claude Code sent");
    let per_tier = apply_captured("opus", &map(&[("JEV_FORCE_EFFORT", "low"), ("JEV_OPUS_FORCE_EFFORT", "xhigh")]));
    assert_eq!(per_tier.get("output_config").get("effort"), &s("xhigh"), "the tier setting beats the global one");
    let other = apply_captured("sonnet", &map(&[("JEV_OPUS_FORCE_EFFORT", "xhigh")]));
    assert_eq!(other.get("output_config").get("effort"), &s("high"), "another tier keeps the effort Claude Code sent");
}

/// "a forced effort is ignored when unrecognised and never reaches Haiku"
#[test]
fn a_forced_effort_is_ignored_when_unrecognised_and_never_reaches_haiku() {
    let bad = apply_captured("opus", &map(&[("JEV_FORCE_EFFORT", "turbo")]));
    assert_eq!(bad.get("output_config").get("effort"), &s("high"), "a bad value leaves the request alone");
    let haiku = apply_captured("haiku", &map(&[("JEV_FORCE_EFFORT", "max")]));
    assert_eq!(haiku.get("output_config"), &Value::Undefined, "Haiku takes no effort, forced or not");
    assert_eq!(forced_effort("haiku", &map(&[("JEV_HAIKU_FORCE_EFFORT", "max")])), None);
    assert_eq!(
        forced_effort("fable", &map(&[("JEV_FORCE_EFFORT", " Max ")])),
        Some("max"),
        "case and spaces are forgiven"
    );
}

fn key(body: &str) -> JsStr {
    conversation_key(&json(body)).unwrap()
}

/// "a conversation keeps one key as it grows, and differs from a sub-agent"
#[test]
fn a_conversation_keeps_one_key_as_it_grows() {
    let main = r#"{"messages":[{"role":"user","content":"main task"}]}"#;
    let grown = r#"{"messages":[{"role":"user","content":"main task"},{"role":"assistant","content":"ok"}]}"#;
    let sub = r#"{"messages":[{"role":"user","content":"sub-agent task"}]}"#;
    assert_eq!(key(main), key(grown));
    assert_ne!(key(main), key(sub));
}

/// "the key ignores the cache_control breakpoint Claude Code moves between requests"
#[test]
fn the_key_ignores_the_cache_control_breakpoint() {
    let first = r#"{"messages":[{"role":"user","content":[{"type":"text","text":"<system-reminder>x</system-reminder>"},{"type":"text","text":"do the thing","cache_control":{"type":"ephemeral","ttl":"1h"}}]}]}"#;
    let later = r#"{"messages":[{"role":"user","content":[{"type":"text","text":"<system-reminder>x</system-reminder>"},{"type":"text","text":"do the thing"}]},{"role":"assistant","content":"working"}]}"#;
    assert_eq!(key(first), key(later));
}

/// "the same opening text in two sessions gets two keys"
#[test]
fn the_same_opening_text_in_two_sessions_gets_two_keys() {
    let mk = |id: &str| {
        format!(
            r#"{{"metadata":{{"user_id":"{{\"session_id\":\"{id}\"}}"}},"messages":[{{"role":"user","content":"same opening"}}]}}"#
        )
    };
    assert_ne!(key(&mk("a")), key(&mk("b")));
}

/// "the key survives metadata that is not JSON"
#[test]
fn the_key_survives_metadata_that_is_not_json() {
    assert!(
        conversation_key(&json(r#"{"metadata":{"user_id":"not-json"},"messages":[{"role":"user","content":"hi"}]}"#))
            .is_ok()
    );
}

fn session_body(session: &str, tools: bool, text: &str) -> Value {
    let tools = if tools { r#""tools":[{"name":"Read"}],"# } else { "" };
    json(&format!(
        r#"{{"metadata":{{"user_id":"{{\"session_id\":\"{session}\"}}"}},{tools}"messages":[{{"role":"user","content":"{text}"}}]}}"#
    ))
}

/// "the first tool-bearing conversation in a session is the main thread"
#[test]
fn the_first_tool_bearing_conversation_in_a_session_is_the_main_thread() {
    let mut mains = Mains::new();
    let main = agent_of(&session_body("s-main", true, "the user's opening prompt"), &mut mains).unwrap();
    let sub = agent_of(&session_body("s-main", true, "search the repo for conversationKey"), &mut mains).unwrap();
    assert!(main.main);
    assert!(!sub.main, "a later conversation in the same session is a sub-agent");
    assert_ne!(main.key, sub.key);
    assert_eq!(sub.label, "search the repo for conversationKey");
    assert!(
        agent_of(&session_body("s-main", true, "the user's opening prompt"), &mut mains).unwrap().main,
        "the main key is stable"
    );
}

/// "auxiliary calls without tools never claim the main slot"
#[test]
fn auxiliary_calls_without_tools_never_claim_the_main_slot() {
    let mut mains = Mains::new();
    let aux = agent_of(&session_body("s-aux", false, "summarise this"), &mut mains).unwrap();
    let real = agent_of(&session_body("s-aux", true, "the real prompt"), &mut mains).unwrap();
    assert!(!aux.main, "a toolless call never registers itself as the main thread");
    assert!(real.main, "the first real agent turn is the main thread");
    assert_ne!(mains.main_of(&s("s-aux")), Some(&aux.key), "the aux call did not take the main slot");
    assert_eq!(mains.main_of(&s("s-aux")), Some(&real.key));
}

/// "agent labels are trimmed of reminders and length"
#[test]
fn agent_labels_are_trimmed_of_reminders_and_length() {
    let label = |body: &str| agent_label(&json(body), 48).unwrap();
    assert_eq!(
        label(r#"{"messages":[{"role":"user","content":"<system-reminder>noise</system-reminder> real task"}]}"#),
        "real task"
    );
    let long = format!(r#"{{"messages":[{{"role":"user","content":"{}"}}]}}"#, "x".repeat(80));
    assert_eq!(label(&long).utf16_len(), 48);
    assert_eq!(label("{}"), "");
}

fn agent(key: &str, label: &str, main: bool) -> Agent {
    Agent { key: key.into(), label: label.into(), main }
}

/// "each agent's model is recorded separately within one session"
#[test]
fn each_agents_model_is_recorded_separately_within_one_session() {
    isolate_status();
    let sid = s(&format!("agents-{}", pid()));
    write_decision(
        &sid,
        &obj(&json(r#"{"tier":"opus","model":"claude-opus-5-5","confidence":0.94,"at":1000}"#)),
        Some(&agent("k-main", "fix the race", true)),
    )
    .unwrap();
    write_decision(
        &sid,
        &obj(&json(r#"{"tier":"haiku","model":"claude-haiku-4-5","confidence":0.81,"at":2000}"#)),
        Some(&agent("k-sub", "grep for callers", false)),
    )
    .unwrap();
    let view = agent_view(&read_status(&sid), 90_000.0, 2000.0);
    assert_eq!(view.main.get("tier"), &s("opus"), "a sub-agent's choice does not overwrite the main thread");
    assert_eq!(view.main.get("label"), &s("fix the race"));
    assert_eq!(view.subagents.len(), 1);
    assert_eq!(view.subagents[0].get("tier"), &s("haiku"));
    assert_eq!(read_status(&sid).get("history").as_array().unwrap().len(), 2, "history still records every decision");
}

/// "stale sub-agents drop out of the live view but the main thread stays"
#[test]
fn stale_sub_agents_drop_out_of_the_live_view() {
    isolate_status();
    let sid = s(&format!("stale-agents-{}", pid()));
    write_decision(&sid, &obj(&json(r#"{"tier":"opus","at":0}"#)), Some(&agent("m", "main", true))).unwrap();
    write_decision(&sid, &obj(&json(r#"{"tier":"haiku","at":0}"#)), Some(&agent("s", "old sub", false))).unwrap();
    let view = agent_view(&read_status(&sid), 90_000.0, 10.0 * 60_000.0);
    assert_eq!(view.main.get("tier"), &s("opus"));
    assert!(view.subagents.is_empty(), "a sub-agent that has not been routed recently is not live");
}

/// "a sub-agent pinned to its own model does not pause the session"
#[test]
fn a_sub_agent_pinned_to_its_own_model_does_not_pause_the_session() {
    isolate_status();
    let sid = s(&format!("manual-agents-{}", pid()));
    write_decision(
        &sid,
        &obj(&json(r#"{"tier":"opus","model":"claude-opus-5-5","at":1000}"#)),
        Some(&agent("m", "main", true)),
    )
    .unwrap();
    mark_manual(&sid, &s("claude-haiku-4-5"), Some(&agent("s", "pinned sub", false)));
    let status = read_status(&sid);
    assert_eq!(status.get("manual"), &Value::Bool(false), "only the main thread's choice pauses routing");
    assert_eq!(agent_view_now(&status).main.get("tier"), &s("opus"), "the main decision survives a sub-agent's write");
    assert_eq!(agent_view_now(&status).subagents[0].get("manual"), &Value::Bool(true));
    mark_manual(&sid, &s("claude-sonnet-5"), Some(&agent("m", "main", true)));
    assert_eq!(read_status(&sid).get("manual"), &Value::Bool(true), "the main thread picking a model does pause it");
}

/// "an id in the old version-first naming never outranks a current model"
#[test]
fn an_id_in_the_old_version_first_naming_never_outranks_a_current_model() {
    let list = catalog(
        r#"[{"id":"claude-3-7-sonnet-20250219"},{"id":"claude-sonnet-5-5"},{"id":"claude-3-5-haiku-20241022"},{"id":"claude-haiku-4-5-20251001"}]"#,
    );
    assert_eq!(
        ids(&newest_per_tier(&claude_models(&list).unwrap())),
        ["claude-sonnet-5-5", "claude-haiku-4-5-20251001"]
    );
    assert!(newer_than_calibrated(&list).unwrap().is_empty(), "a retired model is not news");
}

/// "a provider prefix does not hide the version"
#[test]
fn a_provider_prefix_does_not_hide_the_version() {
    let list = catalog(r#"[{"id":"anthropic.claude-opus-5-5"},{"id":"anthropic.claude-opus-6"}]"#);
    assert_eq!(ids(&newest_per_tier(&claude_models(&list).unwrap())), ["anthropic.claude-opus-6"]);
}

/// "flags a model newer than the router was calibrated for, and nothing else"
#[test]
fn flags_a_model_newer_than_the_router_was_calibrated_for() {
    let newer = |text: &str| -> Vec<String> {
        newer_than_calibrated(&catalog(text)).unwrap().iter().map(JsStr::to_string_lossy).collect()
    };
    assert!(newer("[]").is_empty(), "no catalog yet: nothing to report");
    assert!(
        newer(r#"[{"id":"claude-opus-5-5"},{"id":"claude-sonnet-5-5"},{"id":"claude-opus-4-8"}]"#).is_empty(),
        "the calibrated versions and older ones are not news"
    );
    assert_eq!(
        newer(r#"[{"id":"claude-opus-6"},{"id":"claude-opus-5-5"},{"id":"claude-sonnet-5-5"}]"#),
        ["claude-opus-6"]
    );
}

/// "the calibration notice round-trips and reads empty when absent"
#[test]
fn the_calibration_notice_round_trips_and_reads_empty_when_absent() {
    let file = calibration_file("calibration-test");
    let empty = read_calibration(&file);
    assert!(empty.newer.is_empty() && empty.models.is_empty() && empty.at.is_none());
    write_calibration(&["claude-opus-6".into()], &["claude-opus-6".into(), "claude-sonnet-5-5".into()], &file);
    let read = read_calibration(&file);
    assert_eq!(read.newer, vec![s("claude-opus-6")]);
    assert_eq!(read.models, vec![s("claude-opus-6"), s("claude-sonnet-5-5")]);
    assert!(read.at.is_some());
    write_calibration(&[], &[], &file);
    assert!(read_calibration(&file).newer.is_empty());
    let _ = std::fs::remove_file(file);
}
