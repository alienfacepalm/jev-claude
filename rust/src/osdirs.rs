//! Node's `os.homedir()` and `os.tmpdir()` (SPEC 3.8). Platform helpers such as
//! `std::env::temp_dir` follow different rules and are not used.

use crate::envx::{Env, ProcessEnv};
use std::path::PathBuf;

fn non_empty(env: &dyn Env, key: &str) -> Option<String> {
    env.get(key).filter(|v| !v.is_empty())
}

/// `os.homedir()`, reading the given environment.
pub fn home_dir_in(env: &dyn Env) -> PathBuf {
    if cfg!(windows) {
        if let Some(p) = non_empty(env, "USERPROFILE") {
            return PathBuf::from(p);
        }
        return os_profile_dir().unwrap_or_default();
    }
    if let Some(p) = non_empty(env, "HOME") {
        return PathBuf::from(p);
    }
    os_profile_dir().unwrap_or_default()
}

/// `os.homedir()` for this process.
pub fn home_dir() -> PathBuf {
    home_dir_in(&ProcessEnv)
}

/// `os.tmpdir()`, reading the given environment.
pub fn temp_dir_in(env: &dyn Env) -> PathBuf {
    if cfg!(windows) {
        let mut path = non_empty(env, "TEMP")
            .or_else(|| non_empty(env, "TMP"))
            .or_else(|| non_empty(env, "SystemRoot").map(|r| format!("{r}\\temp")))
            .or_else(|| non_empty(env, "windir").map(|r| format!("{r}\\temp")))
            .unwrap_or_default();
        let drive_root = path.len() == 3 && path.as_bytes()[1] == b':' && path.ends_with('\\');
        if path.len() > 1 && path.ends_with('\\') && !drive_root {
            path.pop();
        }
        return PathBuf::from(path);
    }
    let mut path = non_empty(env, "TMPDIR")
        .or_else(|| non_empty(env, "TMP"))
        .or_else(|| non_empty(env, "TEMP"))
        .unwrap_or_else(|| "/tmp".to_string());
    if path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    PathBuf::from(path)
}

/// `os.tmpdir()` for this process.
pub fn temp_dir() -> PathBuf {
    temp_dir_in(&ProcessEnv)
}

/// Removes the `\\?\` prefix `canonicalize` adds on Windows, so paths print as Node prints them.
pub fn simplify(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path
}

#[cfg(windows)]
#[allow(non_snake_case)]
fn os_profile_dir() -> Option<PathBuf> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStringExt;
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    #[link(name = "userenv")]
    unsafe extern "system" {
        fn GetUserProfileDirectoryW(token: *mut c_void, dir: *mut u16, size: *mut u32) -> i32;
    }
    const TOKEN_QUERY: u32 = 0x0008;
    // SAFETY: plain Win32 calls with valid out-pointers; the token handle is closed after use.
    unsafe {
        let mut token: *mut c_void = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut buf = vec![0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = GetUserProfileDirectoryW(token, buf.as_mut_ptr(), &mut size);
        CloseHandle(token);
        if ok == 0 {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(PathBuf::from(std::ffi::OsString::from_wide(&buf[..len])))
    }
}

#[cfg(unix)]
fn os_profile_dir() -> Option<PathBuf> {
    unsafe extern "C" {
        fn getuid() -> u32;
    }
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { getuid() };
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|line| {
        let f: Vec<&str> = line.split(':').collect();
        (f.len() >= 6 && f[2].parse::<u32>().ok() == Some(uid)).then(|| PathBuf::from(f[5]))
    })
}

#[cfg(not(any(unix, windows)))]
fn os_profile_dir() -> Option<PathBuf> {
    None
}
