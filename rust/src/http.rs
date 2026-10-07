//! HTTP/1.1 plumbing shared by the proxy and the Jev client: URL parsing, plain and TLS
//! connections, one hyper client connection per request, and a streaming body.

use bytes_compat::Bytes;
use http_body_util::BodyExt;
use hyper::body::{Body, Frame, Incoming};
use hyper::client::conn::http1::SendRequest;
use hyper_util::rt::TokioIo;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};
use std::task::{Context, Poll};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

/// Re-export of the bytes type hyper uses, without a direct dependency on the `bytes` crate.
pub mod bytes_compat {
    pub use hyper::body::Bytes;
}

/// The error type for bodies and connections.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// The parts of a URL the proxy and the client use (Node's `new URL`).
#[derive(Debug, Clone, PartialEq)]
pub struct Url {
    /// Whether the scheme is `https:`.
    pub https: bool,
    /// Host name without the port.
    pub hostname: String,
    /// Explicit port, if the URL names one.
    pub port: Option<u16>,
    /// Path, `/` when the URL has none.
    pub pathname: String,
}

impl Url {
    /// `http(s)://host[:port][/path]`; the default port is dropped, as `new URL` drops it.
    pub fn parse(text: &str) -> Result<Url, String> {
        let text = text.trim();
        let (https, rest) = if let Some(r) = strip_prefix_ci(text, "https://") {
            (true, r)
        } else if let Some(r) = strip_prefix_ci(text, "http://") {
            (false, r)
        } else {
            return Err(format!("Invalid URL: {text}"));
        };
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..end];
        let path_part = &rest[end..];
        let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let (hostname, port) = if let Some(stripped) = authority.strip_prefix('[') {
            let close = stripped.find(']').ok_or("Invalid URL")?;
            let host = format!("[{}]", &stripped[..close]);
            let after = &stripped[close + 1..];
            (host, after.strip_prefix(':'))
        } else {
            match authority.rsplit_once(':') {
                Some((h, p)) => (h.to_string(), Some(p)),
                None => (authority.to_string(), None),
            }
        };
        if hostname.is_empty() {
            return Err("Invalid URL".into());
        }
        let port = match port {
            None | Some("") => None,
            Some(p) => Some(p.parse::<u16>().map_err(|_| "Invalid URL")?),
        };
        let default = if https { 443 } else { 80 };
        let port = port.filter(|p| *p != default);
        let path = path_part.split(['?', '#']).next().unwrap_or("");
        let pathname = if path.is_empty() { "/".to_string() } else { path.to_string() };
        Ok(Url { https, hostname: hostname.to_ascii_lowercase(), port, pathname })
    }

    /// `url.host`: the hostname, with the port only when it is not the default.
    pub fn host(&self) -> String {
        match self.port {
            Some(p) => format!("{}:{p}", self.hostname),
            None => self.hostname.clone(),
        }
    }

    /// The explicit port, or the scheme's default (443 or 80).
    pub fn port_or_default(&self) -> u16 {
        self.port.unwrap_or(if self.https { 443 } else { 80 })
    }

    /// `url.origin`.
    pub fn origin(&self) -> String {
        format!("{}://{}", if self.https { "https" } else { "http" }, self.host())
    }
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    (s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix)).then(|| &s[prefix.len()..])
}

static TLS: LazyLock<Arc<rustls::ClientConfig>> = LazyLock::new(|| {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("default TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    Arc::new(config)
});

/// A request body: a single buffer, or nothing.
pub type ReqBody = http_body_util::Full<Bytes>;

/// Opens an HTTP/1.1 client connection to `url`'s origin and drives it in the background.
pub async fn connect(url: &Url) -> Result<SendRequest<ReqBody>, BoxError> {
    let host = url.hostname.trim_start_matches('[').trim_end_matches(']').to_string();
    let tcp = TcpStream::connect((host.as_str(), url.port_or_default())).await?;
    let _ = tcp.set_nodelay(true);
    if url.https {
        let name = rustls::pki_types::ServerName::try_from(host)?;
        let tls = tokio_rustls::TlsConnector::from(TLS.clone()).connect(name, tcp).await?;
        let (send, conn) =
            hyper::client::conn::http1::Builder::new().handshake::<_, ReqBody>(TokioIo::new(tls)).await?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        Ok(send)
    } else {
        let (send, conn) = hyper::client::conn::http1::handshake::<_, ReqBody>(TokioIo::new(tcp)).await?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        Ok(send)
    }
}

/// Reads a whole body.
pub async fn collect(body: Incoming) -> Result<Bytes, BoxError> {
    Ok(body.collect().await?.to_bytes())
}

/// A response body fed chunk by chunk from a channel; an `Err` item aborts the response.
pub struct ChannelBody {
    rx: mpsc::Receiver<Result<Bytes, BoxError>>,
}

impl ChannelBody {
    /// A body with room for `capacity` chunks, and the sender that feeds it.
    pub fn new(capacity: usize) -> (mpsc::Sender<Result<Bytes, BoxError>>, ChannelBody) {
        let (tx, rx) = mpsc::channel(capacity);
        (tx, ChannelBody { rx })
    }
}

impl Body for ChannelBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(Ok(b))) => Poll::Ready(Some(Ok(Frame::data(b)))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// The body every proxy response uses: buffered or streamed.
pub enum ProxyBody {
    /// A complete body (None for an empty one).
    Full(Option<Bytes>),
    /// A body streamed from a channel.
    Stream(ChannelBody),
}

impl Body for ProxyBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        match self.get_mut() {
            ProxyBody::Full(b) => Poll::Ready(b.take().filter(|b| !b.is_empty()).map(|b| Ok(Frame::data(b)))),
            ProxyBody::Stream(s) => Pin::new(s).poll_frame(cx),
        }
    }

    fn is_end_stream(&self) -> bool {
        matches!(self, ProxyBody::Full(None))
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        match self {
            ProxyBody::Full(Some(b)) => hyper::body::SizeHint::with_exact(b.len() as u64),
            ProxyBody::Full(None) => hyper::body::SizeHint::with_exact(0),
            ProxyBody::Stream(_) => hyper::body::SizeHint::default(),
        }
    }
}
