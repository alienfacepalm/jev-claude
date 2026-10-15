//! Loopback HTTP/1.1 for the harness on plain blocking sockets: a recording server that plays the
//! fake Jev and the fake Anthropic API, and a client that sends exactly the headers it is given
//! and notes when each piece of the response arrived. Hand-written rather than built on the crate's
//! own HTTP stack, so the harness checks the programs' bytes on the wire with nothing in common.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// One request a server received, in full.
#[derive(Clone, Debug)]
pub struct Record {
    pub method: String,
    pub url: String,
    /// Every header line in arrival order, the name lowercased.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Record {
    /// A header's value, repeated ones joined with ", " as Node.js's `req.headers` joins them.
    pub fn header(&self, name: &str) -> Option<String> {
        let values: Vec<&str> = self.headers.iter().filter(|(k, _)| k == name).map(|(_, v)| v.as_str()).collect();
        (!values.is_empty()).then(|| values.join(", "))
    }

    /// The distinct header names, lowercased.
    pub fn header_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for (name, _) in &self.headers {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        names
    }

    /// The body as text.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// A start line and its header lines (names lowercased).
type Head = (String, Vec<(String, String)>);

/// Reads one header block: the start line and the header lines. `None` at a clean end of stream.
fn read_head(reader: &mut impl BufRead) -> io::Result<Option<Head>> {
    let mut start = String::new();
    loop {
        start.clear();
        if reader.read_line(&mut start)? == 0 {
            return Ok(None);
        }
        // RFC 9112 2.2: a server ignores empty lines before the request line.
        if !start.trim_end().is_empty() {
            break;
        }
    }
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the stream ended inside a header block"));
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            return Ok(Some((start.trim_end().to_string(), headers)));
        }
        let (name, value) =
            line.split_once(':').ok_or_else(|| io::Error::other(format!("bad header line {line:?}")))?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// Reads a chunked body, calling `each` with every chunk's data as it completes. Returns whether
/// the terminating chunk arrived (false when the stream ended first).
fn read_chunked(reader: &mut impl BufRead, mut each: impl FnMut(Vec<u8>)) -> io::Result<bool> {
    loop {
        let mut size_line = String::new();
        if reader.read_line(&mut size_line)? == 0 {
            return Ok(false);
        }
        let size_text = size_line.trim_end().split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| io::Error::other(format!("bad chunk size {size_line:?}")))?;
        if size == 0 {
            // Trailers, then the blank line.
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line)? == 0 || line.trim_end().is_empty() {
                    return Ok(true);
                }
            }
        }
        let mut data = vec![0; size];
        match reader.read_exact(&mut data) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
            Err(e) => return Err(e),
        }
        each(data);
        let mut crlf = String::new();
        if reader.read_line(&mut crlf)? == 0 {
            return Ok(false);
        }
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        307 => "Temporary Redirect",
        401 => "Unauthorized",
        404 => "Not Found",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        _ => "Status",
    }
}

// -------------------------------------------------------------------------------------------------
// Server

#[derive(PartialEq)]
enum State {
    Fresh,
    Streaming,
    Done,
    Broken,
}

/// The response to one request, written by a server's handler.
pub struct Resp {
    stream: TcpStream,
    state: State,
}

impl Resp {
    fn head(&mut self, status: u16, headers: &[(&str, &str)], framing: &str) -> bool {
        let mut text = format!("HTTP/1.1 {status} {}\r\n", reason(status));
        for (name, value) in headers {
            let _ = write!(text, "{name}: {value}\r\n");
        }
        text.push_str(framing);
        text.push_str("\r\n");
        self.put(text.as_bytes())
    }

    fn put(&mut self, bytes: &[u8]) -> bool {
        if self.state == State::Broken {
            return false;
        }
        let ok = self.stream.write_all(bytes).and_then(|()| self.stream.flush()).is_ok();
        if !ok {
            self.state = State::Broken;
        }
        ok
    }

    /// A whole response with a `content-length`.
    pub fn reply(&mut self, status: u16, headers: &[(&str, &str)], body: &[u8]) {
        if self.head(status, headers, &format!("content-length: {}\r\n", body.len())) && self.put(body) {
            self.state = State::Done;
        }
    }

    /// Starts a chunked response, as Node.js's `res.writeHead` without a length does.
    pub fn start(&mut self, status: u16, headers: &[(&str, &str)]) {
        if self.head(status, headers, "transfer-encoding: chunked\r\n") {
            self.state = State::Streaming;
        }
    }

    /// Sends one chunk; false once the peer has gone (a write failed).
    pub fn write(&mut self, data: &[u8]) -> bool {
        let mut chunk = format!("{:x}\r\n", data.len()).into_bytes();
        chunk.extend_from_slice(data);
        chunk.extend_from_slice(b"\r\n");
        self.put(&chunk)
    }

    /// Ends a chunked response.
    pub fn end(&mut self) {
        if self.put(b"0\r\n\r\n") {
            self.state = State::Done;
        }
    }

    /// Drops the connection where it stands, as Node.js's `socket.destroy()` does.
    pub fn destroy(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
        self.state = State::Broken;
    }
}

/// What a server does with each request.
pub type Handler = Arc<dyn Fn(&Record, &mut Resp) + Send + Sync>;

/// A loopback server recording every request in full before handing it to its handler.
pub struct Server {
    pub port: u16,
    /// `http://127.0.0.1:<port>`.
    pub url: String,
    log: Arc<Mutex<Vec<Record>>>,
    stop: Arc<AtomicBool>,
    conns: Arc<Mutex<Vec<TcpStream>>>,
    accept: Option<JoinHandle<()>>,
}

impl Server {
    pub fn start(handler: Handler) -> Server {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind a loopback port");
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let log = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let conns = Arc::new(Mutex::new(Vec::new()));
        let (log2, stop2, conns2) = (log.clone(), stop.clone(), conns.clone());
        let accept = std::thread::spawn(move || {
            // The listener closes when this loop ends, so a closed server refuses connections.
            while !stop2.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_nodelay(true);
                        if let Ok(copy) = stream.try_clone() {
                            conns2.lock().unwrap().push(copy);
                        }
                        let (handler, log) = (handler.clone(), log2.clone());
                        std::thread::spawn(move || serve_connection(stream, &handler, &log));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(2)),
                }
            }
        });
        Server { port, url: format!("http://127.0.0.1:{port}"), log, stop, conns, accept: Some(accept) }
    }

    /// Every request so far, in arrival order.
    pub fn requests(&self) -> Vec<Record> {
        self.log.lock().unwrap().clone()
    }

    /// Request `i`, which must have arrived.
    pub fn request(&self, i: usize) -> Record {
        let log = self.log.lock().unwrap();
        log.get(i).cloned().unwrap_or_else(|| panic!("request {i} never arrived ({} did)", log.len()))
    }

    /// The `host` header naming this server (`127.0.0.1:<port>`).
    pub fn host(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    /// Stops listening and drops every open connection.
    pub fn close(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        for conn in self.conns.lock().unwrap().drain(..) {
            let _ = conn.shutdown(Shutdown::Both);
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.close();
    }
}

fn serve_connection(stream: TcpStream, handler: &Handler, log: &Mutex<Vec<Record>>) {
    let Ok(write_half) = stream.try_clone() else { return };
    let mut reader = BufReader::new(stream);
    loop {
        let Ok(Some((start, headers))) = read_head(&mut reader) else { return };
        let mut parts = start.split(' ');
        let method = parts.next().unwrap_or("").to_string();
        let url = parts.next().unwrap_or("").to_string();
        let mut body = Vec::new();
        if header(&headers, "transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked")) {
            match read_chunked(&mut reader, |data| body.extend_from_slice(&data)) {
                Ok(true) => {}
                _ => return,
            }
        } else if let Some(length) = header(&headers, "content-length").and_then(|v| v.parse::<usize>().ok()) {
            body.resize(length, 0);
            if reader.read_exact(&mut body).is_err() {
                return;
            }
        }
        let close = header(&headers, "connection").is_some_and(|v| v.eq_ignore_ascii_case("close"));
        let record = Record { method, url, headers, body };
        log.lock().unwrap().push(record.clone());
        let Ok(stream) = write_half.try_clone() else { return };
        let mut resp = Resp { stream, state: State::Fresh };
        handler(&record, &mut resp);
        if resp.state == State::Fresh {
            resp.reply(500, &[], b"the harness handler sent no response");
        }
        if resp.state != State::Done || close {
            let _ = write_half.shutdown(Shutdown::Both);
            return;
        }
    }
}

// -------------------------------------------------------------------------------------------------
// Client

/// A response the client read.
pub struct Response {
    pub status: u16,
    /// Lowercased names, repeated values joined with ", ".
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    /// Each piece of the body with the moment it arrived.
    pub chunks: Vec<(Instant, Vec<u8>)>,
    /// The headers this request was sent with (what Node.js's harness called `sent`).
    pub sent: Vec<(String, String)>,
    /// Whether the body ended where its framing said it would.
    pub complete: bool,
}

impl Response {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }
}

/// Why a request got no response.
#[derive(Debug)]
pub struct ClientError {
    pub timed_out: bool,
    pub message: String,
}

impl ClientError {
    fn from_io(e: &io::Error) -> ClientError {
        let timed_out = matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut);
        ClientError { timed_out, message: e.to_string() }
    }
}

/// An open request: the headers are written and the response is read on demand.
pub struct Conn {
    reader: BufReader<TcpStream>,
    method: String,
    pub sent: Vec<(String, String)>,
}

/// Opens a connection and sends one request with exactly `headers`, plus `content-length` for a
/// body and the `host` and `connection: close` that Node.js's `http.request` (no agent) adds.
/// `idle` limits each wait for the response.
pub fn open(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
    idle: Duration,
) -> Conn {
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap_or_else(|e| panic!("connect to 127.0.0.1:{port}: {e}"));
    stream.set_read_timeout(Some(idle)).unwrap();
    let _ = stream.set_nodelay(true);
    let mut sent: Vec<(String, String)> = headers.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect();
    if let Some(body) = body {
        sent.push(("content-length".into(), body.len().to_string()));
    }
    let mut text = format!("{method} {path} HTTP/1.1\r\n");
    for (name, value) in &sent {
        let _ = write!(text, "{name}: {value}\r\n");
    }
    let _ = write!(text, "host: 127.0.0.1:{port}\r\nconnection: close\r\n\r\n");
    let mut bytes = text.into_bytes();
    bytes.extend_from_slice(body.unwrap_or_default());
    (&stream).write_all(&bytes).unwrap_or_else(|e| panic!("send to 127.0.0.1:{port}: {e}"));
    Conn { reader: BufReader::new(stream), method: method.to_string(), sent }
}

impl Conn {
    /// Reads the status line and headers.
    pub fn read_head(&mut self) -> Result<(u16, BTreeMap<String, String>), ClientError> {
        loop {
            let head = read_head(&mut self.reader).map_err(|e| ClientError::from_io(&e))?;
            let Some((start, lines)) = head else {
                return Err(ClientError {
                    timed_out: false,
                    message: "the connection closed before a response".into(),
                });
            };
            let status = start
                .split(' ')
                .nth(1)
                .and_then(|s| s.parse::<u16>().ok())
                .ok_or_else(|| ClientError { timed_out: false, message: format!("bad status line {start:?}") })?;
            if (100..200).contains(&status) {
                continue;
            }
            let mut headers: BTreeMap<String, String> = BTreeMap::new();
            for (name, value) in lines {
                headers.entry(name).and_modify(|v| *v = format!("{v}, {value}")).or_insert(value);
            }
            return Ok((status, headers));
        }
    }

    /// Reads whatever body bytes arrive next (empty at the end of the stream).
    pub fn read_some(&mut self) -> io::Result<Vec<u8>> {
        let buffer = self.reader.fill_buf()?.to_vec();
        self.reader.consume(buffer.len());
        Ok(buffer)
    }

    /// Reads the whole response.
    pub fn finish(mut self) -> Result<Response, ClientError> {
        let (status, headers) = self.read_head()?;
        let mut chunks: Vec<(Instant, Vec<u8>)> = Vec::new();
        let no_body = self.method == "HEAD" || status == 204 || status == 304;
        let chunked = headers.get("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
        let complete = if no_body {
            true
        } else if chunked {
            match read_chunked(&mut self.reader, |data| chunks.push((Instant::now(), data))) {
                Ok(done) => done,
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                    return Err(ClientError::from_io(&e));
                }
                Err(_) => false,
            }
        } else {
            let length = headers.get("content-length").and_then(|v| v.parse::<usize>().ok());
            let mut got = 0;
            loop {
                if length.is_some_and(|n| got >= n) {
                    break true;
                }
                match self.read_some() {
                    Ok(data) if data.is_empty() => break length.is_none(),
                    Ok(data) => {
                        got += data.len();
                        chunks.push((Instant::now(), data));
                    }
                    Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                        return Err(ClientError::from_io(&e));
                    }
                    Err(_) => break false,
                }
            }
        };
        let body = chunks.iter().flat_map(|(_, data)| data.iter().copied()).collect();
        Ok(Response { status, headers, body, chunks, sent: self.sent, complete })
    }

    /// Drops the connection, as a client that gives up does.
    pub fn abort(self) {
        let _ = self.reader.get_ref().shutdown(Shutdown::Both);
    }
}
