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
