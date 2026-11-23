//! Helpers that the `adapter_*` tests share. Not a test crate itself (it sits in a folder).
#![allow(dead_code)]

use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Once, OnceLock};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tower_core::adapter::{Programs, StartContext};
use tower_core::env::EnvMap;
use tower_core::hookrx::HookMessage;
use tower_core::model::{CliKind, Employee, PermissionPolicy};
use tower_core::programs::Resolver;
use tower_core::pty::{PtyEvent, PtyExit, PtyProcess, SpawnSpec, spawn};

pub fn os(text: &str) -> OsString {
    OsString::from(text)
}

pub fn args_text(args: &[OsString]) -> Vec<String> {
    args.iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

pub fn employee(cli: CliKind, policy: PermissionPolicy) -> Employee {
    Employee {
        name: "Ada".into(),
        slug: "ada".into(),
        role: "engineer".into(),
        manager: None,
        cli,
        model: Some("haiku".into()),
        effort: Some("low".into()),
        permission: policy,
        role_text: "You write code.".into(),
    }
}

/// A fixed context. The paths do not exist; only the pure builders use it.
pub fn fixed_ctx(cli: CliKind, policy: PermissionPolicy) -> StartContext {
    let parent_env: EnvMap = [
        ("HOME", "/home/test"),
        ("LANG", "C.UTF-8"),
        ("TERM", "xterm"),
        ("CLAUDE_CODE_CHILD_SESSION", "1"),
        ("PATH", "/home/test/bin:/opt/cli/bin:/usr/bin"),
        ("CARGO_HOME", "/home/test/.cargo"),
        ("ANTHROPIC_API_KEY", "made-up-not-for-a-session"),
        ("PWD", "/home/test"),
    ]
    .iter()
    .map(|(name, value)| (os(name), os(value)))
    .collect();
    StartContext {
        employee: employee(cli, policy),
        cwd: PathBuf::from("/work/floor/wt1"),
        app_session_id: "app-session-A".into(),
        socket: PathBuf::from("/run/tower/hooks.sock"),
        hook_program: PathBuf::from("/opt/tower/bin/tower-hook"),
        programs: Programs {
            claude: PathBuf::from("/opt/cli/bin/claude"),
            codex: PathBuf::from("/opt/node/bin/codex"),
            node: Some(PathBuf::from("/opt/node/bin/node")),
        },
        parent_env,
        first_message: Some("Reply with only: OK".into()),
        instructions: Some("Be brief.".into()),
        rows: 40,
        cols: 120,
        log: None,
        mcp: None,
        worktrees: false,
        floor_allow: Vec::new(),
        codex_sandbox: false,
        claude_auto: false,
    }
}

/// The real program of this machine, found as the app finds it. `None` (with a message on
/// stderr) when it is not installed: the test then skips.
pub fn real_program(name: &str) -> Option<PathBuf> {
    static RESOLVER: OnceLock<Resolver> = OnceLock::new();
    let resolver = RESOLVER.get_or_init(Resolver::from_env);
    match resolver.resolve(name, None) {
        Ok(path) => Some(path),
        Err(error) => {
            eprintln!("SKIP: {name} is not installed on this machine ({error})");
            None
        }
    }
}

pub fn real_programs() -> Option<Programs> {
    let claude = real_program("claude");
    let codex = real_program("codex");
    let node = real_program("node");
    match (claude, codex) {
        (Some(claude), Some(codex)) => Some(Programs {
            claude,
            codex,
            node,
        }),
        _ => None,
    }
}

/// Path of `tower-hook`. The first call builds it, so `cargo test -p tower-core` is correct
/// also when it was not built or is old.
pub fn hook_bin() -> PathBuf {
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let status = Command::new(cargo)
            .args(["build", "--quiet", "-p", "tower-hook"])
            .current_dir(root)
            .status()
            .expect("run cargo build");
        assert!(status.success(), "cargo build of tower-hook failed");
    });
    let exe = std::env::current_exe().expect("test executable path");
    let path = exe
        .parent()
        .and_then(Path::parent)
        .expect("target folder")
        .join("tower-hook");
    assert!(path.is_file(), "{} is missing", path.display());
    path
}

/// Run a program with the exact environment of a plan, and return (success, stdout, stderr).
pub fn run_plan_command(
    program: &Path,
    args: &[OsString],
    env: &EnvMap,
    cwd: &Path,
) -> (bool, String, String) {
    let output = Command::new(program)
        .args(args)
        .env_clear()
        .envs(env)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run the program");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// True when `flag` is a whole word of the help text (not a prefix of a longer flag).
pub fn help_has_flag(help: &str, flag: &str) -> bool {
    let bytes = help.as_bytes();
    let mut from = 0;
    while let Some(found) = help[from..].find(flag) {
        let start = from + found;
        let end = start + flag.len();
        let before_ok = start == 0 || !is_flag_char(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_flag_char(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = end;
    }
    false
}

fn is_flag_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

/// The flags in an argument vector: each argument that starts with `-`. The tests use values
/// that do not start with `-`.
pub fn flags_of(args: &[OsString]) -> Vec<String> {
    args_text(args)
        .into_iter()
        .filter(|arg| arg.starts_with('-') && arg.len() > 1)
        .collect()
}

/// A git worktree of a repository that Claude trusts. Claude shows no trust dialog in such a
/// worktree (PROBED at step 0). In any other new folder it shows the dialog and fires no hook.
/// The worktree is removed when this value is dropped.
pub struct TrustedWorktree {
    pub repo: PathBuf,
    pub dir: TempDir,
}

impl TrustedWorktree {
    pub fn path(&self) -> PathBuf {
        self.dir.path().join("wt")
    }
}

impl Drop for TrustedWorktree {
    fn drop(&mut self) {
        let path = self.path();
        let git = |args: &[&str]| {
            let _ = Command::new("git")
                .args(args)
                .current_dir(&self.repo)
                .output();
        };
        git(&["worktree", "remove", "--force", &path.to_string_lossy()]);
        git(&["worktree", "prune"]);
    }
}

/// Folders that Claude trusts, from its own config file.
pub fn trusted_folders() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let Ok(text) = std::fs::read_to_string(home.join(".claude.json")) else {
        return Vec::new();
    };
    let Ok(config) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let Some(projects) = config["projects"].as_object() else {
        return Vec::new();
    };
    projects
        .iter()
        .filter(|(_, project)| project["hasTrustDialogAccepted"] == true)
        .map(|(path, _)| PathBuf::from(path))
        .collect()
}

/// A worktree of a trusted repository, or `None` (with a message) when there is none.
/// The repository is `$TOWER_TEST_TRUSTED_REPO`, or one of the repositories of the step 0 probes.
pub fn trusted_worktree() -> Option<TrustedWorktree> {
    let trusted = trusted_folders();
    let mut candidates: Vec<PathBuf> = std::env::var_os("TOWER_TEST_TRUSTED_REPO")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    candidates.push("/tmp/agent-tower-step0/repo".into());
    candidates.push("/tmp/agent-tower-probes/repo".into());
    let repo = candidates
        .into_iter()
        .find(|path| path.join(".git").exists() && trusted.contains(path));
    let Some(repo) = repo else {
        eprintln!(
            "SKIP: no trusted git repository found. Claude shows a trust dialog in an untrusted \
             folder and fires no hook. Set TOWER_TEST_TRUSTED_REPO to a repository that Claude \
             trusts (with at least one commit)."
        );
        return None;
    };
    let dir = TempDir::new().expect("temporary folder");
    let path = dir.path().join("wt");
    let output = Command::new("git")
        .args(["worktree", "add", "--detach", "--quiet"])
        .arg(&path)
        .current_dir(&repo)
        .output()
        .expect("git worktree add");
    assert!(
        output.status.success(),
        "git worktree add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Some(TrustedWorktree { repo, dir })
}

/// Text of a terminal output without escape sequences, lower case, letters and digits only.
/// A TUI moves the cursor between words, so spaces are not reliable.
pub fn squash(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for d in chars.by_ref() {
                        if ('@'..='~').contains(&d) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    for d in chars.by_ref() {
                        if d == '\u{7}' || d == '\\' {
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// The last `count` characters of the terminal output, for a failure message.
pub fn output_tail(bytes: &[u8], count: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    let chars: Vec<char> = text.chars().collect();
    chars[chars.len().saturating_sub(count)..].iter().collect()
}

/// Why a wait ended without the event.
#[derive(Debug)]
pub enum WaitEnd {
    /// The process ended.
    Exited(PtyExit),
    /// The time limit passed. The process still runs.
    TimedOut,
}

/// A CLI process in a PTY, with the hook messages of its session. It never writes to the PTY.
pub struct LiveRun {
    process: PtyProcess,
    events: Receiver<tower_core::pty::PtyEvent>,
    _writer: Box<dyn tower_core::pty::PtyWriter + Send>,
    pub output: Vec<u8>,
    pub exit: Option<PtyExit>,
    /// Each hook message that arrived for this run, in order.
    pub seen: Vec<HookMessage>,
}

impl LiveRun {
    pub fn start(spec: SpawnSpec) -> LiveRun {
        let mut process = spawn(spec).expect("spawn the CLI");
        let events = process.take_events().expect("events");
        let writer = process.take_writer().expect("writer");
        LiveRun {
            process,
            events,
            _writer: writer,
            output: Vec::new(),
            exit: None,
            seen: Vec::new(),
        }
    }

    fn drain_pty(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                PtyEvent::Output { bytes, .. } => self.output.extend_from_slice(&bytes),
                PtyEvent::Exit(status) => self.exit = Some(status),
            }
        }
    }

    /// Wait for a hook message that `accept` takes (by its position in `seen`, so a message that
    /// came earlier counts). Messages of other sessions are ignored.
    pub fn wait_for(
        &mut self,
        messages: &Receiver<HookMessage>,
        session: &str,
        limit: Duration,
        accept: impl Fn(&Value) -> bool,
        from: usize,
    ) -> Result<HookMessage, WaitEnd> {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(found) = self.seen[from.min(self.seen.len())..]
                .iter()
                .find(|message| accept(&message.event))
            {
                return Ok(found.clone());
            }
            if let Some(exit) = &self.exit {
                return Err(WaitEnd::Exited(exit.clone()));
            }
            if Instant::now() >= deadline {
                return Err(WaitEnd::TimedOut);
            }
            match messages.recv_timeout(Duration::from_millis(100)) {
                Ok(message) if message.session == session => self.seen.push(message),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => panic!("the hook receiver stopped"),
            }
            self.drain_pty();
        }
    }

    /// The names of the hook events seen so far.
    pub fn event_names(&self) -> Vec<String> {
        self.seen
            .iter()
            .map(|m| {
                m.event["hook_event_name"]
                    .as_str()
                    .unwrap_or("?")
                    .to_owned()
            })
            .collect()
    }

    /// Ask the process to end and wait until it has ended (15 s at most). Late hook messages
    /// (SessionEnd) are kept in `seen`.
    pub fn stop(&mut self, messages: &Receiver<HookMessage>, session: &str) {
        let _ = self.process.kill();
        let deadline = Instant::now() + Duration::from_secs(15);
        while self.exit.is_none() && Instant::now() < deadline {
            match messages.recv_timeout(Duration::from_millis(100)) {
                Ok(message) if message.session == session => self.seen.push(message),
                _ => {}
            }
            self.drain_pty();
        }
        // Take the messages that came with the end of the process.
        while let Ok(message) = messages.try_recv() {
            if message.session == session {
                self.seen.push(message);
            }
        }
    }
}

pub fn hook_name_is(name: &'static str) -> impl Fn(&Value) -> bool {
    move |event| event["hook_event_name"] == name
}
