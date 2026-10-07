//! File helpers: modes (no-ops on Windows), unique temporary names (SPEC 3.10) and replacing a file
//! that another process may have open (SPEC 3.11).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static SEQ: AtomicU64 = AtomicU64::new(0);

/// `<file>.<pid>.<seq>.tmp`.
pub fn temp_name(file: &Path) -> PathBuf {
    let seq = SEQ.fetch_add(1, Ordering::SeqCst);
    let mut name = file.as_os_str().to_os_string();
    name.push(format!(".{}.{seq}.tmp", std::process::id()));
    PathBuf::from(name)
}

const ATTEMPTS: u32 = 10;
const PAUSE: Duration = Duration::from_millis(20);

/// Whether Windows refused the rename because another process has the target open:
/// `ERROR_ACCESS_DENIED` (5), `ERROR_SHARING_VIOLATION` (32) or `ERROR_LOCK_VIOLATION` (33).
fn transient(err: &std::io::Error) -> bool {
    cfg!(windows) && matches!(err.raw_os_error(), Some(5 | 32 | 33))
}

/// Renames `temp` over `file`, retrying briefly while Windows reports the target as in use. Any
/// other error, or one that outlasts the retries (about 180 ms), removes `temp` and is returned:
/// the temporary file can hold prompt text, and nothing else ever cleans it up.
pub fn rename_over(temp: &Path, file: &Path) -> std::io::Result<()> {
    let mut attempt = 1;
    loop {
        match std::fs::rename(temp, file) {
            Ok(()) => return Ok(()),
            Err(e) if attempt < ATTEMPTS && transient(&e) => {
                attempt += 1;
                std::thread::sleep(PAUSE);
            }
            Err(e) => {
                let _ = std::fs::remove_file(temp);
                return Err(e);
            }
        }
    }
}

#[cfg(unix)]
pub fn chmod(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
/// `fs.chmodSync`: on Windows only checks that `path` exists, as Node's call fails when it does not.
pub fn chmod(path: &Path, _mode: u32) -> std::io::Result<()> {
    // Node's chmodSync throws for a missing path on every platform; keep that.
    std::fs::metadata(path).map(|_| ())
}

/// Writes a file created with `mode` (Unix).
pub fn write_with_mode(path: &Path, data: &[u8], mode: u32) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    opts.open(path)?.write_all(data)
}

/// `mkdirSync(dir, { recursive: true, mode })`.
pub fn mkdir_all(dir: &Path, mode: u32) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    b.create(dir)
}
