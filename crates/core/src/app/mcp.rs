//! The MCP endpoint of the app (spec 4.4): one HTTP server on `127.0.0.1` and a free port.
//!
//! It is hand-written, with `std::net` and threads, because the core has no async runtime. It
//! speaks the part of MCP "Streamable HTTP" that both CLIs need, with JSON answers and no SSE.
//! `probes/mini_mcp.py` is the Python test server that both CLIs accepted at gate 1, and this
//! file follows its protocol handling: `initialize`, `notifications/initialized`, `tools/list`,
//! `tools/call` and `ping`. Any other method gets the JSON-RPC error -32601 (Claude sends
//! `server/discover` first and accepts that answer).
//!
//! # Threads
//!
//! One accept thread, and one thread for each connection. A connection thread reads one request
//! at a time (HTTP keep-alive). It holds a `Weak<TowerCore>` and upgrades it for the time of one
//! request, so these threads never keep a dropped `Tower` alive. `shutdown` connects to the port
//! one time to end the accept thread.
//!
//! # Limits (`McpLimits`)
//!
//! - At most `MAX_CONNECTIONS` connections are open. Over that, the accept thread answers 503 and
//!   closes the new connection.
//! - A request (the head and the body together) must be complete `REQUEST_DEADLINE` after its
//!   first byte. It is one deadline for the whole request, not a time for each read, so a client
//!   that sends one byte at a time cannot keep a thread. A NEW connection must send its first
//!   byte within `FIRST_BYTE_TIMEOUT`, so idle connections cannot fill the cap of
//!   `MAX_CONNECTIONS`. After its first request, a connection with no request is closed after
//!   `IDLE_TIMEOUT` (the CLIs keep one connection for many calls).
//! - The token is checked after the head and BEFORE the body is read. A request with a wrong
//!   token gets 401 and costs the server no body read.
//!
//! # Identity
//!
//! Each request needs `Authorization: Bearer <token>`. The token is the secret that the app made
//! for one session (`bus::new_token`). The map `TowerCore::tokens` gives its [`Identity`]: the
//! floor, the employee slug and the app session ID. No token, a wrong token, a token of a session
//! that has ended: HTTP 401, and the connection closes. The tools get the identity from the
//! token only. No argument of a tool can name another floor or another caller.
//!
//! The token is never written to a log, an error text or a response by this file.

use super::bus::{Claim, Identity, tool_definitions};
use super::{TowerCore, locked};
use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::thread;
use std::time::{Duration, Instant};

/// The newest protocol version that the server names when the client names none.
const DEFAULT_PROTOCOL: &str = "2025-06-18";

/// The longest request body. A message has 20000 characters at most, so this is generous.
const MAX_BODY: usize = 2 * 1024 * 1024;

/// The longest request line or header line, and the most header lines.
const MAX_LINE: usize = 16 * 1024;
const MAX_HEADERS: usize = 100;

/// A connection that has done a request, and then sends nothing for this long, is closed. The
/// client connects again.
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// A new connection must send its first byte within this time, or it is closed. A client sends
/// its first request at once after it connects, so 5 s is generous. Without this limit, 256
/// idle connections would hold all places for `IDLE_TIMEOUT`.
pub const FIRST_BYTE_TIMEOUT: Duration = Duration::from_secs(5);

/// The most open connections. A new one over this gets HTTP 503.
pub const MAX_CONNECTIONS: usize = 256;

/// The time for one full request (head and body), counted from its first byte.
pub const REQUEST_DEADLINE: Duration = Duration::from_secs(30);

/// The limits of the endpoint. The default is the production value. Tests give small ones.
#[derive(Debug, Clone, Copy)]
pub struct McpLimits {
    pub max_connections: usize,
    pub request_deadline: Duration,
    /// How long a new connection may stay silent. See `FIRST_BYTE_TIMEOUT`.
    pub first_byte_timeout: Duration,
}

impl Default for McpLimits {
    fn default() -> McpLimits {
        McpLimits {
            max_connections: MAX_CONNECTIONS,
            request_deadline: REQUEST_DEADLINE,
            first_byte_timeout: FIRST_BYTE_TIMEOUT,
        }
    }
}

/// Bind `127.0.0.1` on a free port. Returns the listener and the address that it really has.
pub(super) fn bind() -> io::Result<(TcpListener, SocketAddr)> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
    let addr = listener.local_addr()?;
    Ok((listener, addr))
}

/// Counts an open connection. The count goes down when the connection thread ends.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Start the accept thread.
pub(super) fn serve(
    listener: TcpListener,
    core: Weak<TowerCore>,
    limits: McpLimits,
) -> io::Result<()> {
    let open = Arc::new(AtomicUsize::new(0));
    thread::Builder::new()
        .name("app-mcp-accept".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Some(strong) = core.upgrade() else {
                    return;
                };
                if strong.closed.load(Ordering::SeqCst) {
                    return;
                }
                drop(strong);
                let Ok(mut stream) = stream else {
                    continue;
                };
                if open.fetch_add(1, Ordering::SeqCst) >= limits.max_connections {
                    open.fetch_sub(1, Ordering::SeqCst);
                    // A short answer on a new connection does not block. The timeout is a guard.
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                    let _ = write_response(&mut stream, 503, &[("Retry-After", "5")], b"", false);
                    continue;
                }
                let slot = Slot(Arc::clone(&open));
                let core = core.clone();
                let _ = thread::Builder::new()
                    .name("app-mcp-conn".into())
                    .spawn(move || serve_connection(stream, &core, limits, slot));
            }
        })?;
    Ok(())
}

/// Wake the accept thread so that it sees `closed` and ends.
pub(super) fn wake_accept(port: u16) {
    let _ = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(500),
    );
}

// ---------------------------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------------------------

struct Request {
    method: String,
    /// Header names in lower case.
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    /// The client asked to close the connection (or speaks HTTP/1.0).
    close: bool,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The token of `Authorization: Bearer <token>`.
    fn bearer(&self) -> Option<&str> {
        let value = self.header("authorization")?.trim();
        let (scheme, token) = value.split_once(' ')?;
        (scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty()).then(|| token.trim())
    }
}

enum ReadError {
    /// The client closed the connection, or a time limit ended.
    Closed,
    /// A bad request. The text goes into a 400 or 411 or 413 answer.
    Bad(u16, &'static str),
}

/// The read half of a connection, with the time limits. Each read gets the time that is left to
/// ONE deadline. Before the first byte of a request, the deadline is `FIRST_BYTE_TIMEOUT` on a
/// new connection and `IDLE_TIMEOUT` after that. The first byte sets the deadline of the whole
/// request (`request_deadline`).
struct Timed {
    stream: TcpStream,
    deadline: Instant,
    begun: bool,
    request_deadline: Duration,
}

impl Timed {
    /// A new request is next. `buffered` is true when bytes of it are in the buffer already.
    /// `silent_limit` is how long the client may stay silent before the first byte of it.
    fn next_request(&mut self, buffered: bool, silent_limit: Duration) {
        self.begun = buffered;
        self.deadline = Instant::now()
            + if buffered {
                self.request_deadline
            } else {
                silent_limit
            };
    }
}

impl Read for Timed {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        self.stream.set_read_timeout(Some(left))?;
        let count = self.stream.read(buf)?;
        if count > 0 && !self.begun {
            self.begun = true;
            self.deadline = Instant::now() + self.request_deadline;
        }
        Ok(count)
    }
}

type Reader = BufReader<Timed>;

fn read_line(reader: &mut impl BufRead) -> Result<Option<String>, ReadError> {
    let mut line = Vec::new();
    let mut limited = (&mut *reader).take(MAX_LINE as u64 + 1);
    match limited.read_until(b'\n', &mut line) {
        Ok(0) => Ok(None),
        Ok(_) => {
            if line.len() > MAX_LINE {
                return Err(ReadError::Bad(431, "a header line is too long"));
            }
            while matches!(line.last(), Some(b'\n' | b'\r')) {
                line.pop();
            }
            Ok(Some(String::from_utf8_lossy(&line).into_owned()))
        }
        Err(_) => Err(ReadError::Closed),
    }
}

/// Read the request line and the headers. The body is NOT read: the caller checks the token
/// first (`read_body`).
fn read_head(reader: &mut Reader) -> Result<Request, ReadError> {
    let line = loop {
        // A client can send an empty line before the request line. Skip it.
        match read_line(reader)? {
            None => return Err(ReadError::Closed),
            Some(line) if line.is_empty() => continue,
            Some(line) => break line,
        }
    };
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_owned();
    let _target = parts.next();
    let version = parts.next().unwrap_or("HTTP/1.1");
    let mut headers = Vec::new();
    loop {
        let Some(line) = read_line(reader)? else {
            return Err(ReadError::Closed);
        };
        if line.is_empty() {
            break;
        }
        if headers.len() >= MAX_HEADERS {
            return Err(ReadError::Bad(431, "too many headers"));
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    let mut request = Request {
        method,
        headers,
        body: Vec::new(),
        close: false,
    };
    request.close = version == "HTTP/1.0"
        || request
            .header("connection")
            .is_some_and(|value| value.eq_ignore_ascii_case("close"));
    Ok(request)
}

/// Read the body that the head announces.
fn read_body(reader: &mut Reader, head: &Request) -> Result<Vec<u8>, ReadError> {
    if head.header("transfer-encoding").is_some() {
        return Err(ReadError::Bad(411, "send a Content-Length header"));
    }
    let Some(length) = head.header("content-length") else {
        return Ok(Vec::new());
    };
    let length: usize = length
        .parse()
        .map_err(|_| ReadError::Bad(400, "bad Content-Length"))?;
    if length > MAX_BODY {
        return Err(ReadError::Bad(413, "the request body is too large"));
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|_| ReadError::Closed)?;
    Ok(body)
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    extra: &[(&str, &str)],
    body: &[u8],
    keep_alive: bool,
) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let mut head = format!("HTTP/1.1 {status} {reason}\r\n");
    for (name, value) in extra {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!(
        "Content-Length: {}\r\nConnection: {}\r\n\r\n",
        body.len(),
        if keep_alive { "keep-alive" } else { "close" }
    ));
    // One write for the head and the body.
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body);
    stream.write_all(&bytes)?;
    stream.flush()
}

fn json_response(stream: &mut TcpStream, status: u16, body: &Value, keep: bool) -> io::Result<()> {
    write_response(
        stream,
        status,
        &[("Content-Type", "application/json")],
        body.to_string().as_bytes(),
        keep,
    )
}

fn serve_connection(stream: TcpStream, core: &Weak<TowerCore>, limits: McpLimits, _slot: Slot) {
    let _ = stream.set_nodelay(true);
    let Ok(reader_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(Timed {
        stream: reader_half,
        deadline: Instant::now() + limits.first_byte_timeout,
        begun: false,
        request_deadline: limits.request_deadline,
    });
    let mut writer = stream;
    // The first request of a connection has the short silent limit. The next ones have the long one.
    let mut silent_limit = limits.first_byte_timeout;
    loop {
        let buffered = !reader.buffer().is_empty();
        reader.get_mut().next_request(buffered, silent_limit);
        silent_limit = IDLE_TIMEOUT;
        let mut request = match read_head(&mut reader) {
            Ok(request) => request,
            Err(ReadError::Closed) => return,
            Err(ReadError::Bad(status, text)) => {
                let _ = json_response(&mut writer, status, &json!({ "error": text }), false);
                return;
            }
        };
        let keep = !request.close;
        // The token is checked BEFORE the body is read. The strong handle lives for this check
        // only, so a slow body does not keep a dropped `Tower` alive.
        let who = {
            let Some(core) = core.upgrade() else {
                return;
            };
            authenticate(&core, &request)
        };
        let Some(who) = who else {
            let _ = json_response(&mut writer, 401, &json!({ "error": "unauthorized" }), false);
            return;
        };
        match read_body(&mut reader, &request) {
            Ok(body) => request.body = body,
            Err(ReadError::Closed) => return,
            Err(ReadError::Bad(status, text)) => {
                let _ = json_response(&mut writer, status, &json!({ "error": text }), false);
                return;
            }
        }
        let Some(core) = core.upgrade() else {
            return;
        };
        let written = handle_request(&core, &who, &request, &mut writer, keep);
        drop(core);
        if written.is_err() || !keep {
            return;
        }
    }
}

/// The identity of the token, if the token is known and its session is still live.
fn authenticate(core: &TowerCore, request: &Request) -> Option<Identity> {
    let token = request.bearer()?;
    let identity = locked(&core.tokens).get(token).cloned()?;
    // A token of a session that has ended is dead, also in the short time before the status
    // thread removes it from the map.
    let live = core.live_of(&identity.path, &identity.slug)?;
    (live.id == identity.session_id).then_some(identity)
}

fn handle_request(
    core: &TowerCore,
    who: &Identity,
    request: &Request,
    stream: &mut TcpStream,
    keep: bool,
) -> io::Result<()> {
    match request.method.as_str() {
        "POST" => {}
        // The client may ask for an SSE stream with GET. This server has none (no SSE).
        "GET" => return write_response(stream, 405, &[("Allow", "POST")], b"", keep),
        // The end of a client session. There is no session state to remove.
        "DELETE" => return write_response(stream, 200, &[], b"", keep),
        _ => return write_response(stream, 405, &[("Allow", "POST")], b"", keep),
    }
    let body: Value = match serde_json::from_slice(&request.body) {
        Ok(body) => body,
        Err(_) => {
            return json_response(
                stream,
                400,
                &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}}),
                keep,
            );
        }
    };
    let (messages, is_batch) = match &body {
        Value::Array(list) => (list.clone(), true),
        other => (vec![other.clone()], false),
    };
    // A tool that takes messages out of the waiting list leaves a claim here. The messages are
    // marked delivered only after the answer was written. If the write fails, they wait again.
    let mut claims: Vec<Claim> = Vec::new();
    let answers: Vec<Value> = messages
        .iter()
        .filter_map(|message| rpc(core, who, message, &mut claims))
        .collect();
    let written = if answers.is_empty() {
        // Only notifications: the client wants no answer.
        write_response(stream, 202, &[], b"", keep)
    } else if !claims.is_empty()
        && core
            .test_hooks
            .fail_next_write
            .swap(false, Ordering::SeqCst)
    {
        Err(io::Error::other("test: the write fails"))
    } else {
        let answer = if is_batch {
            Value::Array(answers)
        } else {
            answers.into_iter().next().unwrap_or(Value::Null)
        };
        json_response(stream, 200, &answer, keep)
    };
    core.settle_claims(claims, written.is_ok());
    written
}

// ---------------------------------------------------------------------------------------------
// JSON-RPC
// ---------------------------------------------------------------------------------------------

fn error_answer(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// Answer one JSON-RPC message. `None` for a notification (a message with no `id`).
pub(super) fn rpc(
    core: &TowerCore,
    who: &Identity,
    message: &Value,
    claims: &mut Vec<Claim>,
) -> Option<Value> {
    let Some(object) = message.as_object() else {
        return Some(error_answer(Value::Null, -32600, "invalid request"));
    };
    let id = object.get("id").cloned()?;
    let method = object.get("method").and_then(Value::as_str).unwrap_or("");
    let params = object.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let version = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_PROTOCOL);
            json!({
                "protocolVersion": version,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "agent-tower", "version": env!("CARGO_PKG_VERSION")}
            })
        }
        "tools/list" => json!({ "tools": tool_definitions() }),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(error_answer(id, -32602, "the tool name is missing"));
            };
            let known = tool_definitions()
                .as_array()
                .is_some_and(|tools| tools.iter().any(|t| t["name"] == name));
            if !known {
                return Some(error_answer(id, -32602, "unknown tool"));
            }
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let (text, is_error) = match core.call_tool(who, name, &args, claims) {
                Ok(text) => (text, false),
                Err(text) => (text, true),
            };
            json!({"content": [{"type": "text", "text": text}], "isError": is_error})
        }
        "ping" => json!({}),
        _ => return Some(error_answer(id, -32601, "method not found")),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}
