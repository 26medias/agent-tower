//! Tests of the PTY layer. They start real children in a real PTY, and they use the real
//! `fake-cli` and `tower-hook` programs. A test must be able to fail: see the table of red
//! checks in the report of step 4a.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tower_core::env::{EnvMap, build_env};
use tower_core::hookrx::{HookMessage, HookReceiver};
use tower_core::pty::{LogConfig, PtyEvent, PtyExit, PtyProcess, PtyWriter, SpawnSpec, spawn};

/// Wait limit for anything that must happen. A test that passes does not wait this long.
const LIMIT: Duration = Duration::from_secs(15);

/// A short wait to check that something does NOT happen. It is the only fixed wait.
const QUIET: Duration = Duration::from_millis(400);

const PASTE_START: &str = "\x1b[200~";
const PASTE_END: &str = "\x1b[201~";
const READY: &str = "\x1b[?2004h";

/// Path of a program of this workspace. The first call builds `fake-cli` and `tower-hook`, so
/// `cargo test -p tower-core` is correct also when they were not built or are old.
fn bin(name: &str) -> PathBuf {
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let status = Command::new(cargo)
            .args(["build", "--quiet", "-p", "fakecli", "-p", "tower-hook"])
            .current_dir(root)
            .status()
            .expect("run cargo build");
        assert!(
            status.success(),
            "cargo build of fake-cli and tower-hook failed"
        );
    });
    let exe = std::env::current_exe().expect("test executable path");
    let path = exe
        .parent()
        .and_then(Path::parent)
        .expect("target folder")
        .join(name);
    assert!(path.is_file(), "{} is missing", path.display());
    path
}

fn os<S: AsRef<std::ffi::OsStr>>(text: S) -> OsString {
    text.as_ref().to_owned()
}

/// An environment with PATH `/usr/bin:/bin`, built by `build_env` from a fixed parent map.
fn env_with(extra: &[(&str, &str)]) -> EnvMap {
    let parent: EnvMap = [("HOME", "/tmp"), ("LANG", "C.UTF-8")]
        .iter()
        .map(|(k, v)| (os(k), os(v)))
        .collect();
    let extra: Vec<(OsString, OsString)> = extra.iter().map(|(k, v)| (os(k), os(v))).collect();
    build_env(&parent, &extra, &[]).expect("build_env")
}

fn wait_for(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + LIMIT;
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timeout while waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A process with its events, its writer and the output seen so far.
struct Live {
    process: PtyProcess,
    events: Receiver<PtyEvent>,
    writer: Box<dyn PtyWriter + Send>,
    output: Vec<u8>,
    /// `expect` searches from here.
    cursor: usize,
    /// Time of the chunk that ended the last `expect`, in ms since the process started.
    last_at_ms: u64,
    exit: Option<PtyExit>,
}

impl Live {
    fn start(spec: SpawnSpec) -> Live {
        let mut process = spawn(spec).expect("spawn");
        let events = process.take_events().expect("events");
        let writer = process.take_writer().expect("writer");
        Live {
            process,
            events,
            writer,
            output: Vec::new(),
            cursor: 0,
            last_at_ms: 0,
            exit: None,
        }
    }

    fn take(&mut self, event: PtyEvent) {
        match event {
            PtyEvent::Output { bytes, at_ms } => {
                self.output.extend_from_slice(&bytes);
                self.last_at_ms = at_ms;
            }
            PtyEvent::Exit(exit) => self.exit = Some(exit),
        }
    }

    fn find(&self, needle: &[u8]) -> Option<usize> {
        self.output[self.cursor..]
            .windows(needle.len())
            .position(|window| window == needle)
            .map(|at| self.cursor + at + needle.len())
    }

    /// Wait until `needle` is in the output after the cursor. Move the cursor behind it.
    fn expect(&mut self, needle: &str) {
        let deadline = Instant::now() + LIMIT;
        loop {
            if let Some(end) = self.find(needle.as_bytes()) {
                self.cursor = end;
                return;
            }
            assert!(
                self.exit.is_none(),
                "the process ended before {needle:?} came. Output: {:?}",
                String::from_utf8_lossy(&self.output)
            );
            let wait = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(wait) {
                Ok(event) => self.take(event),
                Err(error) => panic!(
                    "{error}: waiting for {needle:?}. Output: {:?}",
                    String::from_utf8_lossy(&self.output)
                ),
            }
        }
    }

    /// Wait for `Exit`. All output is in `self.output` after this.
    fn wait_exit(&mut self) -> PtyExit {
        let deadline = Instant::now() + LIMIT;
        while self.exit.is_none() {
            let wait = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(wait) {
                Ok(event) => self.take(event),
                Err(error) => panic!(
                    "{error}: waiting for Exit. Output: {:?}",
                    String::from_utf8_lossy(&self.output)
                ),
            }
        }
        // Exit is the last event.
        assert!(
            matches!(
                self.events.recv_timeout(Duration::from_millis(50)),
                Err(RecvTimeoutError::Disconnected | RecvTimeoutError::Timeout)
            ),
            "an event came after Exit"
        );
        self.exit.clone().expect("exit")
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.output).into_owned()
    }

    fn write(&mut self, text: &str) {
        self.writer.write_all(text.as_bytes()).expect("write");
    }
}

fn plain_spec(program: &str, args: &[&str], cwd: &Path) -> SpawnSpec {
    SpawnSpec {
        program: program.into(),
        args: args.iter().map(os).collect(),
        env: env_with(&[]),
        cwd: cwd.to_path_buf(),
        rows: 24,
        cols: 80,
        log: None,
    }
}

// ---------------------------------------------------------------------------------------------
// Working folder, environment, resize, exit, kill, drop, log
// ---------------------------------------------------------------------------------------------

#[test]
fn child_working_folder_is_the_given_cwd() {
    let dir = TempDir::new().unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    let mut live = Live::start(plain_spec("/bin/pwd", &[], &cwd));
    let exit = live.wait_exit();
    assert!(exit.success(), "{exit:?}");
    assert_eq!(live.text().trim(), cwd.display().to_string());
}

#[test]
fn spawn_with_a_missing_cwd_fails() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("does-not-exist");
    let result = spawn(plain_spec("/bin/pwd", &[], &missing));
    assert!(
        matches!(&result, Err(tower_core::pty::PtyError::CwdMissing(path)) if *path == missing),
        "{:?}",
        result.err()
    );
    // A file is not a folder.
    let file = dir.path().join("file");
    std::fs::write(&file, "x").unwrap();
    assert!(spawn(plain_spec("/bin/pwd", &[], &file)).is_err());
}

/// The PTY layer adds `SHELL` when the map has none (portable-pty 0.9 always sets it). The
/// test says so by name, so a new addition makes it fail.
#[test]
fn child_environment_is_exactly_the_given_map() {
    let dir = TempDir::new().unwrap();
    let given = env_with(&[("TOWER_SESSION", "s-1"), ("MY_VALUE", "a b=c")]);
    let mut spec = plain_spec("/usr/bin/env", &[], dir.path());
    spec.env = given.clone();
    let mut live = Live::start(spec);
    assert!(live.wait_exit().success());

    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for line in live.text().lines() {
        let (name, value) = line.split_once('=').expect("NAME=value line");
        seen.insert(name.to_owned(), value.to_owned());
    }
    // 1. Every given value arrives as it is.
    for (name, value) in &given {
        assert_eq!(
            seen.get(name.to_str().unwrap()).map(String::as_str),
            value.to_str(),
            "{name:?}"
        );
    }
    // 2. Nothing else arrives, except what the PTY layer adds.
    let added: Vec<&String> = seen
        .keys()
        .filter(|name| !given.contains_key(OsString::from(name.as_str()).as_os_str()))
        .collect();
    assert_eq!(added, vec!["SHELL"], "the PTY layer added: {added:?}");
    // 3. The variables of this test process are absent (cargo sets these for every test).
    assert!(!seen.contains_key("CARGO_MANIFEST_DIR"));
    assert!(!seen.contains_key("CARGO"));
}

#[test]
fn resize_is_seen_by_the_child() {
    let dir = TempDir::new().unwrap();
    let mut live = Live::start(plain_spec(
        "/bin/sh",
        &["-c", "read x; stty size"],
        dir.path(),
    ));
    live.process.resize(40, 100).unwrap();
    live.write("\n");
    live.expect("40 100");
    assert!(live.wait_exit().success());
}

#[test]
fn child_that_exits_gives_exit_with_its_code_after_all_output() {
    let dir = TempDir::new().unwrap();
    let mut live = Live::start(plain_spec(
        "/bin/sh",
        &["-c", "echo hello; exit 7"],
        dir.path(),
    ));
    let exit = live.wait_exit();
    assert_eq!(
        exit,
        PtyExit {
            code: 7,
            signal: None
        }
    );
    assert!(!live.process.is_running());
    assert!(live.text().contains("hello"), "{:?}", live.text());
}

#[test]
fn kill_gives_an_exit_event() {
    let dir = TempDir::new().unwrap();
    let mut live = Live::start(plain_spec("/bin/sleep", &["60"], dir.path()));
    assert!(live.process.is_running());
    live.process.kill().unwrap();
    let exit = live.wait_exit();
    assert!(exit.signal.is_some(), "{exit:?}");
    assert!(!exit.success());
    // A second kill after the end is not an error.
    live.process.kill().unwrap();
}

fn process_is_gone(pid: u32) -> bool {
    !Path::new(&format!("/proc/{pid}")).exists()
}

/// Run `drop(process)` in a thread and wait for it. A hang is a failure, not a stuck test.
fn drop_must_not_hang(process: PtyProcess, limit: Duration) {
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(process);
        let _ = done.send(());
    });
    finished.recv_timeout(limit).expect("drop hangs");
}

#[test]
fn drop_kills_the_child_and_does_not_hang() {
    let dir = TempDir::new().unwrap();
    let process = spawn(plain_spec("/bin/sleep", &["60"], dir.path())).unwrap();
    let pid = process.pid().expect("pid");
    drop_must_not_hang(process, Duration::from_secs(5));
    wait_for("the child to be gone", || process_is_gone(pid));
}

#[test]
fn drop_ends_a_child_that_ignores_sighup() {
    let dir = TempDir::new().unwrap();
    // `trap "" HUP` is inherited by `sleep` through `exec`.
    let process = spawn(plain_spec(
        "/bin/sh",
        &["-c", "trap '' HUP; echo ready; exec sleep 60"],
        dir.path(),
    ))
    .unwrap();
    let mut live_events = None;
    let mut process = process;
    if let Some(events) = process.take_events() {
        live_events = Some(events);
    }
    // Wait until the trap is set, or the first SIGHUP would end the shell.
    let events = live_events.expect("events");
    let mut seen = Vec::new();
    let deadline = Instant::now() + LIMIT;
    while !String::from_utf8_lossy(&seen).contains("ready") {
        let wait = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(wait).expect("output") {
            PtyEvent::Output { bytes, .. } => seen.extend(bytes),
            PtyEvent::Exit(exit) => panic!("child ended early: {exit:?}"),
        }
    }
    let pid = process.pid().expect("pid");
    drop_must_not_hang(process, Duration::from_secs(8));
    wait_for("the child to be gone", || process_is_gone(pid));
}

#[test]
fn log_file_gets_the_output_and_rotates_at_the_limit() {
    let dir = TempDir::new().unwrap();
    let log_path = dir.path().join("session.log");
    let limit = 4096u64;
    let mut spec = plain_spec("/bin/sh", &["-c", "seq 1 5000"], dir.path());
    spec.log = Some(LogConfig {
        path: log_path.clone(),
        max_bytes: limit,
    });
    let mut live = Live::start(spec);
    assert!(live.wait_exit().success());
    assert!(live.output.len() as u64 > 3 * limit, "not enough output");

    let old_path = dir.path().join("session.log.1");
    let new = std::fs::read(&log_path).unwrap();
    let old = std::fs::read(&old_path).expect("the old log file");
    assert!(new.len() as u64 <= limit, "log is {} bytes", new.len());
    assert!(old.len() as u64 <= limit, "old log is {} bytes", old.len());
    assert!(!new.is_empty());
    // Only one old file is kept.
    assert!(!dir.path().join("session.log.2").exists());
    // The two files are the tail of the output, with no gap and no change.
    let tail = [old, new].concat();
    assert!(
        live.output.ends_with(&tail),
        "the log is not the end of the output"
    );
}

#[test]
fn log_file_gets_small_output_in_one_file() {
    let dir = TempDir::new().unwrap();
    let log_path = dir.path().join("session.log");
    let mut spec = plain_spec("/bin/sh", &["-c", "echo small"], dir.path());
    spec.log = Some(LogConfig {
        path: log_path.clone(),
        max_bytes: 1 << 20,
    });
    let mut live = Live::start(spec);
    assert!(live.wait_exit().success());
    assert_eq!(std::fs::read(&log_path).unwrap(), live.output);
    assert!(!dir.path().join("session.log.1").exists());
}

// ---------------------------------------------------------------------------------------------
// fake-cli
// ---------------------------------------------------------------------------------------------

struct Fake {
    live: Live,
    hooks: Receiver<HookMessage>,
    _receiver: HookReceiver,
    dir: TempDir,
}

const SESSION: &str = "sess-1";

impl Fake {
    fn start(script: &str, extra_args: &[&str]) -> Fake {
        Fake::start_with_hook(script, extra_args, &bin("tower-hook"))
    }

    fn start_with_hook(script: &str, extra_args: &[&str], hook: &Path) -> Fake {
        let dir = TempDir::new().unwrap();
        let (receiver, hooks) = HookReceiver::bind(dir.path().join("hook.sock")).unwrap();
        let mut spec = plain_spec(bin("fake-cli").to_str().unwrap(), &[], dir.path());
        spec.env = env_with(&[
            ("TOWER_SESSION", SESSION),
            ("TOWER_SOCKET", receiver.path().to_str().unwrap()),
        ]);
        let mut args: Vec<OsString> = vec![os("--script"), os(script)];
        args.extend([os("--record"), dir.path().join("record").into_os_string()]);
        args.extend([os("--hook"), hook.as_os_str().to_owned()]);
        args.extend([os("--marker"), dir.path().join("marker").into_os_string()]);
        args.extend(extra_args.iter().map(os));
        spec.args = args;
        let mut live = Live::start(spec);
        // Raw mode is on before this text is written. Do not write before it.
        live.expect(READY);
        Fake {
            live,
            hooks,
            _receiver: receiver,
            dir,
        }
    }

    fn record(&self) -> Vec<u8> {
        std::fs::read(self.dir.path().join("record")).unwrap_or_default()
    }

    fn marker_exists(&self) -> bool {
        self.dir.path().join("marker").exists()
    }

    fn next_hook(&self) -> HookMessage {
        self.hooks.recv_timeout(LIMIT).expect("a hook event")
    }

    /// The next `count` hook events, as `hook_event_name`, in the order in which `fake-cli`
    /// sent them (field `fake_seq`). The socket server does not keep the arrival order of
    /// events from different connections (see the report), so the arrival order is not used.
    fn events(&self, count: usize) -> Vec<String> {
        let mut messages: Vec<HookMessage> = (0..count).map(|_| self.next_hook()).collect();
        messages.sort_by_key(|message| message.event["fake_seq"].as_u64());
        messages
            .iter()
            .map(|message| {
                assert_eq!(message.session, SESSION);
                message.event["hook_event_name"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }

    /// The next `count` events with their whole payload, in the order of `fake_seq`.
    fn messages(&self, count: usize) -> Vec<HookMessage> {
        let mut messages: Vec<HookMessage> = (0..count).map(|_| self.next_hook()).collect();
        messages.sort_by_key(|message| message.event["fake_seq"].as_u64());
        messages
    }

    fn assert_no_hook(&self) {
        match self.hooks.recv_timeout(QUIET) {
            Err(RecvTimeoutError::Timeout) => {}
            other => panic!("an event came when none was expected: {other:?}"),
        }
    }

    fn paste_prompt(&mut self, text: &str) {
        let bytes = format!("{PASTE_START}{text}{PASTE_END}\r");
        self.live.writer.write_all(bytes.as_bytes()).unwrap();
    }
}

#[test]
fn paste_reaches_fake_cli_byte_for_byte() {
    let mut fake = Fake::start("echo", &[]);
    assert_eq!(fake.events(1), ["SessionStart"]);
    let bytes = format!("{PASTE_START}line one\nline two{PASTE_END}\r");
    // One write, as `Session::paste` does.
    fake.live.writer.write_all(bytes.as_bytes()).unwrap();

    // UserPromptSubmit, then Stop.
    let messages = fake.messages(2);
    let message = &messages[0];
    assert_eq!(message.session, SESSION);
    assert_eq!(message.event["hook_event_name"], "UserPromptSubmit");
    assert_eq!(message.event["prompt"], "line one\nline two");
    assert_eq!(messages[1].event["hook_event_name"], "Stop");
    // The record has the bytes of the paste and nothing else.
    assert_eq!(fake.record(), bytes.as_bytes());
    fake.live.expect("you said: line one");
}

#[test]
fn a_large_write_arrives_whole() {
    // More than the buffer of the terminal: the writer must go on until all bytes are in.
    let text = "0123456789abcdef\n".repeat(1500);
    let mut fake = Fake::start("echo", &["--work-ms", "0"]);
    assert_eq!(fake.events(1), ["SessionStart"]);
    let bytes = format!("{PASTE_START}{text}{PASTE_END}\r");
    fake.live.writer.write_all(bytes.as_bytes()).unwrap();
    let messages = fake.messages(2);
    assert_eq!(messages[0].event["hook_event_name"], "UserPromptSubmit");
    assert_eq!(messages[0].event["prompt"].as_str().unwrap(), text);
    assert_eq!(fake.record(), bytes.as_bytes());
}

#[test]
fn typed_line_ended_by_cr_is_a_prompt() {
    let mut fake = Fake::start("echo", &[]);
    assert_eq!(fake.events(1), ["SessionStart"]);
    fake.live.write("hello\r");
    let messages = fake.messages(2);
    assert_eq!(messages[0].event["hook_event_name"], "UserPromptSubmit");
    assert_eq!(messages[0].event["prompt"], "hello");
    assert_eq!(fake.record(), b"hello\r");
}

#[test]
fn fake_cli_exit_command_gives_exit_with_its_code() {
    let mut fake = Fake::start("echo", &[]);
    assert_eq!(fake.events(1), ["SessionStart"]);
    fake.live.write("/exit 3\r");
    assert_eq!(fake.events(1), ["SessionEnd"]);
    assert_eq!(fake.live.wait_exit().code, 3);
}

#[test]
fn permission_dialog_enter_approves_and_writes_the_marker() {
    let mut fake = Fake::start("perm-dialog", &[]);
    assert_eq!(fake.events(1), ["SessionStart"]);
    fake.paste_prompt("do it");
    assert_eq!(
        fake.events(3),
        ["UserPromptSubmit", "PreToolUse", "PermissionRequest"]
    );
    fake.live.expect("1. Yes  2. No");
    assert!(!fake.marker_exists());
    // A paste with Enter into the dialog approves the action (spec 4.5).
    fake.live.write("\r");
    wait_for("the marker file", || fake.marker_exists());
    assert_eq!(fake.events(2), ["PostToolUse", "Stop"]);
}

#[test]
fn permission_dialog_two_denies_with_no_marker_and_no_event() {
    let mut fake = Fake::start("perm-dialog", &[]);
    assert_eq!(fake.events(1), ["SessionStart"]);
    fake.paste_prompt("do it");
    assert_eq!(
        fake.events(3),
        ["UserPromptSubmit", "PreToolUse", "PermissionRequest"]
    );
    fake.live.expect("1. Yes  2. No");
    fake.live.write("2");
    fake.live.expect("denied");
    // The prompt is back, and nothing was approved or sent.
    fake.live.expect("> ");
    assert!(!fake.marker_exists());
    fake.assert_no_hook();
}

#[test]
fn permission_dialog_esc_denies() {
    let mut fake = Fake::start("perm-dialog", &[]);
    assert_eq!(fake.events(1), ["SessionStart"]);
    fake.paste_prompt("do it");
    assert_eq!(fake.events(3).len(), 3);
    fake.live.expect("1. Yes  2. No");
    fake.live.write("\x1b");
    fake.live.expect("denied");
    assert!(!fake.marker_exists());
    fake.assert_no_hook();
}

#[test]
fn permission_dialog_codex_deny_sends_interrupt() {
    let mut fake = Fake::start("perm-dialog", &["--kind", "codex"]);
    fake.assert_no_hook();
    fake.paste_prompt("do it");
    assert_eq!(
        fake.events(4),
        [
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PermissionRequest"
        ]
    );
    fake.live.expect("1. Yes  2. No");
    fake.live.write("2");
    assert_eq!(fake.events(1), ["Interrupt"]);
    assert!(!fake.marker_exists());
}

/// The hook is called BEFORE the dialog is drawn. The hook here is slow, so the dialog must
/// come after the delay of the hook, and the event must be there when the dialog is.
#[test]
fn hook_is_called_before_the_dialog_is_drawn() {
    let dir = TempDir::new().unwrap();
    let slow = dir.path().join("slow-hook");
    let real = bin("tower-hook");
    std::fs::write(
        &slow,
        format!("#!/bin/sh\nsleep 0.5\nexec {}\n", real.display()),
    )
    .unwrap();
    std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
    // `--work-ms 0`: the only delays are the hooks, 0.5 s each. The time line:
    // SessionStart 0.0-0.5, UserPromptSubmit 0.5-1.0, PreToolUse 1.0-1.5,
    // PermissionRequest 1.5-2.0, and then the dialog. If the dialog came before the
    // PermissionRequest hook, it would show at 1.5 s or earlier.
    let mut fake = Fake::start_with_hook("perm-dialog", &["--work-ms", "0"], &slow);
    fake.paste_prompt("do it");
    fake.live.expect("1. Yes  2. No");
    let dialog_at_ms = fake.live.last_at_ms;
    assert!(
        dialog_at_ms >= 1900,
        "the dialog was drawn at {dialog_at_ms} ms, before the last hook ended"
    );
    assert_eq!(
        fake.events(4),
        [
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PermissionRequest"
        ]
    );
}

#[test]
fn startup_dialog_sends_no_hook_until_input() {
    let mut fake = Fake::start("startup-dialog", &[]);
    fake.live.expect("No, exit");
    fake.assert_no_hook();
    assert!(!fake.marker_exists());
    // Enter takes the dangerous default.
    fake.live.write("\r");
    wait_for("the marker file", || fake.marker_exists());
    assert_eq!(fake.events(1), ["SessionStart"]);
}

#[test]
fn startup_dialog_codex_sends_session_start_only_at_the_first_prompt() {
    let mut fake = Fake::start("startup-dialog", &["--kind", "codex"]);
    fake.live.expect("No, exit");
    fake.live.write("1");
    fake.live.expect("dialog answered");
    fake.live.expect("> ");
    assert!(!fake.marker_exists());
    fake.assert_no_hook();
    fake.paste_prompt("first");
    assert_eq!(fake.events(3), ["SessionStart", "UserPromptSubmit", "Stop"]);
}

#[test]
fn queued_turn_order() {
    let mut fake = Fake::start("queued-turn", &[]);
    assert_eq!(fake.events(1), ["SessionStart"]);
    fake.paste_prompt("first");
    // Only one event is sent until the second prompt comes, so there is no order to keep.
    let first = fake.next_hook();
    assert_eq!(first.event["hook_event_name"], "UserPromptSubmit");
    assert_eq!(first.event["prompt"], "first");
    // The second prompt arrives during the turn.
    fake.paste_prompt("second");
    // UserPromptSubmit of the second prompt, Stop of turn 1, then the queued turn with no
    // event at its start, then Stop.
    let messages = fake.messages(3);
    let names: Vec<&str> = messages
        .iter()
        .map(|m| m.event["hook_event_name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["UserPromptSubmit", "Stop", "Stop"]);
    assert_eq!(messages[0].event["prompt"], "second");
    fake.live.expect("turn 2: second");
    fake.assert_no_hook();
}
