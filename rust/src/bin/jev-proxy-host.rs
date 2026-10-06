//! Hosts one proxy and prints its port (SPEC 16.3).
use jev_router::proxy::{ProxyOptions, start_proxy};
use std::io::Write;

#[tokio::main]
async fn main() {
    jev_router::init();
    jev_router::env::load_process_env();
    let upstream = jev_router::envx::get("ANTHROPIC_BASE_URL").filter(|u| !u.is_empty());
    let proxy = match start_proxy(ProxyOptions { upstream_url: upstream, ..Default::default() }).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[jev] could not start the proxy: {e}");
            std::process::exit(1);
        }
    };
    let mut out = std::io::stdout();
    let _ = writeln!(out, "PORT={}", proxy.port);
    let _ = out.flush();
    std::future::pending::<()>().await;
}
