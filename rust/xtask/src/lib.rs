//! Developer and CI tasks for jev-claude, run as `cargo xtask <command>` from `rust/`. Nothing here
//! is shipped: the release holds only the seven programs of the `jev-router` crate.
//!
//! - [`bump`]: the next release version, from the commits being released (the CI `release` job).
//! - [`package`]: the release archive the installers download (the CI `binaries` job, and
//!   `rust/tests/bundle.rs`).
//! - [`smoke`]: the real installers run against a release archive in a throwaway home (the CI
//!   `installers` and `binaries` jobs). Its stand-in programs (Claude Code, a `jev-check` that
//!   fails) are copies of this program: see `main.rs`.

pub mod bump;
pub mod package;
pub mod smoke;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The repository root: the folder holding `rust/`, `install.sh` and the root `package.json`.
pub fn repo_root() -> PathBuf {
    let xtask = Path::new(env!("CARGO_MANIFEST_DIR"));
    xtask.parent().and_then(Path::parent).expect("xtask lives in rust/xtask").to_path_buf()
}

/// A path given on the command line, made absolute against the current directory.
pub fn absolute(path: &Path) -> Result<PathBuf, String> {
    std::path::absolute(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// A fresh, empty directory under the system temp directory, removed with everything in it when
/// the guard drops.
pub struct TempDir(PathBuf);

impl TempDir {
    /// A new directory whose name starts with `prefix`.
    pub fn new(prefix: &str) -> Result<TempDir, String> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        for _ in 0..100 {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("{prefix}{}-{nanos}-{n}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(TempDir(dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(format!("cannot create {}: {e}", dir.display())),
            }
        }
        Err(format!("cannot create a temporary directory named {prefix}*"))
    }

    /// The directory's path.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
