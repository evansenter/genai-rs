//! A local HTTP stub for offline tests, via `ClientBuilder::with_base_url`.
//!
//! A few dozen lines of tokio rather than a mock-server crate: it only needs
//! to record requests and replay canned responses. Shared by
//! `http_mock_tests.rs` and `http_mock_resources.rs`.

// Each test binary compiles `common` separately and uses a subset of it.
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use genai_rs::Client;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// One request as the stub received it.
#[derive(Clone, Debug)]
pub struct Recorded {
    pub method: String,
    /// Path and query, e.g. `/v1beta/interactions?alt=sse`.
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("request body should be JSON")
    }
}

/// A canned response. Body parts are written in order, each after its delay,
/// and the connection is closed afterwards, so a body without a
/// `Content-Length` ends at EOF.
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub delay: Duration,
    pub parts: Vec<(Duration, Vec<u8>)>,
    pub content_length: bool,
}

impl Reply {
    pub fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            headers: vec![("content-type".into(), "application/json".into())],
            delay: Duration::ZERO,
            parts: vec![(Duration::ZERO, body.to_string().into_bytes())],
            content_length: true,
        }
    }

    pub fn text(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: vec![("content-type".into(), "text/html".into())],
            delay: Duration::ZERO,
            parts: vec![(Duration::ZERO, body.as_bytes().to_vec())],
            content_length: true,
        }
    }

    /// A close-delimited `text/event-stream` body, sent as the given chunks.
    pub fn sse(chunks: &[&str]) -> Self {
        Self {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
            delay: Duration::ZERO,
            parts: chunks
                .iter()
                .map(|c| (Duration::from_millis(5), c.as_bytes().to_vec()))
                .collect(),
            content_length: false,
        }
    }

    /// `{base}` in a value is replaced with the stub's base URL.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn delayed(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

type Handler = dyn Fn(&Recorded, usize) -> Reply + Send + Sync;

pub struct Stub {
    pub base_url: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
}

impl Stub {
    /// Starts a stub answering request `n` (0-based) with `handler(req, n)`.
    pub async fn start(
        handler: impl Fn(&Recorded, usize) -> Reply + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler: Arc<Handler> = Arc::new(handler);
        let counter = Arc::new(AtomicUsize::new(0));

        let (task_requests, task_base) = (requests.clone(), base_url.clone());
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let (requests, handler, base, counter) = (
                    task_requests.clone(),
                    handler.clone(),
                    task_base.clone(),
                    counter.clone(),
                );
                tokio::spawn(async move {
                    serve(socket, requests, handler, base, counter).await;
                });
            }
        });

        Self { base_url, requests }
    }

    /// Answers requests with `replies` in order; any extra request gets a 500.
    pub async fn replying(replies: Vec<Reply>) -> Self {
        let replies = Mutex::new(replies.into_iter().map(Some).collect::<Vec<_>>());
        Self::start(move |_, n| {
            replies
                .lock()
                .unwrap()
                .get_mut(n)
                .and_then(Option::take)
                .unwrap_or_else(|| Reply::text(500, "unexpected request"))
        })
        .await
    }

    pub fn client(&self) -> Client {
        Client::builder("test-key".to_string())
            .with_base_url(&self.base_url)
            .build()
            .unwrap()
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }
}

async fn serve(
    mut socket: TcpStream,
    requests: Arc<Mutex<Vec<Recorded>>>,
    handler: Arc<Handler>,
    base: String,
    counter: Arc<AtomicUsize>,
) {
    let Some(request) = read_request(&mut socket).await else {
        return;
    };
    let n = counter.fetch_add(1, Ordering::SeqCst);
    requests.lock().unwrap().push(request.clone());
    let reply = handler(&request, n);

    tokio::time::sleep(reply.delay).await;
    let mut head = format!("HTTP/1.1 {} Stub\r\nconnection: close\r\n", reply.status);
    for (name, value) in &reply.headers {
        head.push_str(&format!("{name}: {}\r\n", value.replace("{base}", &base)));
    }
    if reply.content_length {
        let len: usize = reply.parts.iter().map(|(_, p)| p.len()).sum();
        head.push_str(&format!("content-length: {len}\r\n"));
    }
    head.push_str("\r\n");
    if socket.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    for (delay, part) in reply.parts {
        tokio::time::sleep(delay).await;
        if socket.write_all(&part).await.is_err() || socket.flush().await.is_err() {
            return;
        }
    }
    let _ = socket.shutdown().await;
}

async fn read_request(socket: &mut TcpStream) -> Option<Recorded> {
    let mut buffer = Vec::new();
    let head_end = loop {
        if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        let mut chunk = [0u8; 4096];
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..n]);
    };

    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_string();
    let target = request_line.next()?.to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();

    let mut body = buffer[head_end + 4..].to_vec();
    let header = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v);
    if let Some(len) = header("content-length").and_then(|v| v.parse::<usize>().ok()) {
        while body.len() < len {
            let mut chunk = [0u8; 65536];
            let n = socket.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
    } else if header("transfer-encoding").is_some_and(|v| v.contains("chunked")) {
        while !body.ends_with(b"0\r\n\r\n") {
            let mut chunk = [0u8; 65536];
            let n = socket.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        body = decode_chunked(&body);
    }

    Some(Recorded {
        method,
        target,
        headers,
        body,
    })
}

fn decode_chunked(mut raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(pos) = raw.windows(2).position(|w| w == b"\r\n") {
        let size =
            usize::from_str_radix(std::str::from_utf8(&raw[..pos]).unwrap_or("0"), 16).unwrap_or(0);
        if size == 0 {
            break;
        }
        out.extend_from_slice(&raw[pos + 2..pos + 2 + size]);
        raw = &raw[pos + 2 + size + 2..];
    }
    out
}
