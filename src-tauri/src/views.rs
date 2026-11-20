//! The JSON types of `docs/ipc-m1.md`. Field names are camelCase. The tests at the end compare
//! each type with a literal JSON text, so a renamed field breaks a test.

use serde::{Deserialize, Serialize};
use std::path::Path;
use tower_core::app;
use tower_core::model::{CliKind, Employee as CoreEmployee, PermissionPolicy};
use tower_core::status::Status;

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cli {
    Claude,
    Codex,
}

impl From<CliKind> for Cli {
    fn from(kind: CliKind) -> Cli {
        match kind {
            CliKind::Claude => Cli::Claude,
            CliKind::Codex => Cli::Codex,
        }
    }
}

impl From<Cli> for CliKind {
    fn from(cli: Cli) -> CliKind {
        match cli {
            Cli::Claude => CliKind::Claude,
            Cli::Codex => CliKind::Codex,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    ReadOnly,
    EditInWorktree,
    AskAlways,
}

impl From<PermissionPolicy> for Policy {
    fn from(policy: PermissionPolicy) -> Policy {
        match policy {
            PermissionPolicy::ReadOnly => Policy::ReadOnly,
            PermissionPolicy::EditInWorktree => Policy::EditInWorktree,
            PermissionPolicy::AskAlways => Policy::AskAlways,
        }
    }
}

impl From<Policy> for PermissionPolicy {
    fn from(policy: Policy) -> PermissionPolicy {
        match policy {
            Policy::ReadOnly => PermissionPolicy::ReadOnly,
            Policy::EditInWorktree => PermissionPolicy::EditInWorktree,
            Policy::AskAlways => PermissionPolicy::AskAlways,
        }
    }
}

/// `FloorEntry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FloorEntry {
    pub path: String,
    pub name: Option<String>,
    pub missing: bool,
    /// Modification time of `logo.png` in ms. `null` when there is no logo.
    pub logo_version: Option<i64>,
}

impl From<app::FloorEntryView> for FloorEntry {
    fn from(entry: app::FloorEntryView) -> FloorEntry {
        FloorEntry {
            path: text(&entry.path),
            name: entry.name,
            missing: entry.missing,
            logo_version: entry.logo_version,
        }
    }
}

/// `Employee`. It goes in both directions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Employee {
    pub slug: String,
    pub name: String,
    pub role: String,
    pub manager: Option<String>,
    pub cli: Cli,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub policy: Policy,
    pub role_text: String,
}

impl From<CoreEmployee> for Employee {
    fn from(e: CoreEmployee) -> Employee {
        Employee {
            slug: e.slug,
            name: e.name,
            role: e.role,
            manager: e.manager,
            cli: e.cli.into(),
            model: e.model,
            effort: e.effort,
            policy: e.permission.into(),
            role_text: e.role_text,
        }
    }
}

impl From<Employee> for CoreEmployee {
    fn from(e: Employee) -> CoreEmployee {
        CoreEmployee {
            slug: e.slug,
            name: e.name,
            role: e.role,
            manager: e.manager,
            cli: e.cli.into(),
            model: e.model,
            effort: e.effort,
            permission: e.policy.into(),
            role_text: e.role_text,
        }
    }
}

/// `SessionView`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub session_id: String,
    pub slug: String,
    pub status: Status,
    pub draft: bool,
    pub message_waiting: bool,
    pub can_resume: bool,
}

impl From<&app::SessionView> for SessionView {
    fn from(view: &app::SessionView) -> SessionView {
        SessionView {
            session_id: view.session_id.clone(),
            slug: view.slug.clone(),
            status: view.status,
            draft: view.draft,
            message_waiting: view.message_waiting,
            can_resume: view.can_resume,
        }
    }
}

impl From<app::SessionView> for SessionView {
    fn from(view: app::SessionView) -> SessionView {
        SessionView::from(&view)
    }
}

/// `FloorView`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FloorView {
    pub path: String,
    pub name: String,
    pub employees: Vec<Employee>,
    pub employee_errors: Vec<String>,
    pub sessions: Vec<SessionView>,
    /// `[brand] description`.
    pub description: Option<String>,
    /// `[brand] logo_prompt`.
    pub logo_prompt: Option<String>,
    pub logo_version: Option<i64>,
    pub logo_busy: bool,
    pub logo_error: Option<String>,
    pub setup_session_id: Option<String>,
}

impl From<app::FloorView> for FloorView {
    fn from(view: app::FloorView) -> FloorView {
        FloorView {
            path: text(&view.path),
            name: view.name,
            employees: view.employees.into_iter().map(Employee::from).collect(),
            employee_errors: view.employee_errors,
            sessions: view.sessions.iter().map(SessionView::from).collect(),
            description: view.description,
            logo_prompt: view.logo_prompt,
            logo_version: view.logo_version,
            logo_busy: view.logo_busy,
            logo_error: view.logo_error,
            setup_session_id: view.setup_session_id,
        }
    }
}

/// Where the OpenAI key comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeySource {
    Config,
    Env,
}

/// `Settings`. It never holds the key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub openai_key_set: bool,
    pub openai_key_source: Option<KeySource>,
    pub image_model: String,
}

impl From<app::SettingsView> for Settings {
    fn from(view: app::SettingsView) -> Settings {
        Settings {
            openai_key_set: view.openai_key_set,
            openai_key_source: view.openai_key_source.map(|source| match source {
                app::KeySource::Config => KeySource::Config,
                app::KeySource::Env => KeySource::Env,
            }),
            image_model: view.image_model,
        }
    }
}

/// `Programs`. `hook` is an empty text when `tower-hook` was not found (the contract has no null
/// for it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Programs {
    pub claude: Option<String>,
    pub codex: Option<String>,
    pub node: Option<String>,
    pub hook: String,
}

impl From<app::ProgramsView> for Programs {
    fn from(view: app::ProgramsView) -> Programs {
        let opt = |path: Option<std::path::PathBuf>| path.map(|p| text(&p));
        Programs {
            claude: opt(view.claude),
            codex: opt(view.codex),
            node: opt(view.node),
            hook: view.hook.map(|p| text(&p)).unwrap_or_default(),
        }
    }
}

/// The result of `session_attach`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachResult {
    /// Base64.
    pub scrollback: String,
}

/// `OutputMessage` on the channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputMessage {
    /// Base64.
    pub data: String,
}

/// Payload of the event `session-changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionChanged {
    pub path: String,
    pub session: SessionView,
}

/// Payload of the event `floor-changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FloorChanged {
    pub path: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn to_json<T: Serialize>(value: &T) -> Value {
        serde_json::to_value(value).unwrap()
    }

    fn employee() -> Employee {
        Employee {
            slug: "eng-1".into(),
            name: "Eli".into(),
            role: "Engineer".into(),
            manager: Some("pm".into()),
            cli: Cli::Codex,
            model: None,
            effort: Some("high".into()),
            policy: Policy::EditInWorktree,
            role_text: "Write code.".into(),
        }
    }

    fn employee_json() -> Value {
        json!({
            "slug": "eng-1", "name": "Eli", "role": "Engineer", "manager": "pm",
            "cli": "codex", "model": null, "effort": "high",
            "policy": "edit_in_worktree", "roleText": "Write code."
        })
    }

    fn session() -> SessionView {
        SessionView {
            session_id: "s-1".into(),
            slug: "ceo".into(),
            status: Status::Question,
            draft: true,
            message_waiting: false,
            can_resume: false,
        }
    }

    fn session_json() -> Value {
        json!({
            "sessionId": "s-1", "slug": "ceo", "status": "question",
            "draft": true, "messageWaiting": false, "canResume": false
        })
    }

    #[test]
    fn employee_has_the_contract_fields_in_both_directions() {
        assert_eq!(to_json(&employee()), employee_json());
        let back: Employee = serde_json::from_value(employee_json()).unwrap();
        assert_eq!(back, employee());
        // Missing optional fields and nulls are both `None`.
        let sparse: Employee = serde_json::from_value(json!({
            "slug": "a", "name": "A", "role": "R", "cli": "claude",
            "policy": "read_only", "roleText": ""
        }))
        .unwrap();
        assert_eq!(sparse.manager, None);
        assert_eq!(sparse.model, None);
    }

    #[test]
    fn all_policies_and_statuses_use_the_contract_words() {
        for (policy, word) in [
            (Policy::ReadOnly, "read_only"),
            (Policy::EditInWorktree, "edit_in_worktree"),
            (Policy::AskAlways, "ask_always"),
        ] {
            assert_eq!(to_json(&policy), json!(word));
        }
        for (status, word) in [
            (Status::Starting, "starting"),
            (Status::Working, "working"),
            (Status::Idle, "idle"),
            (Status::Question, "question"),
            (Status::Done, "done"),
            (Status::Stopped, "stopped"),
        ] {
            assert_eq!(to_json(&status), json!(word));
        }
        assert_eq!(to_json(&Cli::Claude), json!("claude"));
    }

    #[test]
    fn core_conversion_keeps_every_field() {
        let core: CoreEmployee = employee().into();
        assert_eq!(core.permission, PermissionPolicy::EditInWorktree);
        assert_eq!(core.cli, CliKind::Codex);
        assert_eq!(Employee::from(core), employee());
    }

    #[test]
    fn session_view_floor_view_and_floor_entry_have_the_contract_fields() {
        assert_eq!(to_json(&session()), session_json());
        let floor = FloorView {
            path: "/p".into(),
            name: "Acme".into(),
            employees: vec![employee()],
            employee_errors: vec!["bad.md".into()],
            sessions: vec![session()],
            description: Some("A shop.".into()),
            logo_prompt: None,
            logo_version: Some(1_700_000_000_000),
            logo_busy: true,
            logo_error: Some("NO_KEY: x".into()),
            setup_session_id: None,
        };
        assert_eq!(
            to_json(&floor),
            json!({
                "path": "/p", "name": "Acme", "employees": [employee_json()],
                "employeeErrors": ["bad.md"], "sessions": [session_json()],
                "description": "A shop.", "logoPrompt": null,
                "logoVersion": 1_700_000_000_000_i64, "logoBusy": true,
                "logoError": "NO_KEY: x", "setupSessionId": null
            })
        );
        let entry = FloorEntry {
            path: "/p".into(),
            name: None,
            missing: true,
            logo_version: None,
        };
        assert_eq!(
            to_json(&entry),
            json!({"path": "/p", "name": null, "missing": true, "logoVersion": null})
        );
        let settings = Settings {
            openai_key_set: true,
            openai_key_source: Some(KeySource::Env),
            image_model: "m".into(),
        };
        assert_eq!(
            to_json(&settings),
            json!({"openaiKeySet": true, "openaiKeySource": "env", "imageModel": "m"})
        );
        let none = Settings {
            openai_key_set: false,
            openai_key_source: None,
            image_model: "m".into(),
        };
        assert_eq!(
            to_json(&none),
            json!({"openaiKeySet": false, "openaiKeySource": null, "imageModel": "m"})
        );
        assert_eq!(to_json(&KeySource::Config), json!("config"));
    }

    #[test]
    fn programs_output_message_attach_result_and_event_payloads() {
        let programs = Programs {
            claude: Some("/c".into()),
            codex: None,
            node: Some("/n".into()),
            hook: "/h".into(),
        };
        assert_eq!(
            to_json(&programs),
            json!({"claude": "/c", "codex": null, "node": "/n", "hook": "/h"})
        );
        assert_eq!(
            to_json(&OutputMessage {
                data: "AAE=".into()
            }),
            json!({"data": "AAE="})
        );
        assert_eq!(
            to_json(&AttachResult {
                scrollback: "AAE=".into()
            }),
            json!({"scrollback": "AAE="})
        );
        assert_eq!(
            to_json(&SessionChanged {
                path: "/p".into(),
                session: session()
            }),
            json!({"path": "/p", "session": session_json()})
        );
        assert_eq!(
            to_json(&FloorChanged { path: "/p".into() }),
            json!({"path": "/p"})
        );
    }
}
