//! HTTP/1.1 and child-process helpers for the real full-match test.
//!
//! The test is a client: it spawns the real `football-sim serve` process and
//! speaks HTTP over a loopback socket. Keeping that plumbing here leaves the
//! test itself focused on the match contract.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// The workspace root, so the spawned server finds `./fixtures`.
pub fn repository_root(match_id: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root exists");
    assert!(
        root.join("fixtures/matches")
            .join(format!("{match_id}.toml"))
            .is_file(),
        "the committed match fixture must exist under {}",
        root.display()
    );
    root
}

/// A port on the loopback interface that no process is using right now.
pub fn free_local_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is available");
    let port = listener
        .local_addr()
        .expect("the bound listener has a local address")
        .port();
    drop(listener);
    port
}

/// A spawned `football-sim serve` process, killed when it leaves scope.
pub struct ServerProcess {
    child: Child,
    port: u16,
}

impl ServerProcess {
    /// Spawn the real serve command on `port` and wait for it to listen.
    pub fn start(root: &Path, port: u16) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_football-sim"))
            .arg("serve")
            .arg("--bind")
            .arg(format!("127.0.0.1:{port}"))
            .current_dir(root)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the football-sim binary can be spawned");
        let mut server = Self { child, port };
        server.wait_until_listening();
        server
    }

    /// Wait until the server accepts a loopback connection.
    fn wait_until_listening(&mut self) {
        let address = format!("127.0.0.1:{}", self.port);
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Some(status) = self
                .child
                .try_wait()
                .expect("the server process status is readable")
            {
                panic!("the server exited before listening: {status}");
            }
            if TcpStream::connect_timeout(
                &address.parse().expect("invariant: a loopback address parses"),
                Duration::from_millis(100),
            )
            .is_ok()
            {
                return;
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!("the server did not listen on {address} within 10 s");
    }

    /// Stop the server and reap it.
    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One HTTP response: status, headers and body.
pub struct HttpResponse {
    /// The status code.
    pub status: u16,
    /// The headers, names lowercased.
    headers: Vec<(String, String)>,
    /// The body after any transfer coding is removed.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// The value of `name`, if the response carries it.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header == name)
            .map(|(_, value)| value.as_str())
    }

    /// The body as text, for failure messages.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Send one HTTP/1.1 request to the local server and read the response.
pub fn request(
    port: u16,
    method: &str,
    path: &str,
    accept_gzip: bool,
) -> std::io::Result<HttpResponse> {
    let mut stream = TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}")
            .parse()
            .expect("invariant: a loopback address parses"),
        Duration::from_secs(5),
    )?;
    stream.set_read_timeout(Some(Duration::from_mins(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;

    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n"
    );
    if accept_gzip {
        head.push_str("Accept-Encoding: gzip\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse_response(raw)
}

/// Parse a complete HTTP/1.1 response, removing chunked transfer coding.
fn parse_response(raw: Vec<u8>) -> std::io::Result<HttpResponse> {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| invalid_data("the response has no header terminator"))?;
    let head = std::str::from_utf8(&raw[..split])
        .map_err(|error| invalid_data(&format!("the response headers are not UTF-8: {error}")))?;
    let mut lines = head.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| invalid_data("the response has no status line"))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| invalid_data("the status line has no code"))?
        .parse()
        .map_err(|error| invalid_data(&format!("the status code is not a number: {error}")))?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    let body = raw[split + 4..].to_vec();
    let chunked = headers.iter().any(|(name, value)| {
        name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked")
    });
    let body = if chunked { dechunk(&body)? } else { body };
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

/// Decode an HTTP/1.1 chunked body.
fn dechunk(mut input: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut decoded = Vec::new();
    loop {
        let line_end = input
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| invalid_data("a chunk line has no terminator"))?;
        let size_text = std::str::from_utf8(&input[..line_end])
            .map_err(|error| invalid_data(&format!("a chunk size is not UTF-8: {error}")))?;
        let size_text = size_text.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|error| invalid_data(&format!("a chunk size is not hex: {error}")))?;
        input = &input[line_end + 2..];
        if size == 0 {
            return Ok(decoded);
        }
        if input.len() < size + 2 {
            return Err(invalid_data("a chunk is shorter than its declared size"));
        }
        decoded.extend_from_slice(&input[..size]);
        input = &input[size + 2..];
    }
}

/// An invalid test-server response.
fn invalid_data(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}
