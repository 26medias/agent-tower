//! `tower-hook`: the hook program that a CLI runs on each hook event.
//!
//! It reads the hook JSON on stdin and sends one line to the Unix socket in `TOWER_SOCKET`:
//! `{"session": "<TOWER_SESSION>", "event": <stdin JSON>}\n`.
//!
//! Rules: it takes no arguments, it always exits 0, and a hook must never block or change a CLI
//! turn. Most events end within 1 s and print nothing.
//!
//! # The Stop event (spec 4.5)
//!
//! For a `Stop` event the line also has `"reply": true`, and the program waits for one answer
//! line from the app, for 2 s at the most (the whole run, stdin read included). The app answers
//! `{"decision":"block","reason":"<messages>"}` when messages wait for the session. The program
//! then prints that JSON to stdout, and the CLI continues the turn with the reason as a new
//! instruction. In every other case it prints nothing: a closed connection with no answer, a
//! bad answer, a time out, a dead socket. The turn then ends as it does with no hook.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Total time limit for the whole run of an event that gets no answer, stdin read included.
const TOTAL_LIMIT: Duration = Duration::from_millis(1000);
/// Total time limit for a `Stop` event, which waits for the answer of the app.
const STOP_LIMIT: Duration = Duration::from_millis(2000);
/// Write time limit on the socket. It stops a write to an app that does not read.
const WRITE_LIMIT: Duration = Duration::from_millis(1000);
/// The longest answer line that the program reads.
const ANSWER_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// What the worker tells the watchdog.
enum Note {
    /// The event is a `Stop`: the time limit is `STOP_LIMIT`.
    WaitsForAnswer,
    Done,
}

fn main() {
    // A panic must not print to stderr.
    std::panic::set_hook(Box::new(|_| {}));

    let start = Instant::now();
    let (notes, watch) = mpsc::channel::<Note>();
    let worker = std::thread::spawn(move || {
        let _ = run(start, &notes);
        let _ = notes.send(Note::Done);
    });
    // The watchdog: when the worker is stuck (stdin, connect, write or read), end the process.
    let mut limit = TOTAL_LIMIT;
    loop {
        let left = limit.saturating_sub(start.elapsed());
        match watch.recv_timeout(left) {
            Ok(Note::WaitsForAnswer) => limit = STOP_LIMIT,
            Ok(Note::Done) | Err(_) => break,
        }
    }
    drop(worker);
    std::process::exit(0);
}

fn run(start: Instant, notes: &mpsc::Sender<Note>) -> Option<()> {
    let session = std::env::var("TOWER_SESSION")
        .ok()
        .filter(|value| !value.is_empty())?;
    let socket = std::env::var_os("TOWER_SOCKET").filter(|value| !value.is_empty())?;

    let mut input = Vec::new();
    std::io::stdin().lock().read_to_end(&mut input).ok()?;
    let event: serde_json::Value = serde_json::from_slice(&input).ok()?;
    let is_stop = event.get("hook_event_name").and_then(|name| name.as_str()) == Some("Stop");
    if is_stop {
        let _ = notes.send(Note::WaitsForAnswer);
    }

    let mut request = serde_json::json!({ "session": session, "event": event });
    if is_stop {
        request["reply"] = serde_json::Value::Bool(true);
    }
    let mut line = serde_json::to_vec(&request).ok()?;
    line.push(b'\n');

    let mut stream = UnixStream::connect(socket).ok()?;
    stream.set_write_timeout(Some(WRITE_LIMIT)).ok()?;
    stream.write_all(&line).ok()?;
    if !is_stop {
        return Some(());
    }

    let left = STOP_LIMIT.saturating_sub(start.elapsed());
    if left.is_zero() {
        return None;
    }
    stream.set_read_timeout(Some(left)).ok()?;
    let mut answer = Vec::new();
    BufReader::new(stream)
        .take(ANSWER_MAX_BYTES)
        .read_until(b'\n', &mut answer)
        .ok()?;
    let output = block_output(&answer)?;
    // One write, so a stop of the process cannot leave half a line.
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&output).ok()?;
    stdout.flush().ok()
}

/// The text to print for an answer line of the app. `None` means print nothing: the answer is
/// not exactly a `block` decision with a text. The program prints its own JSON, never the bytes
/// of the answer.
fn block_output(answer: &[u8]) -> Option<Vec<u8>> {
    let value: serde_json::Value = serde_json::from_slice(answer).ok()?;
    if value.get("decision")?.as_str()? != "block" {
        return None;
    }
    let reason = value.get("reason")?.as_str()?;
    if reason.is_empty() {
        return None;
    }
    let mut output =
        serde_json::to_vec(&serde_json::json!({ "decision": "block", "reason": reason })).ok()?;
    output.push(b'\n');
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_answer_gives_the_decision_and_the_reason_only() {
        let output =
            block_output(br#"{"decision":"block","reason":"hi\nthere","extra":1}"#).unwrap();
        assert_eq!(
            output,
            b"{\"decision\":\"block\",\"reason\":\"hi\\nthere\"}\n"
        );
    }

    #[test]
    fn any_other_answer_prints_nothing() {
        for answer in [
            &b""[..],
            b"\n",
            b"not json",
            br#"{"decision":"approve","reason":"x"}"#,
            br#"{"decision":"block"}"#,
            br#"{"decision":"block","reason":""}"#,
            br#"{"decision":"block","reason":5}"#,
            br#"["block"]"#,
        ] {
            assert_eq!(block_output(answer), None, "{answer:?}");
        }
    }
}
