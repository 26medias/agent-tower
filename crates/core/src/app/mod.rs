//! The application service of Agent Tower (plan step 7, Rust half).
//!
//! [`Tower`] joins the parts: the floor store, the program lookup, the hook receiver and router,
//! the CLI adapters and the sessions. It has no Tauri dependency. The Tauri shell calls its
//! methods (one method for each command of `docs/ipc-m1.md`) and gives it an [`Observer`].
//!
//! # What it owns
//!
//! - the app config folder (`floors.toml`, `programs.toml`);
//! - the `HookReceiver` (socket) and the `HookRouter`;
//! - the programs (resolved at first use, cached, overrides in the config folder);
//! - the open floors (one `FloorStore` each, in a mutex);
//! - the live sessions (see `live.rs` for the threads and the attach algorithm).
//!
//! # Locks (a thread holds at most the locks of one row at a time, except where it says so)
//!
//! | Lock | Held while |
//! |---|---|
//! | `floors` | a lookup or an insert in the map. Nothing else. |
//! | `FloorHandle::store` | file and database reads and writes of one floor. Short. |
//! | `registry` | a lookup or an insert. Never while a session or the store is used. |
//! | `programs` | a program lookup. The login shell can take seconds. Nothing else is taken. |
//! | `Live::out`, `Live::last_view` | see `live.rs`. |
//! | the lock of a `Session` | inside `Session` only. `pty_write` and `debug_paste` hold only this one while the PTY write runs. |
//!
//! A command that writes to a PTY clones nothing and takes no Tower lock during the write: it
//! takes an `Arc<Live>` out of the registry (short lock), and then calls the `Session`.
//!
//! # Observers
//!
//! `Observer::session_changed` and `floor_changed` are called from several threads, and
//! `session_changed` is called while a lock of the session is held (this orders the calls). An
//! observer must be quick and must not call back into the Tower. `OutputSink::output` is called
//! from the output thread of the session, with no lock held.

mod attention;
mod bus;
mod floors_watch;
mod folder;
mod live;
mod logo_run;
mod look;
mod mcp;
mod merge;
mod models;
mod notify;
mod program_test;
mod resume;
mod sandbox_probe;
mod settings;
mod setup;
mod tasks;
mod tools;

use crate::adapter::{
    ClaudeAdapter, CliAdapter, CodexAdapter, McpAccess, Programs as AdapterPrograms, StartContext,
};
use crate::env::build_env;
use crate::env::{EnvMap, current_env};
use crate::gitwork;
use crate::hookrx::HookReceiver;
use crate::login_env::{LoginEnvSource, SessionEnv};
use crate::logo::{DEFAULT_API_BASE, REQUEST_TIMEOUT};
use crate::model::{
    CliKind, Employee, FloorDefaults, PermissionPolicy, SessionRecord, allow_rule_warnings,
    valid_allow_rules,
};
use crate::programs::Resolver;
use crate::pty::LogConfig;
use crate::session::{HookRouter, PasteRefused, Session};
use crate::status::Status;
use crate::store::{Database, FloorList, FloorStore, StoreError, floor_is_gone};
use live::{Live, LiveParts, Notifier, locked, now_ms, read_log_tail, spawn_pumps};
use logo_run::{LogoEnv, LogoState};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub use attention::{OperatorAsked, OperatorRequest, REQUEST_MAX_CHARS, request_body};
pub use bus::{
    EXCHANGE_LIMIT, EXCHANGE_NOTE_AT, EXCHANGE_WINDOW, START_LIMIT, START_WINDOW, STOP_HOOK_HEADER,
    TestHooks,
};
pub use live::{SCROLLBACK_LIMIT, STOP_BLOCK_LIMIT};
pub use mcp::{FIRST_BYTE_TIMEOUT, MAX_CONNECTIONS, McpLimits, REQUEST_DEADLINE};
pub use merge::{MergeReport, TaskDiffView};
pub use models::{
    CodexSource, ModelCatalog, ModelKind, ModelOption, STANDARD_EFFORTS, is_valid_effort,
    is_valid_model_name,
};
pub use notify::{NOTIFICATION_TITLE, QuestionAsked, question_body, should_notify};
pub use program_test::{PROGRAM_TEST_TIMEOUT, ProgramTest};
pub use resume::ResumeProgress;
pub use sandbox_probe::{PROBE_TIMEOUT, SandboxProbe, SandboxState};
pub use settings::{CodexSandboxMode, KeySource, SandboxEffective, SettingsView};
pub use tools::{FAILURE_TTL, hook_candidates, locate_hook_program, workspace_root};

/// The size limit of the PTY log of one session (spec 4.9; the log keeps 4 to 8 MB).
pub const LOG_LIMIT_BYTES: u64 = 4 * 1024 * 1024;

/// The reserved slug of the setup session. No employee can have it.
pub const SETUP_SLUG: &str = "setup";

/// The name of the operator in task events and in messages. No employee can have this slug.
pub const OPERATOR: &str = "operator";

/// The report files of a floor, relative to the floor folder (spec 4.10).
pub const REPORTS_DIR: &str = ".swarm/reports";

/// The instructions of the setup agent (the "floor-init skill"). They go in through the same path
/// as the role text of an employee.
pub const FLOOR_INIT: &str = include_str!("floor_init.md");

/// The first message of the setup session.
pub const SETUP_FIRST_MESSAGE: &str = "Start the floor setup.";

/// The default pause of the watcher of the floor folders.
pub const FLOOR_WATCH_PAUSE: Duration = Duration::from_secs(3);

/// How long `shutdown` waits for the processes to end before it uses `SIGKILL`.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

/// How long `session_stop` waits for the process to end before it uses `SIGKILL`.
const STOP_GRACE: Duration = Duration::from_secs(3);

/// How long a `works` result of the sandbox probe counts for the start of a Codex session, and
/// the shortest time between two `sandbox_down` notices.
pub const SANDBOX_WINDOW: Duration = Duration::from_secs(60);

/// The text of the notice `sandbox_auto_off` (docs/ipc-final.md).
pub const AUTO_OFF_NOTICE: &str = "The Codex sandbox does not work on this machine, so Codex employees run with full access. See Settings > Codex sandbox.";

/// What the start of a Codex session remembers about the sandbox (memory only). A failed probe
/// is never kept: the next start runs the probe again.
#[derive(Default)]
struct SandboxGate {
    /// When the last probe said `works`.
    works_at: Option<Instant>,
    /// What the last probe said (any result). `settings_get` uses it, so a read of the settings
    /// does not run a probe each time.
    last: Option<SandboxState>,
    /// The "Automatic" notice (`sandbox_auto_off`) went out in this run of the app.
    auto_notice_sent: bool,
    /// When the last `sandbox_down` notice went out.
    notice_at: Option<Instant>,
}

// ---------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------

/// Every error of the Tower. `Display` is the text that the operator reads. The text of
/// `NoFloor` starts with `NO_FLOOR:` and the text of `Refused` starts with `REFUSED:`, as the
/// contract says. The path errors and `FloorGone` have the prefixes of `docs/ipc-final.md`.
#[derive(Debug, thiserror::Error)]
pub enum TowerError {
    #[error("NO_FLOOR: there is no floor at {}. The folder has no .swarm/floor.toml file.", .0.display())]
    NoFloor(PathBuf),
    #[error("REFUSED: {0}")]
    Refused(String),
    #[error("NOT_ABSOLUTE: Type the full path of the folder, for example /home/you/project.")]
    NotAbsolute,
    #[error("NOT_FOUND: The folder {} does not exist.", .0.display())]
    NotFound(PathBuf),
    #[error("NOT_A_FOLDER: {} is a file, not a folder.", .0.display())]
    NotAFolder(PathBuf),
    #[error("NO_WRITE: You cannot write in the folder {}.", .0.display())]
    NoWrite(PathBuf),
    #[error("FLOOR_GONE: The folder of this floor is gone: {}.", .0.display())]
    FloorGone(PathBuf),
    /// A Codex start with the setting "Codex sandbox" at "Always", while the probe does not say
    /// `works`. The start is refused. With "Automatic" the app starts Codex with full access
    /// instead (`sandbox_auto_off` notice).
    #[error(
        "SANDBOX_DOWN: {name} did not start: the Codex sandbox does not work on this machine now ({detail}). The setting \"Codex sandbox\" is \"Always\". Repair the sandbox, or choose \"Automatic\" or \"Never\" in Settings > Codex sandbox."
    )]
    SandboxDown { name: String, detail: String },
    #[error("{slug} already has a live session. Stop it first.")]
    AlreadyLive { slug: String },
    #[error("{slug} has a live session. Stop it first.")]
    HasLiveSession { slug: String },
    #[error("there is no session with the ID \"{0}\"")]
    UnknownSession(String),
    #[error("the app is closing")]
    Closing,
    #[error(transparent)]
    Store(StoreError),
    #[error("{0}")]
    Other(String),
}

impl From<StoreError> for TowerError {
    fn from(error: StoreError) -> TowerError {
        match error {
            StoreError::NoFloor(path) => TowerError::NoFloor(path),
            other => TowerError::Store(other),
        }
    }
}

type Result<T> = std::result::Result<T, TowerError>;

fn other(message: impl Into<String>) -> TowerError {
    TowerError::Other(message.into())
}

// ---------------------------------------------------------------------------------------------
// Views and observers
// ---------------------------------------------------------------------------------------------

/// The result of `floor_permissions_set`: the rules that the app stored, and a warning for each
/// rule that lets an agent run the code of the project, and for each `Bash` rule with a wildcard
/// (`model::allow_rule_warning`). The warnings are how the operator learns what a rule gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionsSaved {
    pub allow: Vec<String>,
    pub warnings: Vec<String>,
}

/// One session as the UI sees it (`SessionView` of the contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionView {
    pub session_id: String,
    pub slug: String,
    pub status: Status,
    pub draft: bool,
    pub message_waiting: bool,
    pub can_resume: bool,
}

/// One session that an employee started with the tools of the app (`employee_start`, or `send`
/// to an employee with no live session). The session itself comes as a normal `session_changed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmployeeStarted {
    /// The app session ID of the new session.
    pub session_id: String,
    /// The employee that was started.
    pub slug: String,
    pub name: String,
    /// The employee that started it.
    pub by_slug: String,
    pub by_name: String,
    /// The app resumed the last CLI session (and did not start a fresh one).
    pub resumed: bool,
}

/// One message between employees (or from the operator) that the app DELIVERED to the session of
/// the receiver: by a paste, by the Stop hook, by `inbox`, or as the first message of a start.
/// It carries a short preview, never the full text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageDelivered {
    pub from_slug: String,
    /// The name of the sender. For the operator it is `Operator`. For a removed employee it is the slug.
    pub from_name: String,
    pub to_slug: String,
    pub to_name: String,
    /// `message` for a message of `send`, `task` for a message that the app wrote about a task.
    pub kind: String,
    pub task_id: Option<i64>,
    /// For a status message about a task: the status that it names (`review`). `None` for an
    /// assignment and for a message that is not about a task.
    pub task_status: Option<String>,
    /// The first 80 characters of the body, with control characters removed and each newline
    /// turned into a space.
    pub preview: String,
}

/// `FloorView` of the contract.
#[derive(Debug, Clone, PartialEq)]
pub struct FloorView {
    pub path: PathBuf,
    pub name: String,
    pub employees: Vec<Employee>,
    pub employee_errors: Vec<String>,
    /// One for each employee that has a live or a last session, and the setup session if there
    /// is one (it has the slug `setup`).
    pub sessions: Vec<SessionView>,
    /// `[brand] description`.
    pub description: Option<String>,
    /// `[brand] logo_prompt`.
    pub logo_prompt: Option<String>,
    /// The modification time of `logo.png` in Unix milliseconds. `None` when there is no file.
    pub logo_version: Option<i64>,
    /// A logo generation runs now for this floor.
    pub logo_busy: bool,
    /// The error text of the last failed generation (in memory only).
    pub logo_error: Option<String>,
    /// The ID of the live setup session, if one runs.
    pub setup_session_id: Option<String>,
    /// `[defaults]` of `floor.toml`.
    pub defaults: FloorDefaults,
    /// The employees that had a live session at the last close of the app, that have no live
    /// session now, and that can resume (M3). The order is the order of the resume: managers first.
    pub resumable: Vec<String>,
    /// The open requests of the employees for the operator, oldest first (memory only).
    pub requests: Vec<OperatorRequest>,
    /// The background resume of the last team runs now (M3).
    pub resume_progress: Option<ResumeProgress>,
    /// The folder of the floor is gone (see `store::floor_is_gone`). Then the other fields are
    /// the last known ones. `false` in normal use.
    pub gone: bool,
}

/// `FloorEntry` of the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloorEntryView {
    pub path: PathBuf,
    pub name: Option<String>,
    pub missing: bool,
    /// The modification time of `logo.png` in Unix milliseconds. `None` when there is no file.
    pub logo_version: Option<i64>,
}

/// `Programs` of the contract. `None` means "not found". `hook` is `None` when `tower-hook`
/// cannot be found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramsView {
    pub claude: Option<PathBuf>,
    pub codex: Option<PathBuf>,
    pub node: Option<PathBuf>,
    pub hook: Option<PathBuf>,
    /// The program `rtk` (Rust Token Killer), found like the others. It is optional: the app
    /// needs it for nothing. The UI uses it to show examples that fit this machine.
    pub rtk: Option<PathBuf>,
    /// The override of the operator for each program, in the order `claude`, `codex`, `node`.
    /// `None`: no override (the app searches).
    pub overrides: [Option<PathBuf>; 3],
}

/// Receives the changes. See the module text for the rules.
pub trait Observer: Send + Sync {
    /// The status, the draft flag, the message-waiting flag or the resume flag of a session
    /// changed, or the session started. `path` is the key of the floor (see `FloorView::path`).
    fn session_changed(&self, path: &Path, session: &SessionView);
    /// The employees of a floor changed.
    fn floor_changed(&self, path: &Path);
    /// An employee started the session of another employee. It comes after the `session_changed`
    /// of the new session. The default does nothing.
    fn employee_started(&self, _path: &Path, _event: &EmployeeStarted) {}
    /// A task was made or changed, or an event was added (by a tool of an employee or by a
    /// command of the operator). `path` is the key of the floor. The default does nothing.
    fn tasks_changed(&self, _path: &Path) {}
    /// Something that the operator must see, and that is not a state of a session or a task: a
    /// merge that was blocked, or a session that the resume of the team could not start. `kind`
    /// is `task_blocked`, `resume_failed` or `sandbox_down`. `text` is the message. The default does nothing.
    fn notice(&self, _path: &Path, _kind: &str, _text: &str) {}
    /// A notice that is about one employee (`slug`), so the UI can go to its tab. The kind is
    /// `exchange_stopped`. The default calls `notice`, so an observer that does not know `slug`
    /// still gets the notice.
    fn notice_for(&self, path: &Path, kind: &str, text: &str, _slug: &str) {
        self.notice(path, kind, text);
    }
    /// A session changed to `question`. The shell decides if the operator needs a desktop
    /// notification (`notify::should_notify`). It comes after the `session_changed` of the same
    /// change. The default does nothing.
    fn question_asked(&self, _path: &Path, _event: &QuestionAsked) {}
    /// An employee asked for the operator with the tool `ask_operator`. It comes after the
    /// `floor_changed` of the same request. The default does nothing.
    fn operator_asked(&self, _path: &Path, _event: &OperatorAsked) {}
    /// The app delivered a message to the session of an employee (a paste, the Stop hook, `inbox`
    /// or the first message of a start). One call for each message, oldest first. The default
    /// does nothing.
    fn message_delivered(&self, _path: &Path, _event: &MessageDelivered) {}
}

/// Receives the output of the attached session, in order (see `live.rs`).
pub trait OutputSink: Send + Sync {
    fn output(&self, bytes: &[u8]);
}

impl<F: Fn(&[u8]) + Send + Sync> OutputSink for F {
    fn output(&self, bytes: &[u8]) {
        self(bytes)
    }
}

// ---------------------------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------------------------

/// Makes the adapter of a CLI kind. The default makes the real ones.
pub type AdapterFactory = Arc<dyn Fn(CliKind) -> Box<dyn CliAdapter> + Send + Sync>;

/// The real adapters.
pub fn real_adapters() -> AdapterFactory {
    Arc::new(|kind| match kind {
        CliKind::Claude => Box::new(ClaudeAdapter),
        CliKind::Codex => Box::new(CodexAdapter),
    })
}

/// How to build a [`Tower`]. `TowerConfig::new` gives the production values.
pub struct TowerConfig {
    /// The app config folder. It holds `floors.toml` and `programs.toml`.
    pub config_dir: PathBuf,
    /// The socket of the hook receiver. `None`: `default_socket_path()`, and a private folder
    /// under the temp dir when `XDG_RUNTIME_DIR` is not set.
    pub socket_path: Option<PathBuf>,
    /// The `tower-hook` program. `None`: next to the app binary, then `target/` of the workspace.
    pub hook_program: Option<PathBuf>,
    /// How to find `claude`, `codex` and `node`.
    pub resolver: Resolver,
    /// The adapters. Tests give an adapter that starts `fake-cli`.
    pub adapters: AdapterFactory,
    /// The environment of the app. The login shell environment goes over it (see `login_env`),
    /// and the result is the parent environment of each session. The logo generation reads
    /// `OPENAI_API_KEY` from this map only.
    pub parent_env: EnvMap,
    /// Where the login shell environment comes from. The default runs `$SHELL -lic` one time, in
    /// a thread, when the `Tower` is made. Tests give `LoginEnvSource::Fixed(..)`, so that no real
    /// shell runs.
    pub login_env: LoginEnvSource,
    /// The size limit of each PTY log file.
    pub log_limit: u64,
    /// The base URL of the image API. Tests give the URL of a local fake server.
    pub image_api_base: String,
    /// The time limit of one logo request.
    pub logo_timeout: Duration,
    /// The Codex home folder. The app reads `models_cache.json` from it and writes nothing there.
    /// `None`: `$CODEX_HOME` of `parent_env`, then `$HOME/.codex`. Tests give a temp folder.
    pub codex_home: Option<PathBuf>,
    /// Test only. Runs in `session_attach` inside the critical section of the output buffer,
    /// after the scrollback was copied and before the sink is installed. A correct build holds
    /// the output thread during the call; a build with a gap or a duplicate does not.
    pub attach_pause: Option<Arc<dyn Fn() + Send + Sync>>,
    /// The limits of the MCP endpoint. Tests give small ones.
    pub mcp_limits: McpLimits,
    /// Test only. See [`TestHooks`].
    pub test_hooks: Arc<TestHooks>,
    /// The pause between two starts of the resume of the last team (spec 4.2, M3). Tests give a
    /// short one.
    pub resume_pause: Duration,
    /// How long a `works` result of the sandbox probe counts, and the time between two
    /// `sandbox_down` notices. `TowerConfig::new` gives 60 seconds. Tests give a short time.
    pub sandbox_window: Duration,
    /// The window of `EXCHANGE_LIMIT` (the plain messages between two employees), and the time
    /// between two `exchange_stopped` notices of one pair. `TowerConfig::new` gives
    /// `EXCHANGE_WINDOW` (5 minutes). Tests give a short time. `Duration::ZERO` turns the limit off.
    pub exchange_window: Duration,
    /// The pause between two checks of the open floors for a folder that is gone or back. `None`
    /// turns the watcher off. `TowerConfig::new` gives 3 seconds. Tests give a short pause, or
    /// `None`.
    pub floor_watch: Option<Duration>,
}

impl TowerConfig {
    pub fn new(config_dir: impl Into<PathBuf>) -> TowerConfig {
        TowerConfig {
            config_dir: config_dir.into(),
            socket_path: None,
            hook_program: None,
            resolver: Resolver::from_env(),
            adapters: real_adapters(),
            parent_env: current_env(),
            login_env: LoginEnvSource::from_env(),
            log_limit: LOG_LIMIT_BYTES,
            image_api_base: DEFAULT_API_BASE.to_owned(),
            logo_timeout: REQUEST_TIMEOUT,
            codex_home: None,
            attach_pause: None,
            mcp_limits: McpLimits::default(),
            test_hooks: Arc::new(TestHooks::default()),
            resume_pause: resume::RESUME_PAUSE,
            sandbox_window: SANDBOX_WINDOW,
            exchange_window: EXCHANGE_WINDOW,
            floor_watch: Some(FLOOR_WATCH_PAUSE),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The Tower
// ---------------------------------------------------------------------------------------------

struct FloorHandle {
    /// The key: the real path of the working folder.
    path: PathBuf,
    store: Mutex<FloorStore>,
    /// The state of the logo generations of this floor (memory only).
    logo: Mutex<LogoState>,
    /// One git action at a time for the main checkout of this floor (a worktree that is made or
    /// removed, a commit, a merge). Taken before the store lock, never while a session starts.
    git: Mutex<()>,
    /// The resume of the last team that runs now (memory only).
    resume: Mutex<Option<ResumeProgress>>,
    /// The open requests for the operator (memory only, see `attention.rs`).
    requests: Mutex<Vec<OperatorRequest>>,
    /// The watcher told the observer that the folder is gone, and did not tell yet that it is
    /// back (see `floor_watch`).
    announced_gone: AtomicBool,
    /// A check saw the floor gone. The database handle in `store` can point at a file that was
    /// deleted or moved, so the next use opens it again from the path (`check_alive`).
    stale: AtomicBool,
    /// The last view that `floor_view` made. `floor_open` gives it, with `gone: true`, when the
    /// folder is gone.
    last_view: Mutex<Option<FloorView>>,
}

impl FloorHandle {
    fn new(path: PathBuf, store: FloorStore) -> FloorHandle {
        FloorHandle {
            path,
            store: Mutex::new(store),
            logo: Mutex::default(),
            git: Mutex::new(()),
            resume: Mutex::new(None),
            requests: Mutex::default(),
            announced_gone: AtomicBool::new(false),
            stale: AtomicBool::new(false),
            last_view: Mutex::new(None),
        }
    }

    fn swarm(&self) -> PathBuf {
        self.path.join(".swarm")
    }

    fn log_path(&self, session_id: &str) -> PathBuf {
        self.swarm()
            .join("state")
            .join("logs")
            .join(format!("{session_id}.log"))
    }
}

#[derive(Default)]
struct Registry {
    live: HashMap<String, Arc<Live>>,
    /// Employees whose start is in progress.
    reserved: HashSet<(PathBuf, String)>,
    /// The IDs of the sessions that the operator stopped by hand. A process needs some time to
    /// end, and the app can close in that time: such a session must not go in the list of the
    /// team to resume (`resume.rs`).
    manual_stops: HashSet<String>,
}

impl Registry {
    fn has_live(&self, path: &Path, slug: &str) -> bool {
        self.live
            .values()
            .any(|live| live.path == path && live.slug == slug && !live.exited())
    }

    fn is_busy(&self, path: &Path, slug: &str) -> bool {
        self.has_live(path, slug)
            || self
                .reserved
                .contains(&(path.to_path_buf(), slug.to_owned()))
    }
}

/// What `launch` starts.
#[derive(Clone, Copy)]
enum Target<'a> {
    /// The employee with this slug.
    Employee(&'a str),
    /// The setup session (the slug is `SETUP_SLUG`), with this CLI.
    Setup(CliKind),
}

/// Removes the reservation of an employee when it goes out of scope.
struct Reservation {
    registry: Arc<Mutex<Registry>>,
    key: (PathBuf, String),
}

impl Drop for Reservation {
    fn drop(&mut self) {
        locked(&self.registry).reserved.remove(&self.key);
    }
}

/// The Tower. It is a handle: the state is in `TowerCore`, which the MCP server threads share
/// (they hold a `Weak`, so a dropped `Tower` is not kept alive by them).
pub struct Tower {
    core: Arc<TowerCore>,
}

impl std::ops::Deref for Tower {
    type Target = TowerCore;

    fn deref(&self) -> &TowerCore {
        &self.core
    }
}

/// The state and the methods of the Tower. Use it through `Tower`.
#[doc(hidden)]
pub struct TowerCore {
    /// A weak handle to this core, for the threads of the sessions and of the MCP server.
    me: Weak<TowerCore>,
    floor_list: FloorList,
    socket: PathBuf,
    hookrx: Mutex<Option<HookReceiver>>,
    router: Arc<HookRouter>,
    programs: Mutex<tools::ProgramBook>,
    hook_override: Option<PathBuf>,
    hook_found: Mutex<Option<PathBuf>>,
    floors: Mutex<HashMap<PathBuf, Arc<FloorHandle>>>,
    registry: Arc<Mutex<Registry>>,
    notifier: Arc<Notifier>,
    adapters: AdapterFactory,
    /// The parent environment of the sessions: the login shell environment over the app one.
    session_env: SessionEnv,
    log_limit: u64,
    logo_env: Arc<LogoEnv>,
    /// The Codex home folder, or `None` when it is not known (the built-in Codex list is used).
    codex_home: Option<PathBuf>,
    attach_pause: Option<Arc<dyn Fn() + Send + Sync>>,
    closed: Arc<AtomicBool>,
    resume_pause: Duration,
    sandbox_window: Duration,
    exchange_window: Duration,
    sandbox_gate: Mutex<SandboxGate>,
    /// The home folder, for `~` in a typed path. From `HOME` of the parent environment.
    home: Option<PathBuf>,
    /// The thread that watches the floor folders (`floor_watch`). `shutdown` joins it.
    watcher: Mutex<Option<thread::JoinHandle<()>>>,
    /// The token of each live session that has the MCP server (see `mcp.rs`).
    tokens: Arc<Mutex<HashMap<String, bus::Identity>>>,
    /// The URL of the MCP endpoint, for example `http://127.0.0.1:41234/mcp`.
    mcp_url: String,
    /// The port of the MCP endpoint (`shutdown` connects to it to end the accept thread).
    mcp_port: u16,
    /// The address that the MCP listener has really bound (from `local_addr`).
    mcp_addr: SocketAddr,
    test_hooks: Arc<TestHooks>,
    bus: bus::Bus,
}

fn normalize(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Read `floor.toml` again (the name, the defaults and the brand), so that an edit on disk applies
/// while the app runs. It opens the store again; the store lock is held for the swap.
fn reload(floor: &FloorHandle) -> Result<()> {
    let mut store = locked(&floor.store);
    *store = FloorStore::open(&floor.path)?;
    Ok(())
}

/// The Codex home folder: `$CODEX_HOME`, or `$HOME/.codex`. `None` when neither is set.
fn default_codex_home(env: &EnvMap) -> Option<PathBuf> {
    let var = |name: &str| {
        env.get(std::ffi::OsStr::new(name))
            .filter(|value| !value.is_empty())
    };
    match var("CODEX_HOME") {
        Some(home) => Some(PathBuf::from(home)),
        None => var("HOME").map(|home| PathBuf::from(home).join(".codex")),
    }
}

fn io_message(what: &str, error: io::Error) -> TowerError {
    other(format!("{what}: {error}"))
}

impl Tower {
    /// Bind the hook socket and start the router. No session starts here.
    pub fn new(config: TowerConfig) -> Result<Tower> {
        fs::create_dir_all(&config.config_dir)
            .map_err(|e| io_message("cannot make the config folder", e))?;
        let socket = match config.socket_path {
            Some(path) => path,
            None => {
                tools::socket_path().map_err(|e| io_message("no folder for the hook socket", e))?
            }
        };
        let (hookrx, messages) = HookReceiver::bind(&socket).map_err(|e| {
            io_message(
                &format!("cannot bind the hook socket {}", socket.display()),
                e,
            )
        })?;
        let router = Arc::new(HookRouter::start(messages));
        let logo_env = Arc::new(LogoEnv {
            config_file: config.config_dir.join("programs.toml"),
            env: config.parent_env.clone(),
            api_base: config.image_api_base,
            timeout: config.logo_timeout,
        });
        let codex_home = config
            .codex_home
            .or_else(|| default_codex_home(&config.parent_env));
        let home = config
            .parent_env
            .get(std::ffi::OsStr::new("HOME"))
            .filter(|home| !home.is_empty())
            .map(PathBuf::from);
        let floor_watch = config.floor_watch;
        let (mcp_listener, mcp_addr) =
            mcp::bind().map_err(|e| io_message("cannot bind the MCP endpoint", e))?;
        let mcp_port = mcp_addr.port();
        let core = Arc::new_cyclic(|me| TowerCore {
            me: me.clone(),
            floor_list: FloorList::new(&config.config_dir),
            socket: hookrx.path().to_path_buf(),
            hookrx: Mutex::new(Some(hookrx)),
            router,
            programs: Mutex::new(tools::ProgramBook::new(&config.config_dir, config.resolver)),
            hook_override: config.hook_program,
            hook_found: Mutex::new(None),
            floors: Mutex::new(HashMap::new()),
            registry: Arc::new(Mutex::new(Registry::default())),
            notifier: Arc::new(Notifier::default()),
            adapters: config.adapters,
            session_env: SessionEnv::start(config.parent_env, config.login_env),
            log_limit: config.log_limit,
            logo_env,
            codex_home,
            attach_pause: config.attach_pause,
            closed: Arc::new(AtomicBool::new(false)),
            resume_pause: config.resume_pause,
            sandbox_window: config.sandbox_window,
            exchange_window: config.exchange_window,
            sandbox_gate: Mutex::new(SandboxGate::default()),
            home,
            watcher: Mutex::new(None),
            tokens: Arc::new(Mutex::new(HashMap::new())),
            mcp_url: format!("http://127.0.0.1:{mcp_port}/mcp"),
            mcp_port,
            mcp_addr,
            test_hooks: config.test_hooks,
            bus: bus::Bus::default(),
        });
        mcp::serve(mcp_listener, Arc::downgrade(&core), config.mcp_limits)
            .map_err(|e| io_message("cannot start the MCP endpoint", e))?;
        if let Some(pause) = floor_watch {
            let handle = floors_watch::spawn(Arc::downgrade(&core), pause)
                .map_err(|e| io_message("cannot start the floor watcher", e))?;
            *locked(&core.watcher) = Some(handle);
        }
        Ok(Tower { core })
    }
}

impl TowerCore {
    /// Set the observer (it replaces the old one).
    pub fn set_observer(&self, observer: Arc<dyn Observer>) {
        self.notifier.set(observer);
    }

    /// The path of the hook socket.
    pub fn socket_path(&self) -> &Path {
        &self.socket
    }

    /// The URL of the MCP endpoint of the app, for example `http://127.0.0.1:41234/mcp`. It is
    /// not secret: a call needs the token of a session.
    pub fn mcp_url(&self) -> &str {
        &self.mcp_url
    }

    /// The address that the MCP listener has bound. It must be a loopback address.
    pub fn mcp_local_addr(&self) -> SocketAddr {
        self.mcp_addr
    }

    // ---- floors ----

    /// The known floors, in the order they were added.
    pub fn floor_list(&self) -> Result<Vec<FloorEntryView>> {
        Ok(self
            .floor_list
            .list()?
            .into_iter()
            .map(|entry| FloorEntryView {
                name: floor_name_on_disk(&entry.path),
                logo_version: logo_run::logo_version(&entry.path),
                path: entry.path,
                missing: entry.missing,
            })
            .collect())
    }

    /// Import `{path}/.swarm/`. The path is checked first (`docs/ipc-final.md`): `~` is expanded
    /// to the home folder, then the errors are `NotAbsolute`, `NotFound`, `NotAFolder` and
    /// `NoWrite`. `TowerError::NoFloor` when a good folder has no `.swarm/`: the UI offers to
    /// create the floor then. A floor that this Tower holds, and whose folder is gone, gives its
    /// last known view with `gone: true`.
    pub fn floor_open(&self, path: &Path) -> Result<FloorView> {
        let path = folder::expand_home(path, self.home.as_deref());
        folder::check_absolute(&path)?;
        match self.held_floor(&path) {
            Some(floor) if floor_is_gone(&floor.path) => {
                floor.stale.store(true, Ordering::SeqCst);
                return self.gone_view(&floor);
            }
            // The Tower opened this floor before and the folder is there now: no new check.
            Some(_) => {}
            None => folder::check_folder(&path)?,
        }
        let floor = self.floor_handle(&path)?;
        reload(&floor)?;
        self.floor_list.add(&floor.path)?;
        self.floor_view(&floor)
    }

    /// Write a new `.swarm/` with the two default employees `ceo` and `pm`. The path is checked
    /// like the path of `floor_open`.
    pub fn floor_create(&self, path: &Path, name: &str) -> Result<FloorView> {
        let path = folder::expand_home(path, self.home.as_deref());
        folder::check_absolute(&path)?;
        folder::check_folder(&path)?;
        let name = name.trim();
        if name.is_empty() {
            return Err(other("the floor name is empty"));
        }
        let key = normalize(&path);
        let store = FloorStore::create(&key, name)?;
        for employee in default_employees() {
            store.write_employee(&employee)?;
        }
        let handle = Arc::new(FloorHandle::new(key.clone(), store));
        locked(&self.floors).insert(key.clone(), Arc::clone(&handle));
        self.floor_list.add(&key)?;
        self.floor_view(&handle)
    }

    /// Remove a floor from the list. Files stay. Refused while a session of the floor lives.
    pub fn floor_forget(&self, path: &Path) -> Result<()> {
        let key = normalize(path);
        {
            let registry = locked(&self.registry);
            let busy = registry
                .live
                .values()
                .any(|live| live.path == key && !live.exited());
            if busy || registry.reserved.iter().any(|(p, _)| *p == key) {
                return Err(other(
                    "the floor has live sessions. Stop them before you forget the floor.",
                ));
            }
        }
        // The rules of the operator for this floor go too. A new floor at the same path starts
        // with no rule.
        if settings::has_floor_rules(&self.logo_env.config_file, &key) {
            settings::update(&self.logo_env.config_file, |table| {
                settings::forget_floor_rules(table, &key);
            })
            .map_err(|e| io_message("cannot write the app config", e))?;
        }
        locked(&self.floors).remove(&key);
        self.floor_list.remove(path)?;
        Ok(())
    }

    /// Create or update an employee.
    pub fn employee_save(&self, path: &Path, employee: Employee) -> Result<FloorView> {
        if employee.name.trim().is_empty() {
            return Err(other("the employee name is empty"));
        }
        if employee.slug == SETUP_SLUG {
            return Err(other(format!(
                "the file name \"{SETUP_SLUG}\" is reserved for the setup agent. Use another name for the employee."
            )));
        }
        if employee.slug == OPERATOR {
            return Err(other(format!(
                "the file name \"{OPERATOR}\" is reserved for the operator. Use another name for the employee."
            )));
        }
        // The model goes to the CLI as an argument. A bad name is refused before it is stored.
        models::check_model("model", employee.model.as_deref()).map_err(other)?;
        models::check_effort("effort", employee.effort.as_deref()).map_err(other)?;
        let floor = self.floor_handle(path)?;
        locked(&floor.store).write_employee(&employee)?;
        self.notifier.floor_changed(&floor.path);
        self.floor_view(&floor)
    }

    /// Remove an employee. Refused while the employee has a live session.
    pub fn employee_remove(&self, path: &Path, slug: &str) -> Result<FloorView> {
        let floor = self.floor_handle(path)?;
        if locked(&self.registry).is_busy(&floor.path, slug) {
            return Err(TowerError::HasLiveSession {
                slug: slug.to_owned(),
            });
        }
        locked(&floor.store).remove_employee(slug)?;
        self.notifier.floor_changed(&floor.path);
        self.floor_view(&floor)
    }

    /// Write `[defaults]` of `floor.toml`. A `None` field goes out of the file. The model and the
    /// effort are checked like those of an employee. The observer gets `floor_changed`.
    pub fn floor_defaults_set(&self, path: &Path, defaults: FloorDefaults) -> Result<FloorView> {
        models::check_model("default model", defaults.model.as_deref()).map_err(other)?;
        models::check_effort("default effort", defaults.effort.as_deref()).map_err(other)?;
        let floor = self.floor_handle(path)?;
        locked(&floor.store).set_defaults(defaults)?;
        self.notifier.floor_changed(&floor.path);
        self.floor_view(&floor)
    }

    /// The rules of the floor for commands with no question, as stored in the app config (table
    /// `[floor_permissions]`, see `settings.rs`). Empty when the operator did not set a list. A
    /// rule that the check refuses now can be in the list (a new version can check more). The app
    /// does not use it for a session.
    pub fn floor_permissions_get(&self, path: &Path) -> Result<Vec<String>> {
        let floor = self.floor_handle(path)?;
        Ok(settings::floor_rules(&self.logo_env.config_file, &floor.path).unwrap_or_default())
    }

    /// Write the rules of the floor in the app config (not in the project folder, so an agent of
    /// the floor cannot change them). Each rule is checked (`check_allow_rule`), and nothing is
    /// written when one is bad. The text is trimmed and a duplicate is dropped. The old
    /// `[permissions]` table of `floor.toml` is removed (the rest of the file stays). The new rules
    /// apply at the next start of a session. The observer gets `floor_changed`. Returns the stored
    /// list, and a warning for each rule that lets the agents run the code of the project (a build
    /// tool, a script) and for each `Bash` rule with a wildcard (`allow_rule_warning`). An exact
    /// `Bash` rule of another program has no warning.
    pub fn floor_permissions_set(
        &self,
        path: &Path,
        allow: Vec<String>,
    ) -> Result<PermissionsSaved> {
        let mut clean: Vec<String> = Vec::new();
        for rule in allow {
            let rule = rule.trim().to_owned();
            crate::model::check_allow_rule(&rule)
                .map_err(|reason| other(format!("the rule \"{rule}\" is refused: {reason}")))?;
            if !clean.contains(&rule) {
                clean.push(rule);
            }
        }
        if clean.len() > crate::model::ALLOW_RULES_MAX {
            return Err(other(format!(
                "there are {} rules, and the limit is {}",
                clean.len(),
                crate::model::ALLOW_RULES_MAX
            )));
        }
        let floor = self.floor_handle(path)?;
        let mut stored = true;
        settings::update(&self.logo_env.config_file, |table| {
            stored = settings::set_floor_rules(table, &floor.path, &clean);
        })
        .map_err(|e| io_message("cannot write the app config", e))?;
        if !stored {
            return Err(other(format!(
                "the path of the floor is not valid text: {}",
                floor.path.display()
            )));
        }
        // The old table is not used any more. The rules of the operator are in the app config now.
        locked(&floor.store).remove_legacy_permissions()?;
        self.notifier.floor_changed(&floor.path);
        Ok(PermissionsSaved {
            warnings: allow_rule_warnings(&clean),
            allow: clean,
        })
    }

    /// The rules of the old `[permissions]` table of `floor.toml` that the operator can take over.
    /// It is empty when the app config has an entry for the floor already (the operator decided),
    /// or when the file has no table. The rules that do not pass `check_allow_rule`, and the
    /// duplicates, are left out. The app never imports them by itself: an agent of the floor can
    /// have written the file. The operator reads the list and saves it with
    /// `floor_permissions_set`, which removes the old table.
    pub fn floor_permissions_proposed(&self, path: &Path) -> Result<Vec<String>> {
        let floor = self.floor_handle(path)?;
        if settings::has_floor_rules(&self.logo_env.config_file, &floor.path) {
            return Ok(Vec::new());
        }
        let old = locked(&floor.store).legacy_permissions()?;
        Ok(valid_allow_rules(&old.unwrap_or_default()))
    }

    /// The handle of a floor that this Tower holds. It never opens a floor.
    fn held_floor(&self, path: &Path) -> Option<Arc<FloorHandle>> {
        locked(&self.floors).get(&normalize(path)).cloned()
    }

    /// The gate of every command that reads or writes a floor. `FloorGone` when the folder, its
    /// `.swarm/` or `floor.toml` is not there (`floor_is_gone`). When the floor was seen gone and
    /// is there again, it opens the database again from the path: the old handle can point at a
    /// file that was deleted or moved.
    fn check_alive(&self, floor: &FloorHandle) -> Result<()> {
        if floor_is_gone(&floor.path) {
            floor.stale.store(true, Ordering::SeqCst);
            return Err(TowerError::FloorGone(floor.path.clone()));
        }
        if floor.stale.load(Ordering::SeqCst) {
            reload(floor)?;
            floor.stale.store(false, Ordering::SeqCst);
        }
        Ok(())
    }

    /// The last known view of a floor whose folder is gone, with `gone: true`. The sessions that
    /// still run show their live state, also those that started after the last view was made. A
    /// session that is not live shows as stopped.
    fn gone_view(&self, floor: &FloorHandle) -> Result<FloorView> {
        let Some(mut view) = locked(&floor.last_view).clone() else {
            return Err(TowerError::FloorGone(floor.path.clone()));
        };
        view.gone = true;
        let registry = locked(&self.registry);
        for session in &mut view.sessions {
            let live = registry
                .live
                .values()
                .find(|l| l.path == floor.path && l.slug == session.slug && !l.exited());
            match live {
                Some(live) => *session = live.view(),
                None => {
                    session.status = Status::Stopped;
                    session.draft = false;
                    session.message_waiting = false;
                }
            }
        }
        // A session that started after the last view was made.
        for live in registry.live.values() {
            if live.path == floor.path
                && !live.exited()
                && !view.sessions.iter().any(|s| s.slug == live.slug)
            {
                view.sessions.push(live.view());
            }
        }
        Ok(view)
    }

    /// The open handle of a floor. It opens the floor if it is not open. `FloorGone` when the
    /// floor is open and its folder is gone (see `check_alive`).
    fn floor_handle(&self, path: &Path) -> Result<Arc<FloorHandle>> {
        let key = normalize(path);
        let held = locked(&self.floors).get(&key).cloned();
        if let Some(handle) = held {
            self.check_alive(&handle)?;
            return Ok(handle);
        }
        let store = FloorStore::open(&key)?;
        repair_orphans(&store);
        let handle = Arc::new(FloorHandle::new(key.clone(), store));
        let mut floors = locked(&self.floors);
        // Another thread can have opened it while we did. Then its handle wins.
        Ok(Arc::clone(floors.entry(key).or_insert(handle)))
    }

    fn floor_view(&self, floor: &FloorHandle) -> Result<FloorView> {
        let (name, brand, defaults, loaded, records, setup_record, marks) = {
            let store = locked(&floor.store);
            let mut loaded = store.read_employees()?;
            // The slug `setup` is reserved. A file with that slug is an error, not an employee.
            if loaded.employees.iter().any(|e| e.slug == SETUP_SLUG) {
                loaded.employees.retain(|e| e.slug != SETUP_SLUG);
                loaded.errors.push(StoreError::Invalid {
                    path: floor.swarm().join("employees").join("setup.md"),
                    message: format!(
                        "the slug \"{SETUP_SLUG}\" is reserved for the setup agent. Rename the file."
                    ),
                });
            }
            let mut records = Vec::with_capacity(loaded.employees.len());
            for employee in &loaded.employees {
                // `can_resume` needs the history of the CLI session (D3).
                let record = match store.db().last_session(&employee.slug)? {
                    Some(record) => {
                        let resumable = match &record.cli_session_id {
                            Some(cli_id) => {
                                cli_session_had_turn(store.db(), &employee.slug, cli_id)?
                            }
                            None => false,
                        };
                        Some((record, resumable))
                    }
                    None => None,
                };
                records.push(record);
            }
            let setup_record = store.db().last_session(SETUP_SLUG)?;
            let marks = store.db().resume_marks()?;
            (
                store.floor().name.clone(),
                store.floor().brand.clone(),
                store.floor().defaults.clone(),
                loaded,
                records,
                setup_record,
                marks,
            )
        };
        let mut sessions = Vec::new();
        let mut setup_session_id = None;
        {
            let registry = locked(&self.registry);
            for (employee, record) in loaded.employees.iter().zip(records) {
                let live = registry
                    .live
                    .values()
                    .find(|l| l.path == floor.path && l.slug == employee.slug && !l.exited());
                if let Some(live) = live {
                    sessions.push(live.view());
                } else if let Some((record, resumable)) = record {
                    let starting = registry
                        .reserved
                        .contains(&(floor.path.clone(), employee.slug.clone()));
                    sessions.push(SessionView {
                        session_id: record.app_session_id,
                        slug: employee.slug.clone(),
                        status: if starting {
                            Status::Starting
                        } else {
                            Status::Stopped
                        },
                        draft: false,
                        message_waiting: false,
                        can_resume: !starting && resumable,
                    });
                }
            }
            // The setup session: the live one, or the last one. It never resumes.
            let live = registry
                .live
                .values()
                .find(|l| l.path == floor.path && l.slug == SETUP_SLUG && !l.exited());
            if let Some(live) = live {
                setup_session_id = Some(live.id.clone());
                sessions.push(live.view());
            } else if let Some(record) = setup_record {
                let starting = registry
                    .reserved
                    .contains(&(floor.path.clone(), SETUP_SLUG.to_owned()));
                sessions.push(SessionView {
                    session_id: record.app_session_id,
                    slug: SETUP_SLUG.to_owned(),
                    status: if starting {
                        Status::Starting
                    } else {
                        Status::Stopped
                    },
                    draft: false,
                    message_waiting: false,
                    can_resume: false,
                });
            }
        }
        let (logo_busy, logo_error) = {
            let state = locked(&floor.logo);
            (state.busy, state.error.clone())
        };
        let resume_progress = locked(&floor.resume).clone();
        let resumable = if resume_progress.is_some() {
            // The resume runs now. The button shows its progress, not a second list.
            Vec::new()
        } else {
            resume::resumable_slugs(&marks, &loaded.employees, &sessions)
        };
        let view = FloorView {
            path: floor.path.clone(),
            name,
            employees: loaded.employees,
            employee_errors: loaded.errors.iter().map(ToString::to_string).collect(),
            sessions,
            description: brand.description,
            logo_prompt: brand.logo_prompt,
            logo_version: logo_run::logo_version(&floor.path),
            logo_busy,
            logo_error,
            setup_session_id,
            defaults,
            resumable,
            requests: locked(&floor.requests).clone(),
            resume_progress,
            gone: false,
        };
        *locked(&floor.last_view) = Some(view.clone());
        Ok(view)
    }

    // ---- logo and settings ----

    /// The bytes of `.swarm/assets/logo.png`, or `None` when there is no file.
    pub fn floor_logo(&self, path: &Path) -> Result<Option<Vec<u8>>> {
        let floor = self.floor_handle(path)?;
        match fs::read(logo_run::logo_path(&floor.path)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(io_message("cannot read the logo", error)),
        }
    }

    /// Start a logo generation in the background and return at once. `prompt` `Some(text)` is
    /// written to `[brand] logo_prompt` first. The error text starts with `NO_KEY:` when no
    /// OpenAI key is known, and with `BUSY:` when a generation runs for this floor already. The
    /// observer gets `floor_changed` when the generation starts and when it ends.
    pub fn logo_generate(&self, path: &Path, prompt: Option<String>) -> Result<()> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(TowerError::Closing);
        }
        let floor = self.floor_handle(path)?;
        logo_run::start(&self.logo_env, &self.notifier, &floor, prompt.as_deref())
    }

    /// The settings of the logo. They never hold the key.
    ///
    /// `codex_sandbox_mode` is the stored state. `codex_sandbox_effective` is what a Codex start
    /// gets now. It comes from the mode and from the LAST probe of this run of the app. If no
    /// probe ran yet, this call runs one. A later change of the machine shows after the next
    /// probe ("Test again" or a Codex start). `codex_sandbox` is `true` when the effective state
    /// is `On`.
    pub fn settings_get(&self) -> SettingsView {
        let mut view = settings::view(&self.logo_env.config_file, &self.logo_env.env);
        view.codex_sandbox_effective = self.sandbox_effective(view.codex_sandbox_mode);
        view.codex_sandbox = view.codex_sandbox_effective == SandboxEffective::On;
        view
    }

    /// The effective state of the sandbox (see `settings_get`).
    fn sandbox_effective(&self, mode: CodexSandboxMode) -> SandboxEffective {
        if mode == CodexSandboxMode::Off {
            return SandboxEffective::Off;
        }
        let last = locked(&self.sandbox_gate).last;
        let state = match last {
            Some(state) => state,
            None => self
                .codex_sandbox_probe()
                .map_or(SandboxState::Fails, |probe| probe.state),
        };
        match (state, mode) {
            (SandboxState::Works, _) => SandboxEffective::On,
            (_, CodexSandboxMode::On) => SandboxEffective::Refused,
            _ => SandboxEffective::Off,
        }
    }

    /// Change the settings. `None` is "no change". An empty text removes the stored value: the
    /// key from the app config, or the image model (the default model is used then).
    pub fn settings_set(
        &self,
        openai_key: Option<String>,
        image_model: Option<String>,
    ) -> Result<SettingsView> {
        if let Some(key) = &openai_key
            && !key.trim().chars().all(|c| c.is_ascii_graphic())
        {
            // The text of the key is not in the message.
            return Err(other(
                "the key has a space or a character that a key cannot have",
            ));
        }
        if let Some(model) = &image_model {
            // An empty text removes the stored model. Any other text must be a valid name.
            models::check_model("image model", Some(model.trim())).map_err(other)?;
        }
        if openai_key.is_some() || image_model.is_some() {
            settings::update(&self.logo_env.config_file, |table| {
                if let Some(key) = &openai_key {
                    settings::set_openai_text(table, "key", key);
                }
                if let Some(model) = &image_model {
                    settings::set_openai_text(table, "image_model", model);
                }
            })
            .map_err(|e| io_message("cannot write the app config", e))?;
        }
        Ok(self.settings_get())
    }

    /// The switch "Desktop notification when an agent asks a question" (`[interface]` of the app
    /// config). `None` is "no change". Returns the settings.
    pub fn settings_set_interface(&self, notify_on_question: Option<bool>) -> Result<SettingsView> {
        if let Some(on) = notify_on_question {
            settings::update(&self.logo_env.config_file, |table| {
                settings::set_notify_on_question(table, on);
            })
            .map_err(|e| io_message("cannot write the app config", e))?;
        }
        Ok(self.settings_get())
    }

    /// The switch "Claude auto mode" (`[claude] auto_mode` of the app config, default `true`).
    /// `None` is "no change". It applies at the next start of a session. Returns the settings.
    pub fn settings_set_claude(&self, auto_mode: Option<bool>) -> Result<SettingsView> {
        if let Some(on) = auto_mode {
            settings::update(&self.logo_env.config_file, |table| {
                settings::set_claude_auto_mode(table, on);
            })
            .map_err(|e| io_message("cannot write the app config", e))?;
        }
        Ok(self.settings_get())
    }

    /// The old switch of the Codex sandbox: `true` is "Always" (`On`) and `false` is "Never"
    /// (`Off`). `None` is "no change". See `settings_set_codex_mode`.
    pub fn settings_set_codex(&self, sandbox: Option<bool>) -> Result<SettingsView> {
        self.settings_set_codex_mode(sandbox.map(|on| {
            if on {
                CodexSandboxMode::On
            } else {
                CodexSandboxMode::Off
            }
        }))
    }

    /// The setting "Codex sandbox" with its three states (`[codex] sandbox` of the app config).
    /// `None` is "no change". Choosing `On` ("Always") runs the probe first and refuses unless
    /// the probe says `works`. `Auto` and `Off` are always allowed. It applies at the next start
    /// of a session. Returns the settings.
    pub fn settings_set_codex_mode(&self, mode: Option<CodexSandboxMode>) -> Result<SettingsView> {
        if let Some(mode) = mode {
            if mode == CodexSandboxMode::On {
                let probe = self.codex_sandbox_probe()?;
                match probe.state {
                    SandboxState::Works => {}
                    SandboxState::Fails => {
                        return Err(other(format!(
                            "the Codex sandbox does not work on this machine, so it cannot be set to \"Always\": {}",
                            probe.detail
                        )));
                    }
                    SandboxState::Missing => {
                        return Err(other(format!(
                            "the Codex sandbox cannot be set to \"Always\": {}",
                            probe.detail
                        )));
                    }
                }
            }
            settings::update(&self.logo_env.config_file, |table| {
                settings::set_codex_sandbox_mode(table, mode);
            })
            .map_err(|e| io_message("cannot write the app config", e))?;
        }
        Ok(self.settings_get())
    }

    /// Test the sandbox of Codex: run `bwrap --unshare-user --ro-bind / / true` with the PATH
    /// and the environment of a session, with a time limit of `PROBE_TIMEOUT`. It starts no
    /// Codex session and no shell.
    pub fn codex_sandbox_probe(&self) -> Result<SandboxProbe> {
        self.codex_sandbox_probe_within(PROBE_TIMEOUT)
    }

    /// `codex_sandbox_probe` with another time limit (the tests use a short one).
    #[doc(hidden)]
    pub fn codex_sandbox_probe_within(&self, timeout: Duration) -> Result<SandboxProbe> {
        let dirs: Vec<PathBuf> = {
            let mut book = locked(&self.programs);
            tools::PROGRAM_NAMES
                .iter()
                .filter_map(|each| book.resolve(each).ok())
                .filter_map(|path| path.parent().map(Path::to_path_buf))
                .collect()
        };
        // The same environment as a session (see `program_test_within`).
        let env = build_env(&self.session_env.get(), &[], &dirs)
            .map_err(|e| other(format!("cannot build the environment: {e}")))?;
        let cwd = self.home.clone().unwrap_or_else(|| PathBuf::from("/"));
        let probe = sandbox_probe::run(&env, &cwd, timeout);
        // Each probe updates what the start of a session remembers. A failure clears it.
        {
            let mut gate = locked(&self.sandbox_gate);
            gate.works_at = (probe.state == SandboxState::Works).then(Instant::now);
            gate.last = Some(probe.state);
        }
        Ok(probe)
    }

    /// The check at the start of a Codex session when the sandbox is wanted. A `works` result
    /// that is less than `sandbox_window` old counts, so the start of a team runs the probe one
    /// time. `Err` holds the detail of any other result. A failure is never kept.
    fn check_sandbox(&self) -> std::result::Result<(), String> {
        let fresh = locked(&self.sandbox_gate)
            .works_at
            .is_some_and(|at| at.elapsed() < self.sandbox_window);
        if fresh {
            return Ok(());
        }
        match self.codex_sandbox_probe() {
            Ok(probe) if probe.state == SandboxState::Works => Ok(()),
            Ok(probe) => Err(probe.detail),
            Err(error) => Err(error.to_string()),
        }
    }

    /// The start of a Codex session with the setting at "Always": a failing check refuses the
    /// start with `SandboxDown`. The app never falls back to full access in this state.
    fn require_sandbox(&self, name: &str) -> Result<()> {
        self.check_sandbox()
            .map_err(|detail| TowerError::SandboxDown {
                name: name.to_owned(),
                detail,
            })
    }

    /// The start of a Codex session with the setting at "Automatic": does the sandbox work now?
    /// When it does not, the session starts with full access, and the observer gets ONE
    /// information notice (kind `sandbox_auto_off`) in each run of the app.
    fn auto_sandbox(&self, path: &Path) -> bool {
        if self.check_sandbox().is_ok() {
            return true;
        }
        let first = {
            let mut gate = locked(&self.sandbox_gate);
            !std::mem::replace(&mut gate.auto_notice_sent, true)
        };
        if first {
            self.notifier
                .notice(path, "sandbox_auto_off", AUTO_OFF_NOTICE);
        }
        false
    }

    /// Tell the observer (kind `sandbox_down`) about a refused start, when `error` is
    /// `SandboxDown`. At most one notice goes out in each `sandbox_window`, so a team start with
    /// five Codex employees gives one toast.
    pub(crate) fn notice_sandbox_down(&self, path: &Path, error: &TowerError) {
        if !matches!(error, TowerError::SandboxDown { .. }) {
            return;
        }
        {
            let mut gate = locked(&self.sandbox_gate);
            if gate
                .notice_at
                .is_some_and(|at| at.elapsed() < self.sandbox_window)
            {
                return;
            }
            gate.notice_at = Some(Instant::now());
        }
        self.notifier
            .notice(path, "sandbox_down", &error.to_string());
    }

    /// The model lists. It reads the Codex file and the app config again on each call.
    pub fn models_get(&self) -> ModelCatalog {
        models::catalog(&self.logo_env.config_file, self.codex_home.as_deref())
    }

    /// Replace the custom models of one kind (`claude`, `codex` or `image`). A bad name refuses
    /// the whole call. A duplicate, and a name that is already in the list, is dropped.
    pub fn models_set_custom(&self, kind: &str, names: &[String]) -> Result<ModelCatalog> {
        let kind = ModelKind::parse(kind).map_err(other)?;
        models::set_custom(
            &self.logo_env.config_file,
            self.codex_home.as_deref(),
            kind,
            names,
        )
        .map_err(other)?;
        Ok(self.models_get())
    }

    // ---- sessions ----

    /// Start a fresh CLI session. `first_message` `None` (or empty) gives the default message.
    pub fn session_start(
        &self,
        path: &Path,
        slug: &str,
        first_message: Option<String>,
        rows: u16,
        cols: u16,
    ) -> Result<SessionView> {
        self.launch(
            path,
            Target::Employee(slug),
            false,
            first_message,
            rows,
            cols,
        )
    }

    /// Start a fresh setup session in the floor folder (slug `setup`). The CLI flags are those of
    /// an employee with the policy `edit-in-worktree`. The model and the effort come from
    /// `[defaults]` of `floor.toml`. The instructions are `FLOOR_INIT`, and the first message is
    /// `SETUP_FIRST_MESSAGE`. Refused while a setup session is live for this floor.
    pub fn setup_start(
        &self,
        path: &Path,
        cli: CliKind,
        rows: u16,
        cols: u16,
    ) -> Result<SessionView> {
        self.launch(
            path,
            Target::Setup(cli),
            false,
            Some(SETUP_FIRST_MESSAGE.to_owned()),
            rows,
            cols,
        )
    }

    /// Resume the last CLI session of the employee. Claude with no stored ID is an error.
    /// Codex with no stored ID starts fresh (spec 4.2).
    pub fn session_resume(
        &self,
        path: &Path,
        slug: &str,
        message: Option<String>,
        rows: u16,
        cols: u16,
    ) -> Result<SessionView> {
        self.launch(path, Target::Employee(slug), true, message, rows, cols)
    }

    fn launch(
        &self,
        path: &Path,
        target: Target<'_>,
        resume: bool,
        message: Option<String>,
        rows: u16,
        cols: u16,
    ) -> Result<SessionView> {
        self.launch_with(path, target, resume, message, Vec::new(), rows, cols)
    }

    /// `launch`, and the IDs of the stored messages that `message` carries (`bus.rs`,
    /// `start_for_message`). Those messages stay "waiting" in the store until the new session
    /// reaches its first `working`. If it stops before that, they stay waiting.
    #[allow(clippy::too_many_arguments)]
    fn launch_with(
        &self,
        path: &Path,
        target: Target<'_>,
        resume: bool,
        message: Option<String>,
        carried: Vec<i64>,
        rows: u16,
        cols: u16,
    ) -> Result<SessionView> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(TowerError::Closing);
        }
        let (slug, setup_cli) = match target {
            Target::Employee(slug) => (slug, None),
            Target::Setup(cli) => (SETUP_SLUG, Some(cli)),
        };
        if setup_cli.is_none() && slug == SETUP_SLUG {
            return Err(other(format!(
                "the slug \"{SETUP_SLUG}\" is the setup session. Use setup_start."
            )));
        }
        let floor = self.floor_handle(path)?;
        let _reservation = self.reserve(&floor.path, slug)?;
        // The defaults are read from the current file, not from the file at app start.
        reload(&floor)?;

        let (employee, previous, team, floor_allow) = {
            let store = locked(&floor.store);
            // The team goes into the instructions. The setup session has none.
            let team = if setup_cli.is_some() {
                Vec::new()
            } else {
                store.read_employees()?.employees
            };
            let mut employee = match setup_cli {
                Some(cli) => setup_employee(cli, &floor.path),
                None => team
                    .iter()
                    .find(|e| e.slug == slug)
                    .cloned()
                    .ok_or_else(|| StoreError::EmployeeNotFound(slug.to_owned()))?,
            };
            // The defaults of `floor.toml` fill what the employee leaves empty. They apply only
            // when the floor has no default CLI, or its default CLI is the CLI of the employee:
            // a Claude model name must not go to Codex (`docs/ipc-settings.md`, section 5).
            let defaults = &store.floor().defaults;
            if defaults.cli.is_none_or(|cli| cli == employee.cli) {
                if employee.model.as_deref().is_none_or(str::is_empty) {
                    employee.model = defaults.model.clone();
                }
                if employee.effort.as_deref().is_none_or(str::is_empty) {
                    employee.effort = defaults.effort.clone();
                }
            }
            // A hand-written employee file or `floor.toml` has no check. The model and the effort
            // go to the CLI as arguments, so a bad one is refused here, before the process starts.
            models::check_model(
                &format!("model of {}", employee.name),
                employee.model.as_deref(),
            )
            .map_err(other)?;
            models::check_effort(
                &format!("effort of {}", employee.name),
                employee.effort.as_deref(),
            )
            .map_err(other)?;
            let previous = if resume {
                match store
                    .db()
                    .last_session(slug)?
                    .and_then(|record| record.cli_session_id)
                {
                    Some(cli_id) => {
                        if !cli_session_had_turn(store.db(), slug, &cli_id)? {
                            return Err(other(format!(
                                "the last session of {} never had a turn, so there is nothing to resume. Start a fresh session.",
                                employee.name
                            )));
                        }
                        Some(cli_id)
                    }
                    None => None,
                }
            } else {
                None
            };
            // The rules of the operator for this floor, from the app config (never from
            // `floor.toml`: an agent can write that file). A rule that does not pass the check
            // is dropped here and never reaches the CLI.
            let floor_allow = if setup_cli.is_some() {
                Vec::new()
            } else {
                valid_allow_rules(
                    &settings::floor_rules(&self.logo_env.config_file, &floor.path)
                        .unwrap_or_default(),
                )
            };
            (employee, previous, team, floor_allow)
        };
        // Worktrees: only a floor that is the top of a git repository with a commit, on a
        // branch, gets them. The setup session never does (it writes `.swarm/` files).
        let worktrees = setup_cli.is_none()
            && matches!(
                gitwork::repo_state(&floor.path),
                gitwork::RepoState::Ready { .. }
            );
        // The Codex sandbox (the switch in Settings, decision O7): only a Codex employee gets it,
        // never the setup session. The switch is read now, so a change applies at the next start.
        // `codex_sandbox` is the EFFECTIVE state of this start. The instructions and the adapter
        // follow it. "Always" needs a sandbox that works now (D1 of the review) and refuses the
        // start otherwise. "Automatic" falls back to full access (decision of 2026-11-21).
        let codex_sandbox = if setup_cli.is_none() && employee.cli == CliKind::Codex {
            match settings::codex_sandbox_mode(&self.logo_env.config_file) {
                CodexSandboxMode::Off => false,
                CodexSandboxMode::On => {
                    self.require_sandbox(&employee.name)?;
                    true
                }
                CodexSandboxMode::Auto => self.auto_sandbox(&floor.path),
            }
        } else {
            false
        };
        if codex_sandbox && employee.permission != PermissionPolicy::AskAlways {
            // The sandbox profile names these folders. A folder that does not exist yet (the
            // worktrees folder, before the first task) must exist when the CLI starts.
            for folder in [".swarm/reports", ".swarm/worktrees"] {
                fs::create_dir_all(floor.path.join(folder)).map_err(|e| {
                    io_message(
                        &format!("cannot make the folder {folder} for the sandbox"),
                        e,
                    )
                })?;
            }
        }
        if resume && previous.is_none() && employee.cli == CliKind::Claude {
            return Err(other(format!(
                "{} has no stored Claude session to resume. Start a fresh session.",
                employee.name
            )));
        }
        // Codex with no stored ID starts fresh, so it gets the default first message.
        // A Codex session sends no hook event until its first prompt, also after a resume. Without
        // a prompt it stays in `starting`, and after 20 s the status becomes a false `question`
        // (found live on 2026-11-21: the resumed TUI was idle). So a Codex resume with no message
        // gets a short default message. A Claude resume sends `SessionStart` by itself and gets none.
        let message = message.filter(|m| !m.trim().is_empty());
        let first_message = match (&previous, message) {
            (Some(_), Some(message)) => Some(message),
            (Some(_), None) if employee.cli == CliKind::Codex => {
                Some(default_resume_message(&employee))
            }
            (Some(_), None) => None,
            (None, message) => Some(message.unwrap_or_else(|| default_first_message(&employee))),
        };

        let programs = self.programs_for(employee.cli)?;
        let hook_program = self.hook_program()?;
        let id = Uuid::new_v4().to_string();
        let log_path = floor.log_path(&id);
        // The token of this session. It leaves the map when the session ends (`status_hook`).
        let token = (setup_cli.is_none()).then(bus::new_token);
        let token_text = token.clone();
        // The carried messages are held (hidden from `inbox` and from the paste) from now on, so
        // a fast first turn cannot run before the hold exists. An error before the session
        // starts drops the hold, and the messages are still waiting in the store.
        let held = (!carried.is_empty()).then(|| {
            self.bus
                .hold_for_session(&id, &floor.path, &employee.slug, carried)
        });
        let has_first_message = first_message.is_some();
        let token_guard = token.as_ref().map(|token| {
            locked(&self.tokens).insert(
                token.clone(),
                bus::Identity {
                    path: floor.path.clone(),
                    slug: employee.slug.clone(),
                    session_id: id.clone(),
                },
            );
            bus::TokenGuard::new(Arc::clone(&self.tokens), token.clone())
        });
        let ctx = StartContext {
            mcp: token.map(|token| McpAccess {
                url: self.mcp_url.clone(),
                token,
            }),
            worktrees,
            floor_allow,
            codex_sandbox,
            claude_auto: settings::claude_auto_mode(&self.logo_env.config_file),
            instructions: Some(if setup_cli.is_some() {
                instructions(&employee, None, &floor.path, false, false)
            } else {
                instructions(
                    &employee,
                    Some(&team),
                    &floor.path,
                    worktrees,
                    codex_sandbox,
                )
            }),
            employee: employee.clone(),
            cwd: floor.path.clone(),
            app_session_id: id.clone(),
            socket: self.socket.clone(),
            hook_program,
            programs,
            parent_env: self.session_env.get(),
            first_message,
            rows: rows.max(1),
            cols: cols.max(1),
            log: Some(LogConfig {
                path: log_path.clone(),
                max_bytes: self.log_limit,
            }),
        };
        let adapter = (self.adapters)(employee.cli);
        let plan = match &previous {
            Some(cli_id) => adapter.resume(&ctx, cli_id),
            None => adapter.start(&ctx),
        }
        .map_err(|e| other(e.to_string()))?;
        drop(adapter);

        // The row is in the database BEFORE the process starts. If the CLI session ID is known
        // (Claude, and every resume), it is in the row at once.
        let db = Database::open(&floor.swarm().join("state").join("floor.db"))?;
        db.create_session(&SessionRecord {
            app_session_id: id.clone(),
            employee_slug: employee.slug.clone(),
            cli_session_id: plan.cli_session_id.clone(),
            started_at: now_ms(),
            ended_at: None,
            last_status: Status::Starting.as_str().to_owned(),
        })?;
        db.append_status_event(&id, Status::Starting.as_str(), now_ms())?;

        // The route is made BEFORE the process starts, so no hook event is lost.
        let from_router = self.router.subscribe_replying(&id);
        // The hook thread of the app applies the events to the session itself (`live.rs`,
        // `run_hooks`), so the session gets a closed channel and its own hook pump ends at once.
        let (_, session_events) = mpsc::channel();
        let (handle, streams) = match Session::start(plan.spec, employee.cli, &id, session_events) {
            Ok(started) => started,
            Err(error) => {
                self.router.unsubscribe(&id);
                let _ = db.append_status_event(&id, Status::Stopped.as_str(), now_ms());
                let _ = db.set_ended_at(&id, now_ms());
                return Err(other(format!("cannot start {}: {error}", employee.name)));
            }
        };
        let session = handle.session().clone();
        if setup_cli.is_some() {
            logo_run::remember_setup_prompt(&floor);
        }
        let learn_id = employee.cli == CliKind::Codex && plan.cli_session_id.is_none();
        let live = Live::new(LiveParts {
            id: id.clone(),
            path: floor.path.clone(),
            slug: employee.slug.clone(),
            session,
            handle,
            cli_session_id: plan.cli_session_id.clone(),
            // A resume only starts for a session that had a turn.
            had_turn: previous.is_some(),
            db,
            notifier: Arc::clone(&self.notifier),
            attach_pause: self.attach_pause.clone(),
            before_status: locked(&self.test_hooks.before_status).clone(),
            after_turn: setup_cli.map(|_| self.setup_hook(&floor)),
            on_status: token_guard
                .as_ref()
                .map(|guard| self.status_hook(&floor.path, slug, guard.token())),
            has_first_message,
            on_question: Some(self.question_hook(&floor, setup_cli.is_some())),
            // The Stop hook delivers messages to Claude and Codex employees (spec 4.5, item N14,
            // closed). The setup session has no token, so it has no hook.
            on_stop: token_guard
                .is_some()
                .then(|| self.stop_hook(&floor.path, slug)),
        });
        locked(&self.registry)
            .live
            .insert(id.clone(), Arc::clone(&live));
        // The restore list: this employee has a live session now. A crash of the app keeps it.
        if setup_cli.is_none() {
            self.add_resume_mark(&floor, slug);
        }
        // From here the status thread removes the token when the session ends.
        if let Some(guard) = token_guard {
            guard.disarm();
        }
        // From here the status thread settles the hold: it is delivered at the first `working`,
        // and it ends at `stopped` (`status_hook`).
        if let Some(held) = held {
            held.disarm();
        }
        // The first notification is `starting`. The threads start after it, so it comes first.
        live.refresh();
        if let Err(error) = spawn_pumps(
            &live,
            streams.status,
            streams.output,
            from_router,
            Arc::clone(&self.router),
            learn_id,
        ) {
            live.kill();
            if setup_cli.is_none() {
                self.clear_resume_mark(&floor.path, slug);
            }
            if let Some(token) = &token_text {
                bus::forget_token(&self.tokens, token);
            }
            self.bus.release_session(&id);
            return Err(io_message("cannot start the threads of the session", error));
        }
        Ok(live.view())
    }

    /// What the status thread of a session runs when the status changes to `question`: tell the
    /// observer, with the name and the role of the employee and the name of the floor, so the
    /// shell can show a desktop notification (`notify`).
    fn question_hook(&self, floor: &Arc<FloorHandle>, is_setup: bool) -> live::QuestionHook {
        let floor = Arc::clone(floor);
        let notifier = Arc::clone(&self.notifier);
        let config_file = self.logo_env.config_file.clone();
        Arc::new(move |live, old| {
            let (floor_name, who) = {
                let store = locked(&floor.store);
                let who = if is_setup {
                    Some(("Setup agent".to_owned(), "Setup".to_owned()))
                } else {
                    store.read_employees().ok().and_then(|loaded| {
                        loaded
                            .employees
                            .into_iter()
                            .find(|e| e.slug == live.slug)
                            .map(|e| (e.name, e.role))
                    })
                };
                (store.floor().name.clone(), who)
            };
            let (name, role) = who.unwrap_or_else(|| (live.slug.clone(), "employee".to_owned()));
            notifier.question_asked(
                &live.path,
                &QuestionAsked {
                    session_id: live.id.clone(),
                    slug: live.slug.clone(),
                    name,
                    role,
                    floor_name,
                    old,
                    new: Status::Question,
                    notifications_on: settings::notify_on_question(&config_file),
                },
            );
        })
    }

    /// What the status thread of the setup session runs after each turn and at the stop: read the
    /// floor again, tell the observer, and start the logo if the conditions hold.
    fn setup_hook(&self, floor: &Arc<FloorHandle>) -> Arc<dyn Fn() + Send + Sync> {
        let floor = Arc::clone(floor);
        let notifier = Arc::clone(&self.notifier);
        let env = Arc::clone(&self.logo_env);
        let closed = Arc::clone(&self.closed);
        Arc::new(move || {
            // A `floor.toml` that does not load (the agent is in the middle of an edit) keeps the
            // old data. The observer still gets the event, because employee files may have changed.
            // The setup agent marks a file that it wants removed; the app removes it here.
            setup::remove_marked_employees(&floor.path);
            let loaded = reload(&floor).is_ok();
            notifier.floor_changed(&floor.path);
            // The app is closing: the stop of the session must not start a request.
            if loaded && !closed.load(Ordering::SeqCst) {
                logo_run::auto_start(&env, &notifier, &floor);
            }
        })
    }

    fn reserve(&self, path: &Path, slug: &str) -> Result<Reservation> {
        let mut registry = locked(&self.registry);
        if registry.is_busy(path, slug) {
            return Err(TowerError::AlreadyLive {
                slug: slug.to_owned(),
            });
        }
        let key = (path.to_path_buf(), slug.to_owned());
        registry.reserved.insert(key.clone());
        Ok(Reservation {
            registry: Arc::clone(&self.registry),
            key,
        })
    }

    /// Stop the process. The status then becomes `stopped` (the observer is told). A process
    /// that does not end in a few seconds gets `SIGKILL`.
    pub fn session_stop(&self, session_id: &str) -> Result<()> {
        let live = self.live(session_id)?;
        if live.exited() {
            return Ok(());
        }
        // The operator stops this session by hand: it is not part of the team to resume (M3).
        self.clear_resume_mark(&live.path, &live.slug);
        locked(&self.registry).manual_stops.insert(live.id.clone());
        live.kill();
        thread::Builder::new()
            .name("app-stop-watch".into())
            .spawn(move || {
                let deadline = Instant::now() + STOP_GRACE;
                while !live.exited() && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(50));
                }
                if !live.exited() {
                    // Dropping the handle ends the process with `SIGKILL` if it still runs.
                    live.release_process();
                }
            })
            .map_err(|e| io_message("cannot start the stop watcher", e))?;
        Ok(())
    }

    /// Attach the output sink to a session. Returns the scrollback (at most 256 KB).
    /// After that, the sink gets every new chunk, in order, with no gap and no duplicate.
    /// A second attach replaces the first sink. A session of an earlier run of the app has no
    /// live output: its scrollback is the tail of its log file.
    pub fn session_attach(&self, session_id: &str, sink: Arc<dyn OutputSink>) -> Result<Vec<u8>> {
        if let Ok(live) = self.live(session_id) {
            return Ok(live.attach(sink));
        }
        self.log_of_old_session(session_id)
            .map(|log| read_log_tail(&log, SCROLLBACK_LIMIT))
            .ok_or_else(|| TowerError::UnknownSession(session_id.to_owned()))
    }

    fn log_of_old_session(&self, session_id: &str) -> Option<PathBuf> {
        // The ID becomes a file name. Only a UUID is accepted.
        Uuid::parse_str(session_id).ok()?;
        let floors: Vec<Arc<FloorHandle>> = locked(&self.floors).values().cloned().collect();
        floors
            .iter()
            .map(|floor| floor.log_path(session_id))
            .find(|log| log.is_file())
    }

    /// Operator input. It goes through in every status. After the process ended it is an error.
    pub fn pty_write(&self, session_id: &str, bytes: &[u8]) -> Result<()> {
        let live = self.live(session_id)?;
        let result = live.session.operator_input(bytes);
        live.refresh();
        result.map_err(|error| match error.kind() {
            io::ErrorKind::BrokenPipe => other("the session has stopped"),
            _ => io_message("cannot write to the terminal", error),
        })
    }

    pub fn pty_resize(&self, session_id: &str, rows: u16, cols: u16) -> Result<()> {
        let live = self.live(session_id)?;
        live.resize(rows.max(1), cols.max(1)).map_err(other)
    }

    /// The operator clears the draft flag.
    pub fn clear_draft(&self, session_id: &str) -> Result<()> {
        let live = self.live(session_id)?;
        live.session.clear_draft();
        live.refresh();
        // A message may have waited for this flag. Deliver it now (a refusal keeps it waiting).
        self.deliver_pending(&live.path, &live.slug);
        Ok(())
    }

    /// The M1 test for message delivery. A refusal is `TowerError::Refused` with the text
    /// `REFUSED: status question`, `REFUSED: draft` and so on. Nothing is written then.
    pub fn debug_paste(&self, session_id: &str, text: &str) -> Result<()> {
        let live = self.live(session_id)?;
        let result = live.session.paste(text);
        live.refresh();
        result.map_err(|refused| TowerError::Refused(refusal_text(&refused)))
    }

    fn live(&self, session_id: &str) -> Result<Arc<Live>> {
        locked(&self.registry)
            .live
            .get(session_id)
            .cloned()
            .ok_or_else(|| TowerError::UnknownSession(session_id.to_owned()))
    }

    // ---- programs ----

    /// The resolved programs. It resolves what is not yet cached; this can take some seconds
    /// the first time (a login shell).
    pub fn programs_get(&self) -> ProgramsView {
        let (claude, codex, node, rtk, overrides) = {
            let mut book = locked(&self.programs);
            (
                book.resolve("claude").ok(),
                book.resolve("codex").ok(),
                book.resolve("node").ok(),
                book.resolve("rtk").ok(),
                tools::PROGRAM_NAMES.map(|name| book.override_of(name)),
            )
        };
        ProgramsView {
            claude,
            codex,
            node,
            hook: self.hook_program().ok(),
            rtk,
            overrides,
        }
    }

    /// Test a program: run `<path> --version` with the environment of a session (the login shell
    /// environment, and the folders of `claude`, `codex` and `node` in `PATH`), with a time limit
    /// of `PROGRAM_TEST_TIMEOUT`. It starts no model session. A program that the app cannot find
    /// gives `ok: false` and no path. `name` is `claude`, `codex` or `node`.
    pub fn program_test(&self, name: &str) -> Result<ProgramTest> {
        self.program_test_within(name, PROGRAM_TEST_TIMEOUT)
    }

    /// `program_test` with another time limit (the tests use a short one).
    #[doc(hidden)]
    pub fn program_test_within(&self, name: &str, timeout: Duration) -> Result<ProgramTest> {
        if !tools::PROGRAM_NAMES.contains(&name) {
            return Err(other(format!(
                "unknown program \"{name}\" (use claude, codex or node)"
            )));
        }
        let (found, dirs) = {
            let mut book = locked(&self.programs);
            let found = book.resolve(name);
            let dirs: Vec<PathBuf> = tools::PROGRAM_NAMES
                .iter()
                .filter_map(|each| book.resolve(each).ok())
                .filter_map(|path| path.parent().map(Path::to_path_buf))
                .collect();
            (found, dirs)
        };
        let path = match found {
            Ok(path) => path,
            Err(error) => return Ok(ProgramTest::not_found(&error)),
        };
        // The same environment as a session: `build_env` over the parent environment, with the
        // folders of the three programs in front of the `PATH`.
        let env = build_env(&self.session_env.get(), &[], &dirs)
            .map_err(|e| other(format!("cannot build the environment: {e}")))?;
        let cwd = self.home.clone().unwrap_or_else(|| PathBuf::from("/"));
        Ok(program_test::run_version(&path, &env, &cwd, timeout))
    }

    /// Store an override of `claude`, `codex` or `node`. An empty path removes it.
    pub fn programs_set(&self, name: &str, path: &str) -> Result<ProgramsView> {
        locked(&self.programs).set(name, path)?;
        Ok(self.programs_get())
    }

    fn programs_for(&self, kind: CliKind) -> Result<AdapterPrograms> {
        let mut book = locked(&self.programs);
        let own = match kind {
            CliKind::Claude => "claude",
            CliKind::Codex => "codex",
        };
        let wanted = book.resolve(own).map_err(TowerError::Other)?;
        // The adapter wants both paths (for the PATH of the session). A CLI that is not
        // installed gets the path of the other one: the folder is the same or it is a no-op.
        let claude = if kind == CliKind::Claude {
            wanted.clone()
        } else {
            book.resolve("claude").unwrap_or_else(|_| wanted.clone())
        };
        let codex = if kind == CliKind::Codex {
            wanted.clone()
        } else {
            book.resolve("codex").unwrap_or_else(|_| wanted.clone())
        };
        let node = book.resolve("node").ok();
        Ok(AdapterPrograms {
            claude,
            codex,
            node,
        })
    }

    fn hook_program(&self) -> Result<PathBuf> {
        if let Some(path) = &self.hook_override {
            return Ok(path.clone());
        }
        let mut found = locked(&self.hook_found);
        if let Some(path) = found.as_ref() {
            return Ok(path.clone());
        }
        let exe =
            std::env::current_exe().map_err(|e| io_message("cannot find the app binary", e))?;
        let path = locate_hook_program(&exe, Some(&workspace_root()))?;
        *found = Some(path.clone());
        Ok(path)
    }

    // ---- shutdown ----

    /// Stop all sessions and remove the socket. Safe to call more than once. It ends in a few
    /// seconds at most, also when a process ignores `SIGHUP`.
    pub fn shutdown(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        // The floor watcher sees `closed` and ends. It is not joined from its own thread.
        let watcher = locked(&self.watcher).take();
        if let Some(watcher) = watcher
            && watcher.thread().id() != thread::current().id()
        {
            let _ = watcher.join();
        }
        let lives: Vec<Arc<Live>> = locked(&self.registry).live.values().cloned().collect();
        // M3: remember who is live, BEFORE the sessions are killed.
        self.mark_live_for_resume(&lives);
        // End each `wait` call and the accept thread of the MCP endpoint.
        self.bus.notify_all();
        mcp::wake_accept(self.mcp_port);
        locked(&self.tokens).clear();
        for live in &lives {
            live.kill();
        }
        wait_until(SHUTDOWN_GRACE, || lives.iter().all(|live| live.exited()));
        // What is left ignores SIGHUP. Dropping the handle sends SIGKILL.
        let stubborn: Vec<_> = lives
            .iter()
            .filter(|live| !live.exited())
            .cloned()
            .map(|live| thread::spawn(move || live.release_process()))
            .collect();
        for handle in stubborn {
            let _ = handle.join();
        }
        // The status threads write the end of each session to the database.
        wait_until(SHUTDOWN_GRACE, || lives.iter().all(|live| live.exited()));
        // Dropping the receiver removes the socket file.
        drop(locked(&self.hookrx).take());
    }
}

impl Drop for Tower {
    fn drop(&mut self) {
        self.core.shutdown();
    }
}

fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + limit;
    while !done() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
}

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

/// The CLI session of an employee had at least one turn: some session row with this CLI session
/// ID has a `working` status event. A resume gives a new row with the same CLI session ID, so
/// all rows of the employee with this ID count. `claude --resume` fails with "No conversation
/// found" for a session with no turn (D3).
fn cli_session_had_turn(db: &Database, slug: &str, cli_session_id: &str) -> Result<bool> {
    for record in db.list_sessions(slug)? {
        if record.cli_session_id.as_deref() == Some(cli_session_id)
            && db
                .status_events(&record.app_session_id)?
                .iter()
                .any(|(status, _)| status == Status::Working.as_str())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The text after `REFUSED: `.
fn refusal_text(refused: &PasteRefused) -> String {
    match refused {
        PasteRefused::Status(status) => format!("status {}", status.as_str()),
        PasteRefused::Draft => "draft".to_owned(),
        PasteRefused::NotAllowedByMachine => {
            "not allowed now (a tool event came after the last turn end, or the CLI has had no prompt)"
                .to_owned()
        }
        PasteRefused::Stopped => "session stopped".to_owned(),
        PasteRefused::Write(kind) => format!("the write failed ({kind})"),
    }
}

/// The name in `floor.toml`, or `None` if the file is missing or not readable.
fn floor_name_on_disk(cwd: &Path) -> Option<String> {
    let text = fs::read_to_string(cwd.join(".swarm").join("floor.toml")).ok()?;
    let table: toml::Table = toml::from_str(&text).ok()?;
    table.get("name")?.as_str().map(str::to_owned)
}

/// A session row with no end time, when no session of this app runs it, is a session that the
/// last run of the app did not close (a crash, or a kill). It ends now (spec 4.2, "App crash").
fn repair_orphans(store: &FloorStore) {
    let Ok(loaded) = store.read_employees() else {
        return;
    };
    let slugs = loaded
        .employees
        .iter()
        .map(|employee| employee.slug.as_str())
        .chain([SETUP_SLUG]);
    for slug in slugs {
        if let Ok(Some(record)) = store.db().last_session(slug)
            && record.ended_at.is_none()
        {
            let now = now_ms();
            let _ = store.db().append_status_event(
                &record.app_session_id,
                Status::Stopped.as_str(),
                now,
            );
            let _ = store.db().set_ended_at(&record.app_session_id, now);
        }
    }
}

/// The first message of a Codex session that the operator resumes with no text (see `start_session`).
fn default_resume_message(employee: &Employee) -> String {
    format!(
        "The operator resumed your session, {}. Say that you are ready in one short sentence.",
        employee.name
    )
}

fn default_first_message(employee: &Employee) -> String {
    format!(
        "You are {}, the {}. Say that you are ready in one short sentence.",
        employee.name, employee.role
    )
}

/// The instructions of a session (spec 4.6). `team` is `None` for the setup session: it gets
/// the short text and its role text only. An employee also gets the team, and the rules for the
/// tools of the app (`bus.rs`, `employee_start` and `send`).
fn instructions(
    employee: &Employee,
    team: Option<&[Employee]>,
    floor: &Path,
    worktrees: bool,
    codex_sandbox: bool,
) -> String {
    let mut text = format!(
        "You are {}, the {} on this floor. The operator can type in your terminal at any time.",
        employee.name, employee.role
    );
    if let Some(team) = team {
        text.push_str("\n\n");
        text.push_str(&bus::team_instructions(employee, team, floor));
        if employee.cli == CliKind::Codex {
            text.push_str("\n\n");
            text.push_str(&bus::codex_permission_instructions(
                employee,
                floor,
                worktrees,
                codex_sandbox,
            ));
        }
    }
    let role_text = employee.role_text.trim();
    if !role_text.is_empty() {
        text.push_str("\n\n");
        text.push_str(role_text);
    }
    text
}

/// The employee record of the setup session. No file has it. Its role text is `FLOOR_INIT`, then
/// the section "The project now" for this folder.
fn setup_employee(cli: CliKind, cwd: &Path) -> Employee {
    Employee {
        name: "Setup".to_owned(),
        slug: SETUP_SLUG.to_owned(),
        role: "Setup agent".to_owned(),
        manager: None,
        cli,
        model: None,
        effort: None,
        permission: PermissionPolicy::EditInWorktree,
        role_text: format!(
            "{}\n\n{}",
            FLOOR_INIT.trim(),
            setup::project_section(cwd, cli)
        ),
    }
}

/// The two employees of a new floor.
fn default_employees() -> Vec<Employee> {
    vec![
        Employee {
            name: "Ada".to_owned(),
            slug: "ceo".to_owned(),
            role: "CEO".to_owned(),
            manager: None,
            cli: CliKind::Claude,
            model: None,
            effort: None,
            permission: PermissionPolicy::ReadOnly,
            role_text: "You lead this floor. You talk to the operator. You pass each request to the right employee. You do not write code yourself. For a feature request, give the work to the pm. When the pm reports that the work is done, tell the operator the result.\n".to_owned(),
        },
        Employee {
            name: "Maya".to_owned(),
            slug: "pm".to_owned(),
            role: "Project Manager".to_owned(),
            manager: Some("ceo".to_owned()),
            cli: CliKind::Claude,
            model: None,
            effort: None,
            permission: PermissionPolicy::ReadOnly,
            role_text: "You plan the work. You break each request into small tasks with task_create, you give each task to an employee with task_assign, and you follow the tasks until they are done. You do not write code yourself.\n".to_owned(),
        },
    ]
}
