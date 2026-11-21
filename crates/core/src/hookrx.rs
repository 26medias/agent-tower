//! Hook receiver: a Unix socket server for the events that `tower-hook` sends.
//!
//! Wire format: one line of JSON for each event, ended by `\n`:
//! `{"session": "<TOWER_SESSION>", "event": <hook JSON of the CLI>}`.
//! One connection carries one line. Bytes after the first line are ignored.
//!
//! # Request and answer (Stop hook, spec 4.5)
//!
//! A line can have the extra field `"reply": true`. The sender then waits for one answer line on
//! the same connection. The server keeps the connection open and gives the message a
//! [`HookReply`]. The app calls [`HookReply::send`] to write the answer line, or drops the
//! reply to close the connection with no answer. The sender treats a close with no answer as
//! "nothing to say". The answer is not part of the order logic: the line is released in order
//! as usual, and the answer is written later by whoever holds the reply.
//!
//! # Order
//!
//! A CLI runs its hooks one after another: each `tower-hook` process ends before the next one
//! starts. So the order in which the server ACCEPTS the connections is the true event order.
//! The channel gives the messages in that order.
//!
//! # Threads
//!
//! std threads, not tokio (low event rate, and `tower-core` stays free of an async runtime).
//! - The accept thread gives each connection a sequence number (0, 1, 2, ...) in accept order.
//! - One short thread reads each connection, so a slow or silent client blocks nobody at read time.
//!   It sends one result for its sequence number to the dispatcher.
//! - The dispatcher thread releases the results in sequence order. It waits for the result of
//!   the oldest open connection, but not for longer than [`GIVE_UP_AFTER`] after that connection
//!   was accepted. Then it gives that connection up and goes on.
//!
//! # Worst-case delay
//!
//! A message is released when every earlier connection has delivered or has been given up.
//! An earlier connection was accepted earlier, so its give-up time is earlier than the give-up
//! time of this connection. The worst-case delay of a ready message is thus [`GIVE_UP_AFTER`]
//! (1.2 s), and only while an earlier connection is silent or slow. A line that comes after the
//! give-up is dropped and counted in [`HookReceiver::dropped_count`].
//!
//! `tower-hook` ends itself after 1 s, so a hook that is alive and healthy delivers within the
//! limit. The connection thread reads for [`GIVE_UP_AFTER`] plus a short grace time (so that a
//! late line can be counted), and then it ends.

use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Longest line that the server accepts. A longer line is dropped with its connection.
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// Time limit of one connection, counted from its accept time. It is the read limit of the
/// connection thread and the longest time that a later message waits for an earlier connection.
/// `tower-hook` ends itself after 1 s, so 1.2 s leaves a margin for a slow machine.
pub const GIVE_UP_AFTER: Duration = Duration::from_millis(1200);

/// After the give-up, a connection thread reads for this long more. A line that comes in this
/// time is dropped and counted, so a late hook shows in [`HookReceiver::dropped_count`].
/// After it, the thread ends. So no connection thread lives longer than
/// `GIVE_UP_AFTER + LATE_GRACE` (1.5 s).
const LATE_GRACE: Duration = Duration::from_millis(300);

/// The longest time that a write of an answer waits for the sender to read it.
const REPLY_WRITE_LIMIT: Duration = Duration::from_millis(500);

/// One hook event, with the session that it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct HookMessage {
    pub session: String,
    pub event: Value,
    /// `Some` when the sender asked for an answer (`"reply": true`).
    pub reply: Option<HookReply>,
}

/// The open connection of a sender that waits for an answer. Clones share one connection. The
/// connection closes when the last clone is dropped, or after [`HookReply::send`].
#[derive(Clone)]
pub struct HookReply {
    inner: Arc<ReplyInner>,
}

struct ReplyInner {
    stream: Mutex<Option<UnixStream>>,
    accepted: Instant,
}

impl HookReply {
    fn new(stream: UnixStream, accepted: Instant) -> HookReply {
        HookReply {
            inner: Arc::new(ReplyInner {
                stream: Mutex::new(Some(stream)),
                accepted,
            }),
        }
    }

    /// The time since the server accepted the connection. The sender has a time limit, so an
    /// answer that is very late has no reader.
    pub fn age(&self) -> Duration {
        self.inner.accepted.elapsed()
    }

    /// Write one answer line (`text` and `\n`) and close the connection. It fails when the
    /// sender is gone or does not read, or when an answer was sent already. The write waits for
    /// at most [`REPLY_WRITE_LIMIT`].
    pub fn send(&self, text: &str) -> io::Result<()> {
        use std::io::Write;
        let stream = self
            .inner
            .stream
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or_else(|| io::Error::other("the answer was sent already"))?;
        stream.set_write_timeout(Some(REPLY_WRITE_LIMIT))?;
        let mut line = Vec::with_capacity(text.len() + 1);
        line.extend_from_slice(text.as_bytes());
        line.push(b'\n');
        (&stream).write_all(&line)?;
        let _ = stream.shutdown(std::net::Shutdown::Both);
        Ok(())
    }
}

impl std::fmt::Debug for HookReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HookReply")
    }
}

impl PartialEq for HookReply {
    fn eq(&self, other: &HookReply) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

/// Parse one wire line. `None` means the line is malformed and must be dropped. The message has
/// no reply: use `parse_request` to learn if the sender wants an answer.
pub fn parse_line(line: &[u8]) -> Option<HookMessage> {
    parse_request(line).map(|(message, _)| message)
}

/// Like `parse_line`, and also tells if the line has `"reply": true`.
fn parse_request(line: &[u8]) -> Option<(HookMessage, bool)> {
    let mut value: Value = serde_json::from_slice(line).ok()?;
    let object = value.as_object_mut()?;
    let session = object.get("session")?.as_str()?.to_owned();
    if session.is_empty() {
        return None;
    }
    let wants_reply = object.get("reply").and_then(Value::as_bool) == Some(true);
    let event = object.remove("event")?;
    Some((
        HookMessage {
            session,
            event,
            reply: None,
        },
        wants_reply,
    ))
}

/// The server. Drop it to stop it and to remove the socket file.
pub struct HookReceiver {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    accepted: Arc<AtomicU64>,
    accept_thread: Option<JoinHandle<()>>,
}

impl HookReceiver {
    /// Bind the socket at `path`. A stale socket file at `path` is removed first.
    /// Valid events arrive on the returned channel, in the order of the accepted connections.
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

        let (output, receiver) = mpsc::channel();
        let (to_dispatcher, from_connections) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicU64::new(0));
        let accepted = Arc::new(AtomicU64::new(0));
        {
            let dropped = Arc::clone(&dropped);
            thread::Builder::new()
                .name("hookrx-dispatch".into())
                .spawn(move || dispatch_loop(from_connections, output, dropped))?;
        }
        let accept_thread = {
            let stop = Arc::clone(&stop);
            let dropped = Arc::clone(&dropped);
            let accepted = Arc::clone(&accepted);
            thread::Builder::new()
                .name("hookrx-accept".into())
                .spawn(move || accept_loop(listener, to_dispatcher, stop, dropped, accepted))?
        };
        let receiver_handle = HookReceiver {
            path,
            stop,
            dropped,
            accepted,
            accept_thread: Some(accept_thread),
        };
        Ok((receiver_handle, receiver))
    }

    /// The socket path. This is the value for `TOWER_SOCKET`.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Count of lines that the server dropped: malformed lines, too long lines, and valid lines
    /// that came after their connection was given up.
    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::SeqCst)
    }

    /// Count of connections that the accept thread has accepted and numbered. Tests use it to
    /// wait until a connection is accepted, so that the accept order is known.
    pub fn accepted_count(&self) -> u64 {
        self.accepted.load(Ordering::SeqCst)
    }
}

impl Drop for HookReceiver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept thread, which blocks in accept(). If the socket file is gone (someone
        // removed it), the connect fails and the thread cannot be woken: do not join it, or
        // the drop would hang for ever. The thread then ends with the process.
        let woke = UnixStream::connect(&self.path).is_ok();
        if let Some(handle) = self.accept_thread.take()
            && woke
        {
            let _ = handle.join();
        }
        let _ = fs::remove_file(&self.path);
    }
}

/// What a connection thread found on its connection.
enum Outcome {
    /// A valid message.
    Message(HookMessage),
    /// No message: bad line, too long (both already counted), closed, or timed out.
    Nothing,
}

enum Event {
    /// Sent by the accept thread, in sequence order, before the connection thread starts.
    Accepted { seq: u64, deadline: Instant },
    /// Sent by the connection thread. Exactly one for each sequence number.
    Finished { seq: u64, outcome: Outcome },
}

fn accept_loop(
    listener: UnixListener,
    to_dispatcher: Sender<Event>,
    stop: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    accepted: Arc<AtomicU64>,
) {
    let mut seq: u64 = 0;
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = stream else {
            continue;
        };
        let accepted_at = Instant::now();
        let deadline = accepted_at + GIVE_UP_AFTER;
        if to_dispatcher
            .send(Event::Accepted { seq, deadline })
            .is_err()
        {
            break;
        }
        accepted.fetch_add(1, Ordering::SeqCst);
        let this_seq = seq;
        seq += 1;
        let read_deadline = deadline + LATE_GRACE;
        let sender = to_dispatcher.clone();
        let dropped = Arc::clone(&dropped);
        let spawned = thread::Builder::new()
            .name("hookrx-conn".into())
            .spawn(move || {
                let outcome = serve_connection(stream, accepted_at, read_deadline, &dropped);
                let _ = sender.send(Event::Finished {
                    seq: this_seq,
                    outcome,
                });
            });
        if spawned.is_err() {
            // The connection is lost, but its number must not block the later ones.
            let _ = to_dispatcher.send(Event::Finished {
                seq: this_seq,
                outcome: Outcome::Nothing,
            });
        }
    }
}

/// Read the one line of a connection. Count a bad or too long line as dropped. A sender that
/// wants an answer keeps its connection in the reply of the message.
fn serve_connection(
    stream: UnixStream,
    accepted: Instant,
    deadline: Instant,
    dropped: &AtomicU64,
) -> Outcome {
    let writer = stream.try_clone().ok();
    let mut reader = BufReader::new(stream);
    match read_line(&mut reader, deadline) {
        Line::Complete(bytes) => match parse_request(&bytes) {
            Some((mut message, wants_reply)) => {
                if wants_reply && let Some(writer) = writer {
                    message.reply = Some(HookReply::new(writer, accepted));
                }
                Outcome::Message(message)
            }
            None => {
                dropped.fetch_add(1, Ordering::SeqCst);
                Outcome::Nothing
            }
        },
        Line::TooLong => {
            dropped.fetch_add(1, Ordering::SeqCst);
            Outcome::Nothing
        }
        Line::Closed => Outcome::Nothing,
    }
}

struct Slot {
    deadline: Instant,
    /// `None` while the connection thread has not finished.
    outcome: Option<Outcome>,
}

/// Release the results of the connections in sequence order.
/// Ends when all senders are gone, or when the owner drops the output channel.
fn dispatch_loop(events: Receiver<Event>, output: Sender<HookMessage>, dropped: Arc<AtomicU64>) {
    let mut next: u64 = 0;
    let mut slots: BTreeMap<u64, Slot> = BTreeMap::new();
    loop {
        // Release what is ready, oldest first.
        while let Some(slot) = slots.get_mut(&next) {
            match slot.outcome.take() {
                Some(outcome) => {
                    if let Outcome::Message(message) = outcome
                        && output.send(message).is_err()
                    {
                        return;
                    }
                }
                None if Instant::now() >= slot.deadline => {} // Given up.
                None => break,
            }
            slots.remove(&next);
            next += 1;
        }
        let event = match slots.get(&next) {
            Some(slot) => {
                let wait = slot.deadline.saturating_duration_since(Instant::now());
                match events.recv_timeout(wait) {
                    Ok(event) => event,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            None => match events.recv() {
                Ok(event) => event,
                Err(_) => break,
            },
        };
        match event {
            Event::Accepted { seq, deadline } => {
                slots.insert(
                    seq,
                    Slot {
                        deadline,
                        outcome: None,
                    },
                );
            }
            Event::Finished { seq, outcome } => {
                if seq < next {
                    // Late: the connection was given up. A valid line is dropped and counted.
                    // A bad line was counted by its connection thread.
                    if matches!(outcome, Outcome::Message(_)) {
                        dropped.fetch_add(1, Ordering::SeqCst);
                    }
                } else if let Some(slot) = slots.get_mut(&seq) {
                    slot.outcome = Some(outcome);
                }
            }
        }
    }
    // All senders are gone: release what is left, in order, with no more waiting.
    for (_, slot) in slots {
        if let Some(Outcome::Message(message)) = slot.outcome
            && output.send(message).is_err()
        {
            return;
        }
    }
}

enum Line {
    Complete(Vec<u8>),
    TooLong,
    /// No data: closed with no bytes, a read error, or the time limit.
    Closed,
}

/// Read bytes up to the next `\n`, and no later than `deadline`.
/// Bytes with no final `\n` at end of stream count as a line.
fn read_line(reader: &mut BufReader<UnixStream>, deadline: Instant) -> Line {
    let mut buffer = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Line::Closed;
        }
        if reader.get_ref().set_read_timeout(Some(remaining)).is_err() {
            return Line::Closed;
        }
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
    use std::time::Instant;

    const WAIT: Duration = Duration::from_secs(5);

    fn send_raw(path: &Path, bytes: &[u8]) {
        let mut stream = UnixStream::connect(path).expect("connect");
        stream.write_all(bytes).expect("write");
    }

    #[test]
    fn drop_does_not_hang_when_the_socket_file_was_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hook.sock");
        let (receiver, _messages) = HookReceiver::bind(&path).unwrap();
        fs::remove_file(&path).unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            drop(receiver);
            let _ = done_tx.send(());
        });
        assert!(
            done_rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "the drop of the receiver hangs"
        );
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

    /// Connect and wait until the server has accepted the connection (and numbered it).
    /// Then the accept order of the connections is the order of the calls.
    fn connect_accepted(server: &HookReceiver) -> UnixStream {
        let before = server.accepted_count();
        let stream = UnixStream::connect(server.path()).expect("connect");
        let started = Instant::now();
        while server.accepted_count() == before {
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "connection not accepted"
            );
            thread::sleep(Duration::from_millis(1));
        }
        stream
    }

    fn line(session: &str) -> Vec<u8> {
        format!("{{\"session\":\"{session}\",\"event\":{{}}}}\n").into_bytes()
    }

    fn recv_sessions(events: &Receiver<HookMessage>, count: usize) -> Vec<String> {
        (0..count)
            .map(|_| events.recv_timeout(WAIT).expect("event").session)
            .collect()
    }

    /// Test 5 (old): a malformed line is dropped and a good event after it still arrives.
    /// Adapted: one line for each connection, and the exact order is checked.
    #[test]
    fn malformed_line_is_dropped_and_server_goes_on() {
        let dir = tempfile::tempdir().unwrap();
        let (rx_server, events) = HookReceiver::bind(dir.path().join("a.sock")).unwrap();
        send_raw(rx_server.path(), b"this is not json\n");
        send_raw(rx_server.path(), b"{\"session\":\"s1\"}\n");
        send_raw(rx_server.path(), &line("s2"));
        send_raw(rx_server.path(), &line("s3"));
        assert_eq!(recv_sessions(&events, 2), vec!["s2", "s3"]);
        assert!(
            events.recv_timeout(Duration::from_millis(200)).is_err(),
            "no extra event"
        );
        assert_eq!(rx_server.dropped_count(), 2);
    }

    /// Order test 1: the connections are accepted as c1, c2, c3. The lines are written as
    /// c3, c2, c1. The channel gives c1, c2, c3.
    #[test]
    fn messages_come_in_accept_order_not_write_order() {
        let dir = tempfile::tempdir().unwrap();
        let (rx_server, events) = HookReceiver::bind(dir.path().join("a.sock")).unwrap();
        let mut c1 = connect_accepted(&rx_server);
        let mut c2 = connect_accepted(&rx_server);
        let mut c3 = connect_accepted(&rx_server);
        c3.write_all(&line("c3")).unwrap();
        c2.write_all(&line("c2")).unwrap();
        // Let the reader threads of c3 and c2 finish before c1 writes.
        thread::sleep(Duration::from_millis(100));
        c1.write_all(&line("c1")).unwrap();
        assert_eq!(recv_sessions(&events, 3), vec!["c1", "c2", "c3"]);
        assert_eq!(rx_server.dropped_count(), 0);
    }

    /// Order test 2: c1 is silent. c2 and c3 wait until c1 is given up, and they arrive in
    /// order. The late line of c1 is dropped and counted.
    #[test]
    fn silent_first_connection_delays_later_ones_by_at_most_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let (rx_server, events) = HookReceiver::bind(dir.path().join("a.sock")).unwrap();
        let mut c1 = connect_accepted(&rx_server);
        let accepted_at = Instant::now();
        send_raw(rx_server.path(), &line("c2"));
        send_raw(rx_server.path(), &line("c3"));
        // Nothing arrives while c1 may still send.
        assert!(
            events.recv_timeout(GIVE_UP_AFTER / 2).is_err(),
            "c2 must wait for c1"
        );
        assert_eq!(recv_sessions(&events, 2), vec!["c2", "c3"]);
        let waited = accepted_at.elapsed();
        assert!(
            waited >= GIVE_UP_AFTER - Duration::from_millis(100),
            "released too early: {waited:?}"
        );
        assert!(
            waited <= GIVE_UP_AFTER + Duration::from_millis(600),
            "released too late: {waited:?}"
        );
        // The late line of c1: still inside the read grace time. It is dropped and counted.
        c1.write_all(&line("c1")).unwrap();
        let started = Instant::now();
        while rx_server.dropped_count() < 1 {
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "late line not counted"
            );
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(rx_server.dropped_count(), 1);
        assert!(
            events.recv_timeout(Duration::from_millis(200)).is_err(),
            "the late line must not arrive"
        );
    }

    /// Order test 3: c1 sends a malformed line and stays open. c2 arrives with no wait for the
    /// limit.
    #[test]
    fn malformed_first_connection_does_not_delay_later_ones() {
        let dir = tempfile::tempdir().unwrap();
        let (rx_server, events) = HookReceiver::bind(dir.path().join("a.sock")).unwrap();
        let mut c1 = connect_accepted(&rx_server);
        c1.write_all(b"this is not json\n").unwrap();
        let started = Instant::now();
        send_raw(rx_server.path(), &line("c2"));
        assert_eq!(recv_sessions(&events, 1), vec!["c2"]);
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "c2 waited {:?}",
            started.elapsed()
        );
        assert_eq!(rx_server.dropped_count(), 1);
        drop(c1);
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

    // ---- the reply of a request (Stop hook) ------------------------------------------------

    fn read_answer(stream: &mut UnixStream) -> Vec<u8> {
        use std::io::Read;
        stream.set_read_timeout(Some(WAIT)).unwrap();
        let mut bytes = Vec::new();
        let _ = stream.read_to_end(&mut bytes);
        bytes
    }

    #[test]
    fn a_line_with_reply_true_gets_a_reply_and_the_answer_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hook.sock");
        let (_receiver, messages) = HookReceiver::bind(&path).unwrap();
        let mut client = UnixStream::connect(&path).unwrap();
        client
            .write_all(b"{\"session\":\"s\",\"event\":{\"a\":1},\"reply\":true}\n")
            .unwrap();
        let message = messages.recv_timeout(WAIT).unwrap();
        assert_eq!(message.event, json!({"a": 1}));
        let reply = message.reply.expect("a reply");
        assert!(reply.age() < Duration::from_secs(2));
        reply.send(r#"{"decision":"block","reason":"x"}"#).unwrap();
        assert_eq!(
            read_answer(&mut client),
            b"{\"decision\":\"block\",\"reason\":\"x\"}\n"
        );
        // One answer only.
        assert!(reply.send("again").is_err());
    }

    #[test]
    fn a_line_without_reply_true_has_no_reply() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hook.sock");
        let (_receiver, messages) = HookReceiver::bind(&path).unwrap();
        for line in [
            &b"{\"session\":\"s\",\"event\":{}}\n"[..],
            b"{\"session\":\"s\",\"event\":{},\"reply\":false}\n",
            b"{\"session\":\"s\",\"event\":{},\"reply\":\"yes\"}\n",
        ] {
            send_raw(&path, line);
            assert!(messages.recv_timeout(WAIT).unwrap().reply.is_none());
        }
    }

    #[test]
    fn a_dropped_reply_closes_the_connection_with_no_answer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hook.sock");
        let (_receiver, messages) = HookReceiver::bind(&path).unwrap();
        let mut client = UnixStream::connect(&path).unwrap();
        client
            .write_all(b"{\"session\":\"s\",\"event\":{},\"reply\":true}\n")
            .unwrap();
        let message = messages.recv_timeout(WAIT).unwrap();
        // The connection stays open while the reply lives, also after the thread that read the
        // line has ended.
        std::thread::sleep(Duration::from_millis(100));
        client
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut byte = [0u8; 1];
        let early = std::io::Read::read(&mut client, &mut byte);
        assert!(
            matches!(&early, Err(e) if e.kind() == io::ErrorKind::WouldBlock),
            "the connection must stay open: {early:?}"
        );
        drop(message);
        assert_eq!(read_answer(&mut client), b"");
    }

    #[test]
    fn an_answer_to_a_client_that_has_gone_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hook.sock");
        let (_receiver, messages) = HookReceiver::bind(&path).unwrap();
        let mut client = UnixStream::connect(&path).unwrap();
        client
            .write_all(b"{\"session\":\"s\",\"event\":{},\"reply\":true}\n")
            .unwrap();
        let reply = messages.recv_timeout(WAIT).unwrap().reply.unwrap();
        // `shutdown`, not `drop`: another test can fork a process while this one runs, and that
        // process holds a copy of the descriptor until it execs. A shutdown acts on the socket.
        client.shutdown(std::net::Shutdown::Both).unwrap();
        assert!(reply.send("late").is_err());
    }
}
