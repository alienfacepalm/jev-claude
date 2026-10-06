//! Logging (SPEC 15; `node/src/log.mjs`).

use crate::osdirs::home_dir;
use crate::timefmt::iso_now;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

/// `<home at start>/.jev-claude.log`.
pub static LOG_FILE: LazyLock<PathBuf> = LazyLock::new(|| home_dir().join(".jev-claude.log"));

/// Whether stdout was a terminal at start.
pub static INTERACTIVE: LazyLock<bool> = LazyLock::new(|| std::io::stdout().is_terminal());

static TIGHTENED: AtomicBool = AtomicBool::new(false);
static WRITE: Mutex<()> = Mutex::new(());

/// `log(line)`: stderr when stdout is not a terminal, else the log file.
pub fn log(line: &str) {
    let text = format!("[jev] {line}\n");
    let _guard = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    if !*INTERACTIVE {
        let _ = std::io::stderr().write_all(text.as_bytes());
        return;
    }
    let _ = (|| -> std::io::Result<()> {
        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&*LOG_FILE)?;
        f.write_all(format!("{} {text}", iso_now()).as_bytes())?;
        if !TIGHTENED.swap(true, Ordering::SeqCst) {
            crate::fsx::chmod(&LOG_FILE, 0o600)?;
        }
        Ok(())
    })();
}

/// `debug(line)`: logs only while `JEV_DEBUG` is truthy.
pub fn debug(line: impl FnOnce() -> String) {
    if crate::envx::truthy("JEV_DEBUG") {
        log(&line());
    }
}
