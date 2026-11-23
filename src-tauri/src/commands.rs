//! The Tauri commands of `docs/ipc-m1.md`. Each one is thin: convert the arguments, call the
//! `Tower`, convert the result, and turn an error into its text.
//!
//! Threads. A `Tower` call can block (a file, the database, a login shell for the program
//! lookup, the start of a process, the stop of a process). So each command is `async` and runs
//! the call with `spawn_blocking`, and the main thread never waits for it. Two commands are not
//! like that, because the ORDER of their calls matters:
//!
//! - `pty_write` is a sync command. It only decodes the bytes and puts them in a queue of the
//!   session; a worker thread of the session does the write. The queue keeps the order of the
//!   keys. Spawned tasks could run in another order.
//! - `pty_resize` is a sync command (it runs on the main thread, in the order of the calls,
//!   so the last size wins). It is one ioctl.

use crate::views::{
    AttachResult, Cli, Employee, FloorDefaults, FloorEntry, FloorView, ModelCatalog, OutputMessage,
    PermissionsSaved, ProgramTestResult, Programs, ResumeProgress, SandboxProbe, SessionView,
    Settings, TaskDetail, TaskDiff, TaskMerge, TaskStatus, TaskView,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use tauri::State;
use tauri::ipc::Channel;
use tower_core::app::{CodexSandboxMode, OutputSink, Tower, TowerError};

/// The Tower in the managed state of Tauri.
pub type Shared = Arc<Tower>;

/// The queues of the PTY writes, one for each session.
#[derive(Default)]
pub struct WriteQueues {
    senders: Mutex<HashMap<String, Sender<Vec<u8>>>>,
}

impl WriteQueues {
    /// Put bytes in the queue of the session, in order. The queue is made at the first use.
    fn push(&self, tower: &Shared, session_id: &str, bytes: Vec<u8>) {
        let mut senders = self.senders.lock().unwrap_or_else(PoisonError::into_inner);
        let mut bytes = Some(bytes);
        // A queue whose worker has ended (the session ended) is replaced once. The new worker
        // gets the error at once and ends again.
        for _ in 0..2 {
            let sender = senders
                .entry(session_id.to_owned())
                .or_insert_with(|| spawn_writer(Arc::clone(tower), session_id.to_owned()));
            match sender.send(bytes.take().unwrap_or_default()) {
                Ok(()) => return,
                Err(failed) => {
                    bytes = Some(failed.0);
                    senders.remove(session_id);
                }
            }
        }
    }
}

fn spawn_writer(tower: Shared, session_id: String) -> Sender<Vec<u8>> {
    let (sender, receiver) = channel::<Vec<u8>>();
    let spawned = std::thread::Builder::new()
        .name("pty-write-queue".into())
        .spawn(move || {
            for bytes in receiver {
                if tower.pty_write(&session_id, &bytes).is_err() {
                    // The session has ended, or it is not known. The status shows it.
                    break;
                }
            }
        });
    if let Err(error) = spawned {
        eprintln!("agent-tower: cannot start a write queue: {error}");
    }
    sender
}

/// The path as typed, with no spaces at the ends. The Tower expands `~` and `~/x` to the home
/// folder in `floor_open` and `floor_create` (`docs/ipc-final.md`), and it checks the path there.
fn path_of(text: &str) -> PathBuf {
    PathBuf::from(text.trim())
}

fn error_text(error: TowerError) -> String {
    error.to_string()
}

/// Run a Tower call on a thread of the blocking pool.
async fn blocking<T, F>(tower: &Shared, work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Tower) -> Result<T, TowerError> + Send + 'static,
{
    let tower = Arc::clone(tower);
    tauri::async_runtime::spawn_blocking(move || work(&tower).map_err(error_text))
        .await
        .map_err(|error| format!("the task did not finish: {error}"))?
}

#[tauri::command]
pub async fn floor_list(tower: State<'_, Shared>) -> Result<Vec<FloorEntry>, String> {
    blocking(&tower, |t| {
        Ok(t.floor_list()?.into_iter().map(FloorEntry::from).collect())
    })
    .await
}

#[tauri::command]
pub async fn floor_open(tower: State<'_, Shared>, path: String) -> Result<FloorView, String> {
    blocking(&tower, move |t| {
        t.floor_open(&path_of(&path)).map(FloorView::from)
    })
    .await
}

#[tauri::command]
pub async fn floor_create(
    tower: State<'_, Shared>,
    path: String,
    name: String,
) -> Result<FloorView, String> {
    blocking(&tower, move |t| {
        t.floor_create(&path_of(&path), &name).map(FloorView::from)
    })
    .await
}

#[tauri::command]
pub async fn floor_forget(tower: State<'_, Shared>, path: String) -> Result<(), String> {
    blocking(&tower, move |t| t.floor_forget(&path_of(&path))).await
}

#[tauri::command]
pub async fn employee_save(
    tower: State<'_, Shared>,
    path: String,
    employee: Employee,
) -> Result<FloorView, String> {
    blocking(&tower, move |t| {
        t.employee_save(&path_of(&path), employee.into())
            .map(FloorView::from)
    })
    .await
}

#[tauri::command]
pub async fn employee_remove(
    tower: State<'_, Shared>,
    path: String,
    slug: String,
) -> Result<FloorView, String> {
    blocking(&tower, move |t| {
        t.employee_remove(&path_of(&path), &slug)
            .map(FloorView::from)
    })
    .await
}

#[tauri::command]
pub async fn session_start(
    tower: State<'_, Shared>,
    path: String,
    slug: String,
    first_message: Option<String>,
    rows: u16,
    cols: u16,
) -> Result<SessionView, String> {
    blocking(&tower, move |t| {
        t.session_start(&path_of(&path), &slug, first_message, rows, cols)
            .map(SessionView::from)
    })
    .await
}

#[tauri::command]
pub async fn session_resume(
    tower: State<'_, Shared>,
    path: String,
    slug: String,
    message: Option<String>,
    rows: u16,
    cols: u16,
) -> Result<SessionView, String> {
    blocking(&tower, move |t| {
        t.session_resume(&path_of(&path), &slug, message, rows, cols)
            .map(SessionView::from)
    })
    .await
}

#[tauri::command]
pub async fn setup_start(
    tower: State<'_, Shared>,
    path: String,
    cli: Cli,
    rows: u16,
    cols: u16,
) -> Result<SessionView, String> {
    blocking(&tower, move |t| {
        t.setup_start(&path_of(&path), cli.into(), rows, cols)
            .map(SessionView::from)
    })
    .await
}

/// Base64 of `logo.png`, or `null` when there is no file.
#[tauri::command]
pub async fn floor_logo(tower: State<'_, Shared>, path: String) -> Result<Option<String>, String> {
    blocking(&tower, move |t| {
        Ok(t.floor_logo(&path_of(&path))?
            .map(|bytes| STANDARD.encode(bytes)))
    })
    .await
}

/// Returns at once. The generation runs in the background (see `Tower::logo_generate`).
#[tauri::command]
pub async fn logo_generate(
    tower: State<'_, Shared>,
    path: String,
    prompt: Option<String>,
) -> Result<(), String> {
    blocking(&tower, move |t| t.logo_generate(&path_of(&path), prompt)).await
}

#[tauri::command]
pub async fn settings_get(tower: State<'_, Shared>) -> Result<Settings, String> {
    blocking(&tower, |t| Ok(Settings::from(t.settings_get()))).await
}

#[tauri::command]
pub async fn settings_set(
    tower: State<'_, Shared>,
    openai_key: Option<String>,
    image_model: Option<String>,
    notify_on_question: Option<bool>,
    codex_sandbox: Option<bool>,
    codex_sandbox_mode: Option<String>,
    claude_auto_mode: Option<bool>,
) -> Result<Settings, String> {
    blocking(&tower, move |t| {
        // `codexSandboxMode` (`auto`, `on` or `off`) is the new argument. The old boolean
        // `codexSandbox` stays: `true` is `on` and `false` is `off`. The new argument wins when
        // a call has both.
        let mode = match codex_sandbox_mode.as_deref() {
            Some(word) => Some(CodexSandboxMode::parse(word).ok_or_else(|| {
                TowerError::Refused(format!(
                    "codexSandboxMode must be \"auto\", \"on\" or \"off\", not \"{word}\""
                ))
            })?),
            None => codex_sandbox.map(|on| {
                if on {
                    CodexSandboxMode::On
                } else {
                    CodexSandboxMode::Off
                }
            }),
        };
        // The sandbox setting goes first: a refusal (the probe does not say `works`) changes
        // nothing else.
        t.settings_set_codex_mode(mode)?;
        t.settings_set(openai_key, image_model)?;
        t.settings_set_claude(claude_auto_mode)?;
        t.settings_set_interface(notify_on_question)
            .map(Settings::from)
    })
    .await
}

/// Test the Codex sandbox: run `bwrap --unshare-user --ro-bind / / true` with the environment of a
/// session. It starts no Codex session. The result is `works`, `fails` (with the first line of
/// the error) or `missing`.
#[tauri::command]
pub async fn codex_sandbox_probe(tower: State<'_, Shared>) -> Result<SandboxProbe, String> {
    blocking(&tower, |t| t.codex_sandbox_probe().map(SandboxProbe::from)).await
}

/// The model lists. The Codex file is read again on each call.
#[tauri::command]
pub async fn models_get(tower: State<'_, Shared>) -> Result<ModelCatalog, String> {
    blocking(&tower, |t| Ok(ModelCatalog::from(t.models_get()))).await
}

/// `kind` is `claude`, `codex` or `image`. It replaces the custom models of that kind.
#[tauri::command]
pub async fn models_set_custom(
    tower: State<'_, Shared>,
    kind: String,
    models: Vec<String>,
) -> Result<ModelCatalog, String> {
    blocking(&tower, move |t| {
        t.models_set_custom(&kind, &models).map(ModelCatalog::from)
    })
    .await
}

/// Write `[defaults]` of `floor.toml`.
#[tauri::command]
pub async fn floor_defaults_set(
    tower: State<'_, Shared>,
    path: String,
    defaults: FloorDefaults,
) -> Result<FloorView, String> {
    blocking(&tower, move |t| {
        t.floor_defaults_set(&path_of(&path), defaults.into())
            .map(FloorView::from)
    })
    .await
}

#[tauri::command]
pub async fn session_stop(tower: State<'_, Shared>, session_id: String) -> Result<(), String> {
    blocking(&tower, move |t| t.session_stop(&session_id)).await
}

/// The sink of the output: base64 of each chunk, on the channel.
struct ChannelSink(Channel<OutputMessage>);

impl OutputSink for ChannelSink {
    fn output(&self, bytes: &[u8]) {
        // A closed channel (the window is gone) is not an error for the session.
        let _ = self.0.send(OutputMessage {
            data: STANDARD.encode(bytes),
        });
    }
}

#[tauri::command]
pub async fn session_attach(
    tower: State<'_, Shared>,
    session_id: String,
    channel: Channel<OutputMessage>,
) -> Result<AttachResult, String> {
    blocking(&tower, move |t| {
        let scrollback = t.session_attach(&session_id, Arc::new(ChannelSink(channel)))?;
        Ok(AttachResult {
            scrollback: STANDARD.encode(scrollback),
        })
    })
    .await
}

#[tauri::command]
pub fn pty_write(
    tower: State<'_, Shared>,
    queues: State<'_, WriteQueues>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    let bytes = STANDARD
        .decode(data.as_bytes())
        .map_err(|error| format!("the data is not valid base64: {error}"))?;
    queues.push(&tower, &session_id, bytes);
    Ok(())
}

#[tauri::command]
pub fn pty_resize(
    tower: State<'_, Shared>,
    session_id: String,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    tower
        .pty_resize(&session_id, rows, cols)
        .map_err(error_text)
}

#[tauri::command]
pub async fn clear_draft(tower: State<'_, Shared>, session_id: String) -> Result<(), String> {
    blocking(&tower, move |t| t.clear_draft(&session_id)).await
}

#[tauri::command]
pub async fn debug_paste(
    tower: State<'_, Shared>,
    session_id: String,
    text: String,
) -> Result<(), String> {
    blocking(&tower, move |t| t.debug_paste(&session_id, &text)).await
}

#[tauri::command]
pub async fn programs_get(tower: State<'_, Shared>) -> Result<Programs, String> {
    blocking(&tower, |t| Ok(Programs::from(t.programs_get()))).await
}

#[tauri::command]
pub async fn programs_set(
    tower: State<'_, Shared>,
    name: String,
    path: String,
) -> Result<Programs, String> {
    blocking(&tower, move |t| {
        t.programs_set(&name, path.trim()).map(Programs::from)
    })
    .await
}

/// Run `<program> --version` with the environment of a session. It starts no model session.
/// `name` is `claude`, `codex` or `node`.
#[tauri::command]
pub async fn program_test(
    tower: State<'_, Shared>,
    name: String,
) -> Result<ProgramTestResult, String> {
    blocking(&tower, move |t| {
        t.program_test(&name).map(ProgramTestResult::from)
    })
    .await
}

/// The tasks of the floor, oldest first (`docs/ipc-m2.md`).
#[tauri::command]
pub async fn task_list(tower: State<'_, Shared>, path: String) -> Result<Vec<TaskView>, String> {
    blocking(&tower, move |t| {
        Ok(t.task_list(&path_of(&path))?
            .into_iter()
            .map(TaskView::from)
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn task_get(
    tower: State<'_, Shared>,
    path: String,
    id: i64,
) -> Result<TaskDetail, String> {
    blocking(&tower, move |t| {
        let (task, events) = t.task_get(&path_of(&path), id)?;
        Ok(TaskDetail {
            task: task.into(),
            events: events.into_iter().map(Into::into).collect(),
        })
    })
    .await
}

/// The operator makes a task. The creator is `operator`.
#[tauri::command]
pub async fn task_create(
    tower: State<'_, Shared>,
    path: String,
    title: String,
    description: String,
) -> Result<TaskView, String> {
    blocking(&tower, move |t| {
        t.task_create(&path_of(&path), &title, &description)
            .map(TaskView::from)
    })
    .await
}

/// The operator gives a task to an employee. The employee gets a message, and the app starts its
/// session if there is none.
#[tauri::command]
pub async fn task_assign(
    tower: State<'_, Shared>,
    path: String,
    id: i64,
    slug: String,
) -> Result<TaskView, String> {
    blocking(&tower, move |t| {
        t.task_assign(&path_of(&path), id, &slug)
            .map(TaskView::from)
    })
    .await
}

#[tauri::command]
pub async fn task_update(
    tower: State<'_, Shared>,
    path: String,
    id: i64,
    status: TaskStatus,
    note: Option<String>,
) -> Result<TaskView, String> {
    blocking(&tower, move |t| {
        t.task_update(&path_of(&path), id, status, note.as_deref())
            .map(TaskView::from)
    })
    .await
}

/// The text of the report file of a task, or `null` when the task has no report.
#[tauri::command]
pub async fn task_report(
    tower: State<'_, Shared>,
    path: String,
    id: i64,
) -> Result<Option<String>, String> {
    blocking(&tower, move |t| t.task_report(&path_of(&path), id)).await
}

/// The operator merges a task (`docs/ipc-m3.md`). A merge that is blocked is a result with
/// `merged: false`, not an error.
#[tauri::command]
pub async fn task_merge(
    tower: State<'_, Shared>,
    path: String,
    id: i64,
) -> Result<TaskMerge, String> {
    blocking(&tower, move |t| {
        t.task_merge(&path_of(&path), id).map(TaskMerge::from)
    })
    .await
}

/// The changes of a task branch: the diffstat and the patch (cut at 60000 bytes).
#[tauri::command]
pub async fn task_diff(
    tower: State<'_, Shared>,
    path: String,
    id: i64,
) -> Result<TaskDiff, String> {
    blocking(&tower, move |t| {
        t.task_diff(&path_of(&path), id).map(TaskDiff::from)
    })
    .await
}

/// The operator discards a task that is not done: worktree and branch go, status `blocked`.
#[tauri::command]
pub async fn task_discard(
    tower: State<'_, Shared>,
    path: String,
    id: i64,
) -> Result<TaskView, String> {
    blocking(&tower, move |t| {
        t.task_discard(&path_of(&path), id).map(TaskView::from)
    })
    .await
}

/// The rules of the floor for commands with no question, from the app config (`docs/ipc-m3.md`).
#[tauri::command]
pub async fn floor_permissions_get(
    tower: State<'_, Shared>,
    path: String,
) -> Result<Vec<String>, String> {
    blocking(&tower, move |t| t.floor_permissions_get(&path_of(&path))).await
}

/// Write the rules of the floor in the app config. A bad rule refuses the whole call. The result
/// has the stored rules and a warning for each rule that runs the code of the project.
#[tauri::command]
pub async fn floor_permissions_set(
    tower: State<'_, Shared>,
    path: String,
    allow: Vec<String>,
) -> Result<PermissionsSaved, String> {
    blocking(&tower, move |t| {
        t.floor_permissions_set(&path_of(&path), allow)
            .map(PermissionsSaved::from)
    })
    .await
}

/// The rules of the old `[permissions]` table of `floor.toml` that the operator can take over.
#[tauri::command]
pub async fn floor_permissions_proposed(
    tower: State<'_, Shared>,
    path: String,
) -> Result<Vec<String>, String> {
    blocking(&tower, move |t| {
        t.floor_permissions_proposed(&path_of(&path))
    })
    .await
}

/// Resume the employees of `FloorView.resumable` in the background (`docs/ipc-m3.md`). It
/// returns at once with the progress at the start.
#[tauri::command]
pub async fn team_resume(tower: State<'_, Shared>, path: String) -> Result<ResumeProgress, String> {
    blocking(&tower, move |t| {
        t.team_resume(&path_of(&path)).map(ResumeProgress::from)
    })
    .await
}

/// Clear a request of an employee for the operator (`docs/ipc-final.md`).
#[tauri::command]
pub async fn operator_request_clear(
    tower: State<'_, Shared>,
    path: String,
    id: u64,
) -> Result<(), String> {
    blocking(&tower, move |t| {
        t.operator_request_clear(&path_of(&path), id)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_is_trimmed_and_the_tilde_is_left_to_the_tower() {
        // The Tower expands `~` (it knows the home folder of its environment), so a test can
        // set a fake home. See `floor_open` in `crates/core/src/app/mod.rs`.
        assert_eq!(path_of("~/x/y"), PathBuf::from("~/x/y"));
        assert_eq!(path_of("  /a/b "), PathBuf::from("/a/b"));
        assert_eq!(path_of("rel"), PathBuf::from("rel"));
    }
}
