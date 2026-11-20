//! Tests of `Session` over a real PTY, with the real `fake-cli`, `tower-hook`, `HookReceiver`
//! and `HookRouter`. Tests with a recording writer and no child are in `src/session.rs`.

mod support;

use serde_json::Value;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use support::{LIMIT, bin, env_with, os};
use tempfile::TempDir;
use tower_core::hookrx::{HookMessage, HookReceiver};
use tower_core::model::CliKind;
use tower_core::pty::SpawnSpec;
use tower_core::session::{
    HookRouter, OutputChunk, PasteRefused, Session, SessionHandle, SessionStreams, StatusChange,
};
use tower_core::status::Status;

const SESSION: &str = "sess-1";
const READY: &[u8] = b"\x1b[?2004h";

/// A short wait to check that something does NOT happen.
const QUIET: Duration = Duration::from_millis(400);

struct Rig {
    handle: SessionHandle,
    streams: SessionStreams,
    _hookrx: HookReceiver,
    _router: HookRouter,
    dir: TempDir,
    output: Vec<u8>,
    chunks: Vec<OutputChunk>,
    changes: Vec<StatusChange>,
    /// `expect_status` looks from here.
    cursor: usize,
}

impl Rig {
    fn start(script: &str, cli: CliKind, extra_args: &[&str]) -> Rig {
        let dir = TempDir::new().unwrap();
        let (hookrx, messages): (HookReceiver, Receiver<HookMessage>) =
            HookReceiver::bind(dir.path().join("hook.sock")).unwrap();
        let router = HookRouter::start(messages);
        // Subscribe before the process starts, so no event is lost.
        let events: Receiver<Value> = router.subscribe(SESSION);

        let mut args: Vec<OsString> = vec![os("--script"), os(script)];
        args.extend([os("--record"), dir.path().join("record").into_os_string()]);
        args.extend([os("--hook"), bin("tower-hook").into_os_string()]);
        args.extend([os("--marker"), dir.path().join("marker").into_os_string()]);
        if cli == CliKind::Codex {
            args.extend([os("--kind"), os("codex")]);
        }
        args.extend(extra_args.iter().map(os));
        let spec = SpawnSpec {
            program: bin("fake-cli"),
            args,
            env: env_with(&[
                ("TOWER_SESSION", SESSION),
                ("TOWER_SOCKET", hookrx.path().to_str().unwrap()),
            ]),
            cwd: dir.path().to_path_buf(),
            rows: 24,
            cols: 80,
            log: None,
        };
        let (handle, streams) = Session::start(spec, cli, SESSION, events).expect("start");
        Rig {
            handle,
            streams,
            _hookrx: hookrx,
            _router: router,
            dir,
            output: Vec::new(),
            chunks: Vec::new(),
            changes: Vec::new(),
            cursor: 0,
        }
    }

    fn pump(&mut self) {
        while let Ok(change) = self.streams.status.try_recv() {
            self.changes.push(change);
        }
        while let Ok(chunk) = self.streams.output.try_recv() {
            self.output.extend_from_slice(&chunk.bytes);
            self.chunks.push(chunk);
        }
    }

    fn wait(&mut self, what: &str, limit: Duration, mut condition: impl FnMut(&mut Rig) -> bool) {
        let deadline = Instant::now() + limit;
        loop {
            self.pump();
            if condition(self) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timeout while waiting for {what}. Statuses: {:?}. Output: {:?}",
                self.statuses(),
                String::from_utf8_lossy(&self.output)
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Wait for the next change to `status`, after the last one that was found.
    fn expect_status(&mut self, status: Status) -> StatusChange {
        self.expect_status_within(status, LIMIT)
    }

    fn expect_status_within(&mut self, status: Status, limit: Duration) -> StatusChange {
        let mut found = None;
        let from = self.cursor;
        self.wait(&format!("status {status:?}"), limit, |rig| {
            match rig.changes[from..].iter().position(|c| c.new == status) {
                Some(at) => {
                    found = Some(from + at);
                    true
                }
                None => false,
            }
        });
        let at = found.expect("found");
        self.cursor = at + 1;
        self.changes[at]
    }

    /// The statuses that were reached, in order.
    fn statuses(&self) -> Vec<Status> {
        self.changes.iter().map(|change| change.new).collect()
    }

    fn expect_output(&mut self, needle: &[u8]) {
        self.wait(&format!("output {needle:?}"), LIMIT, |rig| {
            rig.output.windows(needle.len()).any(|w| w == needle)
        });
    }

    /// The process drew its first output after raw mode: it is safe to write.
    fn ready(&mut self) {
        self.expect_output(READY);
    }

    fn record(&self) -> Vec<u8> {
        std::fs::read(self.dir.path().join("record")).unwrap_or_default()
    }

    fn marker(&self) -> PathBuf {
        self.dir.path().join("marker")
    }

    /// Wait a little and check that fake-cli recorded nothing more than `expected`.
    fn assert_record_stays(&mut self, expected: &[u8]) {
        std::thread::sleep(QUIET);
        self.pump();
        assert_eq!(self.record(), expected, "fake-cli recorded other bytes");
    }
}

fn paste_bytes(text: &str) -> Vec<u8> {
    format!("\x1b[200~{text}\x1b[201~\r").into_bytes()
}

#[test]
fn echo_claude_paste_arrives_and_status_goes_working_then_idle() {
    let mut rig = Rig::start("echo", CliKind::Claude, &[]);
    rig.expect_status(Status::Idle); // SessionStart
    rig.handle.paste("line one\nline two").unwrap();
    rig.expect_status(Status::Working);
    rig.expect_status(Status::Idle);
    assert_eq!(rig.record(), paste_bytes("line one\nline two"));
    assert_eq!(
        rig.statuses(),
        [Status::Idle, Status::Working, Status::Idle]
    );
    assert!(!rig.handle.message_waiting());
    // The output reached the subscriber, on one clock that does not go back.
    rig.expect_output(b"you said: line one");
    assert!(rig.chunks.windows(2).all(|pair| pair[0].at <= pair[1].at));
}

#[test]
fn permission_dialog_refuses_paste_and_enter_approves() {
    let mut rig = Rig::start("perm-dialog", CliKind::Claude, &[]);
    rig.expect_status(Status::Idle);
    rig.handle.paste("do it").unwrap();
    rig.expect_status(Status::Working);
    rig.expect_status(Status::Question);
    rig.expect_output(b"1. Yes  2. No");

    let before = paste_bytes("do it");
    assert_eq!(
        rig.handle.paste("a message"),
        Err(PasteRefused::Status(Status::Question))
    );
    assert!(rig.handle.message_waiting());
    rig.assert_record_stays(&before);
    assert!(
        !rig.marker().exists(),
        "the refused paste approved the action"
    );

    // The operator approves.
    rig.handle.operator_input(b"\r").unwrap();
    rig.wait("the marker file", LIMIT, |rig| rig.marker().exists());
    rig.expect_status(Status::Working); // PostToolUse
    rig.expect_status(Status::Idle); // Stop
    let mut expected = before;
    expected.push(b'\r');
    assert_eq!(rig.record(), expected);
}

#[test]
fn permission_dialog_deny_gives_idle_by_the_quiet_rule() {
    let mut rig = Rig::start("perm-dialog", CliKind::Claude, &[]);
    rig.expect_status(Status::Idle);
    rig.handle.paste("do it").unwrap();
    rig.expect_status(Status::Question);
    rig.expect_output(b"1. Yes  2. No");

    // Deny: Claude sends no hook. Only the quiet rule (2 s with no output) gives idle, and
    // only the timer thread can apply it, because nothing else happens.
    let key_at = Instant::now();
    rig.handle.operator_input(b"2").unwrap();
    rig.expect_output(b"denied");
    rig.expect_status_within(Status::Idle, Duration::from_secs(8));
    let waited = key_at.elapsed();
    assert!(
        waited >= Duration::from_millis(1900),
        "idle came too early: {waited:?}"
    );
    assert!(!rig.marker().exists());
    // After the deny the turn is over, so a paste works again.
    rig.handle.paste("second").unwrap();
    rig.wait("the second paste in the record", LIMIT, |rig| {
        rig.record().ends_with(&paste_bytes("second"))
    });
}

#[test]
fn startup_dialog_stays_starting_and_refuses_paste() {
    let mut rig = Rig::start("startup-dialog", CliKind::Claude, &[]);
    rig.ready();
    rig.expect_output(b"No, exit");
    assert_eq!(rig.handle.status(), Status::Starting);
    assert_eq!(
        rig.handle.paste("a message"),
        Err(PasteRefused::Status(Status::Starting))
    );
    assert!(rig.handle.message_waiting());
    rig.assert_record_stays(b"");
    assert!(!rig.marker().exists());
    assert_eq!(rig.handle.status(), Status::Starting);
    assert!(rig.statuses().is_empty());

    // Operator input always goes through. The operator answers the dialog.
    rig.handle.operator_input(b"\r").unwrap();
    assert!(!rig.handle.draft(), "an answer to a dialog is not a draft");
    rig.wait("the marker file", LIMIT, |rig| rig.marker().exists());
    rig.expect_status(Status::Idle); // SessionStart after the dialog
}

#[test]
fn codex_stays_starting_until_the_first_prompt() {
    let mut rig = Rig::start("echo", CliKind::Codex, &[]);
    rig.ready();
    rig.expect_output(b"> ");
    assert_eq!(rig.handle.status(), Status::Starting);
    assert_eq!(
        rig.handle.paste("m"),
        Err(PasteRefused::Status(Status::Starting))
    );
    rig.assert_record_stays(b"");
    assert!(rig.statuses().is_empty(), "no hook came, so no change");

    // The operator types the first prompt. In `starting` this sets no draft.
    rig.handle.operator_input(b"hello\r").unwrap();
    assert!(!rig.handle.draft());
    rig.expect_status(Status::Working); // SessionStart of Codex comes with the first prompt
    rig.expect_status(Status::Idle);
    rig.handle.paste("second").unwrap();
    let mut expected = b"hello\r".to_vec();
    expected.extend(paste_bytes("second"));
    rig.wait("the paste in the record", LIMIT, |rig| {
        rig.record() == expected
    });
}

#[test]
fn queued_turn_gives_no_idle_between_the_two_stops() {
    // Each turn works 600 ms, so the time between the two Stop events is large.
    let mut rig = Rig::start("queued-turn", CliKind::Claude, &["--work-ms", "600"]);
    rig.expect_status(Status::Idle);
    rig.handle.paste("first").unwrap();
    rig.expect_status(Status::Working);
    // The operator submits a second prompt during the turn.
    rig.handle.operator_input(b"second\r").unwrap();
    rig.expect_output(b"turn 2: second");
    let turn_two = rig
        .chunks
        .iter()
        .find(|chunk| String::from_utf8_lossy(&chunk.bytes).contains("turn 2"))
        .expect("the chunk of turn 2")
        .at;
    let idle = rig.expect_status(Status::Idle);
    // The Stop of turn 1 came just before "turn 2" was drawn. A wrong build would give idle
    // there. The right build gives idle at the Stop of turn 2, 600 ms later.
    assert!(
        idle.at.since(turn_two) >= 300,
        "idle came {} ms after the queued turn began",
        idle.at.since(turn_two)
    );
    assert_eq!(
        rig.statuses(),
        [Status::Idle, Status::Working, Status::Idle]
    );
    // UserPromptSubmit of the second prompt cleared the draft flag.
    assert!(!rig.handle.draft());
}

#[test]
fn a_draft_blocks_the_paste_until_it_is_sent_or_cleared() {
    let mut rig = Rig::start("echo", CliKind::Claude, &[]);
    rig.expect_status(Status::Idle);
    rig.handle.operator_input(b"typing").unwrap();
    assert!(rig.handle.draft());
    assert_eq!(rig.handle.paste("m"), Err(PasteRefused::Draft));
    assert!(rig.handle.message_waiting());
    rig.assert_record_stays(b"typing");

    // The operator sends the draft. UserPromptSubmit clears the flag.
    rig.handle.operator_input(b"\r").unwrap();
    rig.expect_status(Status::Working);
    rig.wait("the draft to be cleared", LIMIT, |rig| !rig.handle.draft());
    rig.expect_status(Status::Idle);
    rig.handle.paste("m").unwrap();

    // And the manual clear.
    rig.expect_status(Status::Working);
    rig.expect_status(Status::Idle);
    rig.handle.operator_input(b"abc").unwrap();
    assert_eq!(rig.handle.paste("m"), Err(PasteRefused::Draft));
    rig.handle.clear_draft();
    rig.handle.paste("m").unwrap();
}

#[test]
fn process_exit_gives_stopped_and_later_input_changes_nothing() {
    // Exit by a SessionEnd hook and then by the process end.
    let mut rig = Rig::start("echo", CliKind::Claude, &[]);
    rig.expect_status(Status::Idle);
    rig.handle.operator_input(b"/exit 3\r").unwrap();
    rig.expect_status(Status::Stopped);
    rig.wait("the writer to close", LIMIT, |rig| {
        rig.handle.operator_input(b"x").is_err()
    });
    let recorded = rig.record();
    let changes = rig.changes.len();
    // "/exit 3" was typed text, so the flag is set. It must stay as it is.
    let draft = rig.handle.draft();
    assert_eq!(rig.handle.paste("m"), Err(PasteRefused::Stopped));
    assert!(rig.handle.operator_input(b"abc").is_err());
    rig.handle.clear_draft();
    assert!(draft);
    assert!(rig.handle.operator_input(b"abc").is_err());
    assert!(
        !rig.handle.draft(),
        "input after the exit must not set the flag"
    );
    rig.assert_record_stays(&recorded);
    assert_eq!(rig.handle.status(), Status::Stopped);
    assert_eq!(rig.changes.len(), changes);
}

#[test]
fn kill_gives_stopped_with_no_hook() {
    let mut rig = Rig::start("echo", CliKind::Codex, &[]);
    rig.ready();
    assert_eq!(rig.handle.status(), Status::Starting);
    rig.handle.kill().unwrap();
    rig.expect_status(Status::Stopped);
    assert_eq!(rig.handle.paste("m"), Err(PasteRefused::Stopped));
    rig.wait("the writer to close", LIMIT, |rig| {
        rig.handle.operator_input(b"x").is_err()
    });
    assert_eq!(rig.handle.status(), Status::Stopped);
}

#[test]
fn dropping_a_running_session_does_not_hang() {
    let mut rig = Rig::start("echo", CliKind::Claude, &[]);
    rig.expect_status(Status::Idle);
    let pid = rig.handle.pid().expect("pid");
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(rig);
        let _ = done.send(());
    });
    finished
        .recv_timeout(Duration::from_secs(8))
        .expect("drop hangs");
    support::wait_for("the child to be gone", || {
        !std::path::Path::new(&format!("/proc/{pid}")).exists()
    });
}
