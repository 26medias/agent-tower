//! The JSON types of `docs/ipc-m1.md`. Field names are camelCase. The tests at the end compare
//! each type with a literal JSON text, so a renamed field breaks a test.

use serde::{Deserialize, Serialize};
use std::path::Path;
use tower_core::app;
use tower_core::model::{
    CliKind, Employee as CoreEmployee, FloorDefaults as CoreDefaults, PermissionPolicy,
};
use tower_core::status::Status;
pub use tower_core::store::TaskStatus;
use tower_core::store::{TaskEventRecord, TaskRecord};

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
    /// `[defaults]` of `floor.toml`.
    pub defaults: FloorDefaults,
    /// Slugs of the employees that were live at the last close and can resume now (M3).
    pub resumable: Vec<String>,
    /// The background resume of the last team. `null` when none runs.
    pub resume_progress: Option<ResumeProgress>,
    /// The folder of the floor is gone (`docs/ipc-final.md`). Then the other fields are the last
    /// known ones. `false` in normal use.
    #[serde(default)]
    pub gone: bool,
}

/// How far the resume of the last team is (`docs/ipc-m3.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeProgress {
    pub done: usize,
    pub total: usize,
}

impl From<app::ResumeProgress> for ResumeProgress {
    fn from(progress: app::ResumeProgress) -> ResumeProgress {
        ResumeProgress {
            done: progress.done,
            total: progress.total,
        }
    }
}

/// The result of `floor_permissions_set` (`docs/ipc-m3.md`): the stored rules, and a warning for
/// each rule that lets an agent run the code of the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionsSaved {
    pub allow: Vec<String>,
    pub warnings: Vec<String>,
}

impl From<app::PermissionsSaved> for PermissionsSaved {
    fn from(saved: app::PermissionsSaved) -> PermissionsSaved {
        PermissionsSaved {
            allow: saved.allow,
            warnings: saved.warnings,
        }
    }
}

/// `FloorDefaults`: the `[defaults]` of `floor.toml`. It goes in both directions. A missing field
/// and a null are both `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FloorDefaults {
    #[serde(default)]
    pub cli: Option<Cli>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub policy: Option<Policy>,
}

impl From<CoreDefaults> for FloorDefaults {
    fn from(d: CoreDefaults) -> FloorDefaults {
        FloorDefaults {
            cli: d.cli.map(Cli::from),
            model: d.model,
            effort: d.effort,
            policy: d.permission.map(Policy::from),
        }
    }
}

impl From<FloorDefaults> for CoreDefaults {
    fn from(d: FloorDefaults) -> CoreDefaults {
        CoreDefaults {
            cli: d.cli.map(CliKind::from),
            model: d.model,
            effort: d.effort,
            permission: d.policy.map(PermissionPolicy::from),
        }
    }
}

/// `ModelOption`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub id: String,
    pub label: String,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
    pub custom: bool,
}

impl From<app::ModelOption> for ModelOption {
    fn from(m: app::ModelOption) -> ModelOption {
        ModelOption {
            id: m.id,
            label: m.label,
            efforts: m.efforts,
            default_effort: m.default_effort,
            custom: m.custom,
        }
    }
}

/// `ModelCatalog`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalog {
    pub claude: Vec<ModelOption>,
    pub codex: Vec<ModelOption>,
    pub image: Vec<ModelOption>,
    /// `"cache"` or `"builtin"`.
    pub codex_source: String,
}

impl From<app::ModelCatalog> for ModelCatalog {
    fn from(c: app::ModelCatalog) -> ModelCatalog {
        let list =
            |models: Vec<app::ModelOption>| models.into_iter().map(ModelOption::from).collect();
        ModelCatalog {
            claude: list(c.claude),
            codex: list(c.codex),
            image: list(c.image),
            codex_source: match c.codex_source {
                app::CodexSource::Cache => "cache".to_owned(),
                app::CodexSource::Builtin => "builtin".to_owned(),
            },
        }
    }
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
            defaults: view.defaults.into(),
            resumable: view.resumable,
            resume_progress: view.resume_progress.map(ResumeProgress::from),
            gone: view.gone,
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
    /// `[interface] notify_on_question` of the app config (default `true`).
    pub notify_on_question: bool,
    /// The home folder of the app, or `null` when it is not known. The UI opens the "Browse..."
    /// dialog there when no floor gives a better place.
    pub home_dir: Option<String>,
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
            notify_on_question: view.notify_on_question,
            home_dir: view.home_dir.map(|home| text(&home)),
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
    /// The program `rtk`, or `null` when the app finds none. It is optional.
    pub rtk: Option<String>,
    /// The path that the operator set for each program. `null`: no override.
    pub overrides: ProgramOverrides,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramOverrides {
    pub claude: Option<String>,
    pub codex: Option<String>,
    pub node: Option<String>,
}

/// The result of `program_test`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramTestResult {
    pub ok: bool,
    pub path: Option<String>,
    pub output: String,
}

impl From<app::ProgramTest> for ProgramTestResult {
    fn from(result: app::ProgramTest) -> ProgramTestResult {
        ProgramTestResult {
            ok: result.ok,
            path: result.path.map(|p| text(&p)),
            output: result.output,
        }
    }
}

impl From<app::ProgramsView> for Programs {
    fn from(view: app::ProgramsView) -> Programs {
        let opt = |path: Option<std::path::PathBuf>| path.map(|p| text(&p));
        Programs {
            claude: opt(view.claude),
            codex: opt(view.codex),
            node: opt(view.node),
            hook: view.hook.map(|p| text(&p)).unwrap_or_default(),
            rtk: opt(view.rtk),
            overrides: {
                let [claude, codex, node] = view.overrides;
                ProgramOverrides {
                    claude: opt(claude),
                    codex: opt(codex),
                    node: opt(node),
                }
            },
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

/// Payload of the event `employee-started`: an employee started the session of another employee
/// with the tools of the app. The session itself comes as a normal `session-changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmployeeStarted {
    pub path: String,
    pub session_id: String,
    /// The employee that was started.
    pub slug: String,
    pub name: String,
    /// The employee that started it.
    pub by_slug: String,
    pub by_name: String,
    /// The last CLI session was resumed (and not a fresh one started).
    pub resumed: bool,
}

impl EmployeeStarted {
    pub fn new(path: &Path, event: &app::EmployeeStarted) -> EmployeeStarted {
        EmployeeStarted {
            path: text(path),
            session_id: event.session_id.clone(),
            slug: event.slug.clone(),
            name: event.name.clone(),
            by_slug: event.by_slug.clone(),
            by_name: event.by_name.clone(),
            resumed: event.resumed,
        }
    }
}

/// Payload of the event `message-delivered`: the app delivered a message to the session of an
/// employee. `kind` is `message` or `task`. `task_status` is the status that a task status
/// message names (empty when the text has none), and `null` for all other messages. `preview` is
/// at most 80 characters of the body, never the full text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDelivered {
    pub path: String,
    pub from_slug: String,
    pub from_name: String,
    pub to_slug: String,
    pub to_name: String,
    pub kind: String,
    pub task_id: Option<i64>,
    pub task_status: Option<String>,
    pub preview: String,
}

impl MessageDelivered {
    pub fn new(path: &Path, event: &app::MessageDelivered) -> MessageDelivered {
        MessageDelivered {
            path: text(path),
            from_slug: event.from_slug.clone(),
            from_name: event.from_name.clone(),
            to_slug: event.to_slug.clone(),
            to_name: event.to_name.clone(),
            kind: event.kind.clone(),
            task_id: event.task_id,
            task_status: event.task_status.clone(),
            preview: event.preview.clone(),
        }
    }
}

/// Payload of the event `floor-changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FloorChanged {
    pub path: String,
}

/// Payload of the event `notice` (`docs/ipc-m3.md`): something that the operator must see.
/// `kind` is `task_blocked` or `resume_failed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub path: String,
    pub kind: String,
    pub text: String,
}

/// Payload of the event `tasks-changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TasksChanged {
    pub path: String,
}

/// `TaskView` (`docs/ipc-m2.md`). The status words are `todo`, `in_progress`, `blocked`, `review`
/// and `done`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    /// The number of the task in its floor. It is shown as `T-<id>`.
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
    /// The git branch of the task, or `null` (`docs/ipc-m3.md`).
    pub branch: Option<String>,
    pub base_branch: Option<String>,
    pub base_commit: Option<String>,
    /// The worktree folder, relative to the floor folder, or `null` when there is none now.
    pub worktree: Option<String>,
}

impl From<TaskRecord> for TaskView {
    fn from(task: TaskRecord) -> TaskView {
        TaskView {
            id: task.id,
            title: task.title,
            description: task.description,
            parent: task.parent,
            creator: task.creator,
            assignee: task.assignee,
            assigner: task.assigner,
            status: task.status,
            report: task.report,
            created_at: task.created_at,
            updated_at: task.updated_at,
            branch: task.branch,
            base_branch: task.base_branch,
            base_commit: task.base_commit,
            worktree: task.worktree,
        }
    }
}

/// `TaskEvent` (`docs/ipc-m2.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    pub id: i64,
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

impl From<TaskEventRecord> for TaskEvent {
    fn from(event: TaskEventRecord) -> TaskEvent {
        TaskEvent {
            id: event.id,
            at: event.at,
            author: event.author,
            kind: event.kind,
            value: event.value,
            text: event.text,
        }
    }
}

/// The result of `task_merge` (`docs/ipc-m3.md`). A merge that is blocked has `merged` false: the
/// status of the task is `blocked` then, and `message` says why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskMerge {
    pub merged: bool,
    pub message: String,
    pub task: TaskView,
}

impl From<app::MergeReport> for TaskMerge {
    fn from(report: app::MergeReport) -> TaskMerge {
        TaskMerge {
            merged: report.merged,
            message: report.text,
            task: report.task.into(),
        }
    }
}

/// The result of `task_diff` (`docs/ipc-m3.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDiff {
    pub branch: Option<String>,
    pub base_branch: Option<String>,
    pub stat: String,
    pub patch: String,
    pub truncated: bool,
    pub note: Option<String>,
}

impl From<app::TaskDiffView> for TaskDiff {
    fn from(view: app::TaskDiffView) -> TaskDiff {
        TaskDiff {
            branch: view.branch,
            base_branch: view.base_branch,
            stat: view.stat,
            patch: view.patch,
            truncated: view.truncated,
            note: view.note,
        }
    }
}

/// `TaskDetail`: the result of `task_get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetail {
    pub task: TaskView,
    pub events: Vec<TaskEvent>,
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
            defaults: FloorDefaults {
                cli: Some(Cli::Codex),
                model: Some("gpt-6-luna".into()),
                effort: None,
                policy: Some(Policy::AskAlways),
            },
            resumable: vec!["pm".into(), "eng".into()],
            resume_progress: Some(ResumeProgress { done: 1, total: 2 }),
            gone: true,
        };
        assert_eq!(
            to_json(&floor),
            json!({
                "path": "/p", "name": "Acme", "employees": [employee_json()],
                "employeeErrors": ["bad.md"], "sessions": [session_json()],
                "description": "A shop.", "logoPrompt": null,
                "logoVersion": 1_700_000_000_000_i64, "logoBusy": true,
                "logoError": "NO_KEY: x", "setupSessionId": null,
                "defaults": {
                    "cli": "codex", "model": "gpt-6-luna", "effort": null, "policy": "ask_always"
                },
                "resumable": ["pm", "eng"],
                "resumeProgress": {"done": 1, "total": 2},
                "gone": true
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
            notify_on_question: false,
            home_dir: Some("/home/a".into()),
        };
        assert_eq!(
            to_json(&settings),
            json!({"openaiKeySet": true, "openaiKeySource": "env", "imageModel": "m", "notifyOnQuestion": false, "homeDir": "/home/a"})
        );
        let none = Settings {
            openai_key_set: false,
            openai_key_source: None,
            image_model: "m".into(),
            notify_on_question: true,
            home_dir: None,
        };
        assert_eq!(
            to_json(&none),
            json!({"openaiKeySet": false, "openaiKeySource": null, "imageModel": "m", "notifyOnQuestion": true, "homeDir": null})
        );
        assert_eq!(to_json(&KeySource::Config), json!("config"));
    }

    #[test]
    fn floor_defaults_convert_both_ways_and_accept_missing_fields() {
        let json_in =
            json!({"cli": "claude", "model": "haiku", "effort": "low", "policy": "read_only"});
        let defaults: FloorDefaults = serde_json::from_value(json_in.clone()).unwrap();
        assert_eq!(to_json(&defaults), json_in);
        let core: CoreDefaults = defaults.clone().into();
        assert_eq!(core.cli, Some(CliKind::Claude));
        assert_eq!(core.permission, Some(PermissionPolicy::ReadOnly));
        assert_eq!(FloorDefaults::from(core), defaults);
        let empty: FloorDefaults = serde_json::from_value(json!({})).unwrap();
        assert_eq!(empty, FloorDefaults::default());
        let nulls: FloorDefaults = serde_json::from_value(
            json!({"cli": null, "model": null, "effort": null, "policy": null}),
        )
        .unwrap();
        assert_eq!(nulls, FloorDefaults::default());
    }

    #[test]
    fn model_catalog_has_the_contract_fields() {
        let catalog = ModelCatalog::from(app::ModelCatalog {
            claude: vec![app::ModelOption {
                id: "haiku".into(),
                label: "Haiku".into(),
                efforts: vec!["low".into()],
                default_effort: Some("low".into()),
                custom: false,
            }],
            codex: vec![],
            image: vec![app::ModelOption {
                id: "img".into(),
                label: "img".into(),
                efforts: vec![],
                default_effort: None,
                custom: true,
            }],
            codex_source: app::CodexSource::Cache,
        });
        assert_eq!(
            to_json(&catalog),
            json!({
                "claude": [{"id": "haiku", "label": "Haiku", "efforts": ["low"],
                    "defaultEffort": "low", "custom": false}],
                "codex": [],
                "image": [{"id": "img", "label": "img", "efforts": [],
                    "defaultEffort": null, "custom": true}],
                "codexSource": "cache"
            })
        );
    }

    #[test]
    fn programs_output_message_attach_result_and_event_payloads() {
        let programs = Programs {
            claude: Some("/c".into()),
            codex: None,
            node: Some("/n".into()),
            hook: "/h".into(),
            rtk: Some("/r".into()),
            overrides: ProgramOverrides {
                claude: Some("/mine/c".into()),
                codex: None,
                node: None,
            },
        };
        assert_eq!(
            to_json(&programs),
            json!({"claude": "/c", "codex": null, "node": "/n", "hook": "/h", "rtk": "/r",
                "overrides": {"claude": "/mine/c", "codex": null, "node": null}})
        );
        assert_eq!(
            to_json(&ProgramTestResult {
                ok: false,
                path: None,
                output: "x".into()
            }),
            json!({"ok": false, "path": null, "output": "x"})
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
        assert_eq!(
            to_json(&TasksChanged { path: "/p".into() }),
            json!({"path": "/p"})
        );
        assert_eq!(
            to_json(&Notice {
                path: "/p".into(),
                kind: "task_blocked".into(),
                text: "T-1 is blocked".into()
            }),
            json!({"path": "/p", "kind": "task_blocked", "text": "T-1 is blocked"})
        );
        assert_eq!(
            to_json(&TaskDiff {
                branch: Some("task/T-1-x".into()),
                base_branch: Some("main".into()),
                stat: " a.txt | 1 +\n".into(),
                patch: "diff".into(),
                truncated: false,
                note: None
            }),
            json!({"branch": "task/T-1-x", "baseBranch": "main", "stat": " a.txt | 1 +\n",
                "patch": "diff", "truncated": false, "note": null})
        );
        assert_eq!(
            to_json(&EmployeeStarted {
                path: "/p".into(),
                session_id: "s2".into(),
                slug: "eng".into(),
                name: "Kenji".into(),
                by_slug: "pm".into(),
                by_name: "Tomas".into(),
                resumed: false,
            }),
            json!({"path": "/p", "sessionId": "s2", "slug": "eng", "name": "Kenji", "bySlug": "pm", "byName": "Tomas", "resumed": false})
        );
        assert_eq!(
            to_json(&MessageDelivered::new(
                Path::new("/p"),
                &app::MessageDelivered {
                    from_slug: "pm".into(),
                    from_name: "Maya".into(),
                    to_slug: "eng".into(),
                    to_name: "Eli".into(),
                    kind: "task".into(),
                    task_id: Some(3),
                    task_status: Some("review".into()),
                    preview: "Title: x".into(),
                }
            )),
            json!({"path": "/p", "fromSlug": "pm", "fromName": "Maya", "toSlug": "eng", "toName": "Eli",
                "kind": "task", "taskId": 3, "taskStatus": "review", "preview": "Title: x"})
        );
    }

    #[test]
    fn the_permissions_result_has_the_allow_and_warnings_fields() {
        assert_eq!(
            to_json(&PermissionsSaved::from(app::PermissionsSaved {
                allow: vec!["Bash(ls:*)".into()],
                warnings: vec!["w".into()],
            })),
            json!({"allow": ["Bash(ls:*)"], "warnings": ["w"]})
        );
    }

    #[test]
    fn task_views_have_the_contract_fields_and_status_words() {
        let record = TaskRecord {
            id: 12,
            title: "Add hello".into(),
            description: "Write hello.txt".into(),
            parent: Some(3),
            creator: "pm".into(),
            assignee: Some("eng".into()),
            assigner: Some("operator".into()),
            status: TaskStatus::InProgress,
            report: Some(".swarm/reports/T-12.md".into()),
            created_at: 1000,
            updated_at: 2000,
            branch: Some("task/T-12-add-hello".into()),
            base_branch: Some("main".into()),
            base_commit: Some("a".repeat(40)),
            worktree: Some(".swarm/worktrees/T-12".into()),
        };
        assert_eq!(
            to_json(&TaskView::from(record.clone())),
            json!({
                "id": 12, "title": "Add hello", "description": "Write hello.txt",
                "parent": 3, "creator": "pm", "assignee": "eng", "assigner": "operator",
                "status": "in_progress", "report": ".swarm/reports/T-12.md",
                "createdAt": 1000, "updatedAt": 2000,
                "branch": "task/T-12-add-hello", "baseBranch": "main",
                "baseCommit": "a".repeat(40), "worktree": ".swarm/worktrees/T-12"
            })
        );
        for (status, word) in [
            (TaskStatus::Todo, "todo"),
            (TaskStatus::InProgress, "in_progress"),
            (TaskStatus::Blocked, "blocked"),
            (TaskStatus::Review, "review"),
            (TaskStatus::Done, "done"),
        ] {
            assert_eq!(to_json(&status), json!(word));
            let back: TaskStatus = serde_json::from_value(json!(word)).unwrap();
            assert_eq!(back, status);
        }
        let event = TaskEvent::from(TaskEventRecord {
            id: 5,
            task_id: 12,
            at: 1500,
            author: "eng".into(),
            kind: "status".into(),
            value: Some("review".into()),
            text: None,
        });
        assert_eq!(
            to_json(&TaskDetail {
                task: record.into(),
                events: vec![event]
            })["events"],
            json!([{"id": 5, "at": 1500, "author": "eng", "kind": "status",
                "value": "review", "text": null}])
        );
    }
}
