//! One live session of the app: the `Session`, its process handle, the output buffer, and the
//! three pump threads that connect it to the database and to the observers.
//!
//! # Threads of one session (all std threads, all end by themselves)
//!
//! - **hooks**: reads the hook events of this session from the `HookRouter`, stores the CLI
//!   session ID of a Codex session from the first event, and forwards each event to the
//!   `Session` (which has its own hook pump). It ends when the router drops the route.
//! - **status**: reads the status changes of the `Session`. It appends one status event to the
//!   database for each change. On `stopped` it sets the end time, marks the session as exited,
//!   removes the hook route and ends.
//! - **output**: reads the output chunks of the `Session`, keeps the latest bytes in a ring
//!   buffer, and gives each chunk to the attached sink. It ends after the session has exited
//!   and the queue is empty.
//!
//! # The attach algorithm (no gap, no duplicate)
//!
//! `OutBuf` holds the ring buffer (the latest `SCROLLBACK_LIMIT` bytes of output) and the sink.
//! The output thread takes the `out` mutex for each chunk, appends the chunk to the ring AND reads
//! the current sink in the same critical section. `attach` takes the same mutex, copies the ring
//! (this is the scrollback) AND installs the new sink in one critical section. So each chunk is
//! in exactly one of two places: the scrollback (it was appended before the attach), or the new
//! sink (it was appended after). The sink is called after the mutex is released, by the output
//! thread only, so one sink sees its chunks in order and a slow sink cannot block `attach`.
//! The old sink can still get a chunk that was appended before the attach. That is harmless: it
//! is a replaced sink.
//!
//! The ring buffer is used, and not the log file, for a session that this process runs: the log
//! is written by the PTY reader BEFORE the chunk goes through the channels (`pty.rs`, read_loop),
//! so the log is ahead of the stream by an unknown amount, and the rotation of the log makes
//! byte offsets of the file differ from byte offsets of the output. The log file has the same
//! bytes, and it serves the sessions of an earlier run of the app (`read_log_tail`).

use super::{Observer, OutputSink, SessionView};
use crate::adapter::cli_session_id_from_event;
use crate::session::{HookRouter, OutputChunk, Session, SessionHandle, StatusChange};
use crate::status::Status;
use crate::store::Database;
use serde_json::Value;
use std::collections::VecDeque;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The most output that `attach` returns as scrollback (the contract says 256 KB).
pub const SCROLLBACK_LIMIT: usize = 256 * 1024;

/// How often the output thread checks whether the session has exited.
const OUTPUT_POLL: Duration = Duration::from_millis(100);

pub(crate) fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Unix milliseconds.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn warn(message: &str) {
    eprintln!("agent-tower: {message}");
}

/// The observer, shared by the Tower and by the threads of the sessions.
#[derive(Default)]
pub(crate) struct Notifier {
    observer: Mutex<Option<Arc<dyn Observer>>>,
}

impl Notifier {
    pub fn set(&self, observer: Arc<dyn Observer>) {
        *locked(&self.observer) = Some(observer);
    }

    fn get(&self) -> Option<Arc<dyn Observer>> {
        locked(&self.observer).clone()
    }

    pub fn session_changed(&self, path: &Path, view: &SessionView) {
        if let Some(observer) = self.get() {
            observer.session_changed(path, view);
        }
    }

    pub fn floor_changed(&self, path: &Path) {
        if let Some(observer) = self.get() {
            observer.floor_changed(path);
        }
    }
}

struct OutBuf {
    ring: VecDeque<u8>,
    sink: Option<Arc<dyn OutputSink>>,
}

pub(crate) struct Live {
    pub id: String,
    /// The key of the floor (a real path).
    pub path: PathBuf,
    pub slug: String,
    pub session: Session,
    handle: Mutex<Option<SessionHandle>>,
    out: Mutex<OutBuf>,
    /// The last view that was sent to the observer. The mutex also orders the notifications.
    last_view: Mutex<Option<SessionView>>,
    exited: AtomicBool,
    /// The CLI session has had at least one turn (a `working` status). `claude --resume` fails
    /// for a session with no turn (D3). A resume of a session that had a turn starts as `true`.
    had_turn: AtomicBool,
    /// The status as the status thread has seen it, change by change. Views use this and not
    /// `Session::status()`, so each change shows in the views, in order, and a thread that is
    /// late cannot send an older status after a newer one.
    reported: Mutex<Status>,
    cli_session_id: Mutex<Option<String>>,
    db: Mutex<Database>,
    notifier: Arc<Notifier>,
    /// Test only. Runs in `attach`, inside the critical section, after the copy of the ring.
    attach_pause: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Setup session only. The status thread runs it after a turn ended (the status went from
    /// `working` to `idle` or `done`) and after the stop, with no lock of the session held.
    after_turn: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// What `Live::new` needs.
pub(crate) struct LiveParts {
    pub id: String,
    pub path: PathBuf,
    pub slug: String,
    pub session: Session,
    pub handle: SessionHandle,
    pub cli_session_id: Option<String>,
    pub had_turn: bool,
    pub db: Database,
    pub notifier: Arc<Notifier>,
    pub attach_pause: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Setup session only. See `Live::after_turn`.
    pub after_turn: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Live {
    pub fn new(parts: LiveParts) -> Arc<Live> {
        Arc::new(Live {
            id: parts.id,
            path: parts.path,
            slug: parts.slug,
            session: parts.session,
            handle: Mutex::new(Some(parts.handle)),
            out: Mutex::new(OutBuf {
                ring: VecDeque::new(),
                sink: None,
            }),
            last_view: Mutex::new(None),
            exited: AtomicBool::new(false),
            had_turn: AtomicBool::new(parts.had_turn),
            reported: Mutex::new(Status::Starting),
            cli_session_id: Mutex::new(parts.cli_session_id),
            db: Mutex::new(parts.db),
            notifier: parts.notifier,
            attach_pause: parts.attach_pause,
            after_turn: parts.after_turn,
        })
    }

    pub fn exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    pub fn cli_session_id(&self) -> Option<String> {
        locked(&self.cli_session_id).clone()
    }

    /// The view of the session now.
    pub fn view(&self) -> SessionView {
        let status = *locked(&self.reported);
        let stopped = self.exited();
        let (draft, message_waiting) = if stopped {
            (false, false)
        } else {
            (self.session.draft(), self.session.message_waiting())
        };
        SessionView {
            session_id: self.id.clone(),
            slug: self.slug.clone(),
            status: if stopped { Status::Stopped } else { status },
            draft,
            message_waiting,
            // The setup session never resumes.
            can_resume: self.slug != super::SETUP_SLUG
                && stopped
                && self.had_turn.load(Ordering::SeqCst)
                && self.cli_session_id().is_some(),
        }
    }

    /// Tell the observer if the view changed since the last notification. The compare, the
    /// update and the call are in one critical section, so two threads cannot send two views
    /// in the wrong order. The observer must not call back into the Tower.
    pub fn refresh(&self) {
        let mut last = locked(&self.last_view);
        let view = self.view();
        if last.as_ref() != Some(&view) {
            *last = Some(view.clone());
            self.notifier.session_changed(&self.path, &view);
        }
    }

    fn run_after_turn(&self) {
        if let Some(after_turn) = &self.after_turn {
            after_turn();
        }
    }

    /// Ask the process to end. The exit event then gives `stopped`.
    pub fn kill(&self) {
        if let Some(handle) = locked(&self.handle).as_mut() {
            let _ = handle.kill();
        }
    }

    /// Resize the terminal. A session that has ended ignores it.
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
        match locked(&self.handle).as_ref() {
            Some(handle) => handle.resize(rows, cols).map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }

    /// Drop the handle: if the process still runs, this kills it and waits for it (at most about
    /// 2 s, `PtyProcess::drop`). The caller holds no other lock.
    pub fn release_process(&self) {
        let handle = locked(&self.handle).take();
        drop(handle);
    }

    /// Install `sink` and return the scrollback, in one critical section. See the module text.
    pub fn attach(&self, sink: Arc<dyn OutputSink>) -> Vec<u8> {
        let mut out = locked(&self.out);
        let scrollback: Vec<u8> = out.ring.iter().copied().collect();
        if let Some(pause) = &self.attach_pause {
            pause();
        }
        out.sink = Some(sink);
        scrollback
    }

    fn deliver(&self, bytes: &[u8]) {
        let sink = {
            let mut out = locked(&self.out);
            out.ring.extend(bytes);
            let excess = out.ring.len().saturating_sub(SCROLLBACK_LIMIT);
            if excess > 0 {
                out.ring.drain(..excess);
            }
            out.sink.clone()
        };
        if let Some(sink) = sink {
            sink.output(bytes);
        }
    }

    fn store_cli_session_id(&self, id: &str) {
        let result = locked(&self.db).set_cli_session_id(&self.id, id);
        match result {
            Ok(()) => *locked(&self.cli_session_id) = Some(id.to_owned()),
            Err(error) => warn(&format!(
                "cannot store the CLI session ID of {}: {error}",
                self.id
            )),
        }
    }

    fn record_status(&self, status: Status) {
        let result = locked(&self.db).append_status_event(&self.id, status.as_str(), now_ms());
        if let Err(error) = result {
            warn(&format!(
                "cannot store a status event of {}: {error}",
                self.id
            ));
        }
    }

    fn record_end(&self) {
        let result = locked(&self.db).set_ended_at(&self.id, now_ms());
        if let Err(error) = result {
            warn(&format!("cannot store the end of {}: {error}", self.id));
        }
    }
}

/// Start the three threads of a session.
pub(crate) fn spawn_pumps(
    live: &Arc<Live>,
    status: Receiver<StatusChange>,
    output: Receiver<OutputChunk>,
    hooks_in: Receiver<Value>,
    hooks_out: Sender<Value>,
    router: Arc<HookRouter>,
    learn_cli_session_id: bool,
) -> std::io::Result<()> {
    {
        let live = Arc::clone(live);
        thread::Builder::new()
            .name("app-hooks".into())
            .spawn(move || run_hooks(&live, hooks_in, hooks_out, learn_cli_session_id))?;
    }
    {
        let live = Arc::clone(live);
        thread::Builder::new()
            .name("app-status".into())
            .spawn(move || run_status(&live, status, &router))?;
    }
    {
        let live = Arc::clone(live);
        thread::Builder::new()
            .name("app-output".into())
            .spawn(move || run_output(&live, output))?;
    }
    Ok(())
}

fn run_hooks(
    live: &Live,
    from_router: Receiver<Value>,
    to_session: Sender<Value>,
    mut learn_cli_session_id: bool,
) {
    for event in from_router {
        // Codex: the ID comes in the first hook event. It is stored before the event goes on.
        if learn_cli_session_id && let Some(id) = cli_session_id_from_event(&event) {
            live.store_cli_session_id(&id);
            learn_cli_session_id = false;
        }
        let _ = to_session.send(event);
        // The session applies the event in its own thread, so this can be a little early. A
        // change that has no status change and no output (a draft cleared by a hook) shows at
        // the next change of the status or the next output.
        live.refresh();
    }
}

fn run_status(live: &Live, status: Receiver<StatusChange>, router: &HookRouter) {
    let mut working = false;
    for change in status {
        live.record_status(change.new);
        if change.new == Status::Stopped {
            // Order matters. A `stopped` view must never be sent while the registry still
            // counts the session as live: the operator would click "resume" and be refused.
            // So `exited` is set before any view can say `stopped`.
            live.record_end();
            live.exited.store(true, Ordering::SeqCst);
            *locked(&live.reported) = Status::Stopped;
            router.unsubscribe(&live.id);
            live.refresh();
            // The process has ended. This frees the PTY handles and does not wait.
            live.release_process();
            live.run_after_turn();
            return;
        }
        if change.new == Status::Working {
            live.had_turn.store(true, Ordering::SeqCst);
            working = true;
        }
        *locked(&live.reported) = change.new;
        live.refresh();
        if matches!(change.new, Status::Idle | Status::Done) && working {
            working = false;
            live.run_after_turn();
        }
    }
}

fn run_output(live: &Live, output: Receiver<OutputChunk>) {
    loop {
        match output.recv_timeout(OUTPUT_POLL) {
            Ok(chunk) => {
                live.deliver(&chunk.bytes);
                live.refresh();
            }
            Err(RecvTimeoutError::Timeout) => {
                // A hook can clear the draft flag (`UserPromptSubmit`) with no status change
                // (a queued message in `working`) and with no output. The session sends no
                // event for flags, so the Tower looks at the flags on each timeout.
                live.refresh();
                if live.exited() {
                    // Every chunk was sent before the status `stopped` (`Session::on_pty_event`
                    // runs in order), so what is in the queue now is all that is left.
                    while let Ok(chunk) = output.try_recv() {
                        live.deliver(&chunk.bytes);
                    }
                    return;
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// The latest `limit` bytes of a log file, with the older file (`<path>.1`) before it.
/// A missing file is an empty log.
pub(crate) fn read_log_tail(path: &Path, limit: usize) -> Vec<u8> {
    let mut old_name = path.as_os_str().to_owned();
    old_name.push(".1");
    let mut bytes = tail_of(Path::new(&old_name), limit);
    bytes.extend(tail_of(path, limit));
    if bytes.len() > limit {
        bytes.drain(..bytes.len() - limit);
    }
    bytes
}

fn tail_of(path: &Path, limit: usize) -> Vec<u8> {
    let Ok(mut file) = fs::File::open(path) else {
        return Vec::new();
    };
    let length = file.metadata().map_or(0, |m| m.len());
    let start = length.saturating_sub(limit as u64);
    let mut bytes = Vec::new();
    if file.seek(SeekFrom::Start(start)).is_ok() {
        let _ = file.read_to_end(&mut bytes);
    }
    bytes
}
