//! Port of node/test/hardening.test.mjs.

// Each test's doc comment is the Node test title, quoted verbatim so the two suites can be
// compared line by line; Markdown backticks would change the titles.
#![allow(clippy::doc_markdown)]

mod common;

use common::*;
use http_body_util::BodyExt;
use hyper::body::Bytes;
use jev_router::config::id_of;
use jev_router::env::{apply_child_env, child_env, load_env};
use jev_router::envx::{EnvMap, map};
use jev_router::jsjson::Value;
use jev_router::launch::{LaunchSpec, command_for, launch_spec, quote_for_cmd, resolve_command, shim_script};
use jev_router::proxy::{ProxyOptions, RouteFn, start_proxy};
use jev_router::status::dump_body;
use std::sync::Arc;
use std::time::Duration;

const HAIKU: &str = "claude-haiku-4-5-20251001";

fn sure(choice: &str) -> RouteFn {
    answer(choice, 0.97)
}

fn metadata(session: &str) -> String {
    format!(r#""metadata":{{"user_id":"{{\"session_id\":\"{session}\"}}"}}"#)
}

const TOOL_USE: &str = r#"{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}]}"#;
const TOOL_RESULT: &str = r#"{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}"#;

/// An upstream that records each body and answers with a small JSON message, plus a proxy.
async fn recording(route: RouteFn) -> (u16, Arc<std::sync::Mutex<Vec<Seen>>>, jev_router::proxy::ProxyHandle) {
    isolate_status();
    let (url, seen) = serve(Arc::new(|_: &Seen| Reply::json(200, r#"{"id":"msg_1","type":"message"}"#))).await;
    let proxy =
        start_proxy(ProxyOptions { upstream_url: Some(url), route: Some(route), ..Default::default() }).await.unwrap();
    (proxy.port, seen, proxy)
}

fn last_model(seen: &std::sync::Mutex<Vec<Seen>>) -> Value {
    seen.lock().unwrap().last().unwrap().json().get("model").clone()
}

/// "the main thread keeps its tier after a session has run 50 sub-agents"
#[tokio::test]
async fn the_main_thread_keeps_its_tier_after_50_sub_agents() {
    let (port, seen, _proxy) = recording(sure(HAIKU)).await;
    let meta = metadata(&format!("lru-{}", std::process::id()));
    let opening = r#"{"role":"user","content":"rename the config loader"}"#;
    let body = |messages: &str| {
        json(&format!(r#"{{"model":"jev-router","tools":[{{"name":"Bash"}}],{meta},"messages":[{messages}]}}"#))
    };
    post_json(port, "/v1/messages", &body(opening)).await;
    for i in 0..51 {
        post_json(port, "/v1/messages", &body(&format!(r#"{{"role":"user","content":"sub-agent {i}"}}"#))).await;
    }
    // A tool continuation of the main thread's turn, which must not change model mid-task.
    post_json(port, "/v1/messages", &body(&format!("{opening},{TOOL_USE},{TOOL_RESULT}"))).await;
    assert_eq!(last_model(&seen), s(HAIKU));
}

/// "a conversation the proxy has not routed yet may still downgrade"
#[tokio::test]
async fn a_conversation_the_proxy_has_not_routed_yet_may_still_downgrade() {
    let (port, seen, _proxy) = recording(sure(HAIKU)).await;
    // A resumed session: a long history, but nothing cached on any model by this process.
    let history = "x".repeat(120_000);
    let body = json(&format!(
        r#"{{"model":"jev-router","tools":[{{"name":"Bash"}}],"messages":[{{"role":"user","content":"{history}"}},{TOOL_USE},{TOOL_RESULT},{{"role":"user","content":"fix the typo"}}]}}"#
    ));
    post_json(port, "/v1/messages", &body).await;
    assert_eq!(last_model(&seen), s(HAIKU));
}

/// "a routing failure never forwards the sentinel"
#[tokio::test]
async fn a_routing_failure_never_forwards_the_sentinel() {
    let failing: RouteFn = Arc::new(|_| Box::pin(async { Err("router blew up".to_string()) }));
    let (port, seen, _proxy) = recording(failing).await;
    post_json(
        port,
        "/v1/messages",
        &json(r#"{"model":"jev-router","tools":[{"name":"Bash"}],"messages":[{"role":"user","content":"hello"}]}"#),
    )
    .await;
    assert_eq!(
        last_model(&seen),
        s(id_of("sonnet").unwrap()),
        "a failure lands on the default tier, never the sentinel"
    );
}

/// "print mode keeps the conversation's tier when the session id appears later"
#[tokio::test]
async fn print_mode_keeps_the_conversations_tier_when_the_session_id_appears_later() {
    let (port, seen, _proxy) = recording(sure(HAIKU)).await;
    let opening = format!(r#"{{"role":"user","content":"print-mode {}"}}"#, std::process::id());
    // `claude -p` sends its first request without metadata.
    post_json(
        port,
        "/v1/messages",
        &json(&format!(r#"{{"model":"jev-router","tools":[{{"name":"Bash"}}],"messages":[{opening}]}}"#)),
    )
    .await;
    let meta = metadata(&format!("late-{}", std::process::id()));
    post_json(
        port,
        "/v1/messages",
        &json(&format!(r#"{{"model":"jev-router","tools":[{{"name":"Bash"}}],{meta},"messages":[{opening},{TOOL_USE},{TOOL_RESULT}]}}"#)),
    )
    .await;
    let models: Vec<Value> = seen.lock().unwrap().iter().map(|s| s.json().get("model").clone()).collect();
    assert_eq!(models, vec![s(HAIKU), s(HAIKU)]);
}

const GO: &str = r#"{"model":"jev-router","tools":[{"name":"Bash"}],"messages":[{"role":"user","content":"go"}]}"#;

/// SPEC 7.2 step 3 and 20.10 (no Node test covers it): a client that disconnects while Jev is
/// being asked still gets its decision recorded, and nothing is sent upstream for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_that_leaves_while_jev_is_asked_is_recorded_but_not_forwarded() {
    use tokio::io::AsyncWriteExt;
    isolate_status();
    let (asked_tx, asked_rx) = tokio::sync::oneshot::channel::<()>();
    let asked_tx = Arc::new(std::sync::Mutex::new(Some(asked_tx)));
    // A Jev that takes 300 ms to answer, and says when it has been asked.
    let slow_jev: RouteFn = Arc::new(move |_args| {
        let asked = asked_tx.lock().unwrap().take();
        Box::pin(async move {
            if let Some(tx) = asked {
                let _ = tx.send(());
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
            let mut o = jev_router::jsjson::Object::new();
            o.insert("choice", Value::from(HAIKU));
            o.insert("confidence", Value::Number(0.97));
            o.insert("ms", Value::Number(300.0));
            Ok(Some(Value::Object(o)))
        })
    });
    let (url, seen) = serve(Arc::new(|_: &Seen| Reply::json(200, r#"{"id":"msg_1","type":"message"}"#))).await;
    let proxy = start_proxy(ProxyOptions { upstream_url: Some(url), route: Some(slow_jev), ..Default::default() })
        .await
        .unwrap();

    let session = format!("left-{}", std::process::id());
    let body = format!(
        r#"{{"model":"jev-router","tools":[{{"name":"Bash"}}],{},"messages":[{{"role":"user","content":"go"}}]}}"#,
        metadata(&session)
    );
    let mut client = tokio::net::TcpStream::connect(("127.0.0.1", proxy.port)).await.unwrap();
    let head = format!(
        "POST /v1/messages HTTP/1.1\r\nhost: 127.0.0.1\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n",
        body.len()
    );
    client.write_all(head.as_bytes()).await.unwrap();
    client.write_all(body.as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), asked_rx).await.expect("Jev was asked").unwrap();
    drop(client); // Claude Code goes away while Jev is still thinking.

    let status = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = jev_router::status::read_status(&Value::from(session.as_str()));
            if status.truthy() {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the decision is recorded even though the client left");
    assert_eq!(status.get("tier"), &Value::from("haiku"));
    // Give a wrongly forwarded request time to arrive before checking that none did.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(seen.lock().unwrap().is_empty(), "nothing is sent upstream for a client that has gone");
}

/// "a client that leaves stops the upstream response"
#[tokio::test]
async fn a_client_that_leaves_stops_the_upstream_response() {
    isolate_status();
    let (gave_up_tx, gave_up_rx) = tokio::sync::oneshot::channel::<bool>();
    let gave_up_tx = Arc::new(std::sync::Mutex::new(Some(gave_up_tx)));
    let (url, _) = serve(Arc::new(move |_: &Seen| {
        let (tx, body) = stream_reply();
        let report = gave_up_tx.clone();
        tokio::spawn(async move {
            let mut sent = 0;
            while sent < 100 {
                tokio::time::sleep(Duration::from_millis(20)).await;
                if tx.send(Ok(Bytes::from(format!("data: {{\"n\":{sent}}}\n\n")))).await.is_err() {
                    break;
                }
                sent += 1;
            }
            // Wait for the connection to report the reader gone, or the stream to finish.
            tokio::time::timeout(Duration::from_secs(5), tx.closed()).await.ok();
            if let Some(r) = report.lock().unwrap().take() {
                let _ = r.send(sent < 100);
            }
        });
        Reply { status: 200, headers: vec![("content-type".into(), "text/event-stream".into())], body }
    }))
    .await;
    let proxy = start_proxy(ProxyOptions { upstream_url: Some(url), route: Some(sure(HAIKU)), ..Default::default() })
        .await
        .unwrap();

    {
        let res = send(proxy.port, "POST", "/v1/messages", &[], GO.as_bytes()).await;
        let mut body = res.into_body();
        let first = body.frame().await;
        assert!(first.is_some(), "a first chunk arrived");
        // Dropping the body and its connection is the client leaving.
    }
    let gave_up = tokio::time::timeout(Duration::from_secs(10), gave_up_rx).await.expect("upstream finished").unwrap();
    assert!(gave_up, "the upstream stream was cut short rather than run to the end");
}

/// "an upstream that drops mid-stream fails the client instead of hanging it"
#[tokio::test]
async fn an_upstream_that_drops_mid_stream_fails_the_client() {
    isolate_status();
    let (url, _) = serve(Arc::new(|_: &Seen| {
        let (tx, body) = stream_reply();
        tokio::spawn(async move {
            let _ = tx.send(Ok(Bytes::from("data: {\"n\":0}\n\n"))).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            // An error frame makes the server abort the connection mid-body.
            let _ = tx.send(Err("socket destroyed".into())).await;
        });
        Reply { status: 200, headers: vec![("content-type".into(), "text/event-stream".into())], body }
    }))
    .await;
    let proxy = start_proxy(ProxyOptions { upstream_url: Some(url), route: Some(sure(HAIKU)), ..Default::default() })
        .await
        .unwrap();

    let ended = tokio::time::timeout(Duration::from_secs(3), async {
        let res = send(proxy.port, "POST", "/v1/messages", &[], GO.as_bytes()).await;
        res.into_body().collect().await.is_ok()
    })
    .await
    .expect("client still waiting after 3s");
    assert!(!ended, "the client sees an incomplete response, not a clean end");
}

/// "a project's .env may only set jev's own keys"
#[test]
fn a_projects_env_may_only_set_jevs_own_keys() {
    let cwd = temp_dir("jev-env-cwd-");
    let home = temp_dir("jev-env-home-");
    std::fs::write(
        cwd.join(".env"),
        [
            "JEV_API_KEY=from-project",
            "JEV_OPUS_EFFORT=low",
            "JEV_FORCE_EFFORT=max",
            "JEV_SONNET_FORCE_EFFORT=low",
            "ANTHROPIC_BASE_URL=https://attacker.example",
            "TYPESAFE_BASE_URL=https://attacker.example",
            "NODE_OPTIONS=--require /tmp/evil.js",
            "JEV_DUMP=/tmp/loot",
            "JEV_DEBUG=project",
        ]
        .join("\n"),
    )
    .unwrap();
    std::fs::write(home.join(".jev-router.env"), "JEV_DEBUG=home\nTYPESAFE_BASE_URL=https://jev.example\n").unwrap();

    let mut env = map(&[("JEV_ALLOW_FABLE", "1")]);
    load_env(&cwd, &home, &mut env);
    let get = |k: &str| env.get(k).cloned();
    assert_eq!(get("JEV_API_KEY").as_deref(), Some("from-project"));
    assert_eq!(get("JEV_OPUS_EFFORT").as_deref(), Some("low"));
    assert_eq!(get("JEV_FORCE_EFFORT").as_deref(), Some("max"));
    assert_eq!(get("JEV_SONNET_FORCE_EFFORT").as_deref(), Some("low"));
    assert_eq!(get("JEV_DEBUG").as_deref(), Some("project"), "the project file still outranks the home file");
    assert_eq!(
        get("TYPESAFE_BASE_URL").as_deref(),
        Some("https://jev.example"),
        "only the user's own file may move Jev"
    );
    assert_eq!(get("ANTHROPIC_BASE_URL"), None);
    assert_eq!(get("NODE_OPTIONS"), None);
    assert_eq!(get("JEV_DUMP"), None);
    assert_eq!(get("JEV_ALLOW_FABLE").as_deref(), Some("1"), "the real environment wins");

    let mut with_more = env.clone();
    with_more.insert("PATH".into(), "/bin".into());
    with_more.insert("TYPESAFE_API_KEY".into(), "k".into());
    let child = child_env(&with_more);
    assert_eq!(child.get("JEV_API_KEY"), None);
    assert_eq!(child.get("TYPESAFE_API_KEY"), None);
    assert_eq!(child.get("PATH").map(String::as_str), Some("/bin"));
}

/// "a Claude API key in the user's own file reaches Claude Code, but never from a project's .env"
#[test]
fn a_claude_api_key_in_the_users_own_file_reaches_claude_code() {
    let cwd = temp_dir("jev-env-cwd-");
    let home = temp_dir("jev-env-home-");
    std::fs::write(cwd.join(".env"), "ANTHROPIC_API_KEY=sk-ant-someone-else\n").unwrap();
    std::fs::write(home.join(".jev-router.env"), "JEV_API_KEY=jev\nANTHROPIC_API_KEY=sk-ant-mine\n").unwrap();

    let mut env = EnvMap::new();
    load_env(&cwd, &home, &mut env);
    assert_eq!(env.get("ANTHROPIC_API_KEY").map(String::as_str), Some("sk-ant-mine"));
    let child = child_env(&env);
    assert_eq!(child.get("ANTHROPIC_API_KEY").map(String::as_str), Some("sk-ant-mine"), "Claude Code needs it");
    assert_eq!(child.get("JEV_API_KEY"), None);

    let mut from_project_only = EnvMap::new();
    load_env(&cwd, &temp_dir("jev-env-home-"), &mut from_project_only);
    assert_eq!(from_project_only.get("ANTHROPIC_API_KEY"), None);
}

/// The launcher's own path (`apply_child_env`): a real child process never sees the Jev keys,
/// whether inherited, set on the command, or added by a settings file, but gets everything else.
#[test]
fn the_launchers_child_process_never_sees_the_jev_keys() {
    let mut command = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/d", "/c", "set"]);
        c
    } else {
        std::process::Command::new("env")
    };
    // What Claude Code would otherwise inherit from the launcher's own environment.
    command.env("JEV_API_KEY", "inherited-jev").env("TYPESAFE_API_KEY", "inherited-typesafe");
    let extra: Vec<(String, String)> = [
        ("ANTHROPIC_BASE_URL", "http://127.0.0.1:9"),
        ("JEV_API_KEY", "from-settings"),
        ("TYPESAFE_API_KEY", "from-settings"),
        ("jev_api_key", "lower-case"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
    .collect();
    apply_child_env(&mut command, &extra);
    let out = command.stdin(std::process::Stdio::null()).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    let seen: Vec<(&str, &str)> = text.lines().filter_map(|l| l.split_once('=')).collect();
    let value = |name: &str| seen.iter().find(|(k, _)| *k == name).map(|(_, v)| *v);
    assert_eq!(value("ANTHROPIC_BASE_URL"), Some("http://127.0.0.1:9"), "ordinary keys reach Claude Code");
    assert!(seen.iter().any(|(k, _)| k.eq_ignore_ascii_case("PATH")), "the inherited environment is kept");
    assert_eq!(value("JEV_API_KEY"), None);
    assert_eq!(value("TYPESAFE_API_KEY"), None);
    if cfg!(windows) {
        // Environment names are case-insensitive on Windows, so `jev_api_key` is the same key.
        assert!(!seen.iter().any(|(k, _)| k.eq_ignore_ascii_case("JEV_API_KEY")), "{text}");
    } else {
        assert_eq!(value("jev_api_key"), Some("lower-case"), "a different name on Unix");
    }
}

/// "a blank key in a copied .env.example does not hide the real one"
#[test]
fn a_blank_key_in_a_copied_env_example_does_not_hide_the_real_one() {
    let cwd = temp_dir("jev-env-cwd-");
    let home = temp_dir("jev-env-home-");
    std::fs::copy(repo_root().join(".env.example"), cwd.join(".env")).unwrap();
    std::fs::write(home.join(".jev-router.env"), "JEV_API_KEY=from-home\n").unwrap();
    let mut env = EnvMap::new();
    load_env(&cwd, &home, &mut env);
    assert_eq!(env.get("JEV_API_KEY").map(String::as_str), Some("from-home"));
    assert_eq!(env.get("JEV_ALLOW_FABLE"), None, "commented-out settings stay unset");
}

/// A directory holding an npm-style `name.cmd` shim, its script, and a `.ps1` beside it.
fn shim_dir(name: &str, with_script: bool) -> (TempDir, std::path::PathBuf) {
    let dir = temp_dir("jev-shim-");
    let script = dir.join("node_modules").join("pkg").join("cli.js");
    std::fs::create_dir_all(script.parent().unwrap()).unwrap();
    std::fs::write(&script, "process.stdout.write(JSON.stringify(process.argv.slice(2)));\n").unwrap();
    let target = if with_script {
        r#""%dp0%\node_modules\pkg\cli.js""#.to_string()
    } else {
        format!("\"{}\"", script.display())
    };
    std::fs::write(dir.join(format!("{name}.cmd")), format!("@ECHO off\r\n\"node\"  {target} %*\r\n")).unwrap();
    std::fs::write(dir.join(format!("{name}.ps1")), "#!/usr/bin/env pwsh\n").unwrap();
    (dir, script)
}

fn run(spec: &LaunchSpec, args: &[String]) -> Value {
    let out = command_for(spec, args).stdin(std::process::Stdio::null()).output().unwrap();
    json(&String::from_utf8_lossy(&out.stdout))
}

// Values whose quoting the old shell path broke: embedded quotes, a space, and cmd metacharacters.
fn awkward() -> Vec<String> {
    ["name=\"Jev Router\"", "fix a&b|c", "50% done", "say \"hi\"", "plain"].iter().map(|s| (*s).to_string()).collect()
}

fn awkward_value() -> Value {
    Value::Array(awkward().into_iter().map(Value::from).collect())
}

/// "prefers Claude's .cmd shim over its .ps1 and runs the script behind it directly"
#[test]
fn prefers_claudes_cmd_shim_over_its_ps1() {
    let (dir, script) = shim_dir("claude", true);
    let file = resolve_command("claude", Some(&[".exe", ".cmd", ".bat", ".ps1"]), Some(&dir.to_string_lossy()), true);
    assert_eq!(file, Some(dir.join("claude.cmd")));
    let file = file.unwrap();
    assert_eq!(shim_script(&file), Some(script.clone()));
    let spec = launch_spec(&file);
    // Node uses its own executable; this port runs the script with `node` from PATH.
    let node = resolve_command("node", None, None, cfg!(windows)).expect("node on PATH");
    assert_eq!(spec, LaunchSpec { command: node, prefix: vec![script.to_string_lossy().into_owned()], shim: None });
    assert_eq!(run(&spec, &awkward()), awkward_value());
}

/// "a PowerShell shim is run past the default execution policy"
#[test]
fn a_powershell_shim_is_run_past_the_default_execution_policy() {
    let spec = launch_spec(std::path::Path::new("C:\\bin\\claude.ps1"));
    assert_eq!(spec.prefix[..4], ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
}

/// "a shim with no script to run directly is quoted for cmd.exe" (Windows only, as in Node)
#[cfg(windows)]
#[test]
fn a_shim_with_no_script_to_run_directly_is_quoted_for_cmd() {
    let (dir, _) = shim_dir("opaque", false);
    let spec = launch_spec(&dir.join("opaque.cmd"));
    assert!(spec.shim.is_some(), "falls back to cmd.exe");
    assert_eq!(run(&spec, &awkward()), awkward_value());
}

/// "cmd quoting escapes metacharacters"
#[test]
fn cmd_quoting_escapes_metacharacters() {
    let unescaped = |q: &str, c: char| {
        let b: Vec<char> = q.chars().collect();
        (1..b.len()).any(|i| b[i] == c && b[i - 1] != '^')
    };
    let q = quote_for_cmd("a&b|c");
    assert!(!unescaped(&q, '&') && !unescaped(&q, '|'), "every & and | is caret-escaped: {q}");
    assert!(!unescaped(&quote_for_cmd("50%"), '%'));
}

/// "JEV_DUMP=1 writes owner-only dumps into the status directory, never over each other"
#[test]
fn jev_dump_1_writes_owner_only_dumps_into_the_status_directory() {
    let dir = isolate_status();
    let first = dump_body(&json(r#"{"a":1}"#), Some("1")).unwrap();
    let second = dump_body(&json(r#"{"a":2}"#), Some("1")).unwrap();
    assert!(first.starts_with(dir));
    assert_ne!(first, second);
    assert_eq!(json(&std::fs::read_to_string(&second).unwrap()), json(r#"{"a":2}"#));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&first).unwrap().permissions().mode() & 0o777, 0o600);
    }
    for f in [first, second] {
        std::fs::remove_file(f).unwrap();
    }
}
