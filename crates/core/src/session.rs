//! One session: a PTY, the status machine, the draft flag and the paste gate (spec 4.3, 4.5).
//!
//! # The lock
//!
//! ONE mutex holds the PTY writer, the [`Machine`], the draft flag and the "message waiting"
//! flag. A hook event changes the machine inside the same mutex that a write holds. This is the
//! write lock of spec 4.5 rule 2: `paste` checks the status inside the lock and writes inside
//! the lock, so no hook event can change the status between the check and the write.
//!
//! # The clock
//!
//! The machine takes times as [`Millis`]. A [`Clock`] gives them: milliseconds since the clock
//! was made (a monotonic `Instant`). Every input is stamped with the clock, inside the lock, and
//! the stamps never go back. The PTY layer stamps its chunks with `at_ms`, the time since the
//! process started. `Session::start` reads the clock just BEFORE it spawns the process
//! (`pty_origin`), so a chunk is on the session clock at `pty_origin + at_ms`. This is a little
//! early (by the time that `spawn` takes), never late. The converted time is cut at "now" and at
//! the last stamp. Tests use [`Clock::manual`].
//!
//! # Threads (`Session::start`)
//!
//! - the PTY pump: PTY events to `on_pty_event`;
//! - the hook pump: hook JSON from a channel to `on_hook`;
//! - the timer: it waits on a condition variable until `Machine::next_deadline()` and then
//!   applies `Input::Tick`. It does not poll. It ends when the process has exited.

use crate::model::CliKind;
use crate::pty::{PtyError, PtyEvent, PtyProcess, PtyWriter, SpawnSpec, spawn};
use crate::status::{HookEvent, HookKind, Input, KeyKind, Machine, Millis, Status};
use serde_json::Value;
use std::collections::HashMap;
use std::io;
use std::ops::Deref;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::thread;
use std::time::{Duration, Instant};

pub const PASTE_START: &[u8] = b"\x1b[200~";
pub const PASTE_END: &[u8] = b"\x1b[201~";

/// The shortest wait of the timer after a tick that did not remove its own deadline. It stops a
/// busy loop if a rule ever keeps a deadline in the past.
const TIMER_FLOOR: Duration = Duration::from_millis(50);

// ---------------------------------------------------------------------------------------------
// Clock
// ---------------------------------------------------------------------------------------------

/// The source of [`Millis`]. Cloning shares the clock.
#[derive(Debug, Clone)]
pub enum Clock {
    /// Milliseconds since this instant (monotonic).
    Real(Instant),
    /// A clock that the test moves by hand.
    Manual(Arc<AtomicU64>),
}

impl Clock {
    pub fn real() -> Clock {
        Clock::Real(Instant::now())
    }

    pub fn manual() -> Clock {
        Clock::Manual(Arc::new(AtomicU64::new(0)))
    }

    pub fn now(&self) -> Millis {
        match self {
            Clock::Real(origin) => Millis(origin.elapsed().as_millis() as u64),
            Clock::Manual(ms) => Millis(ms.load(Ordering::SeqCst)),
        }
    }

    /// Move a manual clock. Does nothing for a real clock.
    pub fn advance(&self, ms: u64) {
        if let Clock::Manual(now) = self {
            now.fetch_add(ms, Ordering::SeqCst);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Key classifier (spec 4.5 rule 4)
// ---------------------------------------------------------------------------------------------

/// What the session learns from one chunk of operator input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyClass {
    /// The chunk sets the draft flag.
    pub sets_draft: bool,
    /// The key that the machine gets. `None` for an empty chunk.
    pub key: Option<KeyKind>,
}

/// Classify a chunk of operator input (what xterm.js sends in one `onData`).
///
/// - A bracketed paste (`ESC[200~ ... ESC[201~`) and printable text (also multi-byte UTF-8 and
///   digits) set the draft flag when `status` is `idle`, `done` or `working`.
/// - They do not set it in `question` or `starting` (answers to a dialog), nor in `stopped`.
/// - Keys that can put text into the input box of a CLI with no printable byte from the operator
///   also set it. See [`puts_text_in_box`] for the list and the reason for each key. A wrong
///   flag only delays a paste (the operator can clear it); a missing flag merges text.
/// - Other escape sequences (arrows and so on), other Alt keys, other control keys, backspace,
///   Enter (`\r`), Ctrl-C (`\x03`) and a lone ESC never set it.
/// - `key`: a lone `\x1b` is `Esc`, `\x03` is `CtrlC`, `\r` is `Enter`, an ASCII digit is `Digit`.
///   Anything else is `Other`. If a chunk has several keys, the first one that is not `Other`
///   wins. A bracketed paste is text, so it is `Other` whatever it contains.
pub fn classify(bytes: &[u8], status: Status) -> KeyClass {
    let mut printable = false;
    let mut key: Option<KeyKind> = None;
    let mut at = 0;
    while at < bytes.len() {
        let rest = &bytes[at..];
        let (kind, is_text, length) = match rest[0] {
            0x1b => {
                if rest.starts_with(PASTE_START) {
                    let body = &rest[PASTE_START.len()..];
                    let length = match find(body, PASTE_END) {
                        Some(end) => PASTE_START.len() + end + PASTE_END.len(),
                        // The end of the paste is in a later chunk.
                        None => rest.len(),
                    };
                    (KeyKind::Other, true, length)
                } else {
                    match rest.get(1) {
                        None => (KeyKind::Esc, false, 1),
                        Some(b'[') => {
                            let length = csi_length(rest);
                            // Up arrow: it recalls an old prompt into the box (history).
                            (KeyKind::Other, &rest[..length] == b"\x1b[A", length)
                        }
                        Some(b'O') => {
                            let length = rest.len().min(3);
                            // Up arrow in application cursor mode: the same as `ESC [ A`.
                            (KeyKind::Other, &rest[..length] == b"\x1bOA", length)
                        }
                        // ESC ESC: the first one is a lone ESC.
                        Some(0x1b) => (KeyKind::Esc, false, 1),
                        // Alt + a key.
                        Some(&alt) => (KeyKind::Other, alt_puts_text_in_box(alt), 2),
                    }
                }
            }
            0x03 => (KeyKind::CtrlC, false, 1),
            b'\r' => (KeyKind::Enter, false, 1),
            b'0'..=b'9' => (KeyKind::Digit, true, 1),
            0x00..=0x1f | 0x7f => (KeyKind::Other, puts_text_in_box(rest[0]), 1),
            _ => (KeyKind::Other, true, 1),
        };
        printable |= is_text;
        key = match key {
            None => Some(kind),
            Some(KeyKind::Other) if kind != KeyKind::Other => Some(kind),
            keep => keep,
        };
        at += length.max(1);
    }
    KeyClass {
        sets_draft: printable && matches!(status, Status::Idle | Status::Done | Status::Working),
        key,
    }
}

/// A single control byte that can put text into the input box of a CLI, with no printable byte
/// from the operator. Each one sets the draft flag, because a paste after it would merge with
/// that text (D1: Ctrl+V and 500 pasted lines merged with an injected message). The list is
/// for Claude Code and Codex. When in doubt, the flag is set.
///
/// | Byte | Key | Why it sets the flag |
/// |---|---|---|
/// | 0x16 | Ctrl+V | The CLI reads the X clipboard itself and puts the text (or an image) in the box. |
/// | 0x19 | Ctrl+Y | Yank: puts the last killed text back in the box. |
/// | 0x12 | Ctrl+R | History search: it ends with an old prompt in the box. |
/// | 0x10 | Ctrl+P | Same as Up: history recall puts an old prompt in the box. |
/// | 0x09 | Tab | Completion (file path, slash command) or accept of a suggestion writes text. |
/// | 0x07 | Ctrl+G | Opens the external editor; its text comes back into the box. |
/// | 0x13 | Ctrl+S | Stash: restores a stashed prompt into the box (Claude Code). |
/// | 0x0a | Ctrl+J | Inserts a new line in the box (also what Shift+Enter can send). |
/// | 0x18 | Ctrl+X | Start of the chord Ctrl+X Ctrl+E (edit in the editor); we do not track chords. |
///
/// These do NOT set it (they move the cursor, delete text, or act on the screen): Ctrl+A, B, E,
/// F (cursor), Ctrl+D (delete or exit), Ctrl+H and 0x7f (backspace), Ctrl+K, U, W (delete),
/// Ctrl+L (redraw), Ctrl+N (history forward, only after Up, which set the flag already),
/// Ctrl+O, Ctrl+T, Ctrl+Z, Ctrl+C (cancel), Enter, and the rest of the control bytes.
fn puts_text_in_box(byte: u8) -> bool {
    matches!(
        byte,
        0x16 | 0x19 | 0x12 | 0x10 | 0x09 | 0x07 | 0x13 | 0x0a | 0x18
    )
}

/// `ESC` + a byte (an Alt key) that puts text in the box.
/// `y`: yank-pop, the Alt form of Ctrl+Y. `v`: paste from the clipboard (image or text) in
/// some CLIs. `\r`: Alt+Enter inserts a new line.
fn alt_puts_text_in_box(byte: u8) -> bool {
    matches!(byte, b'y' | b'Y' | b'v' | b'V' | b'\r')
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The length of the CSI sequence at the start of `rest` (`ESC [ params final`). An unfinished
/// sequence takes the rest of the chunk.
fn csi_length(rest: &[u8]) -> usize {
    for (index, byte) in rest.iter().enumerate().skip(2) {
        if (0x40..=0x7e).contains(byte) {
            return index + 1;
        }
    }
    rest.len()
}

/// Remove every ESC byte and C1 control character from a message, so that it cannot contain
/// `ESC[201~` (or anything that forms it after a removal) and end the paste early.
pub fn sanitize_paste(text: &str) -> String {
    text.chars()
        .filter(|c| *c != '\x1b' && !('\u{80}'..='\u{9f}').contains(c))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------------------------

/// Why a paste was refused. In each case nothing was written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PasteRefused {
    /// The status does not permit a paste (spec 4.5 rule 1 and the table).
    #[error("the status is {}", .0.as_str())]
    Status(Status),
    /// The operator has a draft in the prompt (rule 4).
    #[error("the operator has a draft")]
    Draft,
    /// The status is `idle` or `done`, but the machine says no: a tool event came after the last
    /// turn end (rule 3), or a Codex session has had no prompt.
    #[error("the status machine does not allow a paste")]
    NotAllowedByMachine,
    /// There is no live process.
    #[error("the session has stopped")]
    Stopped,
    /// The write failed.
    #[error("the write failed: {0:?}")]
    Write(io::ErrorKind),
}

/// A status change, as the subscriber gets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusChange {
    pub old: Status,
    pub new: Status,
    /// On the clock of the session.
    pub at: Millis,
}

/// Output of the process, for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputChunk {
    pub bytes: Vec<u8>,
    /// On the clock of the session.
    pub at: Millis,
}

/// The channels of a session. Drop a receiver if you do not need it.
pub struct SessionStreams {
    pub status: Receiver<StatusChange>,
    pub output: Receiver<OutputChunk>,
}

struct State {
    writer: Option<Box<dyn PtyWriter + Send>>,
    machine: Machine,
    /// The operator has text in the input box of the CLI (spec 4.5 rule 4).
    ///
    /// RULE: this flag ends in two ways ONLY. A `UserPromptSubmit` hook clears it (`on_hook`),
    /// or the operator clears it by hand (`clear_draft`). It is NOT cleared by a timer, and an
    /// Enter from the operator does not clear it. Reason (defect N1): after `/help` + Enter the
    /// CLI fires no `UserPromptSubmit`, but its dialog is still open. If a timer cleared the
    /// flag, a paste would be accepted then: the message is lost in the dialog, and the `\r`
    /// of the paste can select an option of a picker (`/model`, `/resume`, `/permissions`).
    /// A flag that stays too long only delays a paste, and the operator can clear it. Do not
    /// add an automatic clear.
    draft: bool,
    message_waiting: bool,
    /// `ProcessExit` was applied. The writer is gone.
    exited: bool,
    last_time: Millis,
    pty_origin: Millis,
    status_tx: Sender<StatusChange>,
    output_tx: Sender<OutputChunk>,
}

struct Inner {
    id: String,
    state: Mutex<State>,
    /// Wakes the timer when the next deadline may have changed, and at the end.
    changed: Condvar,
    clock: Clock,
    /// Test only: runs in `paste`, inside the lock, after the checks and before the write.
    #[cfg(test)]
    pause: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

/// One session. Cloning gives another handle to the same session.
#[derive(Clone)]
pub struct Session {
    inner: Arc<Inner>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Session {
    /// A session over a writer. It has no process of its own: `on_pty_event` and `tick` are
    /// called by the caller (`Session::start` does this with threads).
    pub fn new(
        cli: CliKind,
        id: impl Into<String>,
        writer: Box<dyn PtyWriter + Send>,
        clock: Clock,
    ) -> (Session, SessionStreams) {
        let (status_tx, status) = mpsc::channel();
        let (output_tx, output) = mpsc::channel();
        let now = clock.now();
        let state = State {
            writer: Some(writer),
            machine: Machine::new(cli, now),
            draft: false,
            message_waiting: false,
            exited: false,
            last_time: now,
            pty_origin: now,
            status_tx,
            output_tx,
        };
        let inner = Inner {
            id: id.into(),
            state: Mutex::new(state),
            changed: Condvar::new(),
            clock,
            #[cfg(test)]
            pause: Mutex::new(None),
        };
        (
            Session {
                inner: Arc::new(inner),
            },
            SessionStreams { status, output },
        )
    }

    pub fn id(&self) -> &str {
        &self.inner.id
    }

    pub fn status(&self) -> Status {
        locked(&self.inner.state).machine.status()
    }

    pub fn cli(&self) -> CliKind {
        locked(&self.inner.state).machine.cli()
    }

    /// The draft flag (spec 4.5 rule 4).
    pub fn draft(&self) -> bool {
        locked(&self.inner.state).draft
    }

    /// The operator clears the marks by hand (the click on the mark of the tab). This clears the
    /// draft flag AND the "message waiting" flag (D5). With `UserPromptSubmit`, it is the only
    /// way that the draft flag ends (see the rule at `State::draft`).
    pub fn clear_draft(&self) {
        let mut state = locked(&self.inner.state);
        state.draft = false;
        state.message_waiting = false;
    }

    /// A paste was refused (or failed), and no later paste has worked.
    pub fn message_waiting(&self) -> bool {
        locked(&self.inner.state).message_waiting
    }

    /// For the caller that delivered the waiting messages by another path.
    pub fn clear_message_waiting(&self) {
        locked(&self.inner.state).message_waiting = false;
    }

    /// For the caller that holds a message back with no paste attempt (the first-turn guard of
    /// the app). A paste that works, or `clear_message_waiting`, ends the mark.
    pub fn set_message_waiting(&self) {
        locked(&self.inner.state).message_waiting = true;
    }

    /// The time at which the machine needs a tick, if any.
    pub fn next_deadline(&self) -> Option<Millis> {
        locked(&self.inner.state).machine.next_deadline()
    }

    /// Set the time zero of the PTY layer on the clock of the session (see the module text).
    pub fn set_pty_origin(&self, origin: Millis) {
        locked(&self.inner.state).pty_origin = origin;
    }

    /// A hook event (the JSON of the CLI, as `tower-hook` sent it). `UserPromptSubmit` clears
    /// the draft flag. Returns the new status if it changed.
    pub fn on_hook(&self, event: &Value) -> Option<Status> {
        let event = HookEvent::from_json(event);
        let clears_draft = event.kind == HookKind::UserPromptSubmit;
        let mut state = locked(&self.inner.state);
        if clears_draft {
            state.draft = false;
        }
        let now = self.inner.stamp(&mut state, None);
        self.inner.apply(&mut state, Input::Hook(event), now)
    }

    /// An event of the PTY layer. Output goes to the machine and to the output channel. `Exit`
    /// gives `ProcessExit`, and then the writer is dropped: the child has ended, so the
    /// writer's last bytes (a newline and EOF) reach nobody.
    pub fn on_pty_event(&self, event: PtyEvent) {
        let mut state = locked(&self.inner.state);
        match event {
            PtyEvent::Output { bytes, at_ms } => {
                let converted = state.pty_origin.plus(at_ms);
                let now = self.inner.stamp(&mut state, Some(converted));
                self.inner.apply(&mut state, Input::PtyOutput, now);
                let _ = state.output_tx.send(OutputChunk { bytes, at: now });
            }
            PtyEvent::Exit(_) => {
                let now = self.inner.stamp(&mut state, None);
                self.inner.apply(&mut state, Input::ProcessExit, now);
                state.exited = true;
                state.writer = None;
                self.inner.changed.notify_all();
            }
        }
    }

    /// The agent set its task to `review` or `done`. The next turn end gives `done`.
    pub fn mark_task_done(&self) {
        let mut state = locked(&self.inner.state);
        let now = self.inner.stamp(&mut state, None);
        self.inner.apply(&mut state, Input::TaskMarkedDone, now);
    }

    /// The Stop hook of the app returned `block`. Call this BEFORE the machine gets that `Stop`
    /// (spec 4.3, M2).
    pub fn stop_blocked_by_app(&self) {
        let mut state = locked(&self.inner.state);
        let now = self.inner.stamp(&mut state, None);
        self.inner.apply(&mut state, Input::StopBlockedByApp, now);
    }

    /// Apply a tick: time passed. Returns the new status if it changed.
    pub fn tick(&self) -> Option<Status> {
        let mut state = locked(&self.inner.state);
        let now = self.inner.stamp(&mut state, None);
        self.inner.apply(&mut state, Input::Tick, now)
    }

    /// Input of the operator. It always goes through, in every status (rule 5). The session
    /// classifies it: it can set the draft flag (rule 4) and it feeds the machine (`Esc`,
    /// `CtrlC`, `Enter` and digits matter for the quiet rule). After the process ended, this
    /// changes nothing and returns `BrokenPipe`.
    pub fn operator_input(&self, bytes: &[u8]) -> io::Result<()> {
        let mut state = locked(&self.inner.state);
        if state.exited {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let class = classify(bytes, state.machine.status());
        if class.sets_draft {
            state.draft = true;
        }
        if let Some(key) = class.key {
            let now = self.inner.stamp(&mut state, None);
            self.inner.apply(&mut state, Input::OperatorKey(key), now);
        }
        match state.writer.as_mut() {
            // The write happens inside the lock (spec 4.5 rule 2).
            Some(writer) => writer.write_all(bytes),
            None => Err(io::ErrorKind::BrokenPipe.into()),
        }
    }

    /// Paste a message: `ESC[200~ text ESC[201~ \r` in ONE `write_all` call (spec 4.5).
    ///
    /// The checks and the write are inside the lock. A refusal writes nothing and sets the
    /// "message waiting" flag; a paste that works clears it. ESC bytes and C1 controls are
    /// removed from the text first (`sanitize_paste`), so a message cannot end the paste
    /// early and send keys.
    pub fn paste(&self, text: &str) -> Result<(), PasteRefused> {
        let mut state = locked(&self.inner.state);
        let now = self.inner.stamp(&mut state, None);
        // A rule whose time came must show before the check.
        self.inner.apply(&mut state, Input::Tick, now);

        let verdict = if state.exited {
            Err(PasteRefused::Stopped)
        } else {
            let status = state.machine.status();
            if status == Status::Stopped {
                Err(PasteRefused::Stopped)
            } else if !matches!(status, Status::Idle | Status::Done) {
                Err(PasteRefused::Status(status))
            } else if !state.machine.paste_allowed() {
                Err(PasteRefused::NotAllowedByMachine)
            } else if state.draft {
                Err(PasteRefused::Draft)
            } else {
                Ok(())
            }
        };
        if let Err(reason) = verdict {
            state.message_waiting = true;
            return Err(reason);
        }

        #[cfg(test)]
        self.inner.pause_point();

        let clean = sanitize_paste(text);
        let mut bytes = Vec::with_capacity(PASTE_START.len() + clean.len() + PASTE_END.len() + 1);
        bytes.extend_from_slice(PASTE_START);
        bytes.extend_from_slice(clean.as_bytes());
        bytes.extend_from_slice(PASTE_END);
        bytes.push(b'\r');
        let written = match state.writer.as_mut() {
            Some(writer) => writer.write_all(&bytes),
            None => Err(io::ErrorKind::BrokenPipe.into()),
        };
        match written {
            Ok(()) => {
                state.message_waiting = false;
                Ok(())
            }
            Err(error) => {
                state.message_waiting = true;
                Err(PasteRefused::Write(error.kind()))
            }
        }
    }

    /// Test only: set the pause point of `paste`.
    #[cfg(test)]
    fn set_pause_point(&self, pause: impl Fn() + Send + Sync + 'static) {
        *locked(&self.inner.pause) = Some(Arc::new(pause));
    }

    fn downgrade(&self) -> Weak<Inner> {
        Arc::downgrade(&self.inner)
    }
}

impl Inner {
    /// The time for an input: the clock (or `hint`, cut at the clock), never before the last
    /// stamp.
    fn stamp(&self, state: &mut State, hint: Option<Millis>) -> Millis {
        let now = self.clock.now();
        let time = hint.map_or(now, |hint| hint.min(now)).max(state.last_time);
        state.last_time = time;
        time
    }

    /// Apply an input to the machine. A change goes to the status channel. If the next
    /// deadline moved, the timer wakes.
    fn apply(&self, state: &mut State, input: Input, now: Millis) -> Option<Status> {
        let old = state.machine.status();
        let deadline = state.machine.next_deadline();
        let new = state.machine.apply(input, now);
        if let Some(new) = new {
            let _ = state.status_tx.send(StatusChange { old, new, at: now });
        }
        if state.machine.next_deadline() != deadline {
            self.changed.notify_all();
        }
        new
    }

    #[cfg(test)]
    fn pause_point(&self) {
        let pause = locked(&self.pause).clone();
        if let Some(pause) = pause {
            pause();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------------------------

/// One hook event for a session, with the reply of the sender when it waits for an answer.
#[derive(Debug)]
pub struct Routed {
    pub event: Value,
    /// `Some` when the hook program waits for an answer (the Stop hook). Drop it to close the
    /// connection with no answer.
    pub reply: Option<crate::hookrx::HookReply>,
}

/// Where the events of one session go.
enum Route {
    /// The events only. A reply that comes with an event is dropped, so its connection closes.
    Plain(Sender<Value>),
    /// The events with their replies.
    Replying(Sender<Routed>),
}

impl Route {
    /// Give the message to the session. `false` means the channel is closed.
    fn send(&self, message: crate::hookrx::HookMessage) -> bool {
        match self {
            Route::Plain(sender) => sender.send(message.event).is_ok(),
            Route::Replying(sender) => sender
                .send(Routed {
                    event: message.event,
                    reply: message.reply,
                })
                .is_ok(),
        }
    }
}

type Routes = Arc<Mutex<HashMap<String, Route>>>;

/// Sends the events of one `HookReceiver` to the sessions, by `HookMessage.session`.
///
/// Call `subscribe` BEFORE the process starts, so that no event is lost. An event for a session
/// that nobody subscribed to is counted and dropped. The thread ends when the source channel
/// closes.
pub struct HookRouter {
    routes: Routes,
    unrouted: Arc<AtomicU64>,
}

impl HookRouter {
    pub fn start(source: Receiver<crate::hookrx::HookMessage>) -> HookRouter {
        let routes = Routes::default();
        let unrouted = Arc::new(AtomicU64::new(0));
        {
            let routes = Arc::clone(&routes);
            let unrouted = Arc::clone(&unrouted);
            let _ = thread::Builder::new()
                .name("hook-router".into())
                .spawn(move || {
                    for message in source {
                        let mut routes = locked(&routes);
                        let session = message.session.clone();
                        let delivered = match routes.get(&session) {
                            Some(route) => route.send(message),
                            None => false,
                        };
                        if !delivered {
                            // A closed route is removed.
                            routes.remove(&session);
                            unrouted.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                });
        }
        HookRouter { routes, unrouted }
    }

    /// The channel of the events of one session. A second call for the same ID replaces the
    /// first channel.
    pub fn subscribe(&self, session_id: &str) -> Receiver<Value> {
        let (sender, receiver) = mpsc::channel();
        locked(&self.routes).insert(session_id.to_owned(), Route::Plain(sender));
        receiver
    }

    /// Like `subscribe`, and each event comes with the reply of its sender (if the sender
    /// waits for an answer).
    pub fn subscribe_replying(&self, session_id: &str) -> Receiver<Routed> {
        let (sender, receiver) = mpsc::channel();
        locked(&self.routes).insert(session_id.to_owned(), Route::Replying(sender));
        receiver
    }

    pub fn unsubscribe(&self, session_id: &str) {
        locked(&self.routes).remove(session_id);
    }

    /// Events that had no route.
    pub fn unrouted_count(&self) -> u64 {
        self.unrouted.load(Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------------------------

/// A running session: the process and the session, with their threads.
///
/// Dropping it kills the process (`PtyProcess::drop`) and then drops the session. The writer
/// is dropped when the exit event arrives, which is after the child ended.
pub struct SessionHandle {
    // Order matters for drop: the process first.
    process: PtyProcess,
    session: Session,
}

impl Deref for SessionHandle {
    type Target = Session;

    fn deref(&self) -> &Session {
        &self.session
    }
}

impl SessionHandle {
    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn pid(&self) -> Option<u32> {
        self.process.pid()
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), PtyError> {
        self.process.resize(rows, cols)
    }

    /// Ask the process to end (`SIGHUP`). The exit event then gives `stopped`.
    pub fn kill(&mut self) -> io::Result<()> {
        self.process.kill()
    }
}

impl Session {
    /// Start a process and run a session over it. `hook_events` is the channel of this session
    /// from `HookRouter::subscribe`.
    pub fn start(
        spec: SpawnSpec,
        cli: CliKind,
        id: &str,
        hook_events: Receiver<Value>,
    ) -> Result<(SessionHandle, SessionStreams), PtyError> {
        let clock = Clock::real();
        // Before the spawn, so that a chunk is never later on our clock than in fact.
        let pty_origin = clock.now();
        let mut process = spawn(spec)?;
        let (Some(events), Some(writer)) = (process.take_events(), process.take_writer()) else {
            return Err(PtyError::Open("events or writer already taken".into()));
        };
        let (session, streams) = Session::new(cli, id, writer, clock);
        {
            let mut state = locked(&session.inner.state);
            state.pty_origin = pty_origin;
            // The start limit counts from the start of the process.
            state.machine = Machine::new(cli, pty_origin);
        }

        let pty_pump = session.clone();
        thread::Builder::new()
            .name("session-pty".into())
            .spawn(move || {
                for event in events {
                    let exit = matches!(event, PtyEvent::Exit(_));
                    pty_pump.on_pty_event(event);
                    if exit {
                        break;
                    }
                }
            })?;

        // The hook pump holds only a weak reference. It ends when the channel closes or when
        // the session is gone.
        let weak = session.downgrade();
        thread::Builder::new()
            .name("session-hooks".into())
            .spawn(move || {
                for event in hook_events {
                    let Some(inner) = weak.upgrade() else {
                        break;
                    };
                    Session { inner }.on_hook(&event);
                }
            })?;

        let timer = session.clone();
        thread::Builder::new()
            .name("session-timer".into())
            .spawn(move || timer.run_timer())?;

        Ok((SessionHandle { process, session }, streams))
    }

    /// The timer loop. It sleeps until the next deadline of the machine. When a hook or an
    /// output moves the deadline, `apply` wakes it.
    fn run_timer(&self) {
        let inner = &self.inner;
        let mut state = locked(&inner.state);
        loop {
            if state.exited {
                return;
            }
            let Some(deadline) = state.machine.next_deadline() else {
                state = inner
                    .changed
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
                continue;
            };
            let now = inner.clock.now();
            let wait = if deadline <= now {
                let now = inner.stamp(&mut state, None);
                inner.apply(&mut state, Input::Tick, now);
                match state.machine.next_deadline() {
                    // The tick did not remove the deadline: do not spin.
                    Some(next) if next <= now => TIMER_FLOOR,
                    _ => continue,
                }
            } else {
                Duration::from_millis(deadline.since(now))
            };
            state = inner
                .changed
                .wait_timeout(state, wait)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::RecordingWriter;
    use serde_json::json;
    use std::sync::mpsc::RecvTimeoutError;

    fn hook(name: &str) -> Value {
        json!({ "hook_event_name": name })
    }

    /// A session with a recording writer and a manual clock. `idle`: Claude after SessionStart.
    struct Rig {
        session: Session,
        writer: RecordingWriter,
        clock: Clock,
        streams: SessionStreams,
    }

    impl Rig {
        fn new(cli: CliKind) -> Rig {
            let writer = RecordingWriter::new();
            let clock = Clock::manual();
            let (session, streams) =
                Session::new(cli, "s1", Box::new(writer.clone()), clock.clone());
            Rig {
                session,
                writer,
                clock,
                streams,
            }
        }

        fn claude_idle() -> Rig {
            let rig = Rig::new(CliKind::Claude);
            rig.hook("SessionStart");
            assert_eq!(rig.session.status(), Status::Idle);
            rig
        }

        fn hook(&self, name: &str) {
            self.clock.advance(10);
            self.session.on_hook(&hook(name));
        }

        fn changes(&self) -> Vec<(Status, Status)> {
            self.streams
                .status
                .try_iter()
                .map(|change| (change.old, change.new))
                .collect()
        }
    }

    // ---- classifier ---------------------------------------------------------------------------

    fn class(bytes: &[u8], status: Status) -> (bool, Option<KeyKind>) {
        let class = classify(bytes, status);
        (class.sets_draft, class.key)
    }

    #[test]
    fn classifier_table() {
        use KeyKind::*;
        use Status::*;
        let paste = b"\x1b[200~hello\nworld\x1b[201~";
        type Case<'a> = (&'a str, &'a [u8], Status, (bool, Option<KeyKind>));
        let cases: Vec<Case> = vec![
            ("xterm.js paste, idle", paste, Idle, (true, Some(Other))),
            ("xterm.js paste, done", paste, Done, (true, Some(Other))),
            (
                "xterm.js paste, working",
                paste,
                Working,
                (true, Some(Other)),
            ),
            (
                "xterm.js paste, question",
                paste,
                Question,
                (false, Some(Other)),
            ),
            (
                "xterm.js paste, starting",
                paste,
                Starting,
                (false, Some(Other)),
            ),
            (
                "xterm.js paste, stopped",
                paste,
                Stopped,
                (false, Some(Other)),
            ),
            ("text, idle", b"hello", Idle, (true, Some(Other))),
            ("text, working", b"h", Working, (true, Some(Other))),
            ("text, done", b"h", Done, (true, Some(Other))),
            ("text, question", b"hello", Question, (false, Some(Other))),
            ("text, starting", b"hello", Starting, (false, Some(Other))),
            ("space, idle", b" ", Idle, (true, Some(Other))),
            (
                "utf-8, idle",
                "é€日本".as_bytes(),
                Idle,
                (true, Some(Other)),
            ),
            (
                "utf-8, question",
                "é€日本".as_bytes(),
                Question,
                (false, Some(Other)),
            ),
            ("Enter, question", b"\r", Question, (false, Some(Enter))),
            ("Enter, idle", b"\r", Idle, (false, Some(Enter))),
            ("digit, question", b"1", Question, (false, Some(Digit))),
            ("digit, idle", b"1", Idle, (true, Some(Digit))),
            ("digit, starting", b"2", Starting, (false, Some(Digit))),
            ("lone ESC", b"\x1b", Idle, (false, Some(Esc))),
            ("lone ESC, question", b"\x1b", Question, (false, Some(Esc))),
            ("Ctrl-C", b"\x03", Working, (false, Some(CtrlC))),
            // Up arrow recalls an old prompt into the box (history): a draft (D1).
            ("arrow up", b"\x1b[A", Idle, (true, Some(Other))),
            ("arrow up, SS3", b"\x1bOA", Idle, (true, Some(Other))),
            (
                "arrow up, question",
                b"\x1b[A",
                Question,
                (false, Some(Other)),
            ),
            (
                "arrow up, starting",
                b"\x1b[A",
                Starting,
                (false, Some(Other)),
            ),
            ("arrow up, working", b"\x1b[A", Working, (true, Some(Other))),
            ("arrow down", b"\x1b[B", Idle, (false, Some(Other))),
            ("arrow left", b"\x1b[D", Idle, (false, Some(Other))),
            (
                "arrow up with Ctrl",
                b"\x1b[1;5A",
                Idle,
                (false, Some(Other)),
            ),
            ("Shift+Tab", b"\x1b[Z", Idle, (false, Some(Other))),
            (
                "arrow with modifier",
                b"\x1b[1;5C",
                Idle,
                (false, Some(Other)),
            ),
            ("SS3 key (F1)", b"\x1bOP", Idle, (false, Some(Other))),
            ("Alt+b", b"\x1bb", Idle, (false, Some(Other))),
            ("backspace", b"\x7f", Idle, (false, Some(Other))),
            ("Ctrl-D", b"\x04", Idle, (false, Some(Other))),
            ("Tab", b"\t", Idle, (true, Some(Other))),
            ("Tab, question", b"\t", Question, (false, Some(Other))),
            // Control bytes that put text in the box with no printable byte (D1).
            ("Ctrl+V, idle", b"\x16", Idle, (true, Some(Other))),
            ("Ctrl+V, done", b"\x16", Done, (true, Some(Other))),
            ("Ctrl+V, working", b"\x16", Working, (true, Some(Other))),
            ("Ctrl+V, question", b"\x16", Question, (false, Some(Other))),
            ("Ctrl+V, starting", b"\x16", Starting, (false, Some(Other))),
            ("Ctrl+V, stopped", b"\x16", Stopped, (false, Some(Other))),
            ("Ctrl+Y yank", b"\x19", Idle, (true, Some(Other))),
            ("Ctrl+R history search", b"\x12", Idle, (true, Some(Other))),
            ("Ctrl+P history", b"\x10", Idle, (true, Some(Other))),
            ("Ctrl+G editor", b"\x07", Idle, (true, Some(Other))),
            ("Ctrl+S stash", b"\x13", Idle, (true, Some(Other))),
            ("Ctrl+J new line", b"\n", Idle, (true, Some(Other))),
            ("Ctrl+X chord", b"\x18", Idle, (true, Some(Other))),
            ("Alt+y yank-pop", b"\x1by", Idle, (true, Some(Other))),
            ("Alt+v paste", b"\x1bv", Idle, (true, Some(Other))),
            ("Alt+Enter new line", b"\x1b\r", Idle, (true, Some(Other))),
            // Control bytes that only move the cursor, delete, or act on the screen.
            ("Ctrl+A", b"\x01", Idle, (false, Some(Other))),
            ("Ctrl+E", b"\x05", Idle, (false, Some(Other))),
            ("Ctrl+B", b"\x02", Idle, (false, Some(Other))),
            ("Ctrl+F", b"\x06", Idle, (false, Some(Other))),
            ("Ctrl+K", b"\x0b", Idle, (false, Some(Other))),
            ("Ctrl+U", b"\x15", Idle, (false, Some(Other))),
            ("Ctrl+W", b"\x17", Idle, (false, Some(Other))),
            ("Ctrl+H", b"\x08", Idle, (false, Some(Other))),
            ("Ctrl+L", b"\x0c", Idle, (false, Some(Other))),
            ("Ctrl+N", b"\x0e", Idle, (false, Some(Other))),
            ("Ctrl+O", b"\x0f", Idle, (false, Some(Other))),
            ("Ctrl+T", b"\x14", Idle, (false, Some(Other))),
            ("Ctrl+Z", b"\x1a", Idle, (false, Some(Other))),
            ("Alt+Backspace", b"\x1b\x7f", Idle, (false, Some(Other))),
            (
                "Backspace only, done",
                b"\x7f\x7f",
                Done,
                (false, Some(Other)),
            ),
            ("Esc only, working", b"\x1b", Working, (false, Some(Esc))),
            ("Ctrl-C only, idle", b"\x03", Idle, (false, Some(CtrlC))),
            ("empty", b"", Idle, (false, None)),
            // A chunk with several keys.
            ("text then Enter", b"ab\r", Idle, (true, Some(Enter))),
            ("Esc after text", b"ab\x1b", Idle, (true, Some(Esc))),
            (
                "paste then Enter",
                b"\x1b[200~x\x1b[201~\r",
                Idle,
                (true, Some(Enter)),
            ),
            ("arrow then text", b"\x1b[Ba", Idle, (true, Some(Other))),
            // A paste whose text looks like keys is still text.
            (
                "paste with keys inside",
                b"\x1b[200~\r\x03\x1b1\x1b[201~",
                Idle,
                (true, Some(Other)),
            ),
            // A paste that continues in the next chunk.
            (
                "paste without end",
                b"\x1b[200~abc",
                Idle,
                (true, Some(Other)),
            ),
            ("unfinished CSI", b"\x1b[1;", Idle, (false, Some(Other))),
        ];
        for (name, bytes, status, expected) in cases {
            assert_eq!(class(bytes, status), expected, "{name}");
        }
    }

    // ---- paste gate with a recording writer ---------------------------------------------------

    fn exact_paste(text: &str) -> Vec<u8> {
        format!("\x1b[200~{text}\x1b[201~\r").into_bytes()
    }

    #[test]
    fn paste_in_idle_is_one_write_with_the_exact_bytes() {
        let rig = Rig::claude_idle();
        rig.session.paste("line one\nline two").unwrap();
        assert_eq!(rig.writer.write_count(), 1);
        assert_eq!(rig.writer.writes(), vec![exact_paste("line one\nline two")]);
        assert!(!rig.session.message_waiting());
        // Also in done.
        let rig = Rig::claude_idle();
        rig.session
            .on_hook(&json!({"hook_event_name": "UserPromptSubmit", "prompt": "x"}));
        rig.session.mark_task_done();
        rig.hook("Stop");
        assert_eq!(rig.session.status(), Status::Done);
        rig.session.paste("hi").unwrap();
        assert_eq!(rig.writer.writes(), vec![exact_paste("hi")]);
    }

    #[test]
    fn paste_is_refused_in_each_wrong_status_with_zero_writes() {
        // starting
        let rig = Rig::new(CliKind::Claude);
        assert_eq!(
            rig.session.paste("m"),
            Err(PasteRefused::Status(Status::Starting))
        );
        assert!(rig.session.message_waiting());
        // working
        rig.hook("SessionStart");
        rig.hook("UserPromptSubmit");
        assert_eq!(
            rig.session.paste("m"),
            Err(PasteRefused::Status(Status::Working))
        );
        // question
        rig.hook("PreToolUse");
        rig.hook("PermissionRequest");
        assert_eq!(
            rig.session.paste("m"),
            Err(PasteRefused::Status(Status::Question))
        );
        assert_eq!(rig.writer.write_count(), 0);
        // After the Stop it works, and the flag is cleared by the paste.
        rig.hook("Stop");
        assert!(rig.session.message_waiting());
        rig.session.paste("m").unwrap();
        assert!(!rig.session.message_waiting());
        assert_eq!(rig.writer.write_count(), 1);
    }

    #[test]
    fn paste_is_refused_when_a_tool_event_came_after_the_last_turn_end() {
        // Rule 3: Idle by a SessionStart after a tool event is not possible for Claude; use
        // PreToolUse then SessionStart (status idle, tool_since_stop set).
        let rig = Rig::claude_idle();
        rig.hook("PreToolUse");
        rig.hook("SessionStart");
        assert_eq!(rig.session.status(), Status::Idle);
        assert_eq!(
            rig.session.paste("m"),
            Err(PasteRefused::NotAllowedByMachine)
        );
        assert_eq!(rig.writer.write_count(), 0);
        assert!(rig.session.message_waiting());
    }

    #[test]
    fn paste_is_refused_with_a_draft() {
        let rig = Rig::claude_idle();
        rig.session.operator_input(b"typing").unwrap();
        assert!(rig.session.draft());
        assert_eq!(rig.session.paste("m"), Err(PasteRefused::Draft));
        // Only the typing was written.
        assert_eq!(rig.writer.writes(), vec![b"typing".to_vec()]);
        assert!(rig.session.message_waiting());
        // The operator clears the marks by hand: the draft flag AND "message waiting" (D5).
        rig.session.clear_draft();
        assert!(!rig.session.draft());
        assert!(!rig.session.message_waiting());
        rig.session.paste("m").unwrap();
        assert_eq!(rig.writer.write_count(), 2);
        assert!(!rig.session.message_waiting());
    }

    // ---- the draft flag has no timer (N1) ------------------------------------------------------

    /// Move the manual clock and apply a tick, as the timer thread does.
    fn pass(rig: &Rig, ms: u64) {
        rig.clock.advance(ms);
        rig.session.tick();
    }

    #[test]
    fn enter_with_a_draft_and_no_prompt_event_keeps_the_flag_and_refuses_a_paste() {
        // N1: `/help` + Enter fires no UserPromptSubmit, and the dialog is still open. A timer
        // that cleared the flag would let a paste into the dialog.
        let rig = Rig::claude_idle();
        rig.session.operator_input(b"/help").unwrap();
        rig.session.operator_input(b"\r").unwrap();
        assert!(rig.session.draft());
        for _ in 0..10 {
            pass(&rig, 1000);
        }
        assert!(rig.session.draft(), "10 s later the flag is still set");
        assert_eq!(rig.session.paste("m"), Err(PasteRefused::Draft));
        assert_eq!(rig.session.next_deadline(), None, "no timer for the flag");
        // Only the operator's keys were written.
        assert_eq!(rig.writer.all_bytes(), b"/help\r");
    }

    #[test]
    fn a_prompt_event_clears_the_draft_flag() {
        let rig = Rig::claude_idle();
        rig.session.operator_input(b"hello").unwrap();
        rig.session.operator_input(b"\r").unwrap();
        pass(&rig, 10_000);
        assert!(rig.session.draft());
        rig.hook("UserPromptSubmit");
        assert!(!rig.session.draft());
        rig.hook("Stop");
        rig.session.paste("m").unwrap();
    }

    #[test]
    fn enter_in_working_with_a_draft_keeps_the_flag_until_the_prompt_event() {
        let rig = Rig::claude_idle();
        rig.hook("UserPromptSubmit");
        rig.session.operator_input(b"queued").unwrap();
        rig.session.operator_input(b"\r").unwrap();
        pass(&rig, 10_000);
        assert!(rig.session.draft());
        rig.hook("UserPromptSubmit");
        assert!(!rig.session.draft());
    }

    #[test]
    fn clear_draft_clears_a_flag_that_stayed_after_enter() {
        let rig = Rig::claude_idle();
        rig.session.operator_input(b"/help\r").unwrap();
        pass(&rig, 10_000);
        assert!(rig.session.draft());
        rig.session.clear_draft();
        assert!(!rig.session.draft());
        rig.session.paste("m").unwrap();
    }

    #[test]
    fn ctrl_v_and_history_keys_set_the_draft_and_block_a_paste() {
        for keys in [&b"\x16"[..], b"\x19", b"\x12", b"\x10", b"\x1b[A", b"\t"] {
            let rig = Rig::claude_idle();
            rig.session.operator_input(keys).unwrap();
            assert!(rig.session.draft(), "{keys:?}");
            assert_eq!(rig.session.paste("m"), Err(PasteRefused::Draft), "{keys:?}");
        }
    }

    #[test]
    fn user_prompt_submit_clears_the_draft() {
        let rig = Rig::claude_idle();
        rig.session.operator_input(b"typing").unwrap();
        rig.session.operator_input(b"\r").unwrap();
        assert!(
            rig.session.draft(),
            "Enter does not clear the flag by itself"
        );
        rig.hook("UserPromptSubmit");
        assert!(!rig.session.draft());
        rig.hook("Stop");
        rig.session.paste("m").unwrap();
    }

    #[test]
    fn operator_keys_set_the_draft_only_by_rule_4() {
        let rig = Rig::claude_idle();
        for keys in [&b"\r"[..], b"\x1b", b"\x03", b"\x1b[B", b"\x7f"] {
            rig.session.operator_input(keys).unwrap();
            assert!(!rig.session.draft(), "{keys:?}");
        }
        rig.session
            .operator_input(b"\x1b[200~pasted\x1b[201~")
            .unwrap();
        assert!(rig.session.draft(), "an operator paste sets the flag");
        rig.session.clear_draft();

        // Keys in question and in starting do not set it.
        let starting = Rig::new(CliKind::Claude);
        starting.session.operator_input(b"abc").unwrap();
        assert!(!starting.session.draft());
        let question = Rig::claude_idle();
        question.hook("UserPromptSubmit");
        question.hook("PreToolUse");
        question.hook("PermissionRequest");
        question.session.operator_input(b"1").unwrap();
        question.session.operator_input(b"\r").unwrap();
        question.session.operator_input(b"yes").unwrap();
        assert!(!question.session.draft());
        // And in working they do.
        let working = Rig::claude_idle();
        working.hook("UserPromptSubmit");
        working.session.operator_input(b"x").unwrap();
        assert!(working.session.draft());
        // All input went to the writer, in every status.
        assert_eq!(question.writer.all_bytes(), b"1\ryes");
        assert_eq!(starting.writer.all_bytes(), b"abc");
    }

    #[test]
    fn codex_session_with_no_prompt_gets_no_paste() {
        let rig = Rig::new(CliKind::Codex);
        assert_eq!(
            rig.session.paste("m"),
            Err(PasteRefused::Status(Status::Starting))
        );
        // The first hook event (SessionStart) gives working, not idle.
        rig.hook("SessionStart");
        assert_eq!(rig.session.status(), Status::Working);
        assert_eq!(
            rig.session.paste("m"),
            Err(PasteRefused::Status(Status::Working))
        );
        rig.hook("UserPromptSubmit");
        rig.hook("Stop");
        assert_eq!(rig.session.status(), Status::Idle);
        rig.session.paste("m").unwrap();
        assert_eq!(rig.writer.write_count(), 1);
        // Idle with no prompt: Interrupt (a Codex dialog answered by Esc, for example).
        let rig = Rig::new(CliKind::Codex);
        rig.hook("SessionStart");
        rig.hook("Interrupt");
        assert_eq!(rig.session.status(), Status::Idle);
        assert_eq!(
            rig.session.paste("m"),
            Err(PasteRefused::NotAllowedByMachine)
        );
        assert_eq!(rig.writer.write_count(), 0);
    }

    #[test]
    fn after_process_exit_everything_is_refused_and_nothing_changes() {
        let rig = Rig::claude_idle();
        rig.changes();
        rig.session
            .on_pty_event(PtyEvent::Exit(crate::pty::PtyExit {
                code: 0,
                signal: None,
            }));
        assert_eq!(rig.session.status(), Status::Stopped);
        assert_eq!(rig.changes(), vec![(Status::Idle, Status::Stopped)]);
        assert_eq!(rig.session.paste("m"), Err(PasteRefused::Stopped));
        assert!(rig.session.message_waiting());
        assert!(rig.session.operator_input(b"abc").is_err());
        assert!(!rig.session.draft());
        rig.hook("UserPromptSubmit");
        rig.session.on_pty_event(PtyEvent::Output {
            bytes: b"x".to_vec(),
            at_ms: 5,
        });
        assert_eq!(rig.session.tick(), None);
        assert_eq!(rig.session.status(), Status::Stopped);
        assert_eq!(rig.writer.write_count(), 0);
        assert_eq!(rig.changes(), vec![]);
        assert_eq!(rig.session.next_deadline(), None);
    }

    #[test]
    fn sessionend_hook_gives_stopped_and_refuses_paste() {
        let rig = Rig::claude_idle();
        rig.hook("SessionEnd");
        assert_eq!(rig.session.paste("m"), Err(PasteRefused::Stopped));
    }

    #[test]
    fn the_end_mark_in_the_text_cannot_end_the_paste() {
        let rig = Rig::claude_idle();
        let evil = "before\x1b[201~\rrm -rf ~\r\x1b[200~after";
        rig.session.paste(evil).unwrap();
        let bytes = rig.writer.writes().remove(0);
        // Exactly one end mark, at the end, and no ESC in the middle.
        assert_eq!(
            bytes
                .windows(PASTE_END.len())
                .filter(|w| *w == PASTE_END)
                .count(),
            1
        );
        assert!(bytes.ends_with(b"\x1b[201~\r"));
        let middle = &bytes[PASTE_START.len()..bytes.len() - PASTE_END.len() - 1];
        assert!(!middle.contains(&0x1b));
        // A mark that forms after one removal is also gone.
        let rig = Rig::claude_idle();
        rig.session.paste("a\x1b[2\x1b[201~01~b").unwrap();
        let bytes = rig.writer.writes().remove(0);
        assert_eq!(
            bytes
                .windows(PASTE_END.len())
                .filter(|w| *w == PASTE_END)
                .count(),
            1,
            "{bytes:?}"
        );
        // Plain text, tabs and newlines are not changed.
        assert_eq!(sanitize_paste("a\tb\nc é"), "a\tb\nc é");
    }

    // ---- clock, ticks, streams ----------------------------------------------------------------

    #[test]
    fn status_changes_and_output_go_to_the_streams() {
        let rig = Rig::new(CliKind::Claude);
        rig.clock.advance(100);
        rig.session.set_pty_origin(Millis(0));
        rig.session.on_pty_event(PtyEvent::Output {
            bytes: b"hi".to_vec(),
            at_ms: 40,
        });
        rig.hook("SessionStart");
        let output: Vec<OutputChunk> = rig.streams.output.try_iter().collect();
        assert_eq!(
            output,
            vec![OutputChunk {
                bytes: b"hi".to_vec(),
                at: Millis(40)
            }]
        );
        let change = rig.streams.status.try_recv().unwrap();
        assert_eq!((change.old, change.new), (Status::Starting, Status::Idle));
        assert_eq!(change.at, Millis(110));
        // A converted time never passes "now".
        rig.session.on_pty_event(PtyEvent::Output {
            bytes: vec![1],
            at_ms: 10_000,
        });
        assert_eq!(rig.streams.output.try_recv().unwrap().at, Millis(110));
    }

    #[test]
    fn start_limit_is_applied_by_tick_and_by_paste() {
        let rig = Rig::new(CliKind::Claude);
        assert_eq!(rig.session.next_deadline(), Some(Millis(20_001)));
        rig.clock.advance(20_001);
        assert_eq!(rig.session.tick(), Some(Status::Question));
        assert_eq!(rig.changes(), vec![(Status::Starting, Status::Question)]);
        // A paste applies the timed rules before its check, so the reason is up to date.
        let rig = Rig::new(CliKind::Claude);
        rig.clock.advance(20_001);
        assert_eq!(
            rig.session.paste("m"),
            Err(PasteRefused::Status(Status::Question))
        );
    }

    #[test]
    fn quiet_rule_with_a_manual_clock() {
        let rig = Rig::claude_idle();
        rig.hook("UserPromptSubmit");
        rig.hook("PreToolUse");
        rig.hook("PermissionRequest");
        rig.changes();
        rig.session.operator_input(b"2").unwrap(); // deny: no hook follows
        let deadline = rig.session.next_deadline().expect("quiet watch");
        assert_eq!(rig.session.tick(), None);
        rig.clock.advance(deadline.since(rig.clock.now()));
        assert_eq!(rig.session.tick(), Some(Status::Idle));
        // The deny ended the turn, so a paste is allowed again (spec 4.5 rule 3).
        rig.session.paste("m").unwrap();
    }

    #[test]
    fn router_sends_events_to_the_session_by_id() {
        use crate::hookrx::HookMessage;
        let (source, messages) = mpsc::channel();
        let router = HookRouter::start(messages);
        let one = router.subscribe("one");
        let two = router.subscribe("two");
        let send = |session: &str, name: &str| {
            source
                .send(HookMessage {
                    session: session.into(),
                    event: hook(name),
                    reply: None,
                })
                .unwrap();
        };
        send("one", "A");
        send("two", "B");
        send("none", "C");
        send("one", "D");
        let wait = Duration::from_secs(5);
        assert_eq!(one.recv_timeout(wait).unwrap()["hook_event_name"], "A");
        assert_eq!(one.recv_timeout(wait).unwrap()["hook_event_name"], "D");
        assert_eq!(two.recv_timeout(wait).unwrap()["hook_event_name"], "B");
        assert_eq!(router.unrouted_count(), 1);
        router.unsubscribe("one");
        send("one", "E");
        send("two", "F");
        assert_eq!(two.recv_timeout(wait).unwrap()["hook_event_name"], "F");
        assert_eq!(router.unrouted_count(), 2);
        assert!(matches!(
            one.recv_timeout(Duration::from_millis(50)),
            Err(RecvTimeoutError::Disconnected)
        ));
    }

    // ---- the race test (plan step 4) ------------------------------------------------------------

    /// A recording writer that also writes a line in a shared log.
    struct LoggingWriter {
        log: Arc<Mutex<Vec<&'static str>>>,
        inner: RecordingWriter,
    }

    impl PtyWriter for LoggingWriter {
        fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
            locked(&self.log).push("write");
            self.inner.write_all(bytes)
        }
    }

    /// Thread A pastes and stops at the pause point (inside the lock). Thread B applies
    /// `PermissionRequest`. B must wait for the lock until the write ends. If the status was
    /// read before the lock, B goes first and A writes during `question`.
    #[test]
    fn race_paste_and_permission_request() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let recorder = RecordingWriter::new();
        let writer = LoggingWriter {
            log: Arc::clone(&log),
            inner: recorder.clone(),
        };
        let (session, _streams) =
            Session::new(CliKind::Claude, "s1", Box::new(writer), Clock::manual());
        session.on_hook(&hook("SessionStart"));
        assert_eq!(session.status(), Status::Idle);

        let (reached_tx, reached) = mpsc::channel::<()>();
        let (release_tx, release) = mpsc::channel::<()>();
        let release = Mutex::new(release);
        session.set_pause_point(move || {
            let _ = reached_tx.send(());
            let _ = locked(&release).recv_timeout(Duration::from_secs(10));
        });

        let a = {
            let session = session.clone();
            thread::spawn(move || session.paste("hello"))
        };
        reached
            .recv_timeout(Duration::from_secs(10))
            .expect("A reaches the pause point");

        let (b_done_tx, b_done) = mpsc::channel::<()>();
        let b = {
            let session = session.clone();
            let log = Arc::clone(&log);
            thread::spawn(move || {
                session.on_hook(&hook("PermissionRequest"));
                locked(&log).push("hook applied");
                let _ = b_done_tx.send(());
            })
        };
        // B must be blocked on the mutex while A is paused. In the wrong build B returns at
        // once, so this wait fails on each run. In the right build B cannot return, so the
        // wait always ends by its limit.
        assert!(
            b_done.recv_timeout(Duration::from_millis(300)).is_err(),
            "B did not wait for A: the status was read outside the lock"
        );
        assert!(
            locked(&log).is_empty(),
            "nothing happened while A was paused"
        );

        release_tx.send(()).unwrap();
        a.join().unwrap().expect("paste worked");
        b.join().unwrap();
        assert_eq!(*locked(&log), vec!["write", "hook applied"]);
        assert_eq!(recorder.write_count(), 1);
        assert_eq!(session.status(), Status::Question);
        // And now a paste is refused.
        assert_eq!(
            session.paste("again"),
            Err(PasteRefused::Status(Status::Question))
        );
        assert_eq!(recorder.write_count(), 1);
    }
}
