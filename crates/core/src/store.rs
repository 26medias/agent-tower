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
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
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
    #[error("there is no floor at {}: the file {SWARM_DIR}/{FLOOR_FILE} does not exist", .0.display())]
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
    #[error("there is no task T-{0}")]
    UnknownTask(i64),
    #[error("{0}")]
    Rule(String),
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
/// Milestone M2 adds its steps (messages, tasks) at the end. The schema version is the length.
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
    // 1 -> 2: the messages between employees (spec 4.4 and 4.5).
    "CREATE TABLE messages (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        from_slug    TEXT NOT NULL,
        to_slug      TEXT NOT NULL,
        text         TEXT NOT NULL,
        created_at   INTEGER NOT NULL,
        delivered_at INTEGER
    );
    CREATE INDEX messages_by_target ON messages (to_slug, delivered_at, id);
    CREATE INDEX messages_by_pair ON messages (from_slug, to_slug, created_at);",
    // 2 -> 3: the task board (spec 4.4 and 4.9). A message can belong to a task. `kind` is
    // `message` (an employee wrote it), `task_assign` or `task_status` (the app wrote it).
    "ALTER TABLE messages ADD COLUMN task_id INTEGER;
    ALTER TABLE messages ADD COLUMN kind TEXT NOT NULL DEFAULT 'message';
    CREATE TABLE tasks (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        title       TEXT NOT NULL,
        description TEXT NOT NULL,
        parent_id   INTEGER REFERENCES tasks (id),
        creator     TEXT NOT NULL,
        assignee    TEXT,
        assigner    TEXT,
        status      TEXT NOT NULL DEFAULT 'todo',
        report      TEXT,
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL
    );
    CREATE INDEX tasks_by_status ON tasks (status, id);
    CREATE TABLE task_events (
        id      INTEGER PRIMARY KEY AUTOINCREMENT,
        task_id INTEGER NOT NULL REFERENCES tasks (id),
        at      INTEGER NOT NULL,
        author  TEXT NOT NULL,
        kind    TEXT NOT NULL,
        value   TEXT,
        text    TEXT
    );
    CREATE INDEX task_events_by_task ON task_events (task_id, id);",
    // 3 -> 4: worktrees and restore (M3). A task keeps its branch, the branch it started from, the
    // commit it started from, and the worktree folder (relative to the floor folder; NULL when
    // there is none now). A message can carry the orders that the app wrote for it. The table
    // `resume_marks` holds the employees that had a live session when the app closed.
    "ALTER TABLE tasks ADD COLUMN branch TEXT;
    ALTER TABLE tasks ADD COLUMN base_branch TEXT;
    ALTER TABLE tasks ADD COLUMN base_commit TEXT;
    ALTER TABLE tasks ADD COLUMN worktree TEXT;
    ALTER TABLE messages ADD COLUMN orders TEXT;
    CREATE TABLE resume_marks (
        slug      TEXT PRIMARY KEY,
        marked_at INTEGER NOT NULL
    );",
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
    /// Start a write transaction at once (`BEGIN IMMEDIATE`). Use it for each transaction that
    /// reads and then writes. In WAL mode, a deferred transaction that reads first and then
    /// writes gets SQLITE_BUSY at once when another connection committed in between, and
    /// `busy_timeout` does not help. `BEGIN IMMEDIATE` waits for the write lock instead.
    fn write_transaction(&self) -> Result<Transaction<'_>, StoreError> {
        Ok(Transaction::new_unchecked(
            &self.conn,
            TransactionBehavior::Immediate,
        )?)
    }

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
        let tx = self.write_transaction()?;
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

/// One row of the `messages` table. Times are Unix milliseconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRecord {
    pub id: i64,
    pub from_slug: String,
    pub to_slug: String,
    pub text: String,
    pub created_at: i64,
    pub delivered_at: Option<i64>,
    /// The task that the message is about (`task_assign` and `task_status` messages).
    pub task_id: Option<i64>,
    /// `message`, `task_assign` or `task_status` (the constants `KIND_*`).
    pub kind: String,
    /// The orders that the app wrote for this task message (worktree rules, merge steps). `None`:
    /// the default orders of `bus::format_task_message`.
    pub orders: Option<String>,
}

/// An ordinary message from one employee to another.
pub const KIND_MESSAGE: &str = "message";
/// The message that the app sends to the assignee of a task.
pub const KIND_TASK_ASSIGN: &str = "task_assign";
/// The message that the app sends to the assigner of a task when its status changes.
pub const KIND_TASK_STATUS: &str = "task_status";

/// The most task messages (`task_assign` and `task_status`) that one employee can store for one
/// other employee in `TASK_MESSAGE_WINDOW_MS`. These messages do not count for the loop limit of
/// `send`, so two agents that assign and answer each other need a limit of their own. The messages
/// of the operator do not count.
pub const TASK_MESSAGE_LIMIT: i64 = 60;

/// The window of `TASK_MESSAGE_LIMIT`: a rolling hour, in milliseconds.
pub const TASK_MESSAGE_WINDOW_MS: i64 = 60 * 60 * 1000;

/// The name that the app uses for the operator in the `from_slug` of a message.
const OPERATOR_SLUG: &str = "operator";

fn message_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRecord> {
    Ok(MessageRecord {
        id: row.get(0)?,
        from_slug: row.get(1)?,
        to_slug: row.get(2)?,
        text: row.get(3)?,
        created_at: row.get(4)?,
        delivered_at: row.get(5)?,
        task_id: row.get(6)?,
        kind: row.get(7)?,
        orders: row.get(8)?,
    })
}

const MESSAGE_COLUMNS: &str =
    "id, from_slug, to_slug, text, created_at, delivered_at, task_id, kind, orders";

impl Database {
    /// Store a message. `delivered_at` is `Some` for a message that goes out at once (the first
    /// message of a session that the app starts). Returns the ID.
    pub fn insert_message(
        &self,
        from_slug: &str,
        to_slug: &str,
        text: &str,
        created_at: i64,
        delivered_at: Option<i64>,
    ) -> Result<i64, StoreError> {
        self.conn.execute(
            "INSERT INTO messages (from_slug, to_slug, text, created_at, delivered_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![from_slug, to_slug, text, created_at, delivered_at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// The messages for `to_slug` that are not delivered, oldest first.
    pub fn pending_messages(&self, to_slug: &str) -> Result<Vec<MessageRecord>, StoreError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages
             WHERE to_slug = ?1 AND delivered_at IS NULL ORDER BY id"
        ))?;
        let rows = statement.query_map(params![to_slug], message_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The messages with these IDs, oldest first. An ID that does not exist gives no row.
    pub fn messages_by_ids(&self, ids: &[i64]) -> Result<Vec<MessageRecord>, StoreError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages WHERE id = ?1"
        ))?;
        let mut out = Vec::new();
        for id in ids {
            let mut rows = statement.query_map(params![id], message_from_row)?;
            if let Some(row) = rows.next() {
                out.push(row?);
            }
        }
        out.sort_by_key(|m| m.id);
        Ok(out)
    }

    /// How many messages for `to_slug` are not delivered.
    pub fn count_pending_messages(&self, to_slug: &str) -> Result<i64, StoreError> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE to_slug = ?1 AND delivered_at IS NULL",
            params![to_slug],
            |row| row.get(0),
        )?)
    }

    /// Mark messages as delivered. A message that is delivered already keeps its first time.
    pub fn mark_messages_delivered(&self, ids: &[i64], at: i64) -> Result<(), StoreError> {
        let tx = self.write_transaction()?;
        for id in ids {
            tx.execute(
                "UPDATE messages SET delivered_at = ?2 WHERE id = ?1 AND delivered_at IS NULL",
                params![id, at],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Put messages back in the waiting state (a start that failed after it took them).
    pub fn mark_messages_waiting(&self, ids: &[i64]) -> Result<(), StoreError> {
        let tx = self.write_transaction()?;
        for id in ids {
            tx.execute(
                "UPDATE messages SET delivered_at = NULL WHERE id = ?1",
                params![id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Take the waiting messages for `to_slug` and mark them delivered, in one transaction.
    pub fn take_pending_messages(
        &self,
        to_slug: &str,
        at: i64,
    ) -> Result<Vec<MessageRecord>, StoreError> {
        let tx = self.write_transaction()?;
        let messages = {
            let mut statement = tx.prepare(&format!(
                "SELECT {MESSAGE_COLUMNS} FROM messages
                 WHERE to_slug = ?1 AND delivered_at IS NULL ORDER BY id"
            ))?;
            let rows = statement.query_map(params![to_slug], message_from_row)?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        tx.execute(
            "UPDATE messages SET delivered_at = ?2 WHERE to_slug = ?1 AND delivered_at IS NULL",
            params![to_slug, at],
        )?;
        tx.commit()?;
        Ok(messages)
    }

    /// How many messages `from_slug` sent to `to_slug` at `since` or later (the loop limit). The
    /// messages that the app sends for a task do not count.
    pub fn count_messages_since(
        &self,
        from_slug: &str,
        to_slug: &str,
        since: i64,
    ) -> Result<i64, StoreError> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM messages
             WHERE from_slug = ?1 AND to_slug = ?2 AND created_at >= ?3 AND kind = 'message'",
            params![from_slug, to_slug, since],
            |row| row.get(0),
        )?)
    }

    /// The messages of a floor, oldest first (for tests and later views).
    pub fn all_messages(&self) -> Result<Vec<MessageRecord>, StoreError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages ORDER BY id"
        ))?;
        let rows = statement.query_map([], message_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

// ---------------------------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------------------------

/// The status of a task (spec 4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    InProgress,
    Blocked,
    Review,
    Done,
}

impl TaskStatus {
    pub const ALL: [TaskStatus; 5] = [
        TaskStatus::Todo,
        TaskStatus::InProgress,
        TaskStatus::Blocked,
        TaskStatus::Review,
        TaskStatus::Done,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Todo => "todo",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Review => "review",
            TaskStatus::Done => "done",
        }
    }

    pub fn parse(text: &str) -> Option<TaskStatus> {
        TaskStatus::ALL.into_iter().find(|s| s.as_str() == text)
    }
}

/// One row of the `tasks` table. Times are Unix milliseconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRecord {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub parent: Option<i64>,
    /// An employee slug, or `operator`.
    pub creator: String,
    pub assignee: Option<String>,
    /// Who gave the task to the assignee: an employee slug, or `operator`.
    pub assigner: Option<String>,
    pub status: TaskStatus,
    /// The report file, relative to the floor folder (`.swarm/reports/T-12.md`).
    pub report: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// The branch of the task, `task/T-12-add-login`. `None` when the task has no worktree.
    pub branch: Option<String>,
    /// The branch that the main checkout had when the worktree was made.
    pub base_branch: Option<String>,
    /// The commit that the main checkout had when the worktree was made.
    pub base_commit: Option<String>,
    /// The worktree folder, relative to the floor folder (`.swarm/worktrees/T-12`). `None` when
    /// there is none now (no worktree was made, or it was removed).
    pub worktree: Option<String>,
}

/// What the app records when it makes the worktree of a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRecord {
    pub branch: String,
    pub base_branch: String,
    pub base_commit: String,
    /// Relative to the floor folder.
    pub path: String,
}

/// What `Database::finish_task` does with the git fields of the task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitAfter {
    /// The git fields stay as they are.
    Keep,
    /// The worktree folder is gone (a merge). The branch fields stay, as a record.
    WorktreeGone,
    /// The worktree and the branch are gone (a discard). All git fields are cleared.
    AllGone,
}

/// One row of the `task_events` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskEventRecord {
    pub id: i64,
    pub task_id: i64,
    pub at: i64,
    /// An employee slug, or `operator`.
    pub author: String,
    /// `created`, `assigned`, `status`, `comment` or `report`.
    pub kind: String,
    /// The new status, the assignee, or the report path.
    pub value: Option<String>,
    /// The note or the comment.
    pub text: Option<String>,
}

/// A message that a task change stores in the same transaction as the change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTaskMessage {
    pub from_slug: String,
    pub to_slug: String,
    pub text: String,
    pub kind: &'static str,
    /// The orders that the app writes after the body. `None`: the default orders.
    pub orders: Option<String>,
}

/// What `Database::update_task` changes.
#[derive(Debug, Clone, Copy)]
pub struct TaskChange<'a> {
    pub status: TaskStatus,
    pub note: Option<&'a str>,
    pub report: Option<&'a str>,
    pub author: &'a str,
}

/// The result of `Database::update_task`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskOutcome {
    pub before: TaskRecord,
    pub after: TaskRecord,
    /// The ID of the message that the change stored, if there is one.
    pub message_id: Option<i64>,
}

const TASK_COLUMNS: &str = "id, title, description, parent_id, creator, assignee, assigner, status, report, created_at, updated_at, branch, base_branch, base_commit, worktree";

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRecord> {
    let status: String = row.get(7)?;
    Ok(TaskRecord {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        parent: row.get(3)?,
        creator: row.get(4)?,
        assignee: row.get(5)?,
        assigner: row.get(6)?,
        // A value that this build does not know (a newer app wrote it) shows as `todo`.
        status: TaskStatus::parse(&status).unwrap_or(TaskStatus::Todo),
        report: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        branch: row.get(11)?,
        base_branch: row.get(12)?,
        base_commit: row.get(13)?,
        worktree: row.get(14)?,
    })
}

fn event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskEventRecord> {
    Ok(TaskEventRecord {
        id: row.get(0)?,
        task_id: row.get(1)?,
        at: row.get(2)?,
        author: row.get(3)?,
        kind: row.get(4)?,
        value: row.get(5)?,
        text: row.get(6)?,
    })
}

fn task_in(tx: &Transaction<'_>, id: i64) -> Result<TaskRecord, StoreError> {
    tx.query_row(
        &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"),
        params![id],
        task_from_row,
    )
    .optional()?
    .ok_or(StoreError::UnknownTask(id))
}

fn event_in(
    tx: &Transaction<'_>,
    task_id: i64,
    at: i64,
    author: &str,
    kind: &str,
    value: Option<&str>,
    text: Option<&str>,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO task_events (task_id, at, author, kind, value, text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![task_id, at, author, kind, value, text],
    )?;
    Ok(())
}

fn message_in(
    tx: &Transaction<'_>,
    message: &NewTaskMessage,
    task_id: i64,
    at: i64,
) -> Result<i64, StoreError> {
    if message.from_slug != OPERATOR_SLUG {
        let sent: i64 = tx.query_row(
            "SELECT COUNT(*) FROM messages
             WHERE from_slug = ?1 AND to_slug = ?2 AND created_at >= ?3
               AND kind IN ('task_assign', 'task_status')",
            params![
                message.from_slug,
                message.to_slug,
                at - TASK_MESSAGE_WINDOW_MS
            ],
            |row| row.get(0),
        )?;
        if sent >= TASK_MESSAGE_LIMIT {
            return Err(StoreError::Rule(format!(
                "Task message limit: you sent {sent} task messages (assignments and status changes) to {} in the last hour, and the limit is {TASK_MESSAGE_LIMIT}. Stop now. Do not assign a task to {} or change a task status for {} again. Wait for the operator.",
                message.to_slug, message.to_slug, message.to_slug
            )));
        }
    }
    tx.execute(
        "INSERT INTO messages (from_slug, to_slug, text, created_at, delivered_at, task_id, kind, orders)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7)",
        params![
            message.from_slug,
            message.to_slug,
            message.text,
            at,
            task_id,
            message.kind,
            message.orders
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

impl Database {
    /// Make a task with the status `todo`. A parent that does not exist is refused.
    pub fn create_task(
        &self,
        title: &str,
        description: &str,
        parent: Option<i64>,
        creator: &str,
        now: i64,
    ) -> Result<TaskRecord, StoreError> {
        let tx = self.write_transaction()?;
        if let Some(parent) = parent {
            task_in(&tx, parent)?;
        }
        tx.execute(
            "INSERT INTO tasks (title, description, parent_id, creator, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'todo', ?5, ?5)",
            params![title, description, parent, creator, now],
        )?;
        let id = tx.last_insert_rowid();
        event_in(&tx, id, now, creator, "created", None, None)?;
        let task = task_in(&tx, id)?;
        tx.commit()?;
        Ok(task)
    }

    pub fn get_task(&self, id: i64) -> Result<Option<TaskRecord>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"),
                params![id],
                task_from_row,
            )
            .optional()?)
    }

    /// The tasks, oldest first. A filter that is `None` is not applied.
    pub fn list_tasks(
        &self,
        status: Option<TaskStatus>,
        assignee: Option<&str>,
    ) -> Result<Vec<TaskRecord>, StoreError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks
             WHERE (?1 IS NULL OR status = ?1) AND (?2 IS NULL OR assignee = ?2) ORDER BY id"
        ))?;
        let rows = statement.query_map(
            params![status.map(TaskStatus::as_str), assignee],
            task_from_row,
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The events of a task, oldest first.
    pub fn task_events(&self, id: i64) -> Result<Vec<TaskEventRecord>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT id, task_id, at, author, kind, value, text FROM task_events
             WHERE task_id = ?1 ORDER BY id",
        )?;
        let rows = statement.query_map(params![id], event_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Give the task to `assignee`. The change, its event and the message that `message` builds
    /// are one transaction, so a task is never assigned without its message.
    pub fn assign_task(
        &self,
        id: i64,
        assignee: &str,
        author: &str,
        now: i64,
        worktree: Option<&WorktreeRecord>,
        message: impl FnOnce(&TaskRecord) -> NewTaskMessage,
    ) -> Result<(TaskRecord, i64), StoreError> {
        let tx = self.write_transaction()?;
        task_in(&tx, id)?;
        tx.execute(
            "UPDATE tasks SET assignee = ?2, assigner = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, assignee, author, now],
        )?;
        event_in(&tx, id, now, author, "assigned", Some(assignee), None)?;
        if let Some(worktree) = worktree {
            tx.execute(
                "UPDATE tasks SET branch = ?2, base_branch = ?3, base_commit = ?4, worktree = ?5 WHERE id = ?1",
                params![
                    id,
                    worktree.branch,
                    worktree.base_branch,
                    worktree.base_commit,
                    worktree.path
                ],
            )?;
            let note = format!(
                "Branch {} from {} at {}.",
                worktree.branch,
                worktree.base_branch,
                &worktree.base_commit[..worktree.base_commit.len().min(12)]
            );
            event_in(
                &tx,
                id,
                now,
                author,
                "worktree",
                Some(&worktree.branch),
                Some(&note),
            )?;
        }
        let after = task_in(&tx, id)?;
        let message_id = message_in(&tx, &message(&after), id, now)?;
        tx.commit()?;
        Ok((after, message_id))
    }

    /// Change the status, and add the note and the report path. `guard` reads the task inside the
    /// transaction and can refuse (`StoreError::Rule`). `message` can add one message. Nothing
    /// is written when `guard` refuses.
    pub fn update_task(
        &self,
        id: i64,
        change: &TaskChange<'_>,
        now: i64,
        guard: impl FnOnce(&TaskRecord) -> Result<(), String>,
        message: impl FnOnce(&TaskRecord, &TaskRecord) -> Option<NewTaskMessage>,
    ) -> Result<TaskOutcome, StoreError> {
        let tx = self.write_transaction()?;
        let before = task_in(&tx, id)?;
        guard(&before).map_err(StoreError::Rule)?;
        tx.execute(
            "UPDATE tasks SET status = ?2, report = coalesce(?3, report), updated_at = ?4 WHERE id = ?1",
            params![id, change.status.as_str(), change.report, now],
        )?;
        let note = change.note.filter(|n| !n.trim().is_empty());
        if change.status != before.status {
            // A status change, with the note if there is one.
            event_in(
                &tx,
                id,
                now,
                change.author,
                "status",
                Some(change.status.as_str()),
                note,
            )?;
        } else if note.is_some() {
            // The same status and a note: it is a comment.
            event_in(&tx, id, now, change.author, "comment", None, note)?;
        }
        if let Some(report) = change.report
            && before.report.as_deref() != Some(report)
        {
            event_in(&tx, id, now, change.author, "report", Some(report), None)?;
        }
        let after = task_in(&tx, id)?;
        let message_id = match message(&before, &after) {
            Some(message) => Some(message_in(&tx, &message, id, now)?),
            None => None,
        };
        tx.commit()?;
        Ok(TaskOutcome {
            before,
            after,
            message_id,
        })
    }

    /// Add a comment to the events of a task.
    pub fn comment_task(
        &self,
        id: i64,
        author: &str,
        text: &str,
        now: i64,
    ) -> Result<TaskRecord, StoreError> {
        let tx = self.write_transaction()?;
        task_in(&tx, id)?;
        tx.execute(
            "UPDATE tasks SET updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        event_in(&tx, id, now, author, "comment", None, Some(text))?;
        let task = task_in(&tx, id)?;
        tx.commit()?;
        Ok(task)
    }

    /// Set the status after a git action of the app (a merge, a discard), and change the git
    /// fields as `git` says. The event is of the kind `status` when the status changes, else of
    /// the kind `comment`. It stores no message: `insert_task_message` does that. Returns the task
    /// before and after.
    pub fn finish_task(
        &self,
        id: i64,
        author: &str,
        status: TaskStatus,
        note: &str,
        git: GitAfter,
        now: i64,
    ) -> Result<(TaskRecord, TaskRecord), StoreError> {
        let tx = self.write_transaction()?;
        let before = task_in(&tx, id)?;
        tx.execute(
            "UPDATE tasks SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, status.as_str(), now],
        )?;
        match git {
            GitAfter::Keep => {}
            GitAfter::WorktreeGone => {
                tx.execute(
                    "UPDATE tasks SET worktree = NULL WHERE id = ?1",
                    params![id],
                )?;
            }
            GitAfter::AllGone => {
                tx.execute(
                    "UPDATE tasks SET branch = NULL, base_branch = NULL, base_commit = NULL, worktree = NULL WHERE id = ?1",
                    params![id],
                )?;
            }
        }
        if status != before.status {
            event_in(
                &tx,
                id,
                now,
                author,
                "status",
                Some(status.as_str()),
                Some(note),
            )?;
        } else {
            event_in(&tx, id, now, author, "comment", None, Some(note))?;
        }
        let after = task_in(&tx, id)?;
        tx.commit()?;
        Ok((before, after))
    }

    /// Store one message for a task (outside of `update_task`). Returns its ID.
    pub fn insert_task_message(
        &self,
        task_id: i64,
        message: &NewTaskMessage,
        now: i64,
    ) -> Result<i64, StoreError> {
        let tx = self.write_transaction()?;
        let id = message_in(&tx, message, task_id, now)?;
        tx.commit()?;
        Ok(id)
    }
}

// ---------------------------------------------------------------------------------------------
// The restore list
// ---------------------------------------------------------------------------------------------

impl Database {
    /// Replace the list of employees that had a live session at the last close.
    pub fn replace_resume_marks(&self, slugs: &[String], at: i64) -> Result<(), StoreError> {
        let tx = self.write_transaction()?;
        tx.execute("DELETE FROM resume_marks", [])?;
        for slug in slugs {
            tx.execute(
                "INSERT OR REPLACE INTO resume_marks (slug, marked_at) VALUES (?1, ?2)",
                params![slug, at],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Put one employee in the list (its session started). A second call for the same slug
    /// keeps one row.
    pub fn add_resume_mark(&self, slug: &str, at: i64) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO resume_marks (slug, marked_at) VALUES (?1, ?2)",
            params![slug, at],
        )?;
        Ok(())
    }

    /// Take one employee out of the list (the operator stopped its session by hand, or the
    /// session ended by itself).
    pub fn clear_resume_mark(&self, slug: &str) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM resume_marks WHERE slug = ?1", params![slug])?;
        Ok(())
    }

    /// The slugs in the list, in the order they were stored.
    pub fn resume_marks(&self) -> Result<Vec<String>, StoreError> {
        let mut statement = self
            .conn
            .prepare("SELECT slug FROM resume_marks ORDER BY rowid")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
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

/// The one rule for "the floor is gone": the folder, its `.swarm/` folder or `.swarm/floor.toml`
/// does not exist at this path. Checking the file covers the two folders, because a path that
/// has a file also has the folders above it. A check that fails for another reason (for example,
/// no permission) is not "gone": the app cannot tell, so it does not say that the floor is gone.
pub fn floor_is_gone(cwd: &Path) -> bool {
    match fs::metadata(cwd.join(SWARM_DIR).join(FLOOR_FILE)) {
        Ok(_) => false,
        Err(error) => matches!(
            error.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
        ),
    }
}

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
        // A `.swarm` that is a folder (or a link to a folder) with no `floor.toml` is not a
        // floor. It gets the same answer as a folder with no `.swarm`, not a raw I/O error.
        let floor = match read_floor_file(&swarm.join(FLOOR_FILE)) {
            Err(StoreError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Err(StoreError::NoFloor(cwd.to_path_buf()));
            }
            other => other?,
        };
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

    /// The `allow` list of the old `[permissions]` table of `floor.toml`, as written, with no
    /// check. `None` when the file has no such table. Since 2026-11-21 the app does not use this
    /// table for a session: the rules are in the app config. An agent can write this file, so the
    /// app only shows these rules to the operator as a proposal (`floor_permissions_proposed`).
    /// The file is read again at each call. A value of another type than a list of texts counts as
    /// an empty list.
    pub fn legacy_permissions(&self) -> Result<Option<Vec<String>>, StoreError> {
        let path = self.swarm_dir().join(FLOOR_FILE);
        let text = read_text(&path)?;
        let table: toml::Table = toml::from_str(&text).map_err(|e| invalid(&path, e))?;
        let Some(permissions) = table.get("permissions") else {
            return Ok(None);
        };
        let allow = permissions
            .get("allow")
            .and_then(toml::Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Some(allow))
    }

    /// Remove the old `[permissions]` table from `floor.toml`. The rest of the file stays (see
    /// `set_brand`). Returns `true` when a table was there. Nothing is written when it was not.
    pub fn remove_legacy_permissions(&mut self) -> Result<bool, StoreError> {
        let path = self.swarm_dir().join(FLOOR_FILE);
        let text = read_text(&path)?;
        let mut table: toml::Table = toml::from_str(&text).map_err(|e| invalid(&path, e))?;
        if table.remove("permissions").is_none() {
            return Ok(false);
        }
        let text = toml::to_string(&table).map_err(|e| invalid(&path, e))?;
        write_atomic(&path, text.as_bytes())?;
        Ok(true)
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
    /// The floor is gone now (see [`floor_is_gone`]). The entry stays in the list.
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

    /// The floors in the order they were added. A path whose floor is gone (see [`floor_is_gone`])
    /// is marked `missing` and is not removed. The mark is computed at each call.
    pub fn list(&self) -> Result<Vec<FloorEntry>, StoreError> {
        Ok(self
            .load()?
            .floors
            .into_iter()
            .map(|path| FloorEntry {
                missing: floor_is_gone(&path),
                path,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Arc;
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

    #[test]
    fn messages_wait_then_are_taken_once_and_old_database_migrates() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("floor.db");
        // A version 1 file (before the messages) gets the new table and keeps its rows.
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.execute_batch("PRAGMA user_version = 1").unwrap();
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        let a = db.insert_message("pm", "eng", "first", 100, None).unwrap();
        let b = db.insert_message("pm", "eng", "second", 200, None).unwrap();
        db.insert_message("pm", "qa", "other target", 150, None)
            .unwrap();
        db.insert_message("pm", "eng", "sent with the start", 120, Some(120))
            .unwrap();
        assert_eq!(db.count_pending_messages("eng").unwrap(), 2);
        let waiting = db.pending_messages("eng").unwrap();
        assert_eq!(
            waiting
                .iter()
                .map(|m| (m.id, m.text.as_str()))
                .collect::<Vec<_>>(),
            [(a, "first"), (b, "second")]
        );
        let taken = db.take_pending_messages("eng", 300).unwrap();
        assert_eq!(taken.len(), 2);
        assert!(db.take_pending_messages("eng", 400).unwrap().is_empty());
        assert_eq!(db.count_pending_messages("qa").unwrap(), 1);
        // The first delivery time stays.
        db.mark_messages_delivered(&[a], 999).unwrap();
        let first = db
            .all_messages()
            .unwrap()
            .into_iter()
            .find(|m| m.id == a)
            .unwrap();
        assert_eq!(first.delivered_at, Some(300));
        // The loop-limit count: by pair, and only from `since`.
        assert_eq!(db.count_messages_since("pm", "eng", 0).unwrap(), 3);
        assert_eq!(db.count_messages_since("pm", "eng", 150).unwrap(), 1);
        assert_eq!(db.count_messages_since("eng", "pm", 0).unwrap(), 0);
    }

    /// R1: a live session writes its status events through its own connection while the bus takes
    /// messages through another one. A deferred transaction that reads and then writes gets
    /// SQLITE_BUSY at once when the other connection committed in between (WAL mode;
    /// `busy_timeout` does not help). The three message writes must start as write transactions.
    #[test]
    fn taking_messages_never_gets_database_is_locked_while_another_connection_writes() {
        use std::sync::atomic::AtomicBool;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("floor.db");
        let taker = Database::open(&path).unwrap();
        let writer = Database::open(&path).unwrap();
        // No fsync at each commit: the test is about the lock, and it runs faster.
        writer
            .conn
            .execute_batch("PRAGMA synchronous = OFF")
            .unwrap();
        writer
            .create_session(&session("s1", "eng", 1))
            .expect("create the session");
        let stop = Arc::new(AtomicBool::new(false));
        let writer_stop = Arc::clone(&stop);
        let writer_thread = std::thread::spawn(move || {
            let mut written = 0u32;
            while !writer_stop.load(Ordering::SeqCst) {
                writer
                    .append_status_event("s1", "working", i64::from(written))
                    .expect("the writer must not fail");
                written += 1;
                std::thread::sleep(Duration::from_millis(1));
            }
            written
        });
        let mut failures = Vec::new();
        for round in 0..2000 {
            if let Err(error) = taker.take_pending_messages("eng", 1) {
                failures.push(format!("take {round}: {error}"));
            }
            // The other two writes use the same kind of transaction.
            if round % 10 == 0 {
                if let Err(error) = taker.mark_messages_delivered(&[1, 2], 1) {
                    failures.push(format!("deliver {round}: {error}"));
                }
                if let Err(error) = taker.mark_messages_waiting(&[1, 2]) {
                    failures.push(format!("wait {round}: {error}"));
                }
            }
            // A short pause, so that the writer gets the lock between the rounds.
            std::thread::sleep(Duration::from_micros(300));
        }
        stop.store(true, Ordering::SeqCst);
        let written = writer_thread.join().unwrap();
        assert!(
            written > 50,
            "the writer must run during the test: {written}"
        );
        assert!(
            failures.is_empty(),
            "{} failures, first: {:?}",
            failures.len(),
            failures.first()
        );
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
    fn database_reopens_with_no_change() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("floor.db");
        let first = Database::open(&path).unwrap();
        assert_eq!(first.schema_version().unwrap(), SCHEMA_VERSION);
        first.create_session(&session("s1", "ceo", 1)).unwrap();
        first.set_cli_session_id("s1", "cli").unwrap();
        first.append_status_event("s1", "idle", 2).unwrap();
        let schema = schema_dump(&first.conn);
        let tables: Vec<_> = schema.iter().filter(|s| s.starts_with("table:")).collect();
        assert_eq!(
            tables.len(),
            7,
            "sessions, status_events, messages, tasks, task_events, resume_marks, sqlite_sequence: {tables:?}"
        );
        drop(first);

        let second = Database::open(&path).unwrap();
        assert_eq!(second.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(schema_dump(&second.conn), schema);
        let kept = second.last_session("ceo").unwrap().unwrap();
        assert_eq!(kept.cli_session_id.as_deref(), Some("cli"));
        assert_eq!(second.status_events("s1").unwrap().len(), 1);
    }

    /// A database at version 2 (as the build of M2 slice 1 wrote it) with a message in it.
    fn version_2_database(path: &Path) {
        let conn = Connection::open(path).unwrap();
        for step in &MIGRATIONS[..2] {
            conn.execute_batch(step).unwrap();
        }
        conn.execute_batch("PRAGMA user_version = 2").unwrap();
        conn.execute(
            "INSERT INTO messages (from_slug, to_slug, text, created_at, delivered_at)
             VALUES ('ceo', 'pm', 'old message', 10, NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (from_slug, to_slug, text, created_at, delivered_at)
             VALUES ('pm', 'ceo', 'old answer', 11, 12)",
            [],
        )
        .unwrap();
    }

    #[test]
    fn a_version_2_database_migrates_to_the_task_schema_and_keeps_its_messages() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("floor.db");
        version_2_database(&path);
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), 4);
        assert_eq!(SCHEMA_VERSION, 4);
        let old = db.all_messages().unwrap();
        assert_eq!(old.len(), 2);
        assert_eq!(
            (old[0].text.as_str(), old[0].kind.as_str(), old[0].task_id),
            ("old message", KIND_MESSAGE, None)
        );
        assert_eq!(old[1].delivered_at, Some(12));
        assert_eq!(db.count_pending_messages("pm").unwrap(), 1);
        // The old messages count for the loop limit (their kind is `message`).
        assert_eq!(db.count_messages_since("ceo", "pm", 0).unwrap(), 1);
        // The new tables work, and the first task is T-1.
        let task = db.create_task("Title", "Text", None, "ceo", 20).unwrap();
        assert_eq!(task.id, 1);
        assert_eq!(task.status, TaskStatus::Todo);
        // A second open changes nothing.
        drop(db);
        let again = Database::open(&path).unwrap();
        assert_eq!(again.schema_version().unwrap(), 4);
        assert_eq!(again.list_tasks(None, None).unwrap().len(), 1);
    }

    /// A database at version 3 (as the build of M2 slice 2 wrote it) with a task, an event and
    /// two messages in it.
    fn version_3_database(path: &Path) {
        let conn = Connection::open(path).unwrap();
        for step in &MIGRATIONS[..3] {
            conn.execute_batch(step).unwrap();
        }
        conn.execute_batch("PRAGMA user_version = 3").unwrap();
        conn.execute(
            "INSERT INTO tasks (title, description, creator, assignee, assigner, status, report, created_at, updated_at)
             VALUES ('Old task', 'Text', 'pm', 'eng', 'pm', 'in_progress', '.swarm/reports/T-1.md', 5, 6)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO task_events (task_id, at, author, kind, value, text) VALUES (1, 5, 'pm', 'created', NULL, NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (from_slug, to_slug, text, created_at, delivered_at, task_id, kind)
             VALUES ('pm', 'eng', 'Title: Old task', 7, NULL, 1, 'task_assign')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn a_version_3_database_migrates_to_version_4_and_keeps_its_tasks_and_messages() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("floor.db");
        version_3_database(&path);
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), 4);
        // The old task has no git fields. Nothing else changed.
        let old = db.get_task(1).unwrap().unwrap();
        assert_eq!(
            (old.title.as_str(), old.status),
            ("Old task", TaskStatus::InProgress)
        );
        assert_eq!(old.report.as_deref(), Some(".swarm/reports/T-1.md"));
        assert_eq!(
            (old.branch, old.base_branch, old.base_commit, old.worktree),
            (None, None, None, None)
        );
        let messages = db.all_messages().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(
            (
                messages[0].kind.as_str(),
                messages[0].task_id,
                messages[0].orders.clone()
            ),
            (KIND_TASK_ASSIGN, Some(1), None)
        );
        assert_eq!(db.task_events(1).unwrap().len(), 1);
        // The new table works, and an old task can get a worktree.
        assert_eq!(db.resume_marks().unwrap(), Vec::<String>::new());
        db.replace_resume_marks(&["ceo".to_owned(), "pm".to_owned()], 9)
            .unwrap();
        assert_eq!(db.resume_marks().unwrap(), ["ceo", "pm"]);
        let worktree = WorktreeRecord {
            branch: "task/T-1-old-task".into(),
            base_branch: "main".into(),
            base_commit: "a".repeat(40),
            path: ".swarm/worktrees/T-1".into(),
        };
        let (task, _) = db
            .assign_task(1, "eng", "pm", 10, Some(&worktree), |after| {
                NewTaskMessage {
                    from_slug: "pm".into(),
                    to_slug: "eng".into(),
                    text: after.title.clone(),
                    kind: KIND_TASK_ASSIGN,
                    orders: Some("orders".into()),
                }
            })
            .unwrap();
        assert_eq!(task.branch.as_deref(), Some("task/T-1-old-task"));
        assert_eq!(task.worktree.as_deref(), Some(".swarm/worktrees/T-1"));
        assert_eq!(
            db.all_messages().unwrap()[1].orders.as_deref(),
            Some("orders")
        );
        // A second open changes nothing, also for the version 3 file that was migrated.
        drop(db);
        let again = Database::open(&path).unwrap();
        assert_eq!(again.schema_version().unwrap(), 4);
        assert_eq!(
            again.get_task(1).unwrap().unwrap().branch.as_deref(),
            Some("task/T-1-old-task")
        );
    }

    #[test]
    fn finish_task_sets_the_status_and_the_git_fields_and_writes_one_event() {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("floor.db")).unwrap();
        db.create_task("Work", "", None, "pm", 1).unwrap();
        let worktree = WorktreeRecord {
            branch: "task/T-1-work".into(),
            base_branch: "main".into(),
            base_commit: "b".repeat(40),
            path: ".swarm/worktrees/T-1".into(),
        };
        db.assign_task(1, "eng", "pm", 2, Some(&worktree), |after| NewTaskMessage {
            from_slug: "pm".into(),
            to_slug: "eng".into(),
            text: after.title.clone(),
            kind: KIND_TASK_ASSIGN,
            orders: None,
        })
        .unwrap();
        // A merge: the worktree is gone, the branch stays as a record.
        let (before, after) = db
            .finish_task(
                1,
                "pm",
                TaskStatus::Done,
                "merged",
                GitAfter::WorktreeGone,
                3,
            )
            .unwrap();
        assert_eq!(before.worktree.as_deref(), Some(".swarm/worktrees/T-1"));
        assert_eq!(
            (after.status, after.worktree.clone()),
            (TaskStatus::Done, None)
        );
        assert_eq!(after.branch.as_deref(), Some("task/T-1-work"));
        // The same status with a note is a comment. A discard clears all git fields.
        let (_, again) = db
            .finish_task(
                1,
                "operator",
                TaskStatus::Done,
                "note",
                GitAfter::AllGone,
                4,
            )
            .unwrap();
        assert_eq!(
            (again.branch, again.base_branch, again.base_commit),
            (None, None, None)
        );
        let kinds: Vec<(String, Option<String>)> = db
            .task_events(1)
            .unwrap()
            .into_iter()
            .map(|e| (e.kind, e.value))
            .collect();
        assert_eq!(
            kinds[kinds.len() - 2..],
            [
                ("status".to_owned(), Some("done".to_owned())),
                ("comment".to_owned(), None)
            ]
        );
        assert!(matches!(
            db.finish_task(9, "pm", TaskStatus::Done, "x", GitAfter::Keep, 5),
            Err(StoreError::UnknownTask(9))
        ));
    }

    #[test]
    fn task_changes_write_events_and_a_refused_change_writes_nothing() {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("floor.db")).unwrap();
        let task = db.create_task("Title", "Text", None, "ceo", 1).unwrap();
        let child = db
            .create_task("Child", "", Some(task.id), "ceo", 2)
            .unwrap();
        assert_eq!((child.id, child.parent), (2, Some(1)));
        assert!(matches!(
            db.create_task("Orphan", "", Some(99), "ceo", 3),
            Err(StoreError::UnknownTask(99))
        ));
        let (assigned, message) = db
            .assign_task(1, "eng", "ceo", 4, None, |after| NewTaskMessage {
                from_slug: "ceo".into(),
                to_slug: "eng".into(),
                text: after.title.clone(),
                kind: KIND_TASK_ASSIGN,
                orders: None,
            })
            .unwrap();
        assert_eq!(assigned.assignee.as_deref(), Some("eng"));
        assert_eq!(assigned.assigner.as_deref(), Some("ceo"));
        let stored = db.all_messages().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, message);
        assert_eq!(
            (stored[0].task_id, stored[0].kind.as_str()),
            (Some(1), KIND_TASK_ASSIGN)
        );
        // The message of a task does not count for the loop limit.
        assert_eq!(db.count_messages_since("ceo", "eng", 0).unwrap(), 0);

        let change = TaskChange {
            status: TaskStatus::Review,
            note: Some("ready"),
            report: Some(".swarm/reports/T-1.md"),
            author: "eng",
        };
        // A guard that refuses: nothing is written.
        let refused = db.update_task(1, &change, 5, |_| Err("no".to_owned()), |_, _| None);
        assert!(matches!(refused, Err(StoreError::Rule(ref text)) if text == "no"));
        assert_eq!(db.get_task(1).unwrap().unwrap().status, TaskStatus::Todo);
        assert_eq!(db.task_events(1).unwrap().len(), 2, "created, assigned");
        // A change with a note and a report: a status event and a report event.
        let outcome = db
            .update_task(1, &change, 6, |_| Ok(()), |_, _| None)
            .unwrap();
        assert_eq!(outcome.message_id, None);
        assert_eq!(outcome.after.status, TaskStatus::Review);
        let kinds: Vec<(String, Option<String>, Option<String>)> = db
            .task_events(1)
            .unwrap()
            .into_iter()
            .map(|e| (e.kind, e.value, e.text))
            .collect();
        assert_eq!(
            kinds[2..],
            [
                ("status".into(), Some("review".into()), Some("ready".into())),
                ("report".into(), Some(".swarm/reports/T-1.md".into()), None),
            ]
        );
        // The same status with a note is a comment. A change with no report keeps the report.
        let same = TaskChange {
            status: TaskStatus::Review,
            note: Some("more"),
            report: None,
            author: "ceo",
        };
        let outcome = db
            .update_task(1, &same, 7, |_| Ok(()), |_, _| None)
            .unwrap();
        assert_eq!(
            outcome.after.report.as_deref(),
            Some(".swarm/reports/T-1.md")
        );
        let last = db.task_events(1).unwrap().pop().unwrap();
        assert_eq!(
            (
                last.kind.as_str(),
                last.author.as_str(),
                last.text.as_deref()
            ),
            ("comment", "ceo", Some("more"))
        );
        // Filters.
        assert_eq!(
            db.list_tasks(Some(TaskStatus::Review), None).unwrap().len(),
            1
        );
        assert_eq!(db.list_tasks(None, Some("eng")).unwrap().len(), 1);
        assert_eq!(
            db.list_tasks(Some(TaskStatus::Done), None).unwrap().len(),
            0
        );
        assert!(matches!(
            db.comment_task(77, "ceo", "x", 8),
            Err(StoreError::UnknownTask(77))
        ));
    }

    #[test]
    fn database_with_higher_version_is_refused_and_unchanged() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("floor.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE future (x); PRAGMA user_version = 99;")
                .unwrap();
        }
        let before = fs::read(&path).unwrap();
        let error = Database::open(&path).unwrap_err();
        match error {
            StoreError::DbVersionNewer { found, known, .. } => {
                assert_eq!((found, known), (99, SCHEMA_VERSION))
            }
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
        for folder in [&here, &gone] {
            fs::create_dir_all(folder.join(SWARM_DIR)).unwrap();
            fs::write(folder.join(SWARM_DIR).join(FLOOR_FILE), "name = \"x\"\n").unwrap();
        }
        assert!(list.add(&here).unwrap());
        assert!(list.add(&gone).unwrap());
        assert!(!list.add(&here).unwrap(), "no duplicate");
        fs::remove_dir_all(&gone).unwrap();

        let entries = list.list().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(!entries[0].missing);
        assert!(entries[1].missing, "a missing folder is reported");

        // The folder is there but `.swarm/floor.toml` is not: the floor is gone too. It is
        // computed at each call, so a restored file clears the mark at once.
        fs::remove_file(here.join(SWARM_DIR).join(FLOOR_FILE)).unwrap();
        assert!(list.list().unwrap()[0].missing);
        fs::write(here.join(SWARM_DIR).join(FLOOR_FILE), "name = \"x\"\n").unwrap();
        assert!(!list.list().unwrap()[0].missing);
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
