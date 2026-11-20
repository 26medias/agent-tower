//! `tower-hook`: the hook program that a CLI runs on each hook event.
//!
//! It reads the hook JSON on stdin and sends one line to the Unix socket in `TOWER_SOCKET`:
//! `{"session": "<TOWER_SESSION>", "event": <stdin JSON>}\n`.
//!
//! Rules: it takes no arguments, it prints nothing, it always exits 0, and it ends within 1 s.
//! A hook must never block or change a CLI turn.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::time::Duration;

/// Total time limit for the whole run, stdin read included.
const TOTAL_LIMIT: Duration = Duration::from_millis(1000);
/// Write time limit on the socket. It stops a write to an app that does not read.
const WRITE_LIMIT: Duration = Duration::from_millis(1000);

fn main() {
    // A panic must not print to stderr.
    std::panic::set_hook(Box::new(|_| {}));

    let (done, wait) = mpsc::channel::<()>();
    let worker = std::thread::spawn(move || {
        let _ = run();
        let _ = done.send(());
    });
    // The watchdog: when the worker is stuck (stdin, connect or write), end the process.
    let _ = wait.recv_timeout(TOTAL_LIMIT);
    drop(worker);
    std::process::exit(0);
}

fn run() -> Option<()> {
    let session = std::env::var("TOWER_SESSION")
        .ok()
        .filter(|value| !value.is_empty())?;
    let socket = std::env::var_os("TOWER_SOCKET").filter(|value| !value.is_empty())?;

    let mut input = Vec::new();
    std::io::stdin().lock().read_to_end(&mut input).ok()?;
    let event: serde_json::Value = serde_json::from_slice(&input).ok()?;

    let mut line =
        serde_json::to_vec(&serde_json::json!({ "session": session, "event": event })).ok()?;
    line.push(b'\n');

    let mut stream = UnixStream::connect(socket).ok()?;
    stream.set_write_timeout(Some(WRITE_LIMIT)).ok()?;
    stream.write_all(&line).ok()?;
    Some(())
}
