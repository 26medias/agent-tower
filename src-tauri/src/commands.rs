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
    AttachResult, Cli, Employee, FloorEntry, FloorView, OutputMessage, Programs, SessionView,
    Settings,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use tauri::State;
use tauri::ipc::Channel;
use tower_core::app::{OutputSink, Tower, TowerError};

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

/// `~` and `~/x` mean the home folder (the UI has a text field for the path).
fn path_of(text: &str) -> PathBuf {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(text)
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
) -> Result<Settings, String> {
    blocking(&tower, move |t| {
        t.settings_set(openai_key, image_model).map(Settings::from)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilde_means_home() {
        // SAFETY of the test: it reads `HOME` only.
        let home = std::env::var_os("HOME").map(PathBuf::from);
        if let Some(home) = home {
            assert_eq!(path_of("~/x/y"), home.join("x/y"));
        }
        assert_eq!(path_of("  /a/b "), PathBuf::from("/a/b"));
        assert_eq!(path_of("rel"), PathBuf::from("rel"));
    }
}
