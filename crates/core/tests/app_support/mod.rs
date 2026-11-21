//! Helpers for the `app_*.rs` tests: a `Tower` over a temp folder, with a test adapter that
//! starts the real `fake-cli` (and the real `tower-hook`) in place of Claude and Codex.
#![allow(dead_code)]

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex, Once, PoisonError};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tower_core::adapter::{CliAdapter, McpAccess, SpawnPlan, StartContext};
use tower_core::app::{
    EmployeeStarted, FloorView, MessageDelivered, Observer, OutputSink, QuestionAsked, SessionView,
    Tower, TowerConfig, TowerError,
};
use tower_core::env::{EnvMap, build_env};
use tower_core::login_env::LoginEnvSource;
use tower_core::model::{CliKind, Employee, PermissionPolicy};
use tower_core::programs::Resolver;
use tower_core::pty::SpawnSpec;
use tower_core::status::Status;
use tower_core::store::Database;

pub const LIMIT: Duration = Duration::from_secs(20);

pub fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Path of a program of this workspace (builds `fake-cli` and `tower-hook` once).
pub fn bin(name: &str) -> PathBuf {
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let status = Command::new(cargo)
            .args(["build", "--quiet", "-p", "fakecli", "-p", "tower-hook"])
            .current_dir(root)
            .status()
            .expect("run cargo build");
        assert!(status.success(), "cargo build of the test programs failed");
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

pub fn wait_for(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + LIMIT;
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timeout while waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

// ---------------------------------------------------------------------------------------------
// The observer
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Ev {
    Session(PathBuf, SessionView),
    Floor(PathBuf),
}

#[derive(Default)]
pub struct Events {
    log: Mutex<Vec<Ev>>,
    changed: Condvar,
    /// The `employee_started` events, oldest first.
    pub started: Mutex<Vec<(PathBuf, EmployeeStarted)>>,
    /// The floors of the `tasks_changed` events, oldest first.
    pub tasks: Mutex<Vec<PathBuf>>,
    /// The `notice` events (floor, kind, text), oldest first.
    pub notices: Mutex<Vec<(PathBuf, String, String)>>,
    /// The `notice_for` events (floor, kind, text, slug of the employee), oldest first. They are
    /// also in `notices`.
    pub employee_notices: Mutex<Vec<(PathBuf, String, String, String)>>,
    /// The `question_asked` events, oldest first.
    pub questions: Mutex<Vec<(PathBuf, QuestionAsked)>>,
    /// The `message_delivered` events, oldest first.
    pub delivered: Mutex<Vec<(PathBuf, MessageDelivered)>>,
}

impl Observer for Events {
    fn session_changed(&self, path: &Path, session: &SessionView) {
        lock(&self.log).push(Ev::Session(path.to_path_buf(), session.clone()));
        self.changed.notify_all();
    }

    fn floor_changed(&self, path: &Path) {
        lock(&self.log).push(Ev::Floor(path.to_path_buf()));
        self.changed.notify_all();
    }

    fn employee_started(&self, path: &Path, event: &EmployeeStarted) {
        lock(&self.started).push((path.to_path_buf(), event.clone()));
        self.changed.notify_all();
    }

    fn tasks_changed(&self, path: &Path) {
        lock(&self.tasks).push(path.to_path_buf());
        self.changed.notify_all();
    }

    fn notice(&self, path: &Path, kind: &str, text: &str) {
        lock(&self.notices).push((path.to_path_buf(), kind.to_owned(), text.to_owned()));
        self.changed.notify_all();
    }

    fn notice_for(&self, path: &Path, kind: &str, text: &str, slug: &str) {
        lock(&self.employee_notices).push((
            path.to_path_buf(),
            kind.to_owned(),
            text.to_owned(),
            slug.to_owned(),
        ));
        self.notice(path, kind, text);
    }

    fn question_asked(&self, path: &Path, event: &QuestionAsked) {
        lock(&self.questions).push((path.to_path_buf(), event.clone()));
        self.changed.notify_all();
    }
    fn message_delivered(&self, path: &Path, event: &MessageDelivered) {
        lock(&self.delivered).push((path.to_path_buf(), event.clone()));
        self.changed.notify_all();
    }
}

impl Events {
    /// The `message_delivered` events, oldest first (without the floor path).
    pub fn delivered_events(&self) -> Vec<MessageDelivered> {
        lock(&self.delivered)
            .iter()
            .map(|(_, e)| e.clone())
            .collect()
    }
}

impl Events {
    pub fn all(&self) -> Vec<Ev> {
        lock(&self.log).clone()
    }

    /// The views of one session, oldest first.
    pub fn views(&self, session_id: &str) -> Vec<SessionView> {
        self.all()
            .into_iter()
            .filter_map(|ev| match ev {
                Ev::Session(_, view) if view.session_id == session_id => Some(view),
                _ => None,
            })
            .collect()
    }

    /// The statuses of one session, oldest first, with no repeat in a row.
    pub fn statuses(&self, session_id: &str) -> Vec<Status> {
        let mut out: Vec<Status> = Vec::new();
        for view in self.views(session_id) {
            if out.last() != Some(&view.status) {
                out.push(view.status);
            }
        }
        out
    }

    pub fn latest(&self, session_id: &str) -> Option<SessionView> {
        self.views(session_id).pop()
    }

    pub fn wait(&self, what: &str, mut predicate: impl FnMut(&Events) -> bool) {
        let deadline = Instant::now() + LIMIT;
        while !predicate(self) {
            assert!(
                Instant::now() < deadline,
                "timeout while waiting for {what}. Events: {:?}",
                self.all()
            );
            let guard = lock(&self.log);
            let _ = self.changed.wait_timeout(guard, Duration::from_millis(10));
        }
    }

    pub fn wait_status(&self, session_id: &str, status: Status) {
        self.wait(&format!("status {status:?} of {session_id}"), |events| {
            events
                .latest(session_id)
                .is_some_and(|v| v.status == status)
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Output sink that collects bytes
// ---------------------------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct Collector {
    pub bytes: Arc<Mutex<Vec<u8>>>,
}

impl OutputSink for Collector {
    fn output(&self, bytes: &[u8]) {
        lock(&self.bytes).extend_from_slice(bytes);
    }
}

impl Collector {
    pub fn get(&self) -> Vec<u8> {
        lock(&self.bytes).clone()
    }
}

// ---------------------------------------------------------------------------------------------
// The test adapter
// ---------------------------------------------------------------------------------------------

/// What the test adapter was asked to build.
#[derive(Debug, Clone)]
pub struct Call {
    pub slug: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission: PermissionPolicy,
    pub app_session_id: String,
    /// `Some(id)` for a resume.
    pub resume: Option<String>,
    pub args: Vec<String>,
    pub first_message: Option<String>,
    pub instructions: Option<String>,
    pub cwd: PathBuf,
    pub hook_program: PathBuf,
    pub socket: PathBuf,
    pub log: Option<(PathBuf, u64)>,
    /// The CLI session ID that the plan gave (`None` for a fresh Codex).
    pub plan_cli_session_id: Option<String>,
    /// The parent environment that the app gave to the adapter (`ctx.parent_env`).
    pub parent_env: EnvMap,
    /// The environment of the process (`build_env` of `parent_env`).
    pub env: EnvMap,
    /// The MCP server that the app gave to the session (`None` for the setup session).
    pub mcp: Option<McpAccess>,
    /// `StartContext::worktrees`: the floor is a git repository with worktrees.
    pub worktrees: bool,
    /// `StartContext::floor_allow`: the rules of the floor (from the app config) that the app gave.
    pub floor_allow: Vec<String>,
    /// `StartContext::codex_sandbox`: the Codex sandbox switch at the start of this session.
    pub codex_sandbox: bool,
}

#[derive(Default)]
pub struct Shared {
    pub calls: Mutex<Vec<Call>>,
    /// The `--script` of each employee slug. The default is `echo`.
    pub scripts: Mutex<HashMap<String, String>>,
    /// Arguments that go to each `fake-cli`.
    pub extra_args: Mutex<Vec<String>>,
    /// Replace the program (the test of a process that ignores SIGHUP).
    pub program: Mutex<Option<(PathBuf, Vec<String>)>>,
    /// The adapter refuses to make a plan for these employee slugs (a start that fails).
    pub fail_slugs: Mutex<std::collections::HashSet<String>>,
}

pub struct TestAdapter {
    pub shared: Arc<Shared>,
    pub kind: CliKind,
}

pub fn record_path(cwd: &Path, id: &str) -> PathBuf {
    cwd.join(".swarm/state").join(format!("{id}.record"))
}

pub fn hook_record_path(cwd: &Path, id: &str) -> PathBuf {
    cwd.join(".swarm/state").join(format!("{id}.hooks"))
}

pub fn marker_path(cwd: &Path, id: &str) -> PathBuf {
    cwd.join(".swarm/state").join(format!("{id}.marker"))
}

impl TestAdapter {
    fn build(
        &self,
        ctx: &StartContext,
        resume: Option<&str>,
    ) -> tower_core::adapter::Result<SpawnPlan> {
        let id = &ctx.app_session_id;
        if lock(&self.shared.fail_slugs).contains(&ctx.employee.slug) {
            return Err(tower_core::adapter::AdapterError::BadCliSessionId(
                "forced test failure".to_owned(),
            ));
        }
        let cli_id = match (resume, self.kind) {
            (Some(resume), _) => resume.to_owned(),
            (None, CliKind::Claude) => format!("claude-{}", &id[..8]),
            (None, CliKind::Codex) => format!("codex-{}", &id[..8]),
        };
        let plan_id = match (resume, self.kind) {
            (Some(resume), _) => Some(resume.to_owned()),
            (None, CliKind::Claude) => Some(cli_id.clone()),
            (None, CliKind::Codex) => None,
        };
        let script = lock(&self.shared.scripts)
            .get(&ctx.employee.slug)
            .cloned()
            .unwrap_or_else(|| "echo".to_owned());
        let mut args: Vec<String> = vec!["--script".into(), script];
        args.extend([
            "--record".into(),
            record_path(&ctx.cwd, id).display().to_string(),
            "--hook".into(),
            ctx.hook_program.display().to_string(),
            "--marker".into(),
            marker_path(&ctx.cwd, id).display().to_string(),
            "--hook-record".into(),
            hook_record_path(&ctx.cwd, id).display().to_string(),
            "--cli-session-id".into(),
            cli_id,
        ]);
        if self.kind == CliKind::Codex {
            args.extend(["--kind".into(), "codex".into()]);
        }
        args.extend(lock(&self.shared.extra_args).iter().cloned());
        if let Some(message) = &ctx.first_message {
            args.push(message.clone());
        }
        let extra: Vec<(OsString, OsString)> = vec![
            ("TOWER_SESSION".into(), id.into()),
            ("TOWER_SOCKET".into(), ctx.socket.clone().into_os_string()),
        ];
        let env = build_env(&ctx.parent_env, &extra, &[])?;
        let (program, args) = match lock(&self.shared.program).clone() {
            Some((program, program_args)) => (program, program_args),
            None => (bin("fake-cli"), args.clone()),
        };
        lock(&self.shared.calls).push(Call {
            slug: ctx.employee.slug.clone(),
            model: ctx.employee.model.clone(),
            effort: ctx.employee.effort.clone(),
            permission: ctx.employee.permission,
            app_session_id: id.clone(),
            resume: resume.map(str::to_owned),
            args: args.clone(),
            first_message: ctx.first_message.clone(),
            instructions: ctx.instructions.clone(),
            cwd: ctx.cwd.clone(),
            hook_program: ctx.hook_program.clone(),
            socket: ctx.socket.clone(),
            log: ctx.log.as_ref().map(|l| (l.path.clone(), l.max_bytes)),
            plan_cli_session_id: plan_id.clone(),
            parent_env: ctx.parent_env.clone(),
            env: env.clone(),
            mcp: ctx.mcp.clone(),
            worktrees: ctx.worktrees,
            floor_allow: ctx.floor_allow.clone(),
            codex_sandbox: ctx.codex_sandbox,
        });
        Ok(SpawnPlan {
            spec: SpawnSpec {
                program,
                args: args.into_iter().map(OsString::from).collect(),
                env,
                cwd: ctx.cwd.clone(),
                rows: ctx.rows,
                cols: ctx.cols,
                log: ctx.log.clone(),
            },
            cli_session_id: plan_id,
        })
    }
}

impl CliAdapter for TestAdapter {
    fn start(&self, ctx: &StartContext) -> tower_core::adapter::Result<SpawnPlan> {
        self.build(ctx, None)
    }

    fn resume(
        &self,
        ctx: &StartContext,
        cli_session_id: &str,
    ) -> tower_core::adapter::Result<SpawnPlan> {
        self.build(ctx, Some(cli_session_id))
    }
}

// ---------------------------------------------------------------------------------------------
// The rig
// ---------------------------------------------------------------------------------------------

pub struct Rig {
    /// The working folder of the floor (a real path).
    pub cwd: PathBuf,
    pub config: PathBuf,
    pub socket: PathBuf,
    pub tower: Arc<Tower>,
    pub events: Arc<Events>,
    pub shared: Arc<Shared>,
    /// Last: the temp folder goes after the Tower (drop order is the order of the fields).
    pub root: TempDir,
}

impl Rig {
    pub fn new() -> Rig {
        Rig::with(|_| {})
    }

    pub fn with(tweak: impl FnOnce(&mut TowerConfig)) -> Rig {
        let root = TempDir::new().unwrap();
        let base = std::fs::canonicalize(root.path()).unwrap();
        let cwd = base.join("project");
        std::fs::create_dir(&cwd).unwrap();
        let config = base.join("config");
        let socket = base.join("hook.sock");
        let shared = Arc::new(Shared::default());
        let tower = Rig::make_tower(&config, &socket, &shared, tweak);
        let events = Arc::new(Events::default());
        tower.set_observer(events.clone());
        Rig {
            root,
            cwd,
            config,
            socket,
            tower: Arc::new(tower),
            events,
            shared,
        }
    }

    pub fn make_tower(
        config: &Path,
        socket: &Path,
        shared: &Arc<Shared>,
        tweak: impl FnOnce(&mut TowerConfig),
    ) -> Tower {
        let mut cfg = TowerConfig::new(config);
        cfg.socket_path = Some(socket.to_path_buf());
        cfg.hook_program = Some(bin("tower-hook"));
        cfg.resolver = Resolver {
            shell: None,
            known_dirs: vec![],
            shell_timeout: Duration::from_secs(1),
        };
        cfg.parent_env = [("HOME", "/tmp"), ("LANG", "C.UTF-8")]
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect::<EnvMap>();
        // No test runs a real login shell. A test that wants a map or a shell gives its own in
        // `tweak`.
        cfg.login_env = LoginEnvSource::Fixed(None);
        let adapters = Arc::clone(shared);
        cfg.adapters = Arc::new(move |kind| {
            Box::new(TestAdapter {
                shared: Arc::clone(&adapters),
                kind,
            })
        });
        // The Codex home is a folder of the test, never the real one. It has no file at first.
        cfg.codex_home = config.parent().map(|base| base.join("codex-home"));
        // The floor watcher is off by default in tests. A test of the watcher turns it on.
        cfg.floor_watch = None;
        tweak(&mut cfg);
        let tower = Tower::new(cfg).expect("make the tower");
        for name in ["claude", "codex"] {
            tower
                .programs_set(name, bin("fake-cli").to_str().unwrap())
                .expect("set a program");
        }
        // The session PATH of a test finds this folder first. It holds `node` and a `bwrap` that
        // fails, so the Codex sandbox does not work in a test unless the test says so (the real
        // `/usr/bin/bwrap` of the machine must not change a test). The default state of the
        // setting is "Automatic", so a Codex start in a test gets full access, as before. A test
        // of the sandbox writes its own `bwrap` in its own folder (`app_codex_sandbox.rs`).
        let tools = config
            .parent()
            .map_or_else(|| config.join("tools"), |base| base.join("rig-tools"));
        std::fs::create_dir_all(&tools).expect("make the tools folder");
        let node = tools.join("node");
        if !node.exists() {
            std::os::unix::fs::symlink("/bin/true", &node).expect("link node");
        }
        let bwrap = tools.join("bwrap");
        if !bwrap.exists() {
            std::fs::write(
                &bwrap,
                "#!/bin/sh\necho 'bwrap: the rig has no sandbox' >&2\nexit 1\n",
            )
            .expect("write bwrap");
            std::fs::set_permissions(&bwrap, std::fs::Permissions::from_mode(0o755))
                .expect("make bwrap executable");
        }
        tower
            .programs_set("node", node.to_str().unwrap())
            .expect("set node");
        tower
    }

    pub fn set_script(&self, slug: &str, script: &str) {
        lock(&self.shared.scripts).insert(slug.to_owned(), script.to_owned());
    }

    pub fn calls(&self) -> Vec<Call> {
        lock(&self.shared.calls).clone()
    }

    /// Create the floor with the default employees.
    pub fn create_floor(&self) -> FloorView {
        self.tower
            .floor_create(&self.cwd, "Test Floor")
            .expect("create the floor")
    }

    /// Make the employee a Codex employee.
    pub fn make_codex(&self, slug: &str) {
        let view = self.tower.floor_open(&self.cwd).unwrap();
        let mut employee: Employee = view
            .employees
            .into_iter()
            .find(|e| e.slug == slug)
            .expect("employee");
        employee.cli = CliKind::Codex;
        self.tower.employee_save(&self.cwd, employee).unwrap();
    }

    pub fn start(&self, slug: &str) -> SessionView {
        self.tower
            .session_start(&self.cwd, slug, None, 24, 80)
            .expect("start a session")
    }

    pub fn start_with(&self, slug: &str, message: &str) -> SessionView {
        self.tower
            .session_start(&self.cwd, slug, Some(message.to_owned()), 24, 80)
            .expect("start a session")
    }

    pub fn try_start(&self, slug: &str) -> Result<SessionView, TowerError> {
        self.tower.session_start(&self.cwd, slug, None, 24, 80)
    }

    /// Wait until the session is idle after its first message.
    pub fn wait_first_turn(&self, id: &str) {
        self.events.wait("working then idle", |events| {
            let list = events.statuses(id);
            list.windows(2)
                .any(|pair| pair == [Status::Working, Status::Idle])
        });
    }

    pub fn db(&self) -> Database {
        Database::open(&self.cwd.join(".swarm/state/floor.db")).expect("open the database")
    }

    pub fn log_path(&self, id: &str) -> PathBuf {
        self.cwd.join(".swarm/state/logs").join(format!("{id}.log"))
    }

    /// The bytes of the PTY log (the old file first).
    pub fn log_bytes(&self, id: &str) -> Vec<u8> {
        let current = self.log_path(id);
        let mut old_name = current.as_os_str().to_owned();
        old_name.push(".1");
        let mut bytes = std::fs::read(PathBuf::from(old_name)).unwrap_or_default();
        bytes.extend(std::fs::read(current).unwrap_or_default());
        bytes
    }

    pub fn record(&self, id: &str) -> Vec<u8> {
        std::fs::read(record_path(&self.cwd, id)).unwrap_or_default()
    }

    /// Send a hook event to the socket, as `tower-hook` does (for a process that sends none).
    pub fn send_hook(&self, session_id: &str, event: &str) {
        use std::io::Write;
        let line = format!(
            "{{\"session\":\"{session_id}\",\"event\":{{\"hook_event_name\":\"{event}\"}}}}\n"
        );
        let mut stream = std::os::unix::net::UnixStream::connect(&self.socket).expect("connect");
        stream
            .write_all(line.as_bytes())
            .expect("write the hook line");
    }

    /// Send a `Stop` event that asks for an answer, as `tower-hook` does, and return the answer
    /// line. `None` means the app closed the connection with no answer.
    pub fn stop_request(
        &self,
        session_id: &str,
        stop_hook_active: bool,
    ) -> Option<serde_json::Value> {
        use std::io::{BufRead, BufReader, Write};
        let line = serde_json::json!({
            "session": session_id,
            "event": {"hook_event_name": "Stop", "stop_hook_active": stop_hook_active},
            "reply": true,
        });
        let mut stream = std::os::unix::net::UnixStream::connect(&self.socket).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        writeln!(stream, "{line}").expect("write the request");
        let mut answer = String::new();
        BufReader::new(stream)
            .read_line(&mut answer)
            .expect("the app answers or closes");
        (!answer.is_empty()).then(|| serde_json::from_str(&answer).expect("a JSON answer"))
    }

    /// Send a `Stop` request, then close at once without reading (the hook program is gone).
    pub fn stop_request_and_leave(&self, session_id: &str) {
        use std::io::Write;
        let line = serde_json::json!({
            "session": session_id,
            "event": {"hook_event_name": "Stop", "stop_hook_active": false},
            "reply": true,
        });
        let mut stream = std::os::unix::net::UnixStream::connect(&self.socket).expect("connect");
        writeln!(stream, "{line}").expect("write the request");
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }

    pub fn stop_and_wait(&self, id: &str) {
        self.tower.session_stop(id).expect("stop");
        self.events.wait_status(id, Status::Stopped);
    }
}

// ---------------------------------------------------------------------------------------------
// Git, for the worktree tests. Temporary repositories only.
// ---------------------------------------------------------------------------------------------

/// Run `git` in a folder under the temp folder, and return its output. A test never runs git in a
/// real repository: the folder must be under the temp dir.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let temp = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    let real = std::fs::canonicalize(dir).unwrap();
    assert!(
        real.starts_with(&temp),
        "{} is not under the temp dir: a test must not run git there",
        real.display()
    );
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Like `git`, but the exit code is returned, and the output is stdout and stderr.
pub fn git_try(dir: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run git");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text.trim().to_owned())
}

/// Make `dir` the top of a repository on the branch `main`, with one commit: `notes.txt` has
/// three lines. The repository has a user (`Operator`, `operator@example.test`).
pub fn init_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.name", "Operator"]);
    git(dir, &["config", "user.email", "operator@example.test"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("notes.txt"), "one\ntwo\nthree\n").unwrap();
    git(dir, &["add", "notes.txt"]);
    git(dir, &["commit", "-q", "-m", "first"]);
}

impl Rig {
    /// Add an employee with the policy `edit-in-worktree` (a Claude employee).
    pub fn add_editor(&self, slug: &str, manager: Option<&str>) {
        self.tower
            .employee_save(
                &self.cwd,
                Employee {
                    name: slug.to_uppercase(),
                    slug: slug.to_owned(),
                    role: format!("Role of {slug}"),
                    manager: manager.map(str::to_owned),
                    cli: CliKind::Claude,
                    model: None,
                    effort: None,
                    permission: PermissionPolicy::EditInWorktree,
                    role_text: format!("Role text of {slug}."),
                },
            )
            .expect("save the employee");
    }

    /// Run git in the floor folder (the main checkout).
    pub fn git(&self, args: &[&str]) -> String {
        git(&self.cwd, args)
    }

    /// The worktree folder of a task.
    pub fn worktree(&self, id: i64) -> PathBuf {
        self.cwd.join(format!(".swarm/worktrees/T-{id}"))
    }
}

/// The process IDs of every process whose command line holds `needle`.
pub fn pids_with(needle: &str) -> Vec<u32> {
    let own = std::process::id();
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return found;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == own {
            continue;
        }
        if let Ok(cmdline) = std::fs::read(entry.path().join("cmdline"))
            && String::from_utf8_lossy(&cmdline).contains(needle)
        {
            found.push(pid);
        }
    }
    found
}

// ---------------------------------------------------------------------------------------------
// A plain HTTP client for the MCP endpoint
// ---------------------------------------------------------------------------------------------

/// The answer of one HTTP request.
#[derive(Debug)]
pub struct HttpAnswer {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl HttpAnswer {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body)
            .unwrap_or_else(|e| panic!("not JSON ({e}): {:?}", self.body))
    }
}

/// `http://127.0.0.1:PORT/mcp` to `127.0.0.1:PORT`.
fn host_of(url: &str) -> String {
    url.trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap()
        .to_owned()
}

/// One request on its own connection (`Connection: close`). `bearer` `None` sends no
/// `Authorization` header.
pub fn http(url: &str, method: &str, bearer: Option<&str>, body: &str) -> HttpAnswer {
    use std::io::{Read, Write};
    let host = host_of(url);
    let mut stream = std::net::TcpStream::connect(&host).expect("connect to the MCP endpoint");
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .unwrap();
    let mut request = format!(
        "{method} /mcp HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(token) = bearer {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).unwrap();
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").expect("an HTTP answer");
    let mut lines = head.lines();
    let status: u16 = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    HttpAnswer {
        status,
        headers,
        body: body.to_owned(),
    }
}

/// The result of a `tools/call`.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub text: String,
    pub is_error: bool,
}

pub fn rpc_body(id: u64, method: &str, params: serde_json::Value) -> String {
    serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
}

/// Call a tool with a token. Panics when the call is not a normal MCP answer.
pub fn call_tool_with(url: &str, token: &str, name: &str, args: serde_json::Value) -> ToolResult {
    let answer = http(
        url,
        "POST",
        Some(token),
        &rpc_body(
            1,
            "tools/call",
            serde_json::json!({"name": name, "arguments": args}),
        ),
    );
    assert_eq!(answer.status, 200, "{answer:?}");
    let json = answer.json();
    let result = &json["result"];
    ToolResult {
        text: result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("no text in {json}"))
            .to_owned(),
        is_error: result["isError"].as_bool().unwrap(),
    }
}

impl Rig {
    /// The token that the app gave to the latest session of `slug` in this floor.
    pub fn token_of(&self, slug: &str) -> String {
        self.calls()
            .into_iter()
            .rev()
            .find(|c| c.slug == slug && c.cwd == self.cwd)
            .and_then(|c| c.mcp)
            .unwrap_or_else(|| panic!("no MCP access for {slug}"))
            .token
    }

    /// Call a tool as the session of `slug`.
    pub fn tool(&self, slug: &str, name: &str, args: serde_json::Value) -> ToolResult {
        call_tool_with(self.tower.mcp_url(), &self.token_of(slug), name, args)
    }

    /// Add an employee (a Claude employee with the default policy) to the floor.
    pub fn add_employee(&self, slug: &str, manager: Option<&str>) {
        self.tower
            .employee_save(
                &self.cwd,
                Employee {
                    name: slug.to_uppercase(),
                    slug: slug.to_owned(),
                    role: format!("Role of {slug}"),
                    manager: manager.map(str::to_owned),
                    cli: CliKind::Claude,
                    model: None,
                    effort: None,
                    permission: PermissionPolicy::ReadOnly,
                    role_text: format!("Role text of {slug}."),
                },
            )
            .expect("save the employee");
    }

    /// Wait until the live session of `slug` is idle after its first turn. Returns its ID.
    pub fn live_idle(&self, slug: &str) -> String {
        let id = self
            .calls()
            .into_iter()
            .rev()
            .find(|c| c.slug == slug && c.cwd == self.cwd)
            .map(|c| c.app_session_id)
            .expect("a session of the employee");
        self.wait_first_turn(&id);
        id
    }

    /// The reasons of the stops that the Stop hook blocked in this session (spec 4.5), oldest
    /// first. `fake-cli` writes them as it continues the turn.
    pub fn hook_reasons(&self, id: &str) -> Vec<String> {
        std::fs::read_to_string(hook_record_path(&self.cwd, id))
            .unwrap_or_default()
            .lines()
            // `fake-cli` can be in the middle of a write: a line that does not parse yet is skipped.
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .map(|value| value["reason"].as_str().unwrap().to_owned())
            .collect()
    }

    /// The messages that the Stop hook gave in each block, oldest block first: the text after the
    /// first line of the reason (which tells the agent what the text is), as a paste would carry
    /// it.
    pub fn hook_texts(&self, id: &str) -> Vec<String> {
        self.hook_reasons(id)
            .into_iter()
            .map(|reason| {
                let (first, rest) = reason.split_once("\n\n").expect("a blank line");
                assert_eq!(first, tower_core::app::STOP_HOOK_HEADER, "{reason:?}");
                rest.to_owned()
            })
            .collect()
    }

    /// The messages of the last block of the Stop hook.
    pub fn hook_text(&self, id: &str) -> Option<String> {
        self.hook_texts(id).pop()
    }

    /// The paste that the session got, with the text of the paste only.
    pub fn paste_text(&self, id: &str) -> Option<String> {
        let record = String::from_utf8_lossy(&self.record(id)).into_owned();
        let start = record.find("\u{1b}[200~")?;
        let end = record.rfind("\u{1b}[201~")?;
        Some(record[start + 6..end].to_owned())
    }
}
