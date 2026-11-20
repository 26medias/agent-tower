//! Hook receiver: a Unix socket server for the events that `tower-hook` sends.
//!
//! Wire format: one line of JSON for each event, ended by `\n`:
//! `{"session": "<TOWER_SESSION>", "event": <hook JSON of the CLI>}`.
//!
//! Threads: std threads, not tokio. One thread accepts, and one short thread reads each
//! connection. The load is a few small events for each second, and `tower-core` stays free of
//! an async runtime.

use serde_json::Value;
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Longest line that the server accepts. A longer line is dropped with its connection.
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// A client that sends nothing for this long is dropped. `tower-hook` sends at once.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// One hook event, with the session that it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct HookMessage {
    pub session: String,
    pub event: Value,
}

/// Parse one wire line. `None` means the line is malformed and must be dropped.
pub fn parse_line(line: &[u8]) -> Option<HookMessage> {
    let mut value: Value = serde_json::from_slice(line).ok()?;
    let object = value.as_object_mut()?;
    let session = object.get("session")?.as_str()?.to_owned();
    if session.is_empty() {
        return None;
    }
    let event = object.remove("event")?;
    Some(HookMessage { session, event })
}

/// The server. Drop it to stop it and to remove the socket file.
pub struct HookReceiver {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    accept_thread: Option<JoinHandle<()>>,
}

impl HookReceiver {
    /// Bind the socket at `path`. A stale socket file at `path` is removed first.
    /// Valid events arrive on the returned channel.
    pub fn bind(path: impl AsRef<Path>) -> io::Result<(HookReceiver, Receiver<HookMessage>)> {
        let path = path.as_ref().to_path_buf();
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_socket() => fs::remove_file(&path)?,
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} exists and is not a socket", path.display()),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;

        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicU64::new(0));
        let accept_thread = {
            let stop = Arc::clone(&stop);
            let dropped = Arc::clone(&dropped);
            thread::Builder::new()
                .name("hookrx-accept".into())
                .spawn(move || accept_loop(listener, sender, stop, dropped))?
        };
        let receiver_handle = HookReceiver {
            path,
            stop,
            dropped,
            accept_thread: Some(accept_thread),
        };
        Ok((receiver_handle, receiver))
    }

    /// The socket path. This is the value for `TOWER_SOCKET`.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Count of malformed or too long lines that the server dropped.
    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::SeqCst)
    }
}

impl Drop for HookReceiver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept thread, which blocks in accept().
        let _ = UnixStream::connect(&self.path);
        if let Some(handle) = self.accept_thread.take() {
            let _ = handle.join();
        }
        let _ = fs::remove_file(&self.path);
    }
}

fn accept_loop(
    listener: UnixListener,
    sender: Sender<HookMessage>,
    stop: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = stream else {
            continue;
        };
        let sender = sender.clone();
        let dropped = Arc::clone(&dropped);
        // If the spawn fails, the connection is dropped and the server goes on.
        let _ = thread::Builder::new()
            .name("hookrx-conn".into())
            .spawn(move || serve_connection(stream, sender, dropped));
    }
}

fn serve_connection(stream: UnixStream, sender: Sender<HookMessage>, dropped: Arc<AtomicU64>) {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let mut reader = BufReader::new(stream);
    loop {
        match read_line(&mut reader) {
            Line::Complete(bytes) => match parse_line(&bytes) {
                Some(message) => {
                    if sender.send(message).is_err() {
                        return; // The owner dropped the channel.
                    }
                }
                None => {
                    dropped.fetch_add(1, Ordering::SeqCst);
                }
            },
            Line::TooLong => {
                dropped.fetch_add(1, Ordering::SeqCst);
                return;
            }
            Line::Closed => return,
        }
    }
}

enum Line {
    Complete(Vec<u8>),
    TooLong,
    Closed,
}

/// Read bytes up to the next `\n`. Bytes with no final `\n` at end of stream count as a line.
fn read_line(reader: &mut BufReader<UnixStream>) -> Line {
    let mut buffer = Vec::new();
    loop {
        let chunk = match reader.fill_buf() {
            Ok(chunk) => chunk,
            Err(_) => return Line::Closed,
        };
        if chunk.is_empty() {
            return if buffer.is_empty() {
                Line::Closed
            } else {
                Line::Complete(buffer)
            };
        }
        let newline = chunk.iter().position(|b| *b == b'\n');
        let take = newline.unwrap_or(chunk.len());
        buffer.extend_from_slice(&chunk[..take]);
        reader.consume(newline.map_or(take, |index| index + 1));
        if buffer.len() > MAX_LINE_BYTES {
            return Line::TooLong;
        }
        if newline.is_some() {
            return Line::Complete(buffer);
        }
    }
}

/// Default socket path: `$XDG_RUNTIME_DIR/agent-tower/<pid>.sock`.
/// Creates the directory with mode 0700 (and sets the mode if the directory exists).
/// Fails if `XDG_RUNTIME_DIR` is not set.
pub fn default_socket_path() -> io::Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is not set"))?;
    socket_path_in(Path::new(&runtime), std::process::id())
}

fn socket_path_in(runtime_dir: &Path, pid: u32) -> io::Result<PathBuf> {
    let dir = runtime_dir.join("agent-tower");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    Ok(dir.join(format!("{pid}.sock")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    const WAIT: Duration = Duration::from_secs(5);

    fn send_raw(path: &Path, bytes: &[u8]) {
        let mut stream = UnixStream::connect(path).expect("connect");
        stream.write_all(bytes).expect("write");
    }

    #[test]
    fn parse_line_accepts_a_good_line() {
        let message = parse_line(br#"{"session":"s1","event":{"a":1}}"#).expect("parsed");
        assert_eq!(message.session, "s1");
        assert_eq!(message.event, json!({"a": 1}));
    }

    #[test]
    fn parse_line_rejects_bad_lines() {
        assert!(parse_line(b"not json").is_none());
        assert!(parse_line(b"[1,2]").is_none());
        assert!(parse_line(br#"{"event":{}}"#).is_none());
        assert!(parse_line(br#"{"session":"s"}"#).is_none());
        assert!(parse_line(br#"{"session":5,"event":{}}"#).is_none());
        assert!(parse_line(br#"{"session":"","event":{}}"#).is_none());
    }

    #[test]
    fn delivers_a_line_sent_directly() {
        let dir = tempfile::tempdir().unwrap();
        let (rx_server, events) = HookReceiver::bind(dir.path().join("a.sock")).unwrap();
        send_raw(
            rx_server.path(),
            b"{\"session\":\"s1\",\"event\":{\"k\":\"v\"}}\n",
        );
        let message = events.recv_timeout(WAIT).expect("event");
        assert_eq!(message.session, "s1");
        assert_eq!(message.event, json!({"k": "v"}));
    }

    /// Test 5: a malformed line is dropped and a good event after it still arrives.
    #[test]
    fn malformed_line_is_dropped_and_server_goes_on() {
        let dir = tempfile::tempdir().unwrap();
        let (rx_server, events) = HookReceiver::bind(dir.path().join("a.sock")).unwrap();
        // Bad line first, in its own connection.
        send_raw(rx_server.path(), b"this is not json\n");
        // Then a bad line and a good line on the same connection.
        send_raw(
            rx_server.path(),
            b"{\"session\":\"s1\"}\n{\"session\":\"s2\",\"event\":{\"n\":2}}\n",
        );
        // Then a good event in a new connection.
        send_raw(
            rx_server.path(),
            b"{\"session\":\"s3\",\"event\":{\"n\":3}}\n",
        );

        let mut sessions = Vec::new();
        for _ in 0..2 {
            sessions.push(events.recv_timeout(WAIT).expect("good event").session);
        }
        sessions.sort();
        assert_eq!(sessions, vec!["s2", "s3"]);
        assert!(
            events.recv_timeout(Duration::from_millis(200)).is_err(),
            "no extra event"
        );
        assert_eq!(rx_server.dropped_count(), 2);
    }

    #[test]
    fn many_connections_at_the_same_time() {
        let dir = tempfile::tempdir().unwrap();
        let (rx_server, events) = HookReceiver::bind(dir.path().join("a.sock")).unwrap();
        let path = rx_server.path().to_path_buf();
        let handles: Vec<_> = (0..50)
            .map(|n| {
                let path = path.clone();
                thread::spawn(move || {
                    let line = format!("{{\"session\":\"s{n}\",\"event\":{{\"n\":{n}}}}}\n");
                    send_raw(&path, line.as_bytes());
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let mut seen: Vec<u32> = (0..50)
            .map(|_| {
                events.recv_timeout(WAIT).expect("event").event["n"]
                    .as_u64()
                    .unwrap() as u32
            })
            .collect();
        seen.sort();
        assert_eq!(seen, (0..50).collect::<Vec<_>>());
    }

    #[test]
    fn silent_client_does_not_block_other_clients() {
        let dir = tempfile::tempdir().unwrap();
        let (rx_server, events) = HookReceiver::bind(dir.path().join("a.sock")).unwrap();
        // This client connects and sends nothing. It holds its connection open.
        let _silent = UnixStream::connect(rx_server.path()).unwrap();
        send_raw(rx_server.path(), b"{\"session\":\"s1\",\"event\":{}}\n");
        let message = events.recv_timeout(Duration::from_secs(2)).expect("event");
        assert_eq!(message.session, "s1");
    }

    #[test]
    fn line_without_final_newline_counts_at_end_of_stream() {
        let dir = tempfile::tempdir().unwrap();
        let (rx_server, events) = HookReceiver::bind(dir.path().join("a.sock")).unwrap();
        send_raw(rx_server.path(), b"{\"session\":\"s1\",\"event\":{}}");
        assert_eq!(events.recv_timeout(WAIT).expect("event").session, "s1");
    }

    #[test]
    fn stale_socket_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.sock");
        // A listener that goes away without removing its socket file leaves a stale file.
        let stale = UnixListener::bind(&path).unwrap();
        drop(stale);
        assert!(path.exists(), "the stale file must exist before the test");
        let (rx_server, events) = HookReceiver::bind(&path).expect("bind over stale socket");
        send_raw(rx_server.path(), b"{\"session\":\"s1\",\"event\":{}}\n");
        assert_eq!(events.recv_timeout(WAIT).expect("event").session, "s1");
    }

    #[test]
    fn bind_refuses_to_replace_a_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.sock");
        fs::write(&path, b"data").unwrap();
        assert!(HookReceiver::bind(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"data");
    }

    #[test]
    fn socket_file_is_removed_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.sock");
        let (rx_server, _events) = HookReceiver::bind(&path).unwrap();
        assert!(path.exists());
        drop(rx_server);
        assert!(!path.exists());
    }

    #[test]
    fn too_long_line_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let (rx_server, events) = HookReceiver::bind(dir.path().join("a.sock")).unwrap();
        {
            let mut stream = UnixStream::connect(rx_server.path()).unwrap();
            let chunk = vec![b'x'; 1024 * 1024];
            // The server closes the connection after the limit, so a late write may fail.
            for _ in 0..=(MAX_LINE_BYTES / chunk.len() + 1) {
                if stream.write_all(&chunk).is_err() {
                    break;
                }
            }
        }
        send_raw(rx_server.path(), b"{\"session\":\"s1\",\"event\":{}}\n");
        assert_eq!(events.recv_timeout(WAIT).expect("event").session, "s1");
        assert_eq!(rx_server.dropped_count(), 1);
    }

    #[test]
    fn default_path_creates_directory_with_mode_0700() {
        let runtime = tempfile::tempdir().unwrap();
        let path = socket_path_in(runtime.path(), 4242).unwrap();
        assert_eq!(path, runtime.path().join("agent-tower").join("4242.sock"));
        let mode = fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}
