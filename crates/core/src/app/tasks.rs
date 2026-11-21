//! The task board (spec 4.4, group "tasks", and 4.10): the six MCP tools, and the commands of
//! the operator.
//!
//! # Who may do what
//!
//! | Action | Employee | Operator |
//! |---|---|---|
//! | `task_create`, `task_get`, `task_list`, `task_comment` | any employee of the floor | yes |
//! | `task_assign` | the caller needs `may_start` over the assignee | yes |
//! | `task_update` | the assignee, the creator, the `ceo` and the `pm` | yes |
//!
//! The identity comes from the token. The floor of the token is the only floor that a tool can
//! reach.
//!
//! # Messages
//!
//! `task_assign` and a status change store one message in the same database transaction as the
//! change (`Database::assign_task`, `Database::update_task`). The message has the header
//! `[task T-12 from <Name> (<Role>)]` (see `bus::format_task_message`). It goes through the same
//! delivery paths as a message of `send`, so the app starts a stopped assignee by the rules of
//! `employee_start`. These messages do not count for the loop limit (`kind` is not `message`).
//! A start that an assignment causes counts for the start limit, unless the operator assigns.
//! A status change tells the person who assigned the task, or the creator if nobody assigned it.
//! It tells nobody when that person is the caller (or the operator).
//!
//! # Worktrees (M3, spec 4.8)
//!
//! `task_assign` makes the git worktree and the branch of a task when the assignee has the policy
//! `edit-in-worktree` and the floor folder is the top of a git repository with a commit, on a
//! branch (`gitwork::repo_state`). The worktree is `{floor}/.swarm/worktrees/T-<id>/`. The task
//! keeps the branch, the base branch, the base commit and the worktree folder. The assignment
//! message tells the assignee the rules (`bus::assign_orders`). When the status goes to `review`,
//! the app commits the worktree (`gitwork::snapshot`). The merge, the diff and the discard are in
//! `merge.rs`.
//!
//! # Reports
//!
//! A report is a file under `{floor}/.swarm/reports/`. `check_report` refuses a path outside that
//! folder (also with `..` and with a symbolic link), and, for the status `review` and `done`, a
//! path that has no file. The task keeps the path relative to the floor folder.

use super::bus::{
    Identity, Sent, Slot, Workplace, assign_orders, db_error, may_start, one_line, review_orders,
    sanitize_message,
};
use super::{
    FloorHandle, OPERATOR, REPORTS_DIR, Result, SETUP_SLUG, TowerCore, TowerError, locked, now_ms,
    other,
};
use crate::gitwork::{self, RepoState, Snapshot};
use crate::model::{Employee, PermissionPolicy};
use crate::store::{
    KIND_TASK_ASSIGN, KIND_TASK_STATUS, NewTaskMessage, StoreError, TaskChange, TaskEventRecord,
    TaskRecord, TaskStatus, WorktreeRecord,
};
use serde_json::{Value, json};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// The longest title, in characters.
pub const TITLE_MAX_CHARS: usize = 200;

/// The longest description, note and comment, in characters.
pub const TASK_TEXT_MAX_CHARS: usize = 20_000;

/// The most bytes of a report file that `task_report` returns.
pub const REPORT_READ_MAX_BYTES: usize = 256 * 1024;

/// The task that the operator reads is the task of this floor. `T-12`, `t-12`, `12` and the
/// number 12 all name the same task.
pub(super) fn parse_task_ref(value: Option<&Value>) -> std::result::Result<i64, String> {
    let bad = || "The argument \"task\" must be a task ID, for example \"T-12\".".to_owned();
    let text = match value {
        Some(Value::String(text)) => text.trim().to_owned(),
        Some(Value::Number(number)) => number.to_string(),
        _ => return Err(bad()),
    };
    let digits = text
        .strip_prefix("T-")
        .or_else(|| text.strip_prefix("t-"))
        .unwrap_or(&text);
    match digits.parse::<i64>() {
        Ok(id) if id > 0 => Ok(id),
        _ => Err(bad()),
    }
}

fn text_arg<'a>(args: &'a Value, key: &str) -> std::result::Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("The argument \"{key}\" is missing. It must be a text."))
}

fn optional_text<'a>(args: &'a Value, key: &str) -> std::result::Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(format!("The argument \"{key}\" must be a text.")),
    }
}

/// Check the length, then remove the hidden characters (the rule of a message).
fn clean_text(
    what: &str,
    text: &str,
    max: usize,
    required: bool,
) -> std::result::Result<String, String> {
    let count = text.chars().count();
    if count > max {
        return Err(format!(
            "The {what} is too long: {count} characters. The limit is {max}. Write a shorter text, or put the details in a file."
        ));
    }
    let clean = sanitize_message(text);
    if required && clean.trim().is_empty() {
        return Err(format!("The {what} is empty."));
    }
    Ok(clean)
}

fn clean_title(text: &str) -> std::result::Result<String, String> {
    let count = text.chars().count();
    if count > TITLE_MAX_CHARS {
        return Err(format!(
            "The title is too long: {count} characters. The limit is {TITLE_MAX_CHARS}."
        ));
    }
    // A title is one line.
    let title = one_line(text).trim().to_owned();
    if title.is_empty() {
        return Err("The title is empty.".to_owned());
    }
    Ok(title)
}

fn parse_status(text: &str) -> std::result::Result<TaskStatus, String> {
    TaskStatus::parse(text.trim()).ok_or_else(|| {
        format!(
            "\"{text}\" is not a status. Use one of: {}.",
            TaskStatus::ALL.map(TaskStatus::as_str).join(", ")
        )
    })
}

/// Remove `.` and `..` from a path without looking at the file system.
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The report path that a tool call gives, made safe. The path must be inside
/// `{floor}/.swarm/reports/`. `must_exist` is true for the status `review` and `done`. Returns the
/// path relative to the floor folder.
fn check_report(
    floor: &Path,
    given: &str,
    must_exist: bool,
) -> std::result::Result<String, String> {
    let given = given.trim();
    if given.is_empty() {
        return Err("The report path is empty.".to_owned());
    }
    if given.chars().any(char::is_control) {
        return Err("The report path has a control character.".to_owned());
    }
    let reports = floor.join(REPORTS_DIR);
    let wanted = || {
        format!(
            "The report file must be inside {}. Use {REPORTS_DIR}/<task-id>.md, for example {REPORTS_DIR}/T-12.md.",
            reports.display()
        )
    };
    let candidate = if Path::new(given).is_absolute() {
        PathBuf::from(given)
    } else {
        floor.join(given)
    };
    let normal = normalize_lexically(&candidate);
    let relative = match normal.strip_prefix(&reports) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative.to_path_buf(),
        _ => return Err(wanted()),
    };
    // A symbolic link inside the folder must not lead out of it.
    if let Ok(real) = fs::canonicalize(&normal) {
        let real_reports = fs::canonicalize(&reports).unwrap_or(reports.clone());
        if !real.starts_with(&real_reports) {
            return Err(format!(
                "The report path leads outside the reports folder (a link?). {}",
                wanted()
            ));
        }
    }
    let shown = format!("{REPORTS_DIR}/{}", relative.to_string_lossy());
    if must_exist && !normal.is_file() {
        return Err(format!(
            "The report file {shown} does not exist. Write the report file first, then call task_update again."
        ));
    }
    Ok(shown)
}

/// The text of a status message: the title, the new status, the note and the report path.
pub(super) fn status_body(task: &TaskRecord, note: Option<&str>) -> String {
    let mut lines = vec![
        format!("Title: {}", task.title),
        format!("Status: {}", task.status.as_str()),
    ];
    if let Some(note) = note.filter(|n| !n.trim().is_empty()) {
        lines.push(format!("Note: {note}"));
    }
    if let Some(report) = &task.report {
        lines.push(format!("Report: {report}"));
    }
    lines.join("\n")
}

/// The text of an assignment message: the title and the description.
fn assign_body(task: &TaskRecord) -> String {
    if task.description.trim().is_empty() {
        format!("Title: {}", task.title)
    } else {
        format!("Title: {}\n\n{}", task.title, task.description)
    }
}

/// Who acts: an employee (its slug and name), or the operator.
pub(super) struct Actor {
    pub slug: String,
    pub name: String,
    pub operator: bool,
}

impl Actor {
    pub(super) fn employee(who: &Identity, team: &[Employee]) -> Actor {
        let name = team
            .iter()
            .find(|e| e.slug == who.slug)
            .map_or(who.slug.clone(), |e| e.name.clone());
        Actor {
            slug: who.slug.clone(),
            name,
            operator: false,
        }
    }

    pub(super) fn operator() -> Actor {
        Actor {
            slug: OPERATOR.to_owned(),
            name: "Operator".to_owned(),
            operator: true,
        }
    }
}

impl TowerCore {
    /// The caller must be an employee of its floor. Returns the floor, the team and the actor.
    pub(super) fn task_actor(
        &self,
        who: &Identity,
    ) -> std::result::Result<(Arc<FloorHandle>, Vec<Employee>, Actor), String> {
        let floor = self.floor_handle(&who.path).map_err(|e| e.to_string())?;
        let team = self.team_of(&floor)?;
        if !team.iter().any(|e| e.slug == who.slug) {
            return Err("You are not an employee of this floor now.".to_owned());
        }
        let actor = Actor::employee(who, &team);
        Ok((floor, team, actor))
    }

    // ---- the tools ----

    pub(super) fn tool_task_create(
        &self,
        who: &Identity,
        args: &Value,
    ) -> std::result::Result<String, String> {
        let (floor, _team, actor) = self.task_actor(who)?;
        let title = clean_title(text_arg(args, "title")?)?;
        let description = clean_text(
            "description",
            text_arg(args, "description")?,
            TASK_TEXT_MAX_CHARS,
            false,
        )?;
        let parent = match args.get("parent") {
            None | Some(Value::Null) => None,
            value => Some(parse_task_ref(value)?),
        };
        let task = self.create_task_as(&floor, &actor, &title, &description, parent)?;
        Ok(format!(
            "Task T-{} is made with the status todo. Give it to an employee with the tool task_assign.",
            task.id
        ))
    }

    pub(super) fn tool_task_assign(
        &self,
        who: &Identity,
        args: &Value,
    ) -> std::result::Result<String, String> {
        let (floor, team, actor) = self.task_actor(who)?;
        let id = parse_task_ref(args.get("task"))?;
        let to = text_arg(args, "employee")?;
        self.assign_task_as(&floor, &team, &actor, id, to)
    }

    pub(super) fn tool_task_update(
        &self,
        who: &Identity,
        args: &Value,
    ) -> std::result::Result<String, String> {
        let (floor, team, actor) = self.task_actor(who)?;
        let id = parse_task_ref(args.get("task"))?;
        let status = parse_status(text_arg(args, "status")?)?;
        let note = optional_text(args, "note")?;
        let report = optional_text(args, "report")?;
        let task = self.update_task_as(&floor, &team, &actor, id, status, note, report)?;
        Ok(format!(
            "Task T-{} has the status {}.",
            task.id,
            task.status.as_str()
        ))
    }

    pub(super) fn tool_task_get(
        &self,
        who: &Identity,
        args: &Value,
    ) -> std::result::Result<String, String> {
        let (floor, _team, _actor) = self.task_actor(who)?;
        let id = parse_task_ref(args.get("task"))?;
        let (task, events) = self.read_task(&floor, id).map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&json!({
            "task": task_json(&task),
            "events": events.iter().map(event_json).collect::<Vec<_>>(),
        }))
        .map_err(|e| e.to_string())
    }

    pub(super) fn tool_task_list(
        &self,
        who: &Identity,
        args: &Value,
    ) -> std::result::Result<String, String> {
        let (floor, _team, _actor) = self.task_actor(who)?;
        let status = match optional_text(args, "status")? {
            Some(text) if !text.trim().is_empty() => Some(parse_status(text)?),
            _ => None,
        };
        let assignee = optional_text(args, "assignee")?.filter(|a| !a.trim().is_empty());
        let tasks = locked(&floor.store)
            .db()
            .list_tasks(status, assignee.map(str::trim))
            .map_err(db_error)?;
        serde_json::to_string_pretty(&tasks.iter().map(task_json).collect::<Vec<_>>())
            .map_err(|e| e.to_string())
    }

    pub(super) fn tool_task_comment(
        &self,
        who: &Identity,
        args: &Value,
    ) -> std::result::Result<String, String> {
        let (floor, _team, actor) = self.task_actor(who)?;
        let id = parse_task_ref(args.get("task"))?;
        let text = clean_text(
            "comment",
            text_arg(args, "text")?,
            TASK_TEXT_MAX_CHARS,
            true,
        )?;
        self.comment_task_as(&floor, &actor, id, &text)?;
        Ok(format!("The comment is added to task T-{id}."))
    }

    // ---- the rules, for an employee and for the operator ----

    pub(super) fn read_task(
        &self,
        floor: &FloorHandle,
        id: i64,
    ) -> Result<(TaskRecord, Vec<TaskEventRecord>)> {
        let store = locked(&floor.store);
        let task = store
            .db()
            .get_task(id)?
            .ok_or(TowerError::Store(StoreError::UnknownTask(id)))?;
        let events = store.db().task_events(id)?;
        Ok((task, events))
    }

    fn create_task_as(
        &self,
        floor: &Arc<FloorHandle>,
        actor: &Actor,
        title: &str,
        description: &str,
        parent: Option<i64>,
    ) -> std::result::Result<TaskRecord, String> {
        let task = locked(&floor.store)
            .db()
            .create_task(title, description, parent, &actor.slug, now_ms())
            .map_err(task_error)?;
        self.notifier.tasks_changed(&floor.path);
        Ok(task)
    }

    fn comment_task_as(
        &self,
        floor: &Arc<FloorHandle>,
        actor: &Actor,
        id: i64,
        text: &str,
    ) -> std::result::Result<TaskRecord, String> {
        let task = locked(&floor.store)
            .db()
            .comment_task(id, &actor.slug, text, now_ms())
            .map_err(task_error)?;
        self.notifier.tasks_changed(&floor.path);
        Ok(task)
    }

    /// `task_assign` for an employee or the operator. Returns the text for the caller.
    fn assign_task_as(
        &self,
        floor: &Arc<FloorHandle>,
        team: &[Employee],
        actor: &Actor,
        id: i64,
        to: &str,
    ) -> std::result::Result<String, String> {
        let Some(target) = team.iter().find(|e| e.slug == to && e.slug != SETUP_SLUG) else {
            return Err(format!(
                "There is no employee with the slug \"{to}\" on this floor. Use the tool org_chart to see the team."
            ));
        };
        if !actor.operator {
            if actor.slug == to {
                return Err("You cannot give a task to yourself with task_assign. Set the status with task_update.".to_owned());
            }
            if !may_start(&actor.slug, to, team) {
                return Err(format!(
                    "You may not give a task to {} (slug \"{to}\"). You may give a task to an employee that is below you in the team, and the ceo and the pm may give a task to every employee.",
                    target.name
                ));
            }
        }
        if locked(&floor.store)
            .db()
            .get_task(id)
            .map_err(db_error)?
            .is_none()
        {
            return Err(StoreError::UnknownTask(id).to_string());
        }
        // A start of a session costs money and tokens. Count it before anything is stored. The
        // operator's own starts do not count.
        let needs_start = self.live_of(&floor.path, to).is_none();
        if needs_start && !actor.operator {
            self.check_start_slot(&floor.path)?;
        }
        let from = actor.slug.clone();
        let existing = locked(&floor.store)
            .db()
            .get_task(id)
            .map_err(db_error)?
            .ok_or_else(|| StoreError::UnknownTask(id).to_string())?;
        let (new_worktree, no_worktree_reason) = self.prepare_worktree(floor, target, &existing)?;
        let floor_path = floor.path.clone();
        let stored = locked(&floor.store).db().assign_task(
            id,
            to,
            &actor.slug,
            now_ms(),
            new_worktree.as_ref(),
            |after| {
                let workplace = if after.worktree.is_some() {
                    Workplace::Worktree(after)
                } else if let Some(why) = no_worktree_reason {
                    Workplace::Direct { why }
                } else {
                    Workplace::NoEdit
                };
                NewTaskMessage {
                    from_slug: from.clone(),
                    to_slug: to.to_owned(),
                    text: assign_body(after),
                    kind: KIND_TASK_ASSIGN,
                    orders: assign_orders(&floor_path, after.id, &from, &workplace),
                }
            },
        );
        let (task, message_id) = match stored {
            Ok(done) => done,
            Err(error) => {
                // The worktree was made for this assignment only. Take it away again.
                if let Some(made) = &new_worktree {
                    let _guard = locked(&floor.git);
                    let _ = gitwork::discard_worktree(
                        &floor.path,
                        id,
                        Some(&made.path),
                        Some(&made.branch),
                    );
                }
                return Err(task_error(error));
            }
        };
        // From here on, the task and the message are stored: no `Err` any more.
        self.notifier.tasks_changed(&floor.path);
        let subject = format!("The message for task T-{}", task.id);
        let sent = Sent {
            by_slug: &actor.slug,
            by_name: &actor.name,
            subject: &subject,
            answer_tail: false,
            slot: if actor.operator {
                Slot::Free
            } else {
                Slot::Taken
            },
        };
        let report = self.send_stored(floor, team, to, message_id, true, &sent);
        Ok(format!(
            "Task T-{} is given to {}. {report}",
            task.id, target.name
        ))
    }

    /// The worktree for a task that is given to `target`. Returns the record of a new worktree
    /// (`None` when the task has one already, or none is made) and, when `target` edits files and
    /// no worktree exists, the reason. `Err` is a git failure: nothing is stored then.
    fn prepare_worktree(
        &self,
        floor: &Arc<FloorHandle>,
        target: &Employee,
        task: &TaskRecord,
    ) -> std::result::Result<(Option<WorktreeRecord>, Option<&'static str>), String> {
        if task.worktree.is_some() || target.permission != PermissionPolicy::EditInWorktree {
            return Ok((None, None));
        }
        let _git = locked(&floor.git);
        match gitwork::repo_state(&floor.path) {
            RepoState::Ready { branch, head } => {
                let made = gitwork::create_worktree(&floor.path, task.id, &task.title, &head)
                    .map_err(|e| {
                        format!(
                            "The app could not make the git worktree for task T-{}: {e}. The task is not given.",
                            task.id
                        )
                    })?;
                Ok((
                    Some(WorktreeRecord {
                        branch: made.branch,
                        base_branch: branch,
                        base_commit: head,
                        path: gitwork::worktree_relative(task.id),
                    }),
                    None,
                ))
            }
            other => Ok((None, Some(other.why_no_worktree()))),
        }
    }

    /// May `actor` change the status of the task? (The assignee, the creator, the `ceo`, the
    /// `pm` and the operator.)
    fn may_change_status(actor: &Actor, task: &TaskRecord) -> bool {
        actor.operator
            || task.assignee.as_deref() == Some(actor.slug.as_str())
            || task.creator == actor.slug
            || actor.slug == "ceo"
            || actor.slug == "pm"
    }

    /// The task goes to `review`: commit what the worktree has. Returns the line for the note of
    /// the status event. `Err` is the text for the caller. Nothing changes then.
    fn snapshot_for_review(
        &self,
        floor: &Arc<FloorHandle>,
        team: &[Employee],
        task: &TaskRecord,
    ) -> std::result::Result<String, String> {
        let Some(relative) = &task.worktree else {
            return Ok(String::new());
        };
        let assignee = task
            .assignee
            .as_deref()
            .and_then(|slug| team.iter().find(|e| e.slug == slug));
        let author = gitwork::Author {
            slug: assignee.map_or("employee", |e| e.slug.as_str()),
            name: assignee.map_or("Employee", |e| e.name.as_str()),
            role: assignee.map_or("Employee", |e| e.role.as_str()),
        };
        let path = gitwork::worktree_path(&floor.path, task.id, relative)?;
        let _git = locked(&floor.git);
        let result = gitwork::snapshot(&path, task.id, &task.title, &author).map_err(|e| {
            format!(
                "The app could not commit the changes of the worktree of task T-{}: {e}. The status is not changed.",
                task.id
            )
        })?;
        Ok(match result {
            Snapshot::NoChanges => "[app] No changes in the worktree: no commit.".to_owned(),
            Snapshot::Committed { commit } => format!(
                "[app] The app committed the worktree on {} ({}).",
                task.branch.as_deref().unwrap_or("the task branch"),
                &commit[..commit.len().min(12)]
            ),
        })
    }

    /// `task_update` for an employee or the operator.
    #[allow(clippy::too_many_arguments)]
    fn update_task_as(
        &self,
        floor: &Arc<FloorHandle>,
        team: &[Employee],
        actor: &Actor,
        id: i64,
        status: TaskStatus,
        note: Option<&str>,
        report: Option<&str>,
    ) -> std::result::Result<TaskRecord, String> {
        let note = match note {
            Some(text) => Some(clean_text("note", text, TASK_TEXT_MAX_CHARS, false)?),
            None => None,
        };
        let report = match report.filter(|r| !r.trim().is_empty()) {
            Some(path) => Some(check_report(
                &floor.path,
                path,
                matches!(status, TaskStatus::Review | TaskStatus::Done),
            )?),
            None => None,
        };
        // The status `review` of a task with a worktree: the app commits the worktree first.
        // The rule of the guard is checked before, so a caller that may not change the task
        // makes no commit.
        let mut note = note;
        if status == TaskStatus::Review {
            let current = locked(&floor.store).db().get_task(id).map_err(db_error)?;
            if let Some(task) = current.filter(|t| t.worktree.is_some()) {
                if !Self::may_change_status(actor, &task) {
                    return Err("Only the assignee, the creator, the ceo and the pm may change the status of this task.".to_owned());
                }
                let line = self.snapshot_for_review(floor, team, &task)?;
                note = Some(match note.as_deref().filter(|n| !n.trim().is_empty()) {
                    Some(text) => format!("{text}\n{line}"),
                    None => line,
                });
            }
        }
        let change = TaskChange {
            status,
            note: note.as_deref(),
            report: report.as_deref(),
            author: &actor.slug,
        };
        let floor_path = floor.path.clone();
        let outcome = locked(&floor.store)
            .db()
            .update_task(
                id,
                &change,
                now_ms(),
                |before| {
                    if Self::may_change_status(actor, before) {
                        Ok(())
                    } else {
                        Err("Only the assignee, the creator, the ceo and the pm may change the status of this task.".to_owned())
                    }
                },
                |before, after| {
                    if before.status == after.status {
                        return None;
                    }
                    // The person who gave the task, or the creator when nobody gave it.
                    let to = after.assigner.as_deref().unwrap_or(&after.creator);
                    if to == actor.slug || to == OPERATOR || !team.iter().any(|e| e.slug == to) {
                        return None;
                    }
                    // A task in review with a worktree: the manager gets the merge steps.
                    let orders = (after.status == TaskStatus::Review
                        && after.worktree.is_some()
                        && after.branch.is_some())
                    .then(|| {
                        let may_merge = to == "pm" || to == "ceo";
                        review_orders(&floor_path, after, may_merge)
                    });
                    Some(NewTaskMessage {
                        from_slug: actor.slug.clone(),
                        to_slug: to.to_owned(),
                        text: status_body(after, change.note),
                        kind: KIND_TASK_STATUS,
                        orders,
                    })
                },
            )
            .map_err(task_error)?;
        self.notifier.tasks_changed(&floor.path);
        if let Some(message_id) = outcome.message_id {
            let to = outcome
                .after
                .assigner
                .clone()
                .unwrap_or_else(|| outcome.after.creator.clone());
            let may = actor.operator || may_start(&actor.slug, &to, team);
            let subject = format!("The status message for task T-{id}");
            let sent = Sent {
                by_slug: &actor.slug,
                by_name: &actor.name,
                subject: &subject,
                answer_tail: false,
                slot: if actor.operator {
                    Slot::Free
                } else {
                    Slot::Take
                },
            };
            // The result text is for nobody: the status change is done in each case.
            let _ = self.send_stored(floor, team, &to, message_id, may, &sent);
        }
        Ok(outcome.after)
    }

    // ---- the commands of the operator ----

    /// The tasks of the floor, oldest first.
    pub fn task_list(&self, path: &Path) -> Result<Vec<TaskRecord>> {
        let floor = self.floor_handle(path)?;
        let tasks = locked(&floor.store).db().list_tasks(None, None)?;
        Ok(tasks)
    }

    /// One task with its events.
    pub fn task_get(&self, path: &Path, id: i64) -> Result<(TaskRecord, Vec<TaskEventRecord>)> {
        let floor = self.floor_handle(path)?;
        self.read_task(&floor, id)
    }

    /// The operator makes a task. The creator is `operator`.
    pub fn task_create(&self, path: &Path, title: &str, description: &str) -> Result<TaskRecord> {
        let floor = self.floor_handle(path)?;
        let title = clean_title(title).map_err(other)?;
        let description =
            clean_text("description", description, TASK_TEXT_MAX_CHARS, false).map_err(other)?;
        self.create_task_as(&floor, &Actor::operator(), &title, &description, None)
            .map_err(other)
    }

    /// The operator gives a task to an employee. The employee gets the message
    /// `[task T-12 from the operator]`, and the app starts its session if there is none.
    pub fn task_assign(&self, path: &Path, id: i64, slug: &str) -> Result<TaskRecord> {
        let floor = self.floor_handle(path)?;
        let team = self.team_of(&floor).map_err(other)?;
        self.assign_task_as(&floor, &team, &Actor::operator(), id, slug)
            .map_err(other)?;
        self.read_task(&floor, id).map(|(task, _)| task)
    }

    /// The operator sets the status, with a note. The person who gave the task gets a message
    /// when the status changes.
    pub fn task_update(
        &self,
        path: &Path,
        id: i64,
        status: TaskStatus,
        note: Option<&str>,
    ) -> Result<TaskRecord> {
        let floor = self.floor_handle(path)?;
        let team = self.team_of(&floor).map_err(other)?;
        self.update_task_as(&floor, &team, &Actor::operator(), id, status, note, None)
            .map_err(other)
    }

    /// The text of the report file of a task, or `None` when the task has no report path. At most
    /// `REPORT_READ_MAX_BYTES` are read. The path is checked again: a link that leads out of the
    /// reports folder is refused.
    pub fn task_report(&self, path: &Path, id: i64) -> Result<Option<String>> {
        let floor = self.floor_handle(path)?;
        let (task, _) = self.read_task(&floor, id)?;
        let Some(report) = task.report else {
            return Ok(None);
        };
        let file = check_report(&floor.path, &report, true).map_err(other)?;
        let bytes = {
            use std::io::Read;
            let handle = fs::File::open(floor.path.join(&file))
                .map_err(|e| other(format!("cannot read the report file {file}: {e}")))?;
            let mut buffer = Vec::new();
            handle
                .take(REPORT_READ_MAX_BYTES as u64 + 1)
                .read_to_end(&mut buffer)
                .map_err(|e| other(format!("cannot read the report file {file}: {e}")))?;
            buffer
        };
        let truncated = bytes.len() > REPORT_READ_MAX_BYTES;
        let mut text =
            String::from_utf8_lossy(&bytes[..bytes.len().min(REPORT_READ_MAX_BYTES)]).into_owned();
        if truncated {
            text.push_str("\n[The app shows the first part of this file only.]\n");
        }
        Ok(Some(text))
    }
}

/// A store error as the text for the caller.
pub(super) fn task_error(error: StoreError) -> String {
    match error {
        StoreError::UnknownTask(_) | StoreError::Rule(_) => error.to_string(),
        other => db_error(other),
    }
}

fn task_json(task: &TaskRecord) -> Value {
    json!({
        "id": format!("T-{}", task.id),
        "title": task.title,
        "description": task.description,
        "parent": task.parent.map(|p| format!("T-{p}")),
        "creator": task.creator,
        "assignee": task.assignee,
        "assigned_by": task.assigner,
        "status": task.status.as_str(),
        "report": task.report,
        "branch": task.branch,
        "base_branch": task.base_branch,
        "worktree": task.worktree,
        "created_at": task.created_at,
        "updated_at": task.updated_at,
    })
}

fn event_json(event: &TaskEventRecord) -> Value {
    json!({
        "at": event.at,
        "author": event.author,
        "kind": event.kind,
        "value": event.value,
        "text": event.text,
    })
}
