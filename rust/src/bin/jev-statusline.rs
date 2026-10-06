//! Status line for Claude Code (SPEC 12).
use std::io::{Read, Write};

fn main() {
    jev_router::init();
    let mut input = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut input);
    let line = match jev_router::statusline::render(&input, &jev_router::icons::icons()) {
        Ok(line) => line,
        Err(message) => {
            // Node throws here: an error on stderr, nothing on stdout, exit 1.
            let _ = writeln!(std::io::stderr(), "jev-statusline: {message}");
            std::process::exit(1);
        }
    };
    let mut out = std::io::stdout();
    let _ = out.write_all(line.as_bytes());
    let _ = out.flush();
}
