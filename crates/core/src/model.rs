//! Data types of the store. Other steps add to this file.
//!
//! The status state machine is not here. `status.rs` owns it. A session record keeps the last
//! status as a plain string.

use serde::{Deserialize, Serialize};

/// Which CLI an employee runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CliKind {
    Claude,
    Codex,
}

/// What an employee may do without asking (spec 4.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionPolicy {
    ReadOnly,
    EditInWorktree,
    AskAlways,
}

/// Default values for new employees of a floor. A field that is `None` is not in the file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FloorDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli: Option<CliKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<PermissionPolicy>,
}

/// The data of `.swarm/floor.toml`, without the file version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Floor {
    pub name: String,
    pub defaults: FloorDefaults,
}

/// One employee. The file name `<slug>.md` gives the slug. The role text is the file body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Employee {
    pub name: String,
    pub slug: String,
    pub role: String,
    /// Slug of the manager. `None` means the employee answers to the operator.
    pub manager: Option<String>,
    pub cli: CliKind,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission: PermissionPolicy,
    pub role_text: String,
}

/// One row of the `sessions` table. Times are Unix milliseconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    /// ID that the app gives to the session. The caller makes it.
    pub app_session_id: String,
    pub employee_slug: String,
    pub cli_session_id: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub last_status: String,
}
