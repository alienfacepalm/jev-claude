//! The launcher (SPEC 10; `node/bin/jev-claude.mjs`).

use jev_router::config::AUTO_MODEL;
use jev_router::env::{PRIVATE_KEYS, load_process_env};
use jev_router::envx;
use jev_router::firstrun::{
    Answer, FIRST_RUN_FILE, ask_yes_no, mark_offered, shadows_skill, should_offer, was_offered,
};
use jev_router::jsjson::{self, Object, Value};
use jev_router::launch::{command_for, launch_spec, resolve_command};
use jev_router::log::LOG_FILE;
use jev_router::osdirs::home_dir;
use jev_router::proxy::{ProxyHandle, ProxyOptions, start_proxy};
use jev_router::settings::{USER_SETTINGS, read_saved_model, restore_saved_model, saved_model_memo};
use jev_router::status::{settings_file, write_private};
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex};

/// What runs on every exit path once routing has started: close the proxy, put the saved model
/// back.
#[derive(Default)]
struct Cleanup {
    proxy: Option<ProxyHandle>,
    saved_model: Option<Value>,
}

impl Cleanup {
    fn run(&mut self) {
        if let Some(mut proxy) = self.proxy.take() {
            proxy.close();
        }
        if let Some(previous) = self.saved_model.take() {
            restore_saved_model(&previous, &USER_SETTINGS);
        }
    }
}

type SharedCleanup = Arc<Mutex<Cleanup>>;

fn exit_with(cleanup: &SharedCleanup, code: i32) -> ! {
    cleanup.lock().unwrap_or_else(|e| e.into_inner()).run();
    let _ = std::io::stderr().flush();
    std::process::exit(code);
}

fn eprint(text: &str) {
    let _ = std::io::stderr().write_all(text.as_bytes());
}

/// The `--settings` pair that installs this port's status line, or nothing (10.1).
fn status_line_args() -> Vec<String> {
    if envx::truthy("JEV_NO_STATUSLINE") {
        return vec![];
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    for dir in [cwd.join(".claude"), home_dir().join(".claude")] {
        let parsed = std::fs::read(dir.join("settings.json")).ok().and_then(|b| jsjson::parse_bytes(&b).ok());
        if parsed.is_some_and(|v| !v.is_nullish() && v.get("statusLine").truthy()) {
            return vec![];
        }
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let name = if cfg!(windows) { "jev-statusline.exe" } else { "jev-statusline" };
    let statusline = exe.parent().map_or_else(|| PathBuf::from(name), |d| d.join(name));
    let command = format!("\"{}\"", statusline.to_string_lossy());
    let mut line = Object::new();
    line.insert("type", "command".into());
    line.insert("command", command.into());
    let mut settings = Object::new();
    settings.insert("statusLine", Value::Object(line));
    let file = settings_file();
    if write_private(&file, &jsjson::to_bytes(&Value::Object(settings))).is_err() {
        return vec![];
    }
    vec!["--settings".to_string(), file.to_string_lossy().into_owned()]
}

/// Asks the first-run question, with Ctrl+C as an interrupt.
async fn ask_first_run(question: &'static str) -> Option<Answer> {
    let read = tokio::task::spawn_blocking(move || {
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        ask_yes_no(question, &mut input, &mut std::io::stderr())
    });
    tokio::select! {
        answer = read => Some(answer.unwrap_or(Answer::None)),
        _ = tokio::signal::ctrl_c() => None,
    }
}

/// Keeps Ctrl+C (and Ctrl+Break) from ending the launcher: Claude Code decides (step 8).
fn ignore_interrupts() {
    tokio::spawn(async {
        loop {
            if tokio::signal::ctrl_c().await.is_err() {
                return;
            }
        }
    });
    #[cfg(windows)]
    tokio::spawn(async {
        if let Ok(mut brk) = tokio::signal::windows::ctrl_break() {
            while brk.recv().await.is_some() {}
        }
    });
}

/// Resolves when the console closes, the user logs off, the system shuts down, or (elsewhere)
/// SIGHUP/SIGTERM arrives.
async fn hangup() {
    #[cfg(windows)]
    {
        use tokio::signal::windows::{ctrl_close, ctrl_logoff, ctrl_shutdown};
        let (Ok(mut close), Ok(mut logoff), Ok(mut shutdown)) = (ctrl_close(), ctrl_logoff(), ctrl_shutdown()) else {
            return std::future::pending().await;
        };
        tokio::select! {
            _ = close.recv() => {},
            _ = logoff.recv() => {},
            _ = shutdown.recv() => {},
        }
    }
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let (Ok(mut hup), Ok(mut term)) = (signal(SignalKind::hangup()), signal(SignalKind::terminate())) else {
            return std::future::pending().await;
        };
        tokio::select! {
            _ = hup.recv() => {},
            _ = term.recv() => {},
        }
    }
    #[cfg(not(any(unix, windows)))]
    std::future::pending::<()>().await
}

fn exit_code(status: ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal().is_some() {
            return 1;
        }
    }
    status.code().unwrap_or(1)
}

#[tokio::main]
async fn main() {
    jev_router::init();
    let saved_model_before = read_saved_model(&USER_SETTINGS, &saved_model_memo());
    load_process_env();

    let cli: Vec<String> = std::env::args().skip(1).collect();
    let mut args = cli.clone();
    let root = jev_router::repo::root();
    if let Some(root) = root {
        args.push("--add-dir".into());
        args.push(root.to_string_lossy().into_owned());
    }
    let mut extra_env: Vec<(String, String)> = envx::overlay();

    let Some(claude) = resolve_command("claude", None, None, cfg!(windows)) else {
        eprint(
            "[jev] Claude Code is not installed, or `claude` is not on your PATH.\n\
             [jev] jev-claude runs the real Claude Code CLI; install it first:\n\
             [jev]   https://code.claude.com/docs/en/setup\n",
        );
        std::process::exit(1);
    };

    let cwd = std::env::current_dir().unwrap_or_default();
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if should_offer(&cli, interactive, was_offered(&FIRST_RUN_FILE), shadows_skill(&cwd, root)) {
        let answer = ask_first_run(
            "[jev] First run: check your Jev Router setup now with /jev-calibrate?\n\
             [jev] It only reads your setup and changes nothing, using a little of your Claude usage. [Y/n] ",
        )
        .await;
        match answer {
            None => std::process::exit(130),
            Some(Answer::None) => {}
            Some(a) => {
                mark_offered(a == Answer::Yes, &FIRST_RUN_FILE);
                if a == Answer::Yes {
                    args.insert(0, "/jev-calibrate check".into());
                }
            }
        }
    }

    ignore_interrupts();
    let cleanup: SharedCleanup = Arc::new(Mutex::new(Cleanup::default()));

    if envx::truthy("JEV_API_KEY") || envx::truthy("TYPESAFE_API_KEY") {
        let inherited = envx::get("ANTHROPIC_BASE_URL").filter(|u| !u.is_empty());
        let proxy = match start_proxy(ProxyOptions { upstream_url: inherited.clone(), ..Default::default() }).await {
            Ok(p) => p,
            Err(e) => {
                eprint(&format!("[jev] could not start the proxy: {e}\n"));
                std::process::exit(1);
            }
        };
        if let (Some(url), true) = (&inherited, envx::truthy("JEV_DEBUG")) {
            eprint(&format!("[jev] upstream {url}\n"));
        }
        let mut set = vec![
            ("ANTHROPIC_BASE_URL".to_string(), format!("http://127.0.0.1:{}", proxy.port)),
            ("CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY".to_string(), "1".to_string()),
            ("ANTHROPIC_CUSTOM_MODEL_OPTION".to_string(), AUTO_MODEL.to_string()),
            ("ANTHROPIC_CUSTOM_MODEL_OPTION_NAME".to_string(), "Jev Router".to_string()),
            (
                "ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION".to_string(),
                "Route each turn to the cheapest model that can do it".to_string(),
            ),
            (
                "ANTHROPIC_CUSTOM_MODEL_OPTION_SUPPORTED_CAPABILITIES".to_string(),
                "thinking,adaptive_thinking,interleaved_thinking,effort,max_effort".to_string(),
            ),
            ("CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT".to_string(), "1".to_string()),
        ];
        if !envx::truthy("ANTHROPIC_MODEL") {
            set.push(("ANTHROPIC_MODEL".to_string(), AUTO_MODEL.to_string()));
        }
        extra_env.extend(set);
        {
            let mut c = cleanup.lock().unwrap();
            c.proxy = Some(proxy);
            c.saved_model = Some(saved_model_before);
        }
        args.extend(status_line_args());
        if envx::truthy("JEV_DEBUG") && std::io::stdout().is_terminal() {
            eprint(&format!("[jev] routing decisions -> {}\n", LOG_FILE.to_string_lossy()));
        }
    } else {
        eprint(&format!(
            "[jev] no JEV_API_KEY found - starting Claude Code without routing\n\
             [jev] set it in {} to enable routing\n",
            home_dir().join(".jev-router.env").to_string_lossy()
        ));
    }

    let spec = launch_spec(&claude);
    let mut command = command_for(&spec, &args);
    // `childEnv`: the Jev key stays with jev; Claude Code gets everything else, including what
    // the settings files added.
    for key in PRIVATE_KEYS {
        command.env_remove(key);
    }
    for (k, v) in &extra_env {
        if !PRIVATE_KEYS.contains(&k.as_str()) {
            command.env(k, v);
        }
    }
    let mut child = match tokio::process::Command::from(command).spawn() {
        Ok(c) => c,
        Err(e) => {
            eprint(&format!("[jev] could not start Claude Code: {e}\n"));
            exit_with(&cleanup, 1);
        }
    };

    tokio::select! {
        status = child.wait() => {
            let code = status.map_or(1, exit_code);
            exit_with(&cleanup, code);
        }
        _ = hangup() => {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
            exit_with(&cleanup, 1);
        }
    }
}
