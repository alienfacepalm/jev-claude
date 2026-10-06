//! The status line key behind /jev-legend (SPEC 13).
use std::io::Write;

fn main() {
    jev_router::init();
    let legend = jev_router::legend::format_legend(&jev_router::icons::icons());
    let _ = std::io::stdout().write_all(format!("Status line key\n\n{legend}\n").as_bytes());
}
