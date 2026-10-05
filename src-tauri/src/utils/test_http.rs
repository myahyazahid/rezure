//! A tiny HTTP server for tests that exercise a real `reqwest` request — the
//! sticker catalog and downloads — without reaching the network.
//!
//! It answers fixed replies by path, one connection at a time, and remembers
//! the requests it got so a test can check what the client sent. Test-only:
//! it speaks just enough HTTP/1.1 for `reqwest`, and its thread lives until
//! the test process exits.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

/// What the server answers for one path.
pub struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    send_length: bool,
}

impl Reply {
    pub fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
            send_length: true,
        }
    }

    pub fn bytes(body: &[u8]) -> Self {
        Self {
            body: body.to_vec(),
            ..Self::status(200)
        }
    }

    pub fn json(body: &str) -> Self {
        Self::bytes(body.as_bytes()).with_header("Content-Type", "application/json")
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// Leaves out `Content-Length`, so the body runs until the connection
    /// closes — the case where a client can't tell the size up front and has
    /// to count the bytes itself.
    pub fn without_length(mut self) -> Self {
        self.send_length = false;
        self
    }
}

/// A request the server received, headers lower-cased.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub path: String,
    pub headers: Vec<(String, String)>,
}

pub struct TestServer {
    /// `http://127.0.0.1:<port>`, no trailing slash.
    pub base_url: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
}

impl TestServer {
    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }

    /// The named header (any case) of the most recent request.
    pub fn last_request_header(&self, name: &str) -> Option<String> {
        let name = name.to_ascii_lowercase();
        self.requests()
            .last()?
            .headers
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.clone())
    }
}

/// Starts a server on a free loopback port. A path with no route is a `404`.
pub fn serve(routes: Vec<(&'static str, Reply)>) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
    let base_url = format!("http://{}", listener.local_addr().expect("bound address"));
    let requests = Arc::new(Mutex::new(Vec::new()));

    let recorded = Arc::clone(&requests);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let Some(request) = read_request(&mut stream) else {
                continue;
            };
            let reply = routes
                .iter()
                .find(|(path, _)| *path == request.path)
                .map(|(_, reply)| reply);
            recorded.lock().unwrap().push(request);

            let _ = match reply {
                Some(reply) => write_reply(&mut stream, reply),
                None => write_reply(&mut stream, &Reply::status(404)),
            };
        }
    });

    TestServer { base_url, requests }
}

fn read_request(stream: &mut std::net::TcpStream) -> Option<Recorded> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).ok()? == 0 {
            return None;
        }
        head.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&head);
    let mut lines = text.split("\r\n");
    let path = lines
        .next()?
        .split_whitespace()
        .nth(1)?
        .split('?')
        .next()?
        .to_string();
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    Some(Recorded { path, headers })
}

fn write_reply(stream: &mut std::net::TcpStream, reply: &Reply) -> std::io::Result<()> {
    let mut out = format!("HTTP/1.1 {} Test\r\nConnection: close\r\n", reply.status);
    for (name, value) in &reply.headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    // A 304 has no body, and so no length to state.
    if reply.send_length && reply.status != 304 {
        out.push_str(&format!("Content-Length: {}\r\n", reply.body.len()));
    }
    out.push_str("\r\n");
    stream.write_all(out.as_bytes())?;
    stream.write_all(&reply.body)?;
    stream.flush()
}
