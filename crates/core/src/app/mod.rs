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

mod live;
mod logo_run;
mod settings;
mod tools;

use crate::adapter::{
    ClaudeAdapter, CliAdapter, CodexAdapter, Programs as AdapterPrograms, StartContext,
};
use crate::env::{EnvMap, current_env};
use crate::hookrx::HookReceiver;
use crate::logo::{DEFAULT_API_BASE, REQUEST_TIMEOUT};
use crate::model::{CliKind, Employee, PermissionPolicy, SessionRecord};
use crate::programs::Resolver;
use crate::pty::LogConfig;
use crate::session::{HookRouter, PasteRefused, Session};
use crate::status::Status;
use crate::store::{Database, FloorList, FloorStore, StoreError};
use live::{Live, LiveParts, Notifier, locked, now_ms, read_log_tail, spawn_pumps};
use logo_run::{LogoEnv, LogoState};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub use live::SCROLLBACK_LIMIT;
pub use settings::{KeySource, SettingsView};
pub use tools::{FAILURE_TTL, hook_candidates, locate_hook_program, workspace_root};

/// The size limit of the PTY log of one session (spec 4.9; the log keeps 4 to 8 MB).
pub const LOG_LIMIT_BYTES: u64 = 4 * 1024 * 1024;

/// The reserved slug of the setup session. No employee can have it.
pub const SETUP_SLUG: &str = "setup";

/// The instructions of the setup agent (the "floor-init skill"). They go in through the same path
/// as the role text of an employee.
pub const FLOOR_INIT: &str = include_str!("floor_init.md");

/// The first message of the setup session.
pub const SETUP_FIRST_MESSAGE: &str = "Start the floor setup.";

/// How long `shutdown` waits for the processes to end before it uses `SIGKILL`.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

/// How long `session_stop` waits for the process to end before it uses `SIGKILL`.
const STOP_GRACE: Duration = Duration::from_secs(3);

// ---------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------

/// Every error of the Tower. `Display` is the text that the operator reads. The text of
/// `NoFloor` starts with `NO_FLOOR:` and the text of `Refused` starts with `REFUSED:`, as the
/// contract says.
#[derive(Debug, thiserror::Error)]
pub enum TowerError {
    #[error("NO_FLOOR: there is no floor at {}. The folder has no .swarm/ folder.", .0.display())]
    NoFloor(PathBuf),
    #[error("REFUSED: {0}")]
    Refused(String),
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
}

/// Receives the changes. See the module text for the rules.
pub trait Observer: Send + Sync {
    /// The status, the draft flag, the message-waiting flag or the resume flag of a session
    /// changed, or the session started. `path` is the key of the floor (see `FloorView::path`).
    fn session_changed(&self, path: &Path, session: &SessionView);
    /// The employees of a floor changed.
    fn floor_changed(&self, path: &Path);
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
    /// The environment that the allow-list is copied from.
    pub parent_env: EnvMap,
    /// The size limit of each PTY log file.
    pub log_limit: u64,
    /// The base URL of the image API. Tests give the URL of a local fake server.
    pub image_api_base: String,
    /// The time limit of one logo request.
    pub logo_timeout: Duration,
    /// Test only. Runs in `session_attach` inside the critical section of the output buffer,
    /// after the scrollback was copied and before the sink is installed. A correct build holds
    /// the output thread during the call; a build with a gap or a duplicate does not.
    pub attach_pause: Option<Arc<dyn Fn() + Send + Sync>>,
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
            log_limit: LOG_LIMIT_BYTES,
            image_api_base: DEFAULT_API_BASE.to_owned(),
            logo_timeout: REQUEST_TIMEOUT,
            attach_pause: None,
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
}

impl FloorHandle {
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

pub struct Tower {
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
    parent_env: EnvMap,
    log_limit: u64,
    logo_env: Arc<LogoEnv>,
    attach_pause: Option<Arc<dyn Fn() + Send + Sync>>,
    closed: Arc<AtomicBool>,
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
        Ok(Tower {
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
            parent_env: config.parent_env,
            log_limit: config.log_limit,
            logo_env,
            attach_pause: config.attach_pause,
            closed: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Set the observer (it replaces the old one).
    pub fn set_observer(&self, observer: Arc<dyn Observer>) {
        self.notifier.set(observer);
    }

    /// The path of the hook socket.
    pub fn socket_path(&self) -> &Path {
        &self.socket
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

    /// Import `{path}/.swarm/`. `TowerError::NoFloor` when there is none.
    pub fn floor_open(&self, path: &Path) -> Result<FloorView> {
        let floor = self.floor_handle(path)?;
        reload(&floor)?;
        self.floor_list.add(&floor.path)?;
        self.floor_view(&floor)
    }

    /// Write a new `.swarm/` with the two default employees `ceo` and `pm`.
    pub fn floor_create(&self, path: &Path, name: &str) -> Result<FloorView> {
        let name = name.trim();
        if name.is_empty() {
            return Err(other("the floor name is empty"));
        }
        let key = normalize(path);
        if !key.is_dir() {
            return Err(other(format!(
                "the folder {} does not exist or is not a folder",
                key.display()
            )));
        }
        let store = FloorStore::create(&key, name)?;
        for employee in default_employees() {
            store.write_employee(&employee)?;
        }
        let handle = Arc::new(FloorHandle {
            path: key.clone(),
            store: Mutex::new(store),
            logo: Mutex::default(),
        });
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
                "the slug \"{SETUP_SLUG}\" is reserved for the setup agent. Use another slug."
            )));
        }
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

    /// The open handle of a floor. It opens the floor if it is not open.
    fn floor_handle(&self, path: &Path) -> Result<Arc<FloorHandle>> {
        let key = normalize(path);
        if let Some(handle) = locked(&self.floors).get(&key) {
            return Ok(Arc::clone(handle));
        }
        let store = FloorStore::open(&key)?;
        repair_orphans(&store);
        let handle = Arc::new(FloorHandle {
            path: key.clone(),
            store: Mutex::new(store),
            logo: Mutex::default(),
        });
        let mut floors = locked(&self.floors);
        // Another thread can have opened it while we did. Then its handle wins.
        Ok(Arc::clone(floors.entry(key).or_insert(handle)))
    }

    fn floor_view(&self, floor: &FloorHandle) -> Result<FloorView> {
        let (name, brand, loaded, records, setup_record) = {
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
            (
                store.floor().name.clone(),
                store.floor().brand.clone(),
                loaded,
                records,
                setup_record,
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
        Ok(FloorView {
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
        })
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
    pub fn settings_get(&self) -> SettingsView {
        settings::view(&self.logo_env.config_file, &self.logo_env.env)
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
        if let Some(model) = &image_model
            && model.trim().chars().any(|c| c.is_control())
        {
            return Err(other("the image model has a control character"));
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

        let (employee, previous) = {
            let store = locked(&floor.store);
            let mut employee = match setup_cli {
                Some(cli) => setup_employee(cli),
                None => store
                    .read_employees()?
                    .employees
                    .into_iter()
                    .find(|e| e.slug == slug)
                    .ok_or_else(|| StoreError::EmployeeNotFound(slug.to_owned()))?,
            };
            // The defaults of `floor.toml` fill what the employee leaves empty.
            let defaults = &store.floor().defaults;
            if employee.model.as_deref().is_none_or(str::is_empty) {
                employee.model = defaults.model.clone();
            }
            if employee.effort.as_deref().is_none_or(str::is_empty) {
                employee.effort = defaults.effort.clone();
            }
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
            (employee, previous)
        };
        if resume && previous.is_none() && employee.cli == CliKind::Claude {
            return Err(other(format!(
                "{} has no stored Claude session to resume. Start a fresh session.",
                employee.name
            )));
        }
        // Codex with no stored ID starts fresh, so it gets the default first message.
        let message = message.filter(|m| !m.trim().is_empty());
        let first_message = match (&previous, message) {
            (Some(_), message) => message,
            (None, message) => Some(message.unwrap_or_else(|| default_first_message(&employee))),
        };

        let programs = self.programs_for(employee.cli)?;
        let hook_program = self.hook_program()?;
        let id = Uuid::new_v4().to_string();
        let log_path = floor.log_path(&id);
        let ctx = StartContext {
            instructions: Some(instructions(&employee)),
            employee: employee.clone(),
            cwd: floor.path.clone(),
            app_session_id: id.clone(),
            socket: self.socket.clone(),
            hook_program,
            programs,
            parent_env: self.parent_env.clone(),
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
        let from_router = self.router.subscribe(&id);
        let (to_session, session_events) = mpsc::channel();
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
            after_turn: setup_cli.map(|_| self.setup_hook(&floor)),
        });
        locked(&self.registry)
            .live
            .insert(id.clone(), Arc::clone(&live));
        // The first notification is `starting`. The threads start after it, so it comes first.
        live.refresh();
        if let Err(error) = spawn_pumps(
            &live,
            streams.status,
            streams.output,
            from_router,
            to_session,
            Arc::clone(&self.router),
            learn_id,
        ) {
            live.kill();
            return Err(io_message("cannot start the threads of the session", error));
        }
        Ok(live.view())
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
        let (claude, codex, node) = {
            let mut book = locked(&self.programs);
            (
                book.resolve("claude").ok(),
                book.resolve("codex").ok(),
                book.resolve("node").ok(),
            )
        };
        ProgramsView {
            claude,
            codex,
            node,
            hook: self.hook_program().ok(),
        }
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
        let lives: Vec<Arc<Live>> = locked(&self.registry).live.values().cloned().collect();
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
        self.shutdown();
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

fn default_first_message(employee: &Employee) -> String {
    format!(
        "You are {}, the {}. Say that you are ready in one short sentence.",
        employee.name, employee.role
    )
}

/// The short M1 instructions (spec 4.6 has the full text, which comes with M2).
fn instructions(employee: &Employee) -> String {
    let mut text = format!(
        "You are {}, the {} on this floor. The operator can type in your terminal at any time.",
        employee.name, employee.role
    );
    let role_text = employee.role_text.trim();
    if !role_text.is_empty() {
        text.push_str("\n\n");
        text.push_str(role_text);
    }
    text
}

/// The employee record of the setup session. No file has it. Its role text is `FLOOR_INIT`.
fn setup_employee(cli: CliKind) -> Employee {
    Employee {
        name: "Setup".to_owned(),
        slug: SETUP_SLUG.to_owned(),
        role: "Setup agent".to_owned(),
        manager: None,
        cli,
        model: None,
        effort: None,
        permission: PermissionPolicy::EditInWorktree,
        role_text: FLOOR_INIT.to_owned(),
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
            role_text: "You lead this floor. You talk to the operator. You pass each request to the right employee. You do not write code yourself.\n".to_owned(),
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
            role_text: "You plan the work. You break each request into small tasks, and you follow the tasks until they are done.\n".to_owned(),
        },
    ]
}
