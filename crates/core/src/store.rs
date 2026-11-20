//! The store: the `.swarm/` folder of a floor and the app config.
//!
//! Layout (spec 4.9):
//!
//! ```text
//! {CWD}/.swarm/
//!     floor.toml          version, name, defaults, brand
//!     employees/<slug>.md front matter (TOML between `+++` lines) and the role text
//!     assets/ reports/ state/ state/logs/
//!     state/floor.db      SQLite: `sessions` and `status_events`
//!     .gitignore          state/, worktrees/, reports/
//! ```
//!
//! Front matter is TOML and not YAML: the crate already reads TOML for `floor.toml`, so a
//! YAML parser is not needed (`serde_yaml` is also archived).
//!
//! Every write of a TOML or Markdown file is atomic: a temp file in the same folder, then a rename.
//! The caller gives all times (Unix milliseconds). The store has no clock.

use crate::model::{
    Brand, CliKind, Employee, Floor, FloorDefaults, PermissionPolicy, SessionRecord,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Version of `floor.toml` that this build writes and reads.
pub const FLOOR_FILE_VERSION: u32 = 1;

/// Content of `.swarm/.gitignore`.
pub const GITIGNORE: &str = "state/\nworktrees/\nreports/\n";

const SWARM_DIR: &str = ".swarm";
const FLOOR_FILE: &str = "floor.toml";
const FRONT_MATTER_FENCE: &str = "+++";

/// Every error of the store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("I/O error on {}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("{} is not valid: {message}", path.display())]
    Invalid { path: PathBuf, message: String },
    #[error("a floor already exists at {}", .0.display())]
    FloorExists(PathBuf),
    #[error("there is no floor at {}: the folder {SWARM_DIR}/ does not exist", .0.display())]
    NoFloor(PathBuf),
    #[error(
        "{} has version {found}, and this app knows version {known} at most. Update the app. Nothing was changed.",
        path.display()
    )]
    FloorVersionNewer {
        path: PathBuf,
        found: u32,
        known: u32,
    },
    #[error(
        "the database {} has schema version {found}, and this app knows version {known} at most. Update the app. Nothing was changed.",
        path.display()
    )]
    DbVersionNewer {
        path: PathBuf,
        found: i64,
        known: i64,
    },
    #[error("\"{0}\" is not a valid slug (use a-z, 0-9 and '-', start with a letter or digit)")]
    InvalidSlug(String),
    #[error("there is no employee with the slug \"{0}\"")]
    EmployeeNotFound(String),
    #[error("there is no session with the ID \"{0}\"")]
    UnknownSession(String),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> StoreError + '_ {
    move |source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn invalid(path: &Path, message: impl ToString) -> StoreError {
    StoreError::Invalid {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

// ---------------------------------------------------------------------------------------------
// Atomic file write
// ---------------------------------------------------------------------------------------------

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write `bytes` to `path`: a temp file in the same folder, then a rename.
/// A reader sees the old file or the new file, never a part of the new file.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = parent.join(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if let Err(source) = result {
        let _ = fs::remove_file(&temp);
        return Err(StoreError::Io {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

fn read_text(path: &Path) -> Result<String, StoreError> {
    fs::read_to_string(path).map_err(io_err(path))
}

// ---------------------------------------------------------------------------------------------
// floor.toml
// ---------------------------------------------------------------------------------------------

/// Only the version. The version check runs on this, before the rest of the file is read,
/// because a newer file can have fields that this build cannot parse.
#[derive(Deserialize)]
struct VersionOnly {
    version: u32,
}

#[derive(Serialize, Deserialize)]
struct FloorFile {
    version: u32,
    name: String,
    #[serde(default)]
    defaults: FloorDefaults,
    #[serde(default, skip_serializing_if = "brand_is_empty")]
    brand: Brand,
}

fn brand_is_empty(brand: &Brand) -> bool {
    *brand == Brand::default()
}

fn read_floor_file(path: &Path) -> Result<Floor, StoreError> {
    let text = read_text(path)?;
    let version: VersionOnly = toml::from_str(&text).map_err(|e| invalid(path, e))?;
    if version.version > FLOOR_FILE_VERSION {
        return Err(StoreError::FloorVersionNewer {
            path: path.to_path_buf(),
            found: version.version,
            known: FLOOR_FILE_VERSION,
        });
    }
    let file: FloorFile = toml::from_str(&text).map_err(|e| invalid(path, e))?;
    Ok(Floor {
        name: file.name,
        defaults: file.defaults,
        brand: file.brand,
    })
}

fn write_floor_file(path: &Path, floor: &Floor) -> Result<(), StoreError> {
    let file = FloorFile {
        version: FLOOR_FILE_VERSION,
        name: floor.name.clone(),
        defaults: floor.defaults.clone(),
        brand: floor.brand.clone(),
    };
    let text = toml::to_string(&file).map_err(|e| invalid(path, e))?;
    write_atomic(path, text.as_bytes())
}

// ---------------------------------------------------------------------------------------------
// employees/<slug>.md
// ---------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct FrontMatter {
    name: String,
    role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    manager: Option<String>,
    cli: CliKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    effort: Option<String>,
    permission: PermissionPolicy,
}

/// A slug is a file name: lowercase letters, digits and `-`, and it starts with a letter or digit.
pub fn is_valid_slug(slug: &str) -> bool {
    let mut chars = slug.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
        _ => return false,
    }
    slug.len() <= 64 && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Split a file into the front matter text and the body. `None` means the fences are wrong.
fn split_front_matter(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let first_end = text.find('\n')?;
    if text[..first_end].trim_end_matches('\r') != FRONT_MATTER_FENCE {
        return None;
    }
    let after = &text[first_end + 1..];
    let mut position = 0;
    for line in after.split_inclusive('\n') {
        if line.trim_end_matches(['\n', '\r']) == FRONT_MATTER_FENCE {
            return Some((&after[..position], &after[position + line.len()..]));
        }
        position += line.len();
    }
    None
}

fn parse_employee(path: &Path, slug: &str, text: &str) -> Result<Employee, StoreError> {
    let (front, body) = split_front_matter(text).ok_or_else(|| {
        invalid(
            path,
            "front matter missing: the file must start with a +++ line and have a second +++ line",
        )
    })?;
    let front: FrontMatter = toml::from_str(front).map_err(|e| invalid(path, e))?;
    Ok(Employee {
        name: front.name,
        slug: slug.to_owned(),
        role: front.role,
        manager: front.manager,
        cli: front.cli,
        model: front.model,
        effort: front.effort,
        permission: front.permission,
        role_text: body.to_owned(),
    })
}

fn render_employee(path: &Path, employee: &Employee) -> Result<String, StoreError> {
    let front = FrontMatter {
        name: employee.name.clone(),
        role: employee.role.clone(),
        manager: employee.manager.clone(),
        cli: employee.cli,
        model: employee.model.clone(),
        effort: employee.effort.clone(),
        permission: employee.permission,
    };
    let front = toml::to_string(&front).map_err(|e| invalid(path, e))?;
    if front.lines().any(|line| line == FRONT_MATTER_FENCE) {
        return Err(invalid(path, "a field value has a line that is only +++"));
    }
    Ok(format!(
        "{FRONT_MATTER_FENCE}\n{front}{FRONT_MATTER_FENCE}\n{}",
        employee.role_text
    ))
}

/// The result of reading all employee files. A bad file goes in `errors`, and the others load.
#[derive(Debug, Default)]
pub struct LoadedEmployees {
    /// The employees that parsed, sorted by slug.
    pub employees: Vec<Employee>,
    /// One error for each file that did not parse. Each error names its file.
    pub errors: Vec<StoreError>,
}

// ---------------------------------------------------------------------------------------------
// state/floor.db
// ---------------------------------------------------------------------------------------------

/// `MIGRATIONS[n]` moves a database from `user_version` n to n + 1.
/// Milestone M2 adds its step (tasks, messages) at the end. The schema version is the length.
const MIGRATIONS: &[&str] = &[
    // 0 -> 1
    "CREATE TABLE sessions (
        app_session_id TEXT PRIMARY KEY,
        employee_slug  TEXT NOT NULL,
        cli_session_id TEXT,
        started_at     INTEGER NOT NULL,
        ended_at       INTEGER,
        last_status    TEXT NOT NULL
    );
    CREATE INDEX sessions_by_employee ON sessions (employee_slug, started_at);
    CREATE TABLE status_events (
        id             INTEGER PRIMARY KEY AUTOINCREMENT,
        app_session_id TEXT NOT NULL REFERENCES sessions (app_session_id),
        status         TEXT NOT NULL,
        at             INTEGER NOT NULL
    );
    CREATE INDEX status_events_by_session ON status_events (app_session_id, id);",
];

/// Schema version that this build writes and reads.
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

/// A connection to `state/floor.db`. `Connection` is not `Sync`: wrap this in a `Mutex` to share it,
/// or open one `Database` for each thread. SQLite handles several connections to one file.
#[derive(Debug)]
pub struct Database {
    conn: Connection,
}

const SESSION_COLUMNS: &str =
    "app_session_id, employee_slug, cli_session_id, started_at, ended_at, last_status";

fn session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRecord> {
    Ok(SessionRecord {
        app_session_id: row.get(0)?,
        employee_slug: row.get(1)?,
        cli_session_id: row.get(2)?,
        started_at: row.get(3)?,
        ended_at: row.get(4)?,
        last_status: row.get(5)?,
    })
}

impl Database {
    /// Open the file, and create or migrate it. A file with a higher `user_version` is refused
    /// before anything is written.
    pub fn open(path: &Path) -> Result<Database, StoreError> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        let found: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if found > SCHEMA_VERSION {
            return Err(StoreError::DbVersionNewer {
                path: path.to_path_buf(),
                found,
                known: SCHEMA_VERSION,
            });
        }
        // WAL: a reader does not block the writer. The result row of the pragma is read and dropped.
        conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        })?;
        conn.execute_batch("PRAGMA foreign_keys = ON")?;
        let mut database = Database { conn };
        database.migrate()?;
        Ok(database)
    }

    fn migrate(&mut self) -> Result<(), StoreError> {
        let mut version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        while version < SCHEMA_VERSION {
            let step = MIGRATIONS[version as usize];
            let tx = self.conn.transaction()?;
            tx.execute_batch(step)?;
            tx.execute_batch(&format!("PRAGMA user_version = {}", version + 1))?;
            tx.commit()?;
            version += 1;
        }
        Ok(())
    }

    /// The schema version of the open file.
    pub fn schema_version(&self) -> Result<i64, StoreError> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }

    /// Insert a session row.
    pub fn create_session(&self, record: &SessionRecord) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO sessions (app_session_id, employee_slug, cli_session_id, started_at, ended_at, last_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.app_session_id,
                record.employee_slug,
                record.cli_session_id,
                record.started_at,
                record.ended_at,
                record.last_status
            ],
        )?;
        Ok(())
    }

    /// Store the CLI session ID. The write is committed when this returns, so a second connection
    /// sees it at once. The app calls this when it gets the ID, not at the end of a turn.
    pub fn set_cli_session_id(
        &self,
        app_session_id: &str,
        cli_session_id: &str,
    ) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE sessions SET cli_session_id = ?2 WHERE app_session_id = ?1",
            params![app_session_id, cli_session_id],
        )?;
        require_one(changed, app_session_id)
    }

    /// Set the end time of a session.
    pub fn set_ended_at(&self, app_session_id: &str, ended_at: i64) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE sessions SET ended_at = ?2 WHERE app_session_id = ?1",
            params![app_session_id, ended_at],
        )?;
        require_one(changed, app_session_id)
    }

    /// Append a status event, and set `last_status` of the session. One transaction does both.
    pub fn append_status_event(
        &self,
        app_session_id: &str,
        status: &str,
        at: i64,
    ) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE sessions SET last_status = ?2 WHERE app_session_id = ?1",
            params![app_session_id, status],
        )?;
        require_one(changed, app_session_id)?;
        tx.execute(
            "INSERT INTO status_events (app_session_id, status, at) VALUES (?1, ?2, ?3)",
            params![app_session_id, status, at],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The status events of a session, oldest first, as `(status, time)`.
    pub fn status_events(&self, app_session_id: &str) -> Result<Vec<(String, i64)>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT status, at FROM status_events WHERE app_session_id = ?1 ORDER BY id",
        )?;
        let rows = statement.query_map(params![app_session_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The sessions of an employee, oldest first.
    pub fn list_sessions(&self, employee_slug: &str) -> Result<Vec<SessionRecord>, StoreError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {SESSION_COLUMNS} FROM sessions WHERE employee_slug = ?1
             ORDER BY started_at, rowid"
        ))?;
        let rows = statement.query_map(params![employee_slug], session_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The newest session of an employee, or `None` if the employee never had one.
    pub fn last_session(&self, employee_slug: &str) -> Result<Option<SessionRecord>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {SESSION_COLUMNS} FROM sessions WHERE employee_slug = ?1
                     ORDER BY started_at DESC, rowid DESC LIMIT 1"
                ),
                params![employee_slug],
                session_from_row,
            )
            .optional()?)
    }
}

fn require_one(changed: usize, app_session_id: &str) -> Result<(), StoreError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(StoreError::UnknownSession(app_session_id.to_owned()))
    }
}

// ---------------------------------------------------------------------------------------------
// The floor handle
// ---------------------------------------------------------------------------------------------

/// One open floor: the `.swarm/` folder of one CWD, and its database.
/// (The data of `floor.toml` is `model::Floor`; this is the handle to the files.)
#[derive(Debug)]
pub struct FloorStore {
    cwd: PathBuf,
    floor: Floor,
    db: Database,
}

impl FloorStore {
    /// Create a new `.swarm/` in `cwd`. Fails if `cwd/.swarm` exists, whatever it is.
    pub fn create(cwd: &Path, name: &str) -> Result<FloorStore, StoreError> {
        let swarm = cwd.join(SWARM_DIR);
        match fs::create_dir(&swarm) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                return Err(StoreError::FloorExists(swarm));
            }
            Err(source) => {
                return Err(StoreError::Io {
                    path: swarm,
                    source,
                });
            }
        }
        let floor = Floor {
            name: name.to_owned(),
            defaults: FloorDefaults::default(),
            brand: Brand::default(),
        };
        write_floor_file(&swarm.join(FLOOR_FILE), &floor)?;
        write_atomic(&swarm.join(".gitignore"), GITIGNORE.as_bytes())?;
        ensure_folders(&swarm)?;
        let db = Database::open(&swarm.join("state").join("floor.db"))?;
        Ok(FloorStore {
            cwd: cwd.to_path_buf(),
            floor,
            db,
        })
    }

    /// Import the existing `.swarm/` of `cwd`. A `floor.toml` with a newer version is refused
    /// before anything is written. `state/` is not in git, so a fresh clone has none: this makes it.
    pub fn open(cwd: &Path) -> Result<FloorStore, StoreError> {
        let swarm = cwd.join(SWARM_DIR);
        if !swarm.is_dir() {
            return Err(StoreError::NoFloor(cwd.to_path_buf()));
        }
        let floor = read_floor_file(&swarm.join(FLOOR_FILE))?;
        ensure_folders(&swarm)?;
        let db = Database::open(&swarm.join("state").join("floor.db"))?;
        Ok(FloorStore {
            cwd: cwd.to_path_buf(),
            floor,
            db,
        })
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn swarm_dir(&self) -> PathBuf {
        self.cwd.join(SWARM_DIR)
    }

    pub fn floor(&self) -> &Floor {
        &self.floor
    }

    pub fn db(&self) -> &Database {
        &self.db
    }

    /// Write the `[brand]` table of `floor.toml`. The rest of the file stays: the name, the
    /// defaults and any key that this build does not know. (Comments do not stay: the TOML
    /// reader drops them.) A field that is `None` is removed from the file. An empty brand
    /// removes the table.
    pub fn set_brand(&mut self, brand: Brand) -> Result<(), StoreError> {
        let path = self.swarm_dir().join(FLOOR_FILE);
        let text = read_text(&path)?;
        let mut table: toml::Table = toml::from_str(&text).map_err(|e| invalid(&path, e))?;
        if brand_is_empty(&brand) {
            table.remove("brand");
        } else {
            let value = toml::Value::try_from(&brand).map_err(|e| invalid(&path, e))?;
            table.insert("brand".to_owned(), value);
        }
        let text = toml::to_string(&table).map_err(|e| invalid(&path, e))?;
        write_atomic(&path, text.as_bytes())?;
        self.floor.brand = brand;
        Ok(())
    }

    /// Write the `[defaults]` table of `floor.toml`. The rest of the file stays (see `set_brand`).
    /// A field that is `None` is removed from the file. Empty defaults remove the table.
    pub fn set_defaults(&mut self, defaults: FloorDefaults) -> Result<(), StoreError> {
        let path = self.swarm_dir().join(FLOOR_FILE);
        let text = read_text(&path)?;
        let mut table: toml::Table = toml::from_str(&text).map_err(|e| invalid(&path, e))?;
        if defaults == FloorDefaults::default() {
            table.remove("defaults");
        } else {
            let value = toml::Value::try_from(&defaults).map_err(|e| invalid(&path, e))?;
            table.insert("defaults".to_owned(), value);
        }
        let text = toml::to_string(&table).map_err(|e| invalid(&path, e))?;
        write_atomic(&path, text.as_bytes())?;
        self.floor.defaults = defaults;
        Ok(())
    }

    fn employees_dir(&self) -> PathBuf {
        self.swarm_dir().join("employees")
    }

    fn employee_path(&self, slug: &str) -> Result<PathBuf, StoreError> {
        if !is_valid_slug(slug) {
            return Err(StoreError::InvalidSlug(slug.to_owned()));
        }
        Ok(self.employees_dir().join(format!("{slug}.md")))
    }

    /// Read all employee files. A file that does not parse goes in `errors` with its path,
    /// and the other employees still load.
    pub fn read_employees(&self) -> Result<LoadedEmployees, StoreError> {
        let dir = self.employees_dir();
        let mut paths = Vec::new();
        for entry in fs::read_dir(&dir).map_err(io_err(&dir))? {
            let entry = entry.map_err(io_err(&dir))?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "md") && path.is_file() {
                paths.push(path);
            }
        }
        paths.sort();
        let mut loaded = LoadedEmployees::default();
        for path in paths {
            let slug = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            let result = if is_valid_slug(&slug) {
                read_text(&path).and_then(|text| parse_employee(&path, &slug, &text))
            } else {
                Err(invalid(&path, StoreError::InvalidSlug(slug.clone())))
            };
            match result {
                Ok(employee) => loaded.employees.push(employee),
                Err(error) => loaded.errors.push(error),
            }
        }
        Ok(loaded)
    }

    /// Write one employee file (create or replace), atomically.
    pub fn write_employee(&self, employee: &Employee) -> Result<(), StoreError> {
        let path = self.employee_path(&employee.slug)?;
        let text = render_employee(&path, employee)?;
        write_atomic(&path, text.as_bytes())
    }

    /// Remove one employee file. The sessions of the employee stay in the database.
    pub fn remove_employee(&self, slug: &str) -> Result<(), StoreError> {
        let path = self.employee_path(slug)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                Err(StoreError::EmployeeNotFound(slug.to_owned()))
            }
            Err(source) => Err(StoreError::Io { path, source }),
        }
    }
}

fn ensure_folders(swarm: &Path) -> Result<(), StoreError> {
    for folder in ["employees", "assets", "reports", "state", "state/logs"] {
        let path = swarm.join(folder);
        fs::create_dir_all(&path).map_err(io_err(&path))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// App config: the list of floor paths
// ---------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Default)]
struct FloorsFile {
    #[serde(default)]
    floors: Vec<PathBuf>,
}

/// One entry of the floor list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloorEntry {
    pub path: PathBuf,
    /// The folder does not exist now. The entry stays in the list.
    pub missing: bool,
}

/// The list of floor paths, in `floors.toml` inside a config folder
/// (the caller passes `$XDG_CONFIG_HOME/agent-tower/`).
#[derive(Debug, Clone)]
pub struct FloorList {
    file: PathBuf,
}

impl FloorList {
    pub fn new(config_dir: &Path) -> FloorList {
        FloorList {
            file: config_dir.join("floors.toml"),
        }
    }

    fn load(&self) -> Result<FloorsFile, StoreError> {
        match fs::read_to_string(&self.file) {
            Ok(text) => toml::from_str(&text).map_err(|e| invalid(&self.file, e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(FloorsFile::default()),
            Err(source) => Err(StoreError::Io {
                path: self.file.clone(),
                source,
            }),
        }
    }

    fn save(&self, file: &FloorsFile) -> Result<(), StoreError> {
        if let Some(dir) = self.file.parent() {
            fs::create_dir_all(dir).map_err(io_err(dir))?;
        }
        let text = toml::to_string(file).map_err(|e| invalid(&self.file, e))?;
        write_atomic(&self.file, text.as_bytes())
    }

    /// The real path if the folder exists, else the path as given.
    fn normalize(path: &Path) -> PathBuf {
        fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    /// Add a floor path. Returns `false` if it is in the list already.
    pub fn add(&self, path: &Path) -> Result<bool, StoreError> {
        let path = Self::normalize(path);
        let mut file = self.load()?;
        if file.floors.contains(&path) {
            return Ok(false);
        }
        file.floors.push(path);
        self.save(&file)?;
        Ok(true)
    }

    /// Remove a floor path from the list. Returns `false` if it was not in the list.
    /// The folder on disk is not touched.
    pub fn remove(&self, path: &Path) -> Result<bool, StoreError> {
        let normal = Self::normalize(path);
        let mut file = self.load()?;
        let before = file.floors.len();
        file.floors.retain(|p| *p != normal && p != path);
        if file.floors.len() == before {
            return Ok(false);
        }
        self.save(&file)?;
        Ok(true)
    }

    /// The floors in the order they were added. A path whose folder does not exist is
    /// marked `missing` and is not removed.
    pub fn list(&self) -> Result<Vec<FloorEntry>, StoreError> {
        Ok(self
            .load()?
            .floors
            .into_iter()
            .map(|path| FloorEntry {
                missing: !path.is_dir(),
                path,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    fn employee(slug: &str, manager: Option<&str>) -> Employee {
        Employee {
            name: format!("Name of {slug}"),
            slug: slug.to_owned(),
            role: "Engineer".to_owned(),
            manager: manager.map(str::to_owned),
            cli: CliKind::Codex,
            model: Some("gpt-test".to_owned()),
            effort: Some("high".to_owned()),
            permission: PermissionPolicy::EditInWorktree,
            role_text: "You write code.\n\n+++\nA line after a fence, \"quoted\", with ünïcode.\n"
                .to_owned(),
        }
    }

    fn session(id: &str, slug: &str, started_at: i64) -> SessionRecord {
        SessionRecord {
            app_session_id: id.to_owned(),
            employee_slug: slug.to_owned(),
            cli_session_id: None,
            started_at,
            ended_at: None,
            last_status: "starting".to_owned(),
        }
    }

    /// All files and folders under `root`: relative path to content (`None` for a folder).
    fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let relative = path.strip_prefix(root).unwrap().to_path_buf();
                if path.is_dir() {
                    out.insert(relative, None);
                    walk(root, &path, out);
                } else {
                    out.insert(relative, Some(fs::read(&path).unwrap()));
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(root, root, &mut out);
        out
    }

    fn db_path(cwd: &Path) -> PathBuf {
        cwd.join(".swarm/state/floor.db")
    }

    #[test]
    fn floor_and_employees_round_trip() {
        let dir = TempDir::new().unwrap();
        let store = FloorStore::create(dir.path(), "Acme \"Co\"").unwrap();
        let mut a = employee("ceo", None);
        a.model = None;
        a.effort = None;
        a.cli = CliKind::Claude;
        a.permission = PermissionPolicy::AskAlways;
        a.role_text = String::new();
        let b = employee("eng-1", Some("ceo"));
        let mut c = employee("reader", Some("ceo"));
        c.permission = PermissionPolicy::ReadOnly;
        c.role_text = "\n\nStarts with blank lines, no final newline".to_owned();
        for e in [&a, &b, &c] {
            store.write_employee(e).unwrap();
        }

        let reopened = FloorStore::open(dir.path()).unwrap();
        assert_eq!(reopened.floor(), store.floor());
        assert_eq!(reopened.floor().name, "Acme \"Co\"");
        let loaded = reopened.read_employees().unwrap();
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        assert_eq!(loaded.employees, vec![a, b, c]);
    }

    #[test]
    fn floor_defaults_round_trip() {
        let dir = TempDir::new().unwrap();
        FloorStore::create(dir.path(), "F").unwrap();
        let floor = Floor {
            name: "F".to_owned(),
            defaults: FloorDefaults {
                cli: Some(CliKind::Claude),
                model: Some("haiku".to_owned()),
                effort: None,
                permission: Some(PermissionPolicy::EditInWorktree),
            },
            brand: Brand::default(),
        };
        let path = dir.path().join(".swarm/floor.toml");
        write_floor_file(&path, &floor).unwrap();
        assert_eq!(read_floor_file(&path).unwrap(), floor);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("version = 1\n"), "{text}");
    }

    #[test]
    fn a_floor_file_with_no_brand_loads_and_a_brand_round_trips_with_the_rest_kept() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".swarm/floor.toml");
        FloorStore::create(dir.path(), "Acme").unwrap();
        // An old file: no `[brand]`, a default, and a key that this build does not know.
        fs::write(
            &path,
            "version = 1\nname = \"Acme\"\nfuture_key = 7\n[defaults]\nmodel = \"haiku\"\n",
        )
        .unwrap();
        let mut store = FloorStore::open(dir.path()).unwrap();
        assert_eq!(store.floor().brand, Brand::default());
        assert_eq!(store.floor().defaults.model.as_deref(), Some("haiku"));

        // Save a brand with a description only: the other keys stay, the empty field is absent.
        let brand = Brand {
            description: Some("A \"shop\" for candles.".to_owned()),
            logo_prompt: None,
        };
        store.set_brand(brand.clone()).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("future_key = 7"), "{text}");
        assert!(
            text.contains("[brand]") && !text.contains("logo_prompt"),
            "{text}"
        );
        let reopened = FloorStore::open(dir.path()).unwrap();
        assert_eq!(reopened.floor().brand, brand);
        assert_eq!(reopened.floor().name, "Acme");
        assert_eq!(reopened.floor().defaults.model.as_deref(), Some("haiku"));

        // Both fields. Then an empty brand removes the table and keeps the rest.
        let full = Brand {
            description: Some("d".to_owned()),
            logo_prompt: Some("A bee.".to_owned()),
        };
        store.set_brand(full.clone()).unwrap();
        assert_eq!(FloorStore::open(dir.path()).unwrap().floor().brand, full);
        store.set_brand(Brand::default()).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("[brand]") && text.contains("future_key = 7"),
            "{text}"
        );
        assert_eq!(
            FloorStore::open(dir.path()).unwrap().floor().brand,
            Brand::default()
        );
    }

    #[test]
    fn write_floor_file_keeps_the_brand_and_skips_an_empty_one() {
        let dir = TempDir::new().unwrap();
        FloorStore::create(dir.path(), "F").unwrap();
        let path = dir.path().join(".swarm/floor.toml");
        let mut floor = read_floor_file(&path).unwrap();
        assert!(!fs::read_to_string(&path).unwrap().contains("brand"));
        floor.brand.logo_prompt = Some("A bee.".to_owned());
        write_floor_file(&path, &floor).unwrap();
        assert_eq!(read_floor_file(&path).unwrap(), floor);
    }

    #[test]
    fn newer_floor_version_is_refused_and_nothing_changes() {
        let dir = TempDir::new().unwrap();
        FloorStore::create(dir.path(), "F").unwrap();
        let swarm = dir.path().join(".swarm");
        // A newer file can have fields that this build does not know.
        fs::write(
            swarm.join("floor.toml"),
            "version = 2\nname = 5\n[future]\nx = 1\n",
        )
        .unwrap();
        // A fresh clone has no state/. A refused open must not create it.
        fs::remove_dir_all(swarm.join("state")).unwrap();
        let before = snapshot(dir.path());

        let error = FloorStore::open(dir.path()).unwrap_err();
        match &error {
            StoreError::FloorVersionNewer { found, known, .. } => {
                assert_eq!((*found, *known), (2, 1));
            }
            other => panic!("wrong error: {other}"),
        }
        assert!(error.to_string().contains("floor.toml"));
        assert_eq!(snapshot(dir.path()), before);
    }

    #[test]
    fn one_broken_employee_does_not_hide_the_others() {
        let dir = TempDir::new().unwrap();
        let store = FloorStore::create(dir.path(), "F").unwrap();
        store.write_employee(&employee("a-first", None)).unwrap();
        store.write_employee(&employee("z-last", None)).unwrap();
        let employees = dir.path().join(".swarm/employees");
        fs::write(employees.join("broken.md"), "no front matter at all\n").unwrap();
        fs::write(
            employees.join("badfield.md"),
            "+++\nname = \"X\"\nrole = \"R\"\ncli = \"vim\"\npermission = \"ask-always\"\n+++\nbody\n",
        )
        .unwrap();
        fs::write(employees.join("notes.txt"), "ignored").unwrap();

        let loaded = store.read_employees().unwrap();
        let slugs: Vec<_> = loaded.employees.iter().map(|e| e.slug.as_str()).collect();
        assert_eq!(slugs, ["a-first", "z-last"]);
        assert_eq!(loaded.errors.len(), 2);
        let messages: Vec<String> = loaded.errors.iter().map(ToString::to_string).collect();
        assert!(
            messages.iter().any(|m| m.contains("broken.md")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("badfield.md")),
            "{messages:?}"
        );
    }

    #[test]
    fn create_on_existing_swarm_fails_and_keeps_it() {
        let dir = TempDir::new().unwrap();
        let store = FloorStore::create(dir.path(), "First").unwrap();
        store.write_employee(&employee("ceo", None)).unwrap();
        drop(store);
        let before = snapshot(dir.path());
        let error = FloorStore::create(dir.path(), "Second").unwrap_err();
        assert!(matches!(error, StoreError::FloorExists(_)), "{error}");
        assert_eq!(snapshot(dir.path()), before);
        assert_eq!(FloorStore::open(dir.path()).unwrap().floor().name, "First");
    }

    #[test]
    fn open_without_swarm_fails_and_creates_nothing() {
        let dir = TempDir::new().unwrap();
        let error = FloorStore::open(dir.path()).unwrap_err();
        assert!(matches!(error, StoreError::NoFloor(_)), "{error}");
        assert!(error.to_string().contains(".swarm"));
        assert!(!dir.path().join(".swarm").exists());
    }

    #[test]
    fn create_makes_gitignore_and_folders() {
        let dir = TempDir::new().unwrap();
        FloorStore::create(dir.path(), "F").unwrap();
        let swarm = dir.path().join(".swarm");
        assert_eq!(
            fs::read_to_string(swarm.join(".gitignore")).unwrap(),
            "state/\nworktrees/\nreports/\n"
        );
        for folder in ["employees", "assets", "reports", "state", "state/logs"] {
            assert!(swarm.join(folder).is_dir(), "{folder}");
        }
        assert!(swarm.join("floor.toml").is_file());
        assert!(swarm.join("state/floor.db").is_file());
    }

    #[test]
    fn writes_leave_no_temp_file_and_slug_is_checked() {
        let dir = TempDir::new().unwrap();
        let store = FloorStore::create(dir.path(), "F").unwrap();
        store.write_employee(&employee("ceo", None)).unwrap();
        store.write_employee(&employee("ceo", None)).unwrap(); // replace
        let names: Vec<String> = fs::read_dir(dir.path().join(".swarm/employees"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["ceo.md"]);
        for bad in ["../evil", "Up", "", "a/b", ".hidden", "sp ace"] {
            let error = store.write_employee(&employee(bad, None)).unwrap_err();
            assert!(
                matches!(error, StoreError::InvalidSlug(_)),
                "{bad}: {error}"
            );
        }
        assert!(!dir.path().join(".swarm/evil.md").exists());
    }

    #[test]
    fn remove_employee_removes_one_file() {
        let dir = TempDir::new().unwrap();
        let store = FloorStore::create(dir.path(), "F").unwrap();
        store.write_employee(&employee("a", None)).unwrap();
        store.write_employee(&employee("b", None)).unwrap();
        store.remove_employee("a").unwrap();
        let loaded = store.read_employees().unwrap();
        assert_eq!(loaded.employees.len(), 1);
        assert_eq!(loaded.employees[0].slug, "b");
        let error = store.remove_employee("a").unwrap_err();
        assert!(matches!(error, StoreError::EmployeeNotFound(_)), "{error}");
    }

    #[test]
    fn cli_session_id_is_visible_to_a_second_connection_at_once() {
        let dir = TempDir::new().unwrap();
        let store = FloorStore::create(dir.path(), "F").unwrap();
        store
            .db()
            .create_session(&session("s1", "ceo", 100))
            .unwrap();
        let reader = Database::open(&db_path(dir.path())).unwrap();
        assert_eq!(
            reader.last_session("ceo").unwrap().unwrap().cli_session_id,
            None
        );

        store.db().set_cli_session_id("s1", "cli-abc").unwrap();
        // No turn ended, no event, no close: the second connection must still see the ID.
        let seen = reader.last_session("ceo").unwrap().unwrap();
        assert_eq!(seen.cli_session_id.as_deref(), Some("cli-abc"));
    }

    #[test]
    fn session_functions() {
        let dir = TempDir::new().unwrap();
        let store = FloorStore::create(dir.path(), "F").unwrap();
        let db = store.db();
        assert_eq!(db.last_session("ceo").unwrap(), None);
        db.create_session(&session("s1", "ceo", 100)).unwrap();
        db.create_session(&session("s2", "ceo", 200)).unwrap();
        db.create_session(&session("s3", "eng", 300)).unwrap();
        db.set_ended_at("s1", 150).unwrap();
        db.append_status_event("s2", "idle", 210).unwrap();
        db.append_status_event("s2", "working", 220).unwrap();

        let list = db.list_sessions("ceo").unwrap();
        let ids: Vec<_> = list.iter().map(|s| s.app_session_id.as_str()).collect();
        assert_eq!(ids, ["s1", "s2"]);
        assert_eq!(list[0].ended_at, Some(150));
        assert_eq!(list[1].ended_at, None);
        assert_eq!(list[1].last_status, "working");
        assert_eq!(
            db.last_session("ceo").unwrap().unwrap().app_session_id,
            "s2"
        );
        assert_eq!(
            db.last_session("eng").unwrap().unwrap().app_session_id,
            "s3"
        );
        assert_eq!(
            db.status_events("s2").unwrap(),
            vec![("idle".to_owned(), 210), ("working".to_owned(), 220)]
        );
        for error in [
            db.set_cli_session_id("nope", "x").unwrap_err(),
            db.set_ended_at("nope", 1).unwrap_err(),
            db.append_status_event("nope", "idle", 1).unwrap_err(),
        ] {
            assert!(matches!(error, StoreError::UnknownSession(_)), "{error}");
        }
        assert!(db.status_events("nope").unwrap().is_empty());
    }

    fn schema_dump(conn: &Connection) -> Vec<String> {
        let mut statement = conn
            .prepare("SELECT type || ':' || name || ':' || ifnull(sql, '') FROM sqlite_master ORDER BY 1")
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn database_version_1_reopens_with_no_change() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("floor.db");
        let first = Database::open(&path).unwrap();
        assert_eq!(first.schema_version().unwrap(), 1);
        first.create_session(&session("s1", "ceo", 1)).unwrap();
        first.set_cli_session_id("s1", "cli").unwrap();
        first.append_status_event("s1", "idle", 2).unwrap();
        let schema = schema_dump(&first.conn);
        let tables: Vec<_> = schema.iter().filter(|s| s.starts_with("table:")).collect();
        assert_eq!(
            tables.len(),
            3,
            "sessions, status_events, sqlite_sequence: {tables:?}"
        );
        drop(first);

        let second = Database::open(&path).unwrap();
        assert_eq!(second.schema_version().unwrap(), 1);
        assert_eq!(schema_dump(&second.conn), schema);
        let kept = second.last_session("ceo").unwrap().unwrap();
        assert_eq!(kept.cli_session_id.as_deref(), Some("cli"));
        assert_eq!(second.status_events("s1").unwrap().len(), 1);
    }

    #[test]
    fn database_with_higher_version_is_refused_and_unchanged() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("floor.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE future (x); PRAGMA user_version = 2;")
                .unwrap();
        }
        let before = fs::read(&path).unwrap();
        let error = Database::open(&path).unwrap_err();
        match error {
            StoreError::DbVersionNewer { found, known, .. } => assert_eq!((found, known), (2, 1)),
            other => panic!("wrong error: {other}"),
        }
        assert_eq!(fs::read(&path).unwrap(), before);
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1, "no -wal or -shm file: {names:?}");
    }

    #[test]
    fn floor_list_add_remove_and_missing_folder() {
        let config = TempDir::new().unwrap();
        let floors = TempDir::new().unwrap();
        let list = FloorList::new(&config.path().join("agent-tower"));
        assert!(list.list().unwrap().is_empty());

        let here = floors.path().join("here");
        let gone = floors.path().join("gone");
        fs::create_dir(&here).unwrap();
        fs::create_dir(&gone).unwrap();
        assert!(list.add(&here).unwrap());
        assert!(list.add(&gone).unwrap());
        assert!(!list.add(&here).unwrap(), "no duplicate");
        fs::remove_dir(&gone).unwrap();

        let entries = list.list().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(!entries[0].missing);
        assert!(entries[1].missing, "a missing folder is reported");
        assert!(
            entries[1].path.ends_with("gone"),
            "and it stays in the list"
        );

        // A second FloorList on the same folder reads the same file.
        assert_eq!(
            FloorList::new(&config.path().join("agent-tower"))
                .list()
                .unwrap(),
            entries
        );

        assert!(
            list.remove(&gone).unwrap(),
            "a missing path can be removed by name"
        );
        assert!(!list.remove(&gone).unwrap());
        let entries = list.list().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].path.ends_with("here"));
        assert!(here.is_dir(), "remove does not touch the folder");
    }

    #[test]
    fn floor_list_with_broken_file_names_the_file() {
        let config = TempDir::new().unwrap();
        fs::write(config.path().join("floors.toml"), "floors = 5").unwrap();
        let error = FloorList::new(config.path()).list().unwrap_err();
        assert!(error.to_string().contains("floors.toml"), "{error}");
    }
}
