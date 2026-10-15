//! The local stand-in for GitHub's release downloads and Anthropic's installer address.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A running server: its base URL, and every request line it has answered.
#[derive(Clone)]
pub struct Server {
    /// `http://127.0.0.1:<port>`.
    pub url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Server {
    /// How many requests have been answered so far.
    pub fn count(&self) -> usize {
        self.requests.lock().map_or(0, |r| r.len())
    }

    /// The request lines answered after the first `since` of them.
    pub fn since(&self, since: usize) -> Vec<String> {
        self.requests.lock().map(|r| r.iter().skip(since).cloned().collect()).unwrap_or_default()
    }
}

/// Serves `root/<path>` on 127.0.0.1 from a thread of its own and logs nothing to the terminal.
///
/// Two first path segments are special: `status500/...` answers every request with HTTP 500, and
/// `truncated/<path>` serves `good/<path>` with a content-length 100000 bytes longer than the body
/// and then closes the connection, as a transfer that breaks off does.
pub fn serve(root: PathBuf) -> Result<Server, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("cannot listen on 127.0.0.1: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let root = Arc::new(root);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&requests);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let root = Arc::clone(&root);
            let log = Arc::clone(&log);
            std::thread::spawn(move || answer(stream, &root, &log));
        }
    });
    Ok(Server { url: format!("http://127.0.0.1:{port}"), requests })
}

/// An address on this machine where nothing is listening: the port is bound, noted, and released.
pub fn closed_port_url() -> Result<String, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("cannot listen on 127.0.0.1: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    drop(listener);
    Ok(format!("http://127.0.0.1:{port}"))
}

/// Answers one request: the file under `root` for a GET or HEAD of its path, 404 otherwise.
fn answer(mut stream: TcpStream, root: &Path, log: &Mutex<Vec<String>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let mut head = Vec::new();
    let mut buf = [0u8; 4096];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 65536 {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    let head = String::from_utf8_lossy(&head);
    let request_line = head.lines().next().unwrap_or_default().to_string();
    let mut words = request_line.split(' ');
    let (method, target) = (words.next().unwrap_or_default(), words.next().unwrap_or_default());
    if let Ok(mut log) = log.lock() {
        log.push(request_line.clone());
    }
    let path = target.split(['?', '#']).next().unwrap_or_default();
    // Plain segments only: nothing above root can be asked for.
    let segments: Option<Vec<&str>> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| (s != "." && s != ".." && !s.contains('\\')).then_some(s))
        .collect();
    let Some(mut segments) = segments else {
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 9\r\nconnection: close\r\n\r\nnot found");
        return;
    };
    let mut truncated = false;
    match segments.first().copied() {
        Some("status500") => {
            let _ = stream.write_all(
                b"HTTP/1.1 500 Internal Server Error\r\ncontent-length: 5\r\nconnection: close\r\n\r\nerror",
            );
            return;
        }
        Some("truncated") => {
            segments[0] = "good";
            truncated = true;
        }
        _ => {}
    }
    let file = segments.iter().fold(root.to_path_buf(), |file, s| file.join(s));
    let body = Some(file).filter(|f| f.is_file()).and_then(|f| std::fs::read(f).ok());
    let _ = match body {
        Some(body) => {
            let length = body.len() + if truncated { 100_000 } else { 0 };
            let _ = write!(stream, "HTTP/1.1 200 OK\r\ncontent-length: {length}\r\nconnection: close\r\n\r\n");
            if method == "HEAD" { Ok(()) } else { stream.write_all(&body) }
        }
        None => stream.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 9\r\nconnection: close\r\n\r\nnot found"),
    };
    let _ = stream.flush();
}
