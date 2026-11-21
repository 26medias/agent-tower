//! End-to-end tests of the REAL `tower-hook` binary against the REAL `HookReceiver`.

use serde_json::Value;
use std::io::Write;
use std::os::unix::net::UnixListener;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use tower_core::hookrx::HookReceiver;

const WAIT: Duration = Duration::from_secs(5);
/// A hook that runs longer than this is killed, and the test fails.
const HARD_DEADLINE: Duration = Duration::from_secs(8);
/// Payloads copied from the `data` field of lines 1, 17, 22, 23 and 24 of
/// `probes/logs/claude-hooks.jsonl` (real hook payloads of Claude Code).
const FIXTURES: &str = include_str!("fixtures/claude-hook-payloads.jsonl");

/// Start `tower-hook` with the given environment and stdin bytes. Wait for it to end.
/// Returns its output and its run time.
fn run_hook(
    session: Option<&str>,
    socket: Option<&std::path::Path>,
    stdin: &[u8],
) -> (Output, Duration) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tower-hook"));
    command
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(session) = session {
        command.env("TOWER_SESSION", session);
    }
    if let Some(socket) = socket {
        command.env("TOWER_SOCKET", socket);
    }
    let started = Instant::now();
    let mut child = command.spawn().expect("spawn tower-hook");
    let mut child_stdin = child.stdin.take().unwrap();
    let payload = stdin.to_vec();
    // A separate thread feeds stdin: a large payload must not block the test thread.
    let feeder = std::thread::spawn(move || {
        let _ = child_stdin.write_all(&payload); // An early exit of the hook gives EPIPE. That is fine.
    });
    // The test owns a hard deadline, so a hung hook makes the test fail instead of hanging.
    let elapsed = loop {
        if child.try_wait().expect("try_wait").is_some() {
            break started.elapsed();
        }
        if started.elapsed() > HARD_DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            panic!("tower-hook was still running after {HARD_DEADLINE:?}: it hangs");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let output = child.wait_with_output().expect("wait tower-hook");
    let _ = feeder.join();
    (output, elapsed)
}

fn assert_silent_success(output: &Output) {
    assert_eq!(output.status.code(), Some(0), "exit code");
    assert!(
        output.stdout.is_empty(),
        "stdout must be empty: {:?}",
        output.stdout
    );
    assert!(
        output.stderr.is_empty(),
        "stderr must be empty: {:?}",
        output.stderr
    );
}

/// Test 1: real binary, real receiver, real payloads. Session ID and event are equal.
#[test]
fn hook_delivers_real_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let (receiver, events) = HookReceiver::bind(dir.path().join("t.sock")).unwrap();
    let payloads: Vec<&str> = FIXTURES.lines().collect();
    assert_eq!(payloads.len(), 5);
    for (index, payload) in payloads.iter().enumerate() {
        let session = format!("tower-session-{index}");
        let (output, _) = run_hook(Some(&session), Some(receiver.path()), payload.as_bytes());
        assert_silent_success(&output);
        let message = events.recv_timeout(WAIT).expect("event arrives");
        let expected: Value = serde_json::from_str(payload).unwrap();
        assert_eq!(message.session, session);
        assert_eq!(message.event, expected);
    }
}

/// Test 2: no socket file. Exit 0, fast, silent. Also: missing env and bad stdin.
#[test]
fn hook_with_missing_socket_exits_zero_fast_and_silent() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist.sock");
    let payload = FIXTURES.lines().next().unwrap().as_bytes();
    let (output, elapsed) = run_hook(Some("s"), Some(&missing), payload);
    assert_silent_success(&output);
    assert!(elapsed < Duration::from_millis(1500), "took {elapsed:?}");
}

#[test]
fn hook_with_missing_env_or_bad_stdin_exits_zero_and_silent() {
    let dir = tempfile::tempdir().unwrap();
    let (receiver, events) = HookReceiver::bind(dir.path().join("t.sock")).unwrap();
    let payload = FIXTURES.lines().next().unwrap().as_bytes();
    // No session, no socket, neither.
    for (session, socket) in [
        (None, Some(receiver.path())),
        (Some("s"), None),
        (None, None),
    ] {
        let (output, elapsed) = run_hook(session, socket, payload);
        assert_silent_success(&output);
        assert!(elapsed < Duration::from_millis(1500), "took {elapsed:?}");
    }
    // Stdin is not JSON. Stdin is empty.
    for stdin in [&b"this is not json"[..], b""] {
        let (output, _) = run_hook(Some("s"), Some(receiver.path()), stdin);
        assert_silent_success(&output);
    }
    assert!(
        events.recv_timeout(Duration::from_millis(300)).is_err(),
        "nothing must reach the receiver in these cases"
    );
}

/// Test 3: the socket exists, nobody accepts, and the payload is 2 MB. The hook must not hang.
#[test]
fn hook_does_not_hang_when_nobody_reads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.sock");
    let _listener = UnixListener::bind(&path).unwrap(); // Bound, never accepted.
    let big = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_response": "x".repeat(2 * 1024 * 1024),
    });
    let payload = serde_json::to_vec(&big).unwrap();
    assert!(payload.len() > 2 * 1024 * 1024);
    let (output, elapsed) = run_hook(Some("s"), Some(&path), &payload);
    assert_silent_success(&output);
    assert!(elapsed < Duration::from_millis(1500), "took {elapsed:?}");
}

/// Test 3 (variant): the receiver accepts the connection but never reads.
#[test]
fn hook_does_not_hang_when_peer_accepts_but_never_reads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let held = std::thread::spawn(move || {
        let connection = listener.accept().unwrap();
        std::thread::sleep(Duration::from_secs(4));
        drop(connection);
    });
    let big = serde_json::json!({ "tool_response": "x".repeat(2 * 1024 * 1024) });
    let payload = serde_json::to_vec(&big).unwrap();
    let (output, elapsed) = run_hook(Some("s"), Some(&path), &payload);
    assert_silent_success(&output);
    assert!(elapsed < Duration::from_millis(1500), "took {elapsed:?}");
    held.join().unwrap();
}

/// Test 3 (variant): stdin never closes. The hook must still end in time.
#[test]
fn hook_does_not_hang_when_stdin_never_closes() {
    let dir = tempfile::tempdir().unwrap();
    let (receiver, _events) = HookReceiver::bind(dir.path().join("t.sock")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_tower-hook"))
        .env_clear()
        .env("TOWER_SESSION", "s")
        .env("TOWER_SOCKET", receiver.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _stdin_kept_open = child.stdin.take().unwrap();
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > HARD_DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            panic!("tower-hook was still running after {HARD_DEADLINE:?}: it hangs");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let elapsed = started.elapsed();
    let output = child.wait_with_output().unwrap();
    assert_silent_success(&output);
    assert!(elapsed < Duration::from_millis(1500), "took {elapsed:?}");
}

/// Test 4: 20 hooks at the same time all arrive, each with its own session.
#[test]
fn twenty_hooks_at_the_same_time_all_arrive() {
    let dir = tempfile::tempdir().unwrap();
    let (receiver, events) = HookReceiver::bind(dir.path().join("t.sock")).unwrap();
    let path = receiver.path().to_path_buf();
    let payloads: Vec<String> = FIXTURES.lines().map(str::to_owned).collect();
    let handles: Vec<_> = (0..20)
        .map(|n| {
            let path = path.clone();
            let payload = payloads[n % payloads.len()].clone();
            std::thread::spawn(move || {
                let (output, _) = run_hook(
                    Some(&format!("concurrent-{n}")),
                    Some(&path),
                    payload.as_bytes(),
                );
                assert_silent_success(&output);
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let mut sessions = Vec::new();
    for _ in 0..20 {
        let message = events.recv_timeout(WAIT).expect("all 20 events arrive");
        let n: usize = message
            .session
            .strip_prefix("concurrent-")
            .unwrap()
            .parse()
            .unwrap();
        let expected: Value = serde_json::from_str(&payloads[n % payloads.len()]).unwrap();
        assert_eq!(message.event, expected, "event of {}", message.session);
        sessions.push(n);
    }
    sessions.sort();
    assert_eq!(sessions, (0..20).collect::<Vec<_>>());
    assert!(
        events.recv_timeout(Duration::from_millis(200)).is_err(),
        "no extra event"
    );
}

/// Order test 4: 200 runs of the real `tower-hook`, one after another (each process ends before
/// the next starts, as a CLI runs its hooks). The counter in the payload must come out in
/// strictly increasing order, with none lost.
#[test]
fn two_hundred_sequential_hooks_arrive_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let (receiver, events) = HookReceiver::bind(dir.path().join("t.sock")).unwrap();
    for counter in 0..200u32 {
        let payload = serde_json::json!({ "hook_event_name": "PreToolUse", "counter": counter });
        let (output, _) = run_hook(
            Some("ordered"),
            Some(receiver.path()),
            payload.to_string().as_bytes(),
        );
        assert_silent_success(&output);
    }
    let mut previous: Option<u64> = None;
    for expected in 0..200u64 {
        let message = events.recv_timeout(WAIT).expect("event arrives");
        let counter = message.event["counter"].as_u64().expect("counter");
        assert_eq!(
            counter, expected,
            "out of order: got {counter} after {previous:?}, expected {expected}"
        );
        previous = Some(counter);
    }
    assert!(
        events.recv_timeout(Duration::from_millis(200)).is_err(),
        "no extra event"
    );
    assert_eq!(receiver.dropped_count(), 0);
}

// ---------------------------------------------------------------------------------------------
// The Stop event waits for an answer (spec 4.5)
// ---------------------------------------------------------------------------------------------

const STOP_PAYLOAD: &str =
    r#"{"hook_event_name":"Stop","session_id":"claude-1","stop_hook_active":false}"#;

/// A Stop event gets the answer `block` from the app, and the program prints it, as JSON with
/// the decision and the reason only. The request line has `"reply": true`.
#[test]
fn a_stop_event_gets_the_block_answer_and_prints_it() {
    let dir = tempfile::tempdir().unwrap();
    let (receiver, events) = HookReceiver::bind(dir.path().join("t.sock")).unwrap();
    let app = std::thread::spawn(move || {
        let message = events.recv_timeout(WAIT).expect("the Stop event arrives");
        assert_eq!(message.event["hook_event_name"], "Stop");
        let reply = message.reply.expect("the Stop event asks for an answer");
        reply
            .send(r#"{"decision":"block","reason":"line one\nline \"two\"","extra":true}"#)
            .expect("the program reads the answer");
    });
    let (output, elapsed) = run_hook(Some("s"), Some(receiver.path()), STOP_PAYLOAD.as_bytes());
    app.join().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"decision\":\"block\",\"reason\":\"line one\\nline \\\"two\\\"\"}\n"
    );
    assert!(elapsed < Duration::from_millis(1500), "took {elapsed:?}");
}

/// The other events do not ask for an answer: the message has no reply.
#[test]
fn an_event_that_is_not_a_stop_asks_for_no_answer() {
    let dir = tempfile::tempdir().unwrap();
    let (receiver, events) = HookReceiver::bind(dir.path().join("t.sock")).unwrap();
    let (output, _) = run_hook(
        Some("s"),
        Some(receiver.path()),
        br#"{"hook_event_name":"PreToolUse","tool_name":"Bash"}"#,
    );
    assert_silent_success(&output);
    let message = events.recv_timeout(WAIT).unwrap();
    assert!(message.reply.is_none());
}

/// The app has nothing to say: it drops the reply. The program prints nothing and ends at once.
#[test]
fn a_stop_event_with_no_answer_prints_nothing_and_ends_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let (receiver, events) = HookReceiver::bind(dir.path().join("t.sock")).unwrap();
    let app = std::thread::spawn(move || {
        let message = events.recv_timeout(WAIT).unwrap();
        drop(message.reply);
    });
    let (output, elapsed) = run_hook(Some("s"), Some(receiver.path()), STOP_PAYLOAD.as_bytes());
    app.join().unwrap();
    assert_silent_success(&output);
    assert!(elapsed < Duration::from_millis(1500), "took {elapsed:?}");
}

/// A dead socket: nothing is printed, and the program ends fast.
#[test]
fn a_stop_event_with_a_dead_socket_prints_nothing_and_ends_fast() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist.sock");
    let (output, elapsed) = run_hook(Some("s"), Some(&missing), STOP_PAYLOAD.as_bytes());
    assert_silent_success(&output);
    assert!(elapsed < Duration::from_millis(1500), "took {elapsed:?}");

    // A socket file that nobody listens on (the app crashed).
    let stale = dir.path().join("stale.sock");
    drop(UnixListener::bind(&stale).unwrap());
    let (output, elapsed) = run_hook(Some("s"), Some(&stale), STOP_PAYLOAD.as_bytes());
    assert_silent_success(&output);
    assert!(elapsed < Duration::from_millis(1500), "took {elapsed:?}");
}

/// A slow app: it takes the request and never answers. The program ends inside its limit of 2 s,
/// prints nothing, and exits 0.
#[test]
fn a_stop_event_with_a_slow_app_ends_in_2_seconds_with_no_output() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("slow.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let app = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        // Hold the connection open for longer than the limit of the program.
        std::thread::sleep(Duration::from_millis(3500));
        drop(stream);
    });
    let (output, elapsed) = run_hook(Some("s"), Some(&path), STOP_PAYLOAD.as_bytes());
    assert_silent_success(&output);
    assert!(
        elapsed >= Duration::from_millis(1800) && elapsed < Duration::from_millis(2800),
        "took {elapsed:?}"
    );
    app.join().unwrap();
}

/// A late answer has no reader: the program is gone, and the write of the app fails. The app
/// then marks nothing.
#[test]
fn an_answer_after_the_program_has_ended_fails_to_write() {
    let dir = tempfile::tempdir().unwrap();
    let (receiver, events) = HookReceiver::bind(dir.path().join("t.sock")).unwrap();
    let hook_path = receiver.path().to_path_buf();
    let runner =
        std::thread::spawn(move || run_hook(Some("s"), Some(&hook_path), STOP_PAYLOAD.as_bytes()));
    let message = events.recv_timeout(WAIT).unwrap();
    let reply = message.reply.unwrap();
    let (output, _) = runner.join().unwrap(); // Ends after 2 s, with no answer.
    assert_silent_success(&output);
    assert!(
        reply
            .send(r#"{"decision":"block","reason":"too late"}"#)
            .is_err()
    );
}

/// The program prints only a real `block` decision with a text. Any other answer prints nothing.
#[test]
fn a_bad_answer_to_a_stop_event_prints_nothing() {
    for answer in [
        r#"{"decision":"approve","reason":"x"}"#,
        r#"{"decision":"block"}"#,
        r#"{"decision":"block","reason":""}"#,
        "not json at all",
        "[]",
        "",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (receiver, events) = HookReceiver::bind(dir.path().join("t.sock")).unwrap();
        let app = std::thread::spawn(move || {
            let message = events.recv_timeout(WAIT).unwrap();
            let _ = message.reply.unwrap().send(answer);
        });
        let (output, _) = run_hook(Some("s"), Some(receiver.path()), STOP_PAYLOAD.as_bytes());
        app.join().unwrap();
        assert_silent_success(&output);
    }
}
