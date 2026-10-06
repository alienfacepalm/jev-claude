//! Port of node/test/proxy-routing.test.mjs, driven by a request captured from the real Claude
//! Code CLI: print mode posts to `/v1/messages?beta=true`, carries adaptive thinking and a
//! thinking-clearing context edit, and ends in a `role: "system"` message.

mod common;

use common::*;
use jev_router::jsjson::{self, Value};
use jev_router::proxy::{ProxyOptions, RouteFn, start_proxy};
use jev_router::status::read_status;
use std::sync::{Arc, Mutex};

fn captured() -> Value {
    jsjson::parse_bytes(
        &std::fs::read(repo_root().join("conformance/fixtures/claude-code-print-request.json")).unwrap(),
    )
    .unwrap()
}

fn real_request() -> Value {
    captured().get("body").clone()
}

fn session() -> Value {
    jev_router::proxy::session_of(&real_request())
}

/// The same conversation one step later: Claude ran a tool and is sending back its result.
fn continuation(body: &Value) -> Value {
    let mut out = body.spread_of();
    let messages = body.get("messages").as_array().unwrap();
    let mut next = vec![messages[0].clone()];
    next.push(json(
        r#"{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"ls"}}]}"#,
    ));
    next.push(json(
        r#"{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"utils.js"}]}"#,
    ));
    next.extend(messages[1..].iter().cloned());
    out.insert("messages", Value::Array(next));
    Value::Object(out)
}

struct Harness {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
    prompts: Arc<Mutex<Vec<String>>>,
    _proxy: jev_router::proxy::ProxyHandle,
}

impl Harness {
    async fn post(&self, body: &Value) -> Got {
        let url = captured().get("url").as_str().unwrap().to_string_lossy();
        post_json(self.port, &url, body).await
    }
    fn bodies(&self) -> Vec<Value> {
        self.seen.lock().unwrap().iter().map(Seen::json).collect()
    }
}

/// A proxy in front of an upstream that records what it got and, like the API, rejects the
/// sentinel.
async fn harness(route: impl Fn(&str) -> &'static str + Send + Sync + 'static, confidence: f64) -> Harness {
    isolate_status();
    let (url, seen) = serve(Arc::new(|r: &Seen| {
        if r.json().get("model").is_str("jev-router") {
            return Reply::json(
                400,
                r#"{"type":"error","error":{"type":"invalid_request_error","message":"model: jev-router"}}"#,
            );
        }
        Reply::json(200, r#"{"id":"msg_1","type":"message"}"#)
    }))
    .await;
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let log = prompts.clone();
    let route: RouteFn = Arc::new(move |args| {
        let prompt = args.prompt.to_string_lossy();
        log.lock().unwrap().push(prompt.clone());
        let choice = route(&prompt);
        Box::pin(async move { Ok(Some(json(&format!(r#"{{"choice":"{choice}","confidence":{confidence},"ms":1}}"#)))) })
    });
    let proxy =
        start_proxy(ProxyOptions { upstream_url: Some(url), route: Some(route), ..Default::default() }).await.unwrap();
    Harness { port: proxy.port, seen, prompts, _proxy: proxy }
}

/// "a real print-mode request is routed on the user's prompt"
#[tokio::test]
async fn a_real_print_mode_request_is_routed_on_the_users_prompt() {
    let h = harness(|_| "claude-haiku-4-5-20251001", 0.92).await;
    let res = h.post(&real_request()).await;
    assert_eq!(res.status, 200);
    assert_eq!(*h.prompts.lock().unwrap(), vec!["rename the variable x to count in utils.js"], "reminders stripped");
    let seen = h.seen.lock().unwrap()[0].clone();
    assert_eq!(seen.url, "/v1/messages?beta=true");
    let sent = seen.json();
    assert_eq!(sent.get("model"), &s("claude-haiku-4-5-20251001"));
    // Haiku takes neither adaptive thinking nor effort; sending them as captured would be rejected.
    assert_eq!(sent.get("thinking"), &Value::Undefined);
    assert_eq!(sent.get("context_management"), &Value::Undefined);
    assert_eq!(sent.get("output_config").get("effort"), &Value::Undefined);
    assert_eq!(read_status(&session()).get("tier"), &s("haiku"), "the decision reaches the status line");
}

/// "an unwritable JEV_DUMP path does not stop the turn being routed"
#[tokio::test]
async fn an_unwritable_jev_dump_path_does_not_stop_the_turn_being_routed() {
    // A real misconfiguration: the dump directory does not exist, so the dump cannot be written.
    let missing = std::env::temp_dir().join("jev-no-such-dir").join("nested").join("dump");
    jev_router::envx::set("JEV_DUMP", &missing.to_string_lossy());
    let h = harness(|_| "claude-haiku-4-5-20251001", 0.92).await;
    let res = h.post(&real_request()).await;
    jev_router::envx::unset_overlay("JEV_DUMP");
    assert_eq!(res.status, 200, "the API would reject the sentinel with a 400");
    assert_eq!(h.bodies()[0].get("model"), &s("claude-haiku-4-5-20251001"), "routed as usual, dump or no dump");
}

/// "tool-call continuations keep the tier the turn was routed to"
#[tokio::test]
async fn tool_call_continuations_keep_the_tier_the_turn_was_routed_to() {
    let h = harness(|_| "claude-sonnet-5-5", 0.92).await;
    let opening = real_request();
    h.post(&opening).await;
    h.post(&continuation(&opening)).await;
    h.post(&continuation(&opening)).await;
    assert_eq!(h.prompts.lock().unwrap().len(), 1, "Jev is asked once per turn, not once per tool call");
    let models: Vec<Value> = h.bodies().iter().map(|b| b.get("model").clone()).collect();
    assert_eq!(models, vec![s("claude-sonnet-5-5"); 3]);
}

/// "the main thread keeps its tier after more than 50 sub-agents start"
#[tokio::test]
async fn the_main_thread_keeps_its_tier_after_more_than_50_sub_agents_start() {
    let h = harness(
        |prompt| {
            if prompt.starts_with("Search the codebase") { "claude-haiku-4-5-20251001" } else { "claude-sonnet-5-5" }
        },
        0.9,
    )
    .await;
    let main = real_request();
    h.post(&main).await;
    // Sub-agents share the session id and differ by their opening task.
    for i in 0..55 {
        let mut sub = real_request();
        let first = sub.as_object_mut().unwrap().get_mut("messages").unwrap();
        let Value::Array(messages) = first else { unreachable!() };
        let Value::Array(blocks) = messages[0].as_object_mut().unwrap().get_mut("content").unwrap() else {
            unreachable!()
        };
        let last = blocks.last_mut().unwrap().as_object_mut().unwrap();
        last.insert("text", s(&format!("Search the codebase for callers of handler {i} and report them")));
        h.post(&sub).await;
    }
    h.post(&continuation(&main)).await;
    assert_eq!(h.bodies().last().unwrap().get("model"), &s("claude-sonnet-5-5"), "not reset to the default");
}
