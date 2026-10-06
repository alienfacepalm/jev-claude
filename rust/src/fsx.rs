//! File helpers: modes (no-ops on Windows) and unique temporary names (SPEC 3.10).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// `<file>.<pid>.<seq>.tmp`.
pub fn temp_name(file: &Path) -> PathBuf {
    let seq = SEQ.fetch_add(1, Ordering::SeqCst);
    let mut name = file.as_os_str().to_os_string();
    name.push(format!(".{}.{seq}.tmp", std::process::id()));
    PathBuf::from(name)
}

#[cfg(unix)]
pub fn chmod(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
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
