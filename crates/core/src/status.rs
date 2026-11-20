//! Status state machine of one session (spec 4.3).
//!
//! The machine is pure. It does no I/O and it reads no clock. The caller gives the time with each
//! input as a [`Millis`] value, so tests are exact. Two rules depend on time: the quiet watch of
//! Claude after Esc, Ctrl-C or an answer key (2 s with no PTY output), and the start limit
//! (20 s with no hook). The machine checks both rules at the start of each [`Machine::apply`]. The
//! caller should also send [`Input::Tick`] at [`Machine::next_deadline`], so that the change shows
//! at the right time and not at the next input.
//!
//! "Last event wins": the machine never counts submits against stops. "Last event" means the last
//! event that the table of spec 4.3 maps. Ignored events (`SubagentStop`, `Notification` of the
//! type `idle_prompt`, unknown names) do not change it. They do not cancel a quiet watch either.

use crate::model::CliKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A session with no hook event for longer than this gets the status `question` (spec 4.2).
pub const STARTING_LIMIT_MS: u64 = 20_000;

/// Time with no PTY output that ends a Claude interrupt or deny (spec 4.3).
pub const QUIET_MS: u64 = 2_000;

/// How recent the last PTY output must be, at 2000 ms after an answer key, to count as "output
/// still comes" (spec 4.3, approval of a long tool). The output counts if it is less than this
/// many milliseconds old at that time. A deny stops its output within 0.03 to 0.14 s of the key
/// (PROBED, c4), so its last output is about 1900 ms old. A tool that runs draws each 0.1 s, a
/// permission prompt each 0.6 s.
pub const APPROVAL_ACTIVE_MS: u64 = 1_000;

/// The status of a session (spec 4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Starting,
    Working,
    Idle,
    Question,
    Done,
    Stopped,
}

impl Status {
    /// The lower-case name of the spec. It is also the serde name.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Starting => "starting",
            Status::Working => "working",
            Status::Idle => "idle",
            Status::Question => "question",
            Status::Done => "done",
            Status::Stopped => "stopped",
        }
    }

    /// The reverse of [`Status::as_str`]. `None` for an unknown name.
    pub fn parse(name: &str) -> Option<Status> {
        Some(match name {
            "starting" => Status::Starting,
            "working" => Status::Working,
            "idle" => Status::Idle,
            "question" => Status::Question,
            "done" => Status::Done,
            "stopped" => Status::Stopped,
            _ => return None,
        })
    }
}

/// Time in milliseconds. The caller chooses the zero point. The machine only subtracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Millis(pub u64);

impl Millis {
    /// This time plus `ms` milliseconds.
    pub fn plus(self, ms: u64) -> Millis {
        Millis(self.0.saturating_add(ms))
    }

    /// Milliseconds from `earlier` to this time. Zero if `earlier` is later.
    pub fn since(self, earlier: Millis) -> u64 {
        self.0.saturating_sub(earlier.0)
    }
}

/// The name of a hook event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookKind {
    SessionStart,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    PermissionRequest,
    Stop,
    Interrupt,
    /// Gives `Stopped` for both CLIs. The Codex logs show a `SessionEnd` too.
    SessionEnd,
    Notification,
    SubagentStop,
    /// Any other name, also the empty name of a payload with no `hook_event_name`.
    Other(String),
}

impl HookKind {
    pub fn from_name(name: &str) -> HookKind {
        match name {
            "SessionStart" => HookKind::SessionStart,
            "UserPromptSubmit" => HookKind::UserPromptSubmit,
            "PreToolUse" => HookKind::PreToolUse,
            "PostToolUse" => HookKind::PostToolUse,
            "PermissionRequest" => HookKind::PermissionRequest,
            "Stop" => HookKind::Stop,
            "Interrupt" => HookKind::Interrupt,
            "SessionEnd" => HookKind::SessionEnd,
            "Notification" => HookKind::Notification,
            "SubagentStop" => HookKind::SubagentStop,
            other => HookKind::Other(other.to_owned()),
        }
    }
}

/// The fields of a hook payload that the rules need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookEvent {
    pub kind: HookKind,
    /// `notification_type` of a Claude `Notification`.
    pub notification_type: Option<String>,
    pub session_id: Option<String>,
    /// `source` of a `SessionStart` (`startup`, `resume`, ...).
    pub source: Option<String>,
    /// The payload has a `prompt` field (`UserPromptSubmit`).
    pub has_prompt: bool,
}

impl HookEvent {
    /// An event with only a kind. Tests and adapters use it as a start.
    pub fn new(kind: HookKind) -> HookEvent {
        HookEvent {
            kind,
            notification_type: None,
            session_id: None,
            source: None,
            has_prompt: false,
        }
    }

    /// A `Notification` with a type.
    pub fn notification(notification_type: &str) -> HookEvent {
        HookEvent {
            notification_type: Some(notification_type.to_owned()),
            ..HookEvent::new(HookKind::Notification)
        }
    }

    /// Read the raw hook JSON of a CLI. This never fails: a payload with no usable
    /// `hook_event_name` gives `Other("")`, and the machine ignores it.
    pub fn from_json(value: &Value) -> HookEvent {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        let name = value
            .get("hook_event_name")
            .and_then(Value::as_str)
            .unwrap_or("");
        HookEvent {
            kind: HookKind::from_name(name),
            notification_type: text("notification_type"),
            session_id: text("session_id"),
            source: text("source"),
            has_prompt: value.get("prompt").is_some_and(|prompt| !prompt.is_null()),
        }
    }
}

/// An operator key that matters to the machine. The key classifier of the session makes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    Esc,
    CtrlC,
    /// A key that can answer a prompt: Enter.
    Enter,
    /// A key that can answer a prompt: a digit.
    Digit,
    /// Any other key (arrows, letters). The machine ignores it.
    Other,
}

/// What the machine receives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Hook(HookEvent),
    OperatorKey(KeyKind),
    /// Some bytes came from the PTY. The content does not matter.
    PtyOutput,
    /// Time passed. No other meaning.
    Tick,
    ProcessExit,
    /// The agent set its task to `review` or `done`. The next turn end gives `done`.
    TaskMarkedDone,
    /// The Stop hook of the app returned `block`. The app sends this BEFORE the machine gets that
    /// `Stop` event. That `Stop` keeps `working` (spec 4.3, M2).
    StopBlockedByApp,
}

/// The last event that the table of spec 4.3 mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappedEvent {
    SessionStart,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    PermissionRequest,
    Stop,
    Interrupt,
    SessionEnd,
    /// A Claude `Notification` of the type `permission_prompt` that gave `question`.
    PermissionNotification,
}

/// The state machine of one session.
#[derive(Debug, Clone)]
pub struct Machine {
    cli: CliKind,
    status: Status,
    started_at: Millis,
    last_event: Option<MappedEvent>,
    /// Claude only: a prompt was submitted during a turn. The next `Stop` keeps `working`.
    queued_turn: bool,
    last_output: Option<Millis>,
    /// Claude only: time of the key that started the quiet watch.
    quiet_watch: Option<Millis>,
    /// The quiet watch started with an answer key (Enter or a digit) in `question`, and the check
    /// at key + 2000 ms did not run yet.
    approval_check: bool,
    task_done: bool,
    stop_blocked: bool,
    /// A `PreToolUse` or `PermissionRequest` came after the last `Stop`.
    tool_since_stop: bool,
    /// A `UserPromptSubmit` came.
    prompt_seen: bool,
    /// `question` came from the start limit. A key then answers a startup dialog, not a prompt.
    startup_question: bool,
}

impl Machine {
    /// A new machine in `starting`. `started_at` is the time of the process start.
    pub fn new(cli: CliKind, started_at: Millis) -> Machine {
        Machine {
            cli,
            status: Status::Starting,
            started_at,
            last_event: None,
            queued_turn: false,
            last_output: None,
            quiet_watch: None,
            approval_check: false,
            task_done: false,
            stop_blocked: false,
            tool_since_stop: false,
            prompt_seen: false,
            startup_question: false,
        }
    }

    pub fn status(&self) -> Status {
        self.status
    }

    pub fn cli(&self) -> CliKind {
        self.cli
    }

    pub fn started_at(&self) -> Millis {
        self.started_at
    }

    pub fn last_event(&self) -> Option<MappedEvent> {
        self.last_event
    }

    /// The Claude queued-turn mark.
    pub fn queued_turn(&self) -> bool {
        self.queued_turn
    }

    pub fn last_output(&self) -> Option<Millis> {
        self.last_output
    }

    /// The time of the key that started the quiet watch, if a watch runs.
    pub fn quiet_watch_since(&self) -> Option<Millis> {
        self.quiet_watch
    }

    /// The time at which a rule fires if nothing else happens. `None` if no rule waits.
    /// The caller sends [`Input::Tick`] at this time.
    pub fn next_deadline(&self) -> Option<Millis> {
        if self.status == Status::Stopped {
            return None;
        }
        let starting =
            (self.status == Status::Starting).then(|| self.started_at.plus(STARTING_LIMIT_MS + 1));
        let quiet = self.quiet_deadline();
        let approval = self.approval_deadline();
        [starting, quiet, approval].into_iter().flatten().min()
    }

    /// Whether the paste gate of spec 4.5 lets a paste pass, as far as the status goes: the status
    /// is `idle` or `done`, no `PreToolUse` or `PermissionRequest` came after the last `Stop`,
    /// and a Codex session has had a prompt. The session adds the draft flag and the lock.
    pub fn paste_allowed(&self) -> bool {
        matches!(self.status, Status::Idle | Status::Done)
            && !self.tool_since_stop
            && (self.cli != CliKind::Codex || self.prompt_seen)
    }

    /// Apply one input at time `now`. Returns the new status if it changed.
    ///
    /// The timed rules are checked first, so that a rule whose time came is not lost. If a timed
    /// rule and the input both change the status, the result is the net change from the status
    /// before the call. After `Stopped` nothing changes the status.
    pub fn apply(&mut self, input: Input, now: Millis) -> Option<Status> {
        let before = self.status;
        if self.status != Status::Stopped {
            self.expire(now);
            self.handle(input, now);
        }
        (self.status != before).then_some(self.status)
    }

    fn quiet_deadline(&self) -> Option<Millis> {
        let key = self.quiet_watch?;
        let from = self.last_output.map_or(key, |output| output.max(key));
        Some(from.plus(QUIET_MS))
    }

    /// The time of the approval check: 2000 ms after an answer key in `question`.
    fn approval_deadline(&self) -> Option<Millis> {
        self.approval_check
            .then(|| self.quiet_watch.map(|key| key.plus(QUIET_MS)))
            .flatten()
    }

    fn clear_watch(&mut self) {
        self.quiet_watch = None;
        self.approval_check = false;
    }

    /// The timed rules.
    fn expire(&mut self, now: Millis) {
        if self.status == Status::Starting && now.since(self.started_at) > STARTING_LIMIT_MS {
            self.status = Status::Question;
            self.startup_question = true;
        }
        // Approval of a long tool. 2000 ms after an answer key, if the output still comes (the last
        // output is less than APPROVAL_ACTIVE_MS old at key + 2000 ms), the operator approved and
        // the tool runs with no hook: `working`. A false `working` is safe, because the app pastes
        // only in `idle` and `done`. A prompt that stays and redraws (an option that asks for
        // text) also gives `working`: accepted. The next hook event or Esc rule corrects it.
        if let Some(deadline) = self.approval_deadline()
            && now >= deadline
        {
            self.approval_check = false;
            let key = deadline.since(Millis(QUIET_MS));
            let still_comes = self.last_output.is_some_and(|output| {
                deadline.since(output) < APPROVAL_ACTIVE_MS && output.0 > key
            });
            if still_comes {
                self.clear_watch();
                self.status = Status::Working;
                return;
            }
        }
        if let Some(deadline) = self.quiet_deadline()
            && now >= deadline
        {
            self.clear_watch();
            self.queued_turn = false;
            self.tool_since_stop = false;
            self.status = Status::Idle;
        }
    }

    fn handle(&mut self, input: Input, now: Millis) {
        match input {
            Input::Hook(event) => self.on_hook(&event),
            Input::OperatorKey(key) => self.on_key(key, now),
            Input::PtyOutput => self.last_output = Some(now),
            Input::Tick => {}
            Input::ProcessExit => self.stop(),
            Input::TaskMarkedDone => self.task_done = true,
            Input::StopBlockedByApp => self.stop_blocked = true,
        }
    }

    fn on_key(&mut self, key: KeyKind, now: Millis) {
        if self.cli != CliKind::Claude || self.startup_question {
            return;
        }
        let starts_watch = match self.status {
            Status::Working => matches!(key, KeyKind::Esc | KeyKind::CtrlC),
            Status::Question => !matches!(key, KeyKind::Other),
            _ => false,
        };
        if starts_watch {
            self.quiet_watch = Some(now);
            self.approval_check =
                self.status == Status::Question && matches!(key, KeyKind::Enter | KeyKind::Digit);
        }
    }

    fn on_hook(&mut self, event: &HookEvent) {
        let claude = self.cli == CliKind::Claude;
        let (mapped, status) = match &event.kind {
            HookKind::SessionStart => (
                MappedEvent::SessionStart,
                if claude {
                    Status::Idle
                } else {
                    Status::Working
                },
            ),
            HookKind::UserPromptSubmit => {
                self.prompt_seen = true;
                if self.status == Status::Working {
                    // Claude starts a queued turn after the next Stop, with no event at its start.
                    // A Codex steering prompt does not queue a turn.
                    self.queued_turn |= claude;
                } else {
                    self.queued_turn = false;
                    self.task_done = false;
                }
                (MappedEvent::UserPromptSubmit, Status::Working)
            }
            HookKind::PreToolUse => {
                self.tool_since_stop = true;
                (MappedEvent::PreToolUse, Status::Working)
            }
            HookKind::PostToolUse => (MappedEvent::PostToolUse, Status::Working),
            HookKind::PermissionRequest => {
                self.tool_since_stop = true;
                (MappedEvent::PermissionRequest, Status::Question)
            }
            HookKind::Stop => {
                self.tool_since_stop = false;
                (MappedEvent::Stop, self.status_after_stop(claude))
            }
            HookKind::Interrupt if !claude => {
                // A turn end with no Stop: it resets the tool condition like a Stop does.
                self.tool_since_stop = false;
                (MappedEvent::Interrupt, Status::Idle)
            }
            // Stopped for both CLIs (the Codex logs show a SessionEnd too).
            HookKind::SessionEnd => (MappedEvent::SessionEnd, Status::Stopped),
            HookKind::Notification
                if claude
                    && event.notification_type.as_deref() == Some("permission_prompt")
                    && self.last_event == Some(MappedEvent::PreToolUse) =>
            {
                (MappedEvent::PermissionNotification, Status::Question)
            }
            // Ignored: SubagentStop, idle_prompt, a late permission_prompt, Interrupt for Claude,
            // unknown names. They change nothing and they do not cancel a quiet watch.
            _ => return,
        };
        self.last_event = Some(mapped);
        self.clear_watch();
        self.startup_question = false;
        if status == Status::Idle && mapped != MappedEvent::Stop {
            self.queued_turn = false;
        }
        if status == Status::Stopped {
            self.stop();
        } else {
            self.status = status;
        }
    }

    /// The status that a `Stop` gives, and the marks that it uses up.
    fn status_after_stop(&mut self, claude: bool) -> Status {
        if self.stop_blocked {
            self.stop_blocked = false;
            Status::Working
        } else if claude && self.queued_turn {
            self.queued_turn = false;
            Status::Working
        } else if self.task_done {
            self.task_done = false;
            Status::Done
        } else {
            Status::Idle
        }
    }

    fn stop(&mut self) {
        self.status = Status::Stopped;
        self.clear_watch();
        self.queued_turn = false;
        self.startup_question = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A machine with a clock that the test moves by hand.
    struct Rig {
        machine: Machine,
        now: u64,
    }

    impl Rig {
        fn new(cli: CliKind) -> Rig {
            Rig {
                machine: Machine::new(cli, Millis(0)),
                now: 0,
            }
        }

        fn claude() -> Rig {
            Rig::new(CliKind::Claude)
        }

        fn codex() -> Rig {
            Rig::new(CliKind::Codex)
        }

        /// Move the clock to `ms`, then apply the input.
        fn at(&mut self, ms: u64, input: Input) -> Option<Status> {
            self.now = ms;
            self.machine.apply(input, Millis(ms))
        }

        /// Apply the input `ms` after the last input.
        fn after(&mut self, ms: u64, input: Input) -> Option<Status> {
            self.at(self.now + ms, input)
        }

        fn hook(&mut self, kind: HookKind) -> Option<Status> {
            self.after(10, Input::Hook(HookEvent::new(kind)))
        }

        fn notify(&mut self, notification_type: &str) -> Option<Status> {
            self.after(10, Input::Hook(HookEvent::notification(notification_type)))
        }

        fn status(&self) -> Status {
            self.machine.status()
        }

        /// A Claude rig in `working` (after `SessionStart` and `UserPromptSubmit`).
        fn claude_working() -> Rig {
            let mut rig = Rig::claude();
            rig.hook(HookKind::SessionStart);
            rig.hook(HookKind::UserPromptSubmit);
            assert_eq!(rig.status(), Status::Working);
            rig
        }

        /// A Claude rig in `question` (permission prompt on screen).
        fn claude_question() -> Rig {
            let mut rig = Rig::claude_working();
            rig.hook(HookKind::PreToolUse);
            rig.hook(HookKind::PermissionRequest);
            assert_eq!(rig.status(), Status::Question);
            rig
        }
    }

    // ---- parsing -----------------------------------------------------------------------------

    #[test]
    fn status_names_and_serde() {
        let all = [
            (Status::Starting, "starting"),
            (Status::Working, "working"),
            (Status::Idle, "idle"),
            (Status::Question, "question"),
            (Status::Done, "done"),
            (Status::Stopped, "stopped"),
        ];
        for (status, name) in all {
            assert_eq!(status.as_str(), name);
            assert_eq!(Status::parse(name), Some(status));
            assert_eq!(
                serde_json::to_string(&status).unwrap(),
                format!("\"{name}\"")
            );
            assert_eq!(
                serde_json::from_str::<Status>(&format!("\"{name}\"")).unwrap(),
                status
            );
        }
        assert_eq!(Status::parse("busy"), None);
    }

    #[test]
    fn hook_event_parses_the_fields_that_the_rules_need() {
        let value = serde_json::json!({
            "hook_event_name": "Notification",
            "notification_type": "permission_prompt",
            "session_id": "abc",
        });
        let event = HookEvent::from_json(&value);
        assert_eq!(event.kind, HookKind::Notification);
        assert_eq!(
            event.notification_type.as_deref(),
            Some("permission_prompt")
        );
        assert_eq!(event.session_id.as_deref(), Some("abc"));
        assert!(!event.has_prompt);

        let value = serde_json::json!({
            "hook_event_name": "SessionStart",
            "source": "resume",
        });
        assert_eq!(
            HookEvent::from_json(&value).source.as_deref(),
            Some("resume")
        );

        let value = serde_json::json!({"hook_event_name": "UserPromptSubmit", "prompt": "hi"});
        let event = HookEvent::from_json(&value);
        assert_eq!(event.kind, HookKind::UserPromptSubmit);
        assert!(event.has_prompt);
    }

    #[test]
    fn unknown_and_missing_event_names_parse_to_other_and_are_ignored() {
        let value = serde_json::json!({"hook_event_name": "PreCompact"});
        assert_eq!(
            HookEvent::from_json(&value).kind,
            HookKind::Other("PreCompact".into())
        );
        assert_eq!(
            HookEvent::from_json(&serde_json::json!({})).kind,
            HookKind::Other(String::new())
        );
        assert_eq!(
            HookEvent::from_json(&serde_json::json!("text")).kind,
            HookKind::Other(String::new())
        );

        let mut rig = Rig::claude_working();
        assert_eq!(rig.hook(HookKind::Other("PreCompact".into())), None);
        assert_eq!(rig.status(), Status::Working);
    }

    // ---- the table of spec 4.3 -----------------------------------------------------------------

    #[test]
    fn claude_session_start_gives_idle() {
        let mut rig = Rig::claude();
        assert_eq!(rig.status(), Status::Starting);
        assert_eq!(rig.hook(HookKind::SessionStart), Some(Status::Idle));
    }

    #[test]
    fn codex_stays_starting_on_pty_output_and_tick() {
        let mut rig = Rig::codex();
        assert_eq!(rig.at(100, Input::PtyOutput), None);
        assert_eq!(rig.at(5_000, Input::Tick), None);
        assert_eq!(rig.at(5_100, Input::PtyOutput), None);
        assert_eq!(rig.at(15_000, Input::Tick), None);
        assert_eq!(rig.status(), Status::Starting);
        assert!(!rig.machine.paste_allowed());
    }

    #[test]
    fn codex_session_start_gives_working_not_idle() {
        let mut rig = Rig::codex();
        assert_eq!(rig.hook(HookKind::SessionStart), Some(Status::Working));
        // The prompt that follows keeps working.
        assert_eq!(rig.hook(HookKind::UserPromptSubmit), None);
        assert_eq!(rig.status(), Status::Working);
    }

    #[test]
    fn codex_first_event_other_than_session_start_also_leaves_starting() {
        let mut rig = Rig::codex();
        assert_eq!(rig.hook(HookKind::UserPromptSubmit), Some(Status::Working));
    }

    #[test]
    fn working_events_give_working() {
        for kind in [
            HookKind::UserPromptSubmit,
            HookKind::PreToolUse,
            HookKind::PostToolUse,
        ] {
            let mut rig = Rig::claude();
            rig.hook(HookKind::SessionStart);
            assert_eq!(rig.hook(kind.clone()), Some(Status::Working), "{kind:?}");
        }
    }

    #[test]
    fn permission_request_gives_question_and_approval_gives_working() {
        let mut rig = Rig::claude_working();
        rig.hook(HookKind::PreToolUse);
        assert_eq!(
            rig.hook(HookKind::PermissionRequest),
            Some(Status::Question)
        );
        assert_eq!(rig.hook(HookKind::PostToolUse), Some(Status::Working));
        assert_eq!(rig.hook(HookKind::Stop), Some(Status::Idle));
    }

    #[test]
    fn stop_gives_idle_for_both_clis() {
        for mut rig in [Rig::claude(), Rig::codex()] {
            rig.hook(HookKind::UserPromptSubmit);
            assert_eq!(rig.hook(HookKind::Stop), Some(Status::Idle));
        }
    }

    #[test]
    fn codex_interrupt_gives_idle_and_claude_ignores_it() {
        let mut codex = Rig::codex();
        codex.hook(HookKind::UserPromptSubmit);
        codex.hook(HookKind::PreToolUse);
        codex.hook(HookKind::PermissionRequest);
        assert_eq!(codex.status(), Status::Question);
        assert_eq!(codex.hook(HookKind::Interrupt), Some(Status::Idle));

        let mut claude = Rig::claude_working();
        assert_eq!(claude.hook(HookKind::Interrupt), None);
        assert_eq!(claude.status(), Status::Working);
    }

    #[test]
    fn session_end_and_process_exit_give_stopped() {
        let mut rig = Rig::claude_working();
        assert_eq!(rig.hook(HookKind::SessionEnd), Some(Status::Stopped));

        let mut rig = Rig::claude_working();
        assert_eq!(rig.after(10, Input::ProcessExit), Some(Status::Stopped));

        let mut rig = Rig::codex();
        rig.hook(HookKind::UserPromptSubmit);
        assert_eq!(rig.after(10, Input::ProcessExit), Some(Status::Stopped));

        // A session that never left `starting` also stops.
        let mut rig = Rig::codex();
        assert_eq!(rig.after(10, Input::ProcessExit), Some(Status::Stopped));
    }

    #[test]
    fn nothing_changes_stopped() {
        let mut rig = Rig::claude_working();
        rig.after(10, Input::ProcessExit);
        let inputs = [
            Input::Hook(HookEvent::new(HookKind::SessionStart)),
            Input::Hook(HookEvent::new(HookKind::UserPromptSubmit)),
            Input::Hook(HookEvent::new(HookKind::PermissionRequest)),
            Input::Hook(HookEvent::new(HookKind::Stop)),
            Input::OperatorKey(KeyKind::Esc),
            Input::PtyOutput,
            Input::Tick,
            Input::TaskMarkedDone,
            Input::StopBlockedByApp,
            Input::ProcessExit,
        ];
        for input in inputs {
            assert_eq!(rig.after(5_000, input.clone()), None, "{input:?}");
            assert_eq!(rig.status(), Status::Stopped);
        }
        assert_eq!(rig.machine.next_deadline(), None);
        assert!(!rig.machine.paste_allowed());
    }

    #[test]
    fn stopped_during_a_quiet_watch_stays_stopped() {
        let mut rig = Rig::claude_working();
        rig.after(10, Input::OperatorKey(KeyKind::CtrlC));
        rig.after(10, Input::ProcessExit);
        assert_eq!(rig.after(10_000, Input::Tick), None);
        assert_eq!(rig.status(), Status::Stopped);
    }

    #[test]
    fn apply_returns_a_status_only_when_it_changes() {
        let mut rig = Rig::claude();
        assert_eq!(rig.hook(HookKind::SessionStart), Some(Status::Idle));
        assert_eq!(rig.hook(HookKind::SessionStart), None);
        assert_eq!(rig.after(1, Input::PtyOutput), None);
        assert_eq!(rig.after(1, Input::Tick), None);
    }

    // ---- last event wins -----------------------------------------------------------------------

    #[test]
    fn claude_queued_turn_keeps_working_for_one_stop() {
        let mut rig = Rig::claude();
        rig.hook(HookKind::SessionStart);
        rig.hook(HookKind::UserPromptSubmit);
        assert!(!rig.machine.queued_turn());
        // A prompt during the turn sets the mark.
        assert_eq!(rig.hook(HookKind::UserPromptSubmit), None);
        assert!(rig.machine.queued_turn());
        // First Stop: the queued turn runs with no event. Working stays.
        assert_eq!(rig.hook(HookKind::Stop), None);
        assert_eq!(rig.status(), Status::Working);
        assert!(!rig.machine.queued_turn());
        // Second Stop: idle.
        assert_eq!(rig.hook(HookKind::Stop), Some(Status::Idle));
    }

    #[test]
    fn claude_prompt_in_idle_sets_no_queued_mark() {
        let mut rig = Rig::claude();
        rig.hook(HookKind::SessionStart);
        rig.hook(HookKind::UserPromptSubmit);
        rig.hook(HookKind::Stop);
        rig.hook(HookKind::UserPromptSubmit);
        assert!(!rig.machine.queued_turn());
        assert_eq!(rig.hook(HookKind::Stop), Some(Status::Idle));
    }

    #[test]
    fn codex_steering_two_prompts_one_stop_gives_idle() {
        let mut rig = Rig::codex();
        rig.hook(HookKind::SessionStart);
        rig.hook(HookKind::UserPromptSubmit);
        rig.hook(HookKind::UserPromptSubmit);
        assert!(!rig.machine.queued_turn());
        assert_eq!(rig.hook(HookKind::Stop), Some(Status::Idle));
    }

    #[test]
    fn submits_are_not_counted_against_stops() {
        // Three prompts and one Stop: idle for Codex. The last event wins.
        let mut rig = Rig::codex();
        for _ in 0..3 {
            rig.hook(HookKind::UserPromptSubmit);
        }
        assert_eq!(rig.hook(HookKind::Stop), Some(Status::Idle));
        // Claude: one queued mark for any number of extra prompts. Stop, Stop gives idle.
        let mut rig = Rig::claude_working();
        rig.hook(HookKind::UserPromptSubmit);
        rig.hook(HookKind::UserPromptSubmit);
        rig.hook(HookKind::Stop);
        assert_eq!(rig.hook(HookKind::Stop), Some(Status::Idle));
    }

    #[test]
    fn stop_without_a_submit_still_gives_idle() {
        // A Stop with no UserPromptSubmit before it (a resumed session, a queued turn end).
        let mut rig = Rig::claude();
        rig.hook(HookKind::SessionStart);
        rig.hook(HookKind::PostToolUse);
        assert_eq!(rig.hook(HookKind::Stop), Some(Status::Idle));
    }

    // ---- Notification (Claude) -----------------------------------------------------------------

    #[test]
    fn notification_backup_gives_question_when_pre_tool_use_is_last() {
        // PermissionRequest was lost: the late notification is the backup.
        let mut rig = Rig::claude_working();
        rig.hook(HookKind::PreToolUse);
        assert_eq!(rig.notify("permission_prompt"), Some(Status::Question));
        assert_eq!(
            rig.machine.last_event(),
            Some(MappedEvent::PermissionNotification)
        );
    }

    #[test]
    fn late_notification_after_the_answer_does_not_give_question() {
        // PreToolUse, PermissionRequest, answer, PostToolUse, then the late notification.
        let mut rig = Rig::claude_working();
        rig.hook(HookKind::PreToolUse);
        rig.hook(HookKind::PermissionRequest);
        rig.hook(HookKind::PostToolUse);
        assert_eq!(rig.status(), Status::Working);
        assert_eq!(rig.notify("permission_prompt"), None);
        assert_eq!(rig.status(), Status::Working);
        assert_eq!(rig.machine.last_event(), Some(MappedEvent::PostToolUse));
    }

    #[test]
    fn late_notification_after_the_stop_does_not_give_question() {
        let mut rig = Rig::claude_working();
        rig.hook(HookKind::PreToolUse);
        rig.hook(HookKind::PermissionRequest);
        rig.hook(HookKind::PostToolUse);
        rig.hook(HookKind::Stop);
        assert_eq!(rig.notify("permission_prompt"), None);
        assert_eq!(rig.status(), Status::Idle);
    }

    #[test]
    fn notification_after_permission_request_changes_nothing() {
        let mut rig = Rig::claude_question();
        assert_eq!(rig.notify("permission_prompt"), None);
        assert_eq!(rig.status(), Status::Question);
        assert_eq!(
            rig.machine.last_event(),
            Some(MappedEvent::PermissionRequest)
        );
    }

    #[test]
    fn idle_prompt_notification_is_ignored_in_every_status() {
        let mut rig = Rig::claude_working();
        assert_eq!(rig.notify("idle_prompt"), None);
        assert_eq!(rig.status(), Status::Working);
        rig.hook(HookKind::Stop);
        assert_eq!(rig.notify("idle_prompt"), None);
        assert_eq!(rig.status(), Status::Idle);
        // Also when PreToolUse is the last event.
        let mut rig = Rig::claude_working();
        rig.hook(HookKind::PreToolUse);
        assert_eq!(rig.notify("idle_prompt"), None);
        assert_eq!(rig.status(), Status::Working);
        // And a notification with no type.
        let event = HookEvent::new(HookKind::Notification);
        assert_eq!(rig.after(10, Input::Hook(event)), None);
    }

    #[test]
    fn codex_ignores_notifications() {
        let mut rig = Rig::codex();
        rig.hook(HookKind::UserPromptSubmit);
        rig.hook(HookKind::PreToolUse);
        assert_eq!(rig.notify("permission_prompt"), None);
        assert_eq!(rig.status(), Status::Working);
    }

    #[test]
    fn subagent_stop_is_ignored_and_does_not_count_as_a_stop() {
        let mut rig = Rig::claude_working();
        assert_eq!(rig.hook(HookKind::SubagentStop), None);
        assert_eq!(rig.status(), Status::Working);
        // Also in a queued turn: SubagentStop does not use up the mark.
        rig.hook(HookKind::UserPromptSubmit);
        assert_eq!(rig.hook(HookKind::SubagentStop), None);
        assert!(rig.machine.queued_turn());
        // Also in idle.
        let mut rig = Rig::claude_working();
        rig.hook(HookKind::Stop);
        assert_eq!(rig.hook(HookKind::SubagentStop), None);
        assert_eq!(rig.status(), Status::Idle);
    }

    #[test]
    fn ignored_events_do_not_change_the_last_event() {
        // PreToolUse, SubagentStop, idle_prompt, then the backup notification still works.
        let mut rig = Rig::claude_working();
        rig.hook(HookKind::PreToolUse);
        rig.hook(HookKind::SubagentStop);
        rig.notify("idle_prompt");
        assert_eq!(rig.machine.last_event(), Some(MappedEvent::PreToolUse));
        assert_eq!(rig.notify("permission_prompt"), Some(Status::Question));
    }

    // ---- Claude quiet watch ----------------------------------------------------------------------

    #[test]
    fn esc_in_working_then_two_seconds_of_quiet_gives_idle() {
        let mut rig = Rig::claude_working();
        let key = rig.now + 1_000;
        rig.at(key, Input::OperatorKey(KeyKind::Esc));
        rig.at(key + 30, Input::PtyOutput);
        assert_eq!(rig.at(key + 100, Input::Tick), None);
        assert_eq!(rig.at(key + 30 + 1_999, Input::Tick), None);
        assert_eq!(rig.status(), Status::Working);
        assert_eq!(rig.at(key + 30 + 2_000, Input::Tick), Some(Status::Idle));
    }

    #[test]
    fn ctrl_c_in_working_then_quiet_gives_idle() {
        let mut rig = Rig::claude_working();
        let key = rig.now + 1_000;
        rig.at(key, Input::OperatorKey(KeyKind::CtrlC));
        assert_eq!(rig.at(key + 1_999, Input::Tick), None);
        assert_eq!(rig.at(key + 2_000, Input::Tick), Some(Status::Idle));
    }

    #[test]
    fn output_after_the_key_moves_the_deadline() {
        // A second Ctrl-C adds one redraw at +0.8 s (spec 4.3). The deadline moves to +2.8 s.
        let mut rig = Rig::claude_working();
        let key = rig.now + 1_000;
        rig.at(key, Input::OperatorKey(KeyKind::CtrlC));
        assert_eq!(rig.machine.next_deadline(), Some(Millis(key + 2_000)));
        rig.at(key + 800, Input::PtyOutput);
        assert_eq!(rig.machine.next_deadline(), Some(Millis(key + 2_800)));
        assert_eq!(rig.at(key + 2_000, Input::Tick), None);
        assert_eq!(rig.at(key + 2_799, Input::Tick), None);
        assert_eq!(rig.status(), Status::Working);
        assert_eq!(rig.at(key + 2_800, Input::Tick), Some(Status::Idle));
    }

    #[test]
    fn continuous_output_keeps_the_watch_open() {
        let mut rig = Rig::claude_working();
        let key = rig.now + 1_000;
        rig.at(key, Input::OperatorKey(KeyKind::Esc));
        for step in 1..=50 {
            assert_eq!(rig.at(key + step * 100, Input::PtyOutput), None);
        }
        assert_eq!(rig.status(), Status::Working);
    }

    #[test]
    fn output_before_the_key_does_not_shorten_the_wait() {
        let mut rig = Rig::claude_working();
        rig.at(5_000, Input::PtyOutput);
        rig.at(8_000, Input::OperatorKey(KeyKind::Esc));
        assert_eq!(rig.machine.next_deadline(), Some(Millis(10_000)));
        assert_eq!(rig.at(9_999, Input::Tick), None);
        assert_eq!(rig.at(10_000, Input::Tick), Some(Status::Idle));
    }

    #[test]
    fn a_hook_event_cancels_the_quiet_watch_and_sets_the_status() {
        let mut rig = Rig::claude_working();
        let key = rig.now + 1_000;
        rig.at(key, Input::OperatorKey(KeyKind::Esc));
        assert_eq!(
            rig.at(
                key + 500,
                Input::Hook(HookEvent::new(HookKind::PostToolUse))
            ),
            None
        );
        assert_eq!(rig.machine.quiet_watch_since(), None);
        // No idle later, even with no output.
        assert_eq!(rig.at(key + 60_000, Input::Tick), None);
        assert_eq!(rig.status(), Status::Working);
        // A Stop in the watch gives idle by its own rule.
        rig.at(key + 61_000, Input::OperatorKey(KeyKind::Esc));
        assert_eq!(
            rig.at(key + 61_500, Input::Hook(HookEvent::new(HookKind::Stop))),
            Some(Status::Idle)
        );
        assert_eq!(rig.machine.quiet_watch_since(), None);
    }

    #[test]
    fn an_ignored_event_does_not_cancel_the_quiet_watch() {
        let mut rig = Rig::claude_working();
        let key = rig.now + 1_000;
        rig.at(key, Input::OperatorKey(KeyKind::Esc));
        rig.at(
            key + 100,
            Input::Hook(HookEvent::new(HookKind::SubagentStop)),
        );
        rig.at(
            key + 200,
            Input::Hook(HookEvent::notification("idle_prompt")),
        );
        assert_eq!(rig.at(key + 2_000, Input::Tick), Some(Status::Idle));
    }

    #[test]
    fn esc_and_ctrl_c_do_not_start_a_watch_in_idle_starting_or_done() {
        let mut rig = Rig::claude();
        rig.at(100, Input::OperatorKey(KeyKind::Esc));
        assert_eq!(rig.machine.quiet_watch_since(), None);
        rig.hook(HookKind::SessionStart);
        assert_eq!(rig.status(), Status::Idle);
        rig.after(10, Input::OperatorKey(KeyKind::CtrlC));
        assert_eq!(rig.machine.quiet_watch_since(), None);
        assert_eq!(rig.after(10_000, Input::Tick), None);
        assert_eq!(rig.status(), Status::Idle);
    }

    #[test]
    fn keys_do_not_start_a_watch_in_codex() {
        let mut rig = Rig::codex();
        rig.hook(HookKind::UserPromptSubmit);
        rig.after(10, Input::OperatorKey(KeyKind::Esc));
        assert_eq!(rig.machine.quiet_watch_since(), None);
        assert_eq!(rig.after(10_000, Input::Tick), None);
        assert_eq!(rig.status(), Status::Working);
        rig.hook(HookKind::PermissionRequest);
        rig.after(10, Input::OperatorKey(KeyKind::Enter));
        assert_eq!(rig.after(10_000, Input::Tick), None);
        assert_eq!(rig.status(), Status::Question);
    }

    #[test]
    fn other_keys_do_not_start_a_watch() {
        let mut rig = Rig::claude_working();
        rig.after(10, Input::OperatorKey(KeyKind::Other));
        rig.after(10, Input::OperatorKey(KeyKind::Enter));
        rig.after(10, Input::OperatorKey(KeyKind::Digit));
        assert_eq!(rig.machine.quiet_watch_since(), None);
        let mut rig = Rig::claude_question();
        rig.after(10, Input::OperatorKey(KeyKind::Other));
        assert_eq!(rig.machine.quiet_watch_since(), None);
        assert_eq!(rig.after(10_000, Input::Tick), None);
        assert_eq!(rig.status(), Status::Question);
    }

    #[test]
    fn the_quiet_watch_fires_at_the_next_input_even_with_no_tick() {
        let mut rig = Rig::claude_working();
        let key = rig.now + 1_000;
        rig.at(key, Input::OperatorKey(KeyKind::Esc));
        // No tick. A late output arrives: the watch fired first, so the status is idle.
        assert_eq!(rig.at(key + 5_000, Input::PtyOutput), Some(Status::Idle));
        // A hook after the deadline: idle first, then the hook sets working (net: no change).
        let mut rig = Rig::claude_working();
        let key = rig.now + 1_000;
        rig.at(key, Input::OperatorKey(KeyKind::Esc));
        assert_eq!(
            rig.at(
                key + 5_000,
                Input::Hook(HookEvent::new(HookKind::PreToolUse))
            ),
            None
        );
        assert_eq!(rig.status(), Status::Working);
    }

    // ---- Claude deny and approval in question --------------------------------------------------

    #[test]
    fn answer_key_in_question_then_no_hook_gives_idle_deny() {
        for key in [KeyKind::Enter, KeyKind::Digit, KeyKind::Esc] {
            let mut rig = Rig::claude_question();
            let at = rig.now + 5_000;
            rig.at(at, Input::OperatorKey(key));
            rig.at(at + 30, Input::PtyOutput);
            assert_eq!(rig.at(at + 2_029, Input::Tick), None, "{key:?}");
            assert_eq!(rig.status(), Status::Question);
            assert_eq!(
                rig.at(at + 2_030, Input::Tick),
                Some(Status::Idle),
                "{key:?}"
            );
        }
    }

    #[test]
    fn ctrl_c_in_question_then_quiet_gives_idle() {
        let mut rig = Rig::claude_question();
        let at = rig.now + 5_000;
        rig.at(at, Input::OperatorKey(KeyKind::CtrlC));
        assert_eq!(rig.at(at + 2_000, Input::Tick), Some(Status::Idle));
    }

    #[test]
    fn answer_key_in_question_then_post_tool_use_gives_working_approval() {
        let mut rig = Rig::claude_question();
        let at = rig.now + 5_000;
        rig.at(at, Input::OperatorKey(KeyKind::Enter));
        rig.at(at + 25, Input::PtyOutput);
        assert_eq!(
            rig.at(at + 900, Input::Hook(HookEvent::new(HookKind::PostToolUse))),
            Some(Status::Working)
        );
        // The watch is gone: no idle later.
        assert_eq!(rig.at(at + 30_000, Input::Tick), None);
        assert_eq!(rig.status(), Status::Working);
    }

    #[test]
    fn answer_key_in_question_then_a_new_pre_tool_use_gives_working() {
        let mut rig = Rig::claude_question();
        let at = rig.now + 5_000;
        rig.at(at, Input::OperatorKey(KeyKind::Enter));
        assert_eq!(
            rig.at(at + 500, Input::Hook(HookEvent::new(HookKind::PreToolUse))),
            Some(Status::Working)
        );
    }

    #[test]
    fn question_with_no_key_never_goes_idle_by_quiet() {
        let mut rig = Rig::claude_question();
        assert_eq!(rig.after(600_000, Input::Tick), None);
        assert_eq!(rig.status(), Status::Question);
    }

    // ---- approval of a long tool (Claude) -------------------------------------------------------
    //
    // Definition: an answer key (Enter or a digit) in `question` starts the watch at time K. At
    // K + 2000 ms the output "still comes" if the last PTY output is more than K + 1000 ms and not
    // more than K + 2000 ms: less than 1000 ms old (APPROVAL_ACTIVE_MS) at K + 2000 ms. Then the
    // status is `working`. Otherwise the quiet rule gives `idle` at max(K, last output) + 2000 ms.

    #[test]
    fn approve_then_a_long_tool_gives_working_at_key_plus_two_seconds() {
        for key in [KeyKind::Enter, KeyKind::Digit] {
            let mut rig = Rig::claude_question();
            let at = rig.now + 5_000;
            rig.at(at, Input::OperatorKey(key));
            for step in 1..=19 {
                assert_eq!(rig.at(at + step * 100, Input::PtyOutput), None);
            }
            assert_eq!(rig.machine.next_deadline(), Some(Millis(at + 2_000)));
            assert_eq!(rig.at(at + 1_999, Input::Tick), None, "{key:?}");
            assert_eq!(rig.status(), Status::Question);
            assert_eq!(
                rig.at(at + 2_000, Input::Tick),
                Some(Status::Working),
                "{key:?}"
            );
            // The output goes on for 30 s: still working, and no later idle.
            for step in 21..=300 {
                assert_eq!(rig.at(at + step * 100, Input::PtyOutput), None);
            }
            assert_eq!(rig.status(), Status::Working);
            assert_eq!(rig.machine.quiet_watch_since(), None);
            assert_eq!(rig.machine.next_deadline(), None);
        }
    }

    #[test]
    fn approve_then_post_tool_use_after_500_ms_gives_working_at_once() {
        let mut rig = Rig::claude_question();
        let at = rig.now + 5_000;
        rig.at(at, Input::OperatorKey(KeyKind::Enter));
        rig.at(at + 100, Input::PtyOutput);
        let event = Input::Hook(HookEvent::new(HookKind::PostToolUse));
        assert_eq!(rig.at(at + 500, event), Some(Status::Working));
        assert_eq!(rig.machine.quiet_watch_since(), None);
        assert_eq!(rig.machine.next_deadline(), None);
        assert_eq!(rig.at(at + 5_000, Input::Tick), None);
    }

    #[test]
    fn deny_gives_idle_and_never_working() {
        // The output stops 30 ms after the key. At key + 2000 ms its age is 1970 ms: not active.
        let mut rig = Rig::claude_question();
        let at = rig.now + 5_000;
        rig.at(at, Input::OperatorKey(KeyKind::Enter));
        rig.at(at + 30, Input::PtyOutput);
        assert_eq!(rig.at(at + 2_000, Input::Tick), None);
        assert_eq!(rig.status(), Status::Question);
        assert_eq!(rig.at(at + 2_030, Input::Tick), Some(Status::Idle));
        // The same with a deny that draws once more at +800 ms: the age at +2000 is 1200 ms.
        let mut rig = Rig::claude_question();
        let at = rig.now + 5_000;
        rig.at(at, Input::OperatorKey(KeyKind::Digit));
        rig.at(at + 30, Input::PtyOutput);
        rig.at(at + 800, Input::PtyOutput);
        assert_eq!(rig.at(at + 2_000, Input::Tick), None);
        assert_eq!(rig.status(), Status::Question);
        assert_eq!(rig.at(at + 2_800, Input::Tick), Some(Status::Idle));
    }

    #[test]
    fn approval_check_edge_is_one_second_of_age() {
        // Last output at K + 1000: age 1000 ms at K + 2000, not less than 1000: idle path.
        let mut rig = Rig::claude_question();
        let at = rig.now + 5_000;
        rig.at(at, Input::OperatorKey(KeyKind::Enter));
        rig.at(at + 1_000, Input::PtyOutput);
        assert_eq!(rig.at(at + 2_000, Input::Tick), None);
        assert_eq!(rig.at(at + 3_000, Input::Tick), Some(Status::Idle));
        // Last output at K + 1001: age 999 ms: working.
        let mut rig = Rig::claude_question();
        let at = rig.now + 5_000;
        rig.at(at, Input::OperatorKey(KeyKind::Enter));
        rig.at(at + 1_001, Input::PtyOutput);
        assert_eq!(rig.at(at + 2_000, Input::Tick), Some(Status::Working));
    }

    #[test]
    fn a_prompt_that_stays_and_redraws_each_600_ms_gives_working_then_stop_gives_idle() {
        // ACCEPTED false working: an option that asks for text keeps the prompt on screen, and the
        // prompt redraws each 0.6 s. The machine cannot tell it from a running tool. A false
        // `working` is safe (the app pastes only in idle and done). The next Stop corrects it.
        let mut rig = Rig::claude_question();
        let at = rig.now + 5_000;
        rig.at(at, Input::OperatorKey(KeyKind::Digit));
        for step in 1..=3 {
            rig.at(at + step * 600, Input::PtyOutput);
        }
        assert_eq!(rig.at(at + 2_000, Input::Tick), Some(Status::Working));
        assert!(!rig.machine.paste_allowed());
        assert_eq!(rig.hook(HookKind::Stop), Some(Status::Idle));
    }

    #[test]
    fn esc_and_ctrl_c_in_question_never_give_working() {
        for key in [KeyKind::Esc, KeyKind::CtrlC] {
            let mut rig = Rig::claude_question();
            let at = rig.now + 5_000;
            rig.at(at, Input::OperatorKey(key));
            for step in 1..=50 {
                assert_eq!(rig.at(at + step * 100, Input::PtyOutput), None, "{key:?}");
            }
            assert_eq!(rig.status(), Status::Question);
            assert_eq!(
                rig.machine.next_deadline().map(|d| d.0 > at + 5_000),
                Some(true)
            );
        }
    }

    #[test]
    fn a_second_key_replaces_the_approval_check() {
        // Enter, then Ctrl-C 500 ms later with output going on: no working at Enter + 2000 ms.
        let mut rig = Rig::claude_question();
        let at = rig.now + 5_000;
        rig.at(at, Input::OperatorKey(KeyKind::Enter));
        rig.at(at + 400, Input::PtyOutput);
        rig.at(at + 500, Input::OperatorKey(KeyKind::CtrlC));
        for step in 6..=30 {
            assert_eq!(rig.at(at + step * 100, Input::PtyOutput), None);
        }
        assert_eq!(rig.status(), Status::Question);
        // The output stops: idle 2000 ms after the last output.
        assert_eq!(rig.at(at + 3_000 + 1_999, Input::Tick), None);
        assert_eq!(rig.at(at + 3_000 + 2_000, Input::Tick), Some(Status::Idle));
    }

    #[test]
    fn approve_then_long_tool_then_ctrl_c_gives_working_then_idle() {
        // The shape of run c4: Enter, output goes on, Working at +2000, Ctrl-C, quiet, idle.
        let mut rig = Rig::claude_question();
        let at = rig.now + 5_000;
        rig.at(at, Input::OperatorKey(KeyKind::Enter));
        for step in 1..=65 {
            rig.at(at + step * 100, Input::PtyOutput);
        }
        assert_eq!(rig.status(), Status::Working);
        rig.at(at + 6_650, Input::OperatorKey(KeyKind::CtrlC));
        rig.at(at + 6_670, Input::PtyOutput);
        assert_eq!(rig.at(at + 8_669, Input::Tick), None);
        assert_eq!(rig.at(at + 8_670, Input::Tick), Some(Status::Idle));
    }

    #[test]
    fn codex_answer_key_never_gives_working_by_output() {
        let mut rig = Rig::codex();
        rig.hook(HookKind::UserPromptSubmit);
        rig.hook(HookKind::PermissionRequest);
        let at = rig.now + 1_000;
        rig.at(at, Input::OperatorKey(KeyKind::Enter));
        for step in 1..=50 {
            assert_eq!(rig.at(at + step * 100, Input::PtyOutput), None);
        }
        assert_eq!(rig.status(), Status::Question);
        assert_eq!(rig.machine.next_deadline(), None);
    }

    #[test]
    fn the_startup_question_has_no_approval_check() {
        let mut rig = Rig::claude();
        rig.at(25_000, Input::Tick);
        rig.at(26_000, Input::OperatorKey(KeyKind::Enter));
        for step in 1..=50 {
            assert_eq!(rig.at(26_000 + step * 100, Input::PtyOutput), None);
        }
        assert_eq!(rig.status(), Status::Question);
    }

    // ---- Starting ------------------------------------------------------------------------------

    #[test]
    fn starting_for_more_than_twenty_seconds_gives_question() {
        for cli in [CliKind::Claude, CliKind::Codex] {
            let mut rig = Rig::new(cli);
            assert_eq!(rig.machine.next_deadline(), Some(Millis(20_001)));
            assert_eq!(rig.at(20_000, Input::Tick), None, "{cli:?}");
            assert_eq!(rig.status(), Status::Starting);
            assert_eq!(
                rig.at(20_001, Input::Tick),
                Some(Status::Question),
                "{cli:?}"
            );
        }
    }

    #[test]
    fn the_start_limit_counts_from_the_start_time_of_the_machine() {
        let mut machine = Machine::new(CliKind::Claude, Millis(100_000));
        assert_eq!(machine.apply(Input::Tick, Millis(120_000)), None);
        assert_eq!(
            machine.apply(Input::Tick, Millis(120_001)),
            Some(Status::Question)
        );
    }

    #[test]
    fn pty_output_does_not_reset_the_start_limit() {
        let mut rig = Rig::codex();
        for step in 1..=19 {
            rig.at(step * 1_000, Input::PtyOutput);
        }
        assert_eq!(rig.at(20_001, Input::PtyOutput), Some(Status::Question));
    }

    #[test]
    fn a_hook_event_before_the_limit_leaves_starting_for_good() {
        let mut rig = Rig::claude();
        rig.at(19_000, Input::Hook(HookEvent::new(HookKind::SessionStart)));
        assert_eq!(rig.at(60_000, Input::Tick), None);
        assert_eq!(rig.status(), Status::Idle);
        assert_eq!(rig.machine.next_deadline(), None);
    }

    #[test]
    fn a_hook_after_the_startup_question_sets_the_status_by_its_rule() {
        let mut rig = Rig::claude();
        rig.at(25_000, Input::Tick);
        assert_eq!(rig.status(), Status::Question);
        assert_eq!(
            rig.at(26_000, Input::Hook(HookEvent::new(HookKind::SessionStart))),
            Some(Status::Idle)
        );

        let mut rig = Rig::codex();
        rig.at(25_000, Input::Tick);
        assert_eq!(rig.status(), Status::Question);
        assert_eq!(
            rig.at(26_000, Input::Hook(HookEvent::new(HookKind::SessionStart))),
            Some(Status::Working)
        );
    }

    #[test]
    fn a_key_in_the_startup_question_starts_no_watch() {
        // The answer to a startup dialog is not a permission answer. No idle before a hook.
        let mut rig = Rig::claude();
        rig.at(25_000, Input::Tick);
        rig.at(26_000, Input::OperatorKey(KeyKind::Enter));
        assert_eq!(rig.machine.quiet_watch_since(), None);
        assert_eq!(rig.at(60_000, Input::Tick), None);
        assert_eq!(rig.status(), Status::Question);
    }

    #[test]
    fn a_key_in_starting_starts_no_watch() {
        let mut rig = Rig::claude();
        rig.at(1_000, Input::OperatorKey(KeyKind::Enter));
        rig.at(2_000, Input::OperatorKey(KeyKind::Esc));
        assert_eq!(rig.machine.quiet_watch_since(), None);
    }

    // ---- done, blocked stop ------------------------------------------------------------------------

    #[test]
    fn task_marked_done_then_stop_gives_done() {
        let mut rig = Rig::claude_working();
        assert_eq!(rig.after(10, Input::TaskMarkedDone), None);
        assert_eq!(rig.status(), Status::Working);
        assert_eq!(rig.hook(HookKind::Stop), Some(Status::Done));
        assert!(rig.machine.paste_allowed());
    }

    #[test]
    fn done_applies_to_one_stop_only() {
        let mut rig = Rig::claude_working();
        rig.after(10, Input::TaskMarkedDone);
        rig.hook(HookKind::Stop);
        // The next turn starts and ends: idle.
        assert_eq!(rig.hook(HookKind::UserPromptSubmit), Some(Status::Working));
        assert_eq!(rig.hook(HookKind::Stop), Some(Status::Idle));
    }

    #[test]
    fn a_new_prompt_after_idle_clears_an_old_done_mark() {
        let mut rig = Rig::claude_working();
        rig.after(10, Input::TaskMarkedDone);
        rig.after(10, Input::OperatorKey(KeyKind::Esc));
        assert_eq!(rig.after(2_000, Input::Tick), Some(Status::Idle));
        rig.hook(HookKind::UserPromptSubmit);
        assert_eq!(rig.hook(HookKind::Stop), Some(Status::Idle));
    }

    #[test]
    fn task_marked_done_works_for_codex() {
        let mut rig = Rig::codex();
        rig.hook(HookKind::UserPromptSubmit);
        rig.after(10, Input::TaskMarkedDone);
        assert_eq!(rig.hook(HookKind::Stop), Some(Status::Done));
    }

    #[test]
    fn done_waits_for_the_stop_that_ends_a_queued_turn() {
        let mut rig = Rig::claude_working();
        rig.hook(HookKind::UserPromptSubmit); // queued mark
        rig.after(10, Input::TaskMarkedDone);
        assert_eq!(rig.hook(HookKind::Stop), None);
        assert_eq!(rig.status(), Status::Working);
        assert_eq!(rig.hook(HookKind::Stop), Some(Status::Done));
    }

    #[test]
    fn stop_blocked_by_the_app_keeps_working_once() {
        for mut rig in [Rig::claude(), Rig::codex()] {
            rig.hook(HookKind::UserPromptSubmit);
            assert_eq!(rig.after(10, Input::StopBlockedByApp), None);
            assert_eq!(rig.hook(HookKind::Stop), None);
            assert_eq!(rig.status(), Status::Working);
            assert_eq!(rig.hook(HookKind::Stop), Some(Status::Idle));
        }
    }

    #[test]
    fn stop_blocked_by_the_app_does_not_use_up_the_queued_mark() {
        let mut rig = Rig::claude_working();
        rig.hook(HookKind::UserPromptSubmit); // queued mark
        rig.after(10, Input::StopBlockedByApp);
        assert_eq!(rig.hook(HookKind::Stop), None); // blocked Stop
        assert!(rig.machine.queued_turn());
        assert_eq!(rig.hook(HookKind::Stop), None); // queued turn ends
        assert_eq!(rig.status(), Status::Working);
        assert_eq!(rig.hook(HookKind::Stop), Some(Status::Idle));
    }

    #[test]
    fn done_waits_for_the_stop_after_a_blocked_stop() {
        let mut rig = Rig::claude_working();
        rig.after(10, Input::TaskMarkedDone);
        rig.after(10, Input::StopBlockedByApp);
        assert_eq!(rig.hook(HookKind::Stop), None);
        assert_eq!(rig.hook(HookKind::Stop), Some(Status::Done));
    }

    // ---- paste_allowed -------------------------------------------------------------------------

    #[test]
    fn paste_allowed_in_idle_after_a_stop() {
        let mut rig = Rig::claude_working();
        assert!(!rig.machine.paste_allowed(), "working");
        rig.hook(HookKind::Stop);
        assert!(rig.machine.paste_allowed(), "idle");
    }

    #[test]
    fn paste_allowed_in_claude_idle_right_after_session_start() {
        let mut rig = Rig::claude();
        assert!(!rig.machine.paste_allowed(), "starting");
        rig.hook(HookKind::SessionStart);
        assert!(rig.machine.paste_allowed());
    }

    #[test]
    fn paste_refused_in_question_starting_stopped() {
        let rig = Rig::claude_question();
        assert!(!rig.machine.paste_allowed(), "question");
        let rig = Rig::claude();
        assert!(!rig.machine.paste_allowed(), "starting");
        let mut rig = Rig::claude_working();
        rig.hook(HookKind::Stop);
        rig.after(10, Input::ProcessExit);
        assert!(!rig.machine.paste_allowed(), "stopped");
    }

    #[test]
    fn paste_allowed_after_a_deny_gives_idle() {
        // Deny: no Stop comes. The quiet-watch idle is a turn end: it resets the tool condition.
        let mut rig = Rig::claude_question();
        assert!(!rig.machine.paste_allowed(), "question");
        let at = rig.now + 1_000;
        rig.at(at, Input::OperatorKey(KeyKind::Enter));
        rig.at(at + 30, Input::PtyOutput);
        assert_eq!(rig.at(at + 2_030, Input::Tick), Some(Status::Idle));
        assert!(rig.machine.paste_allowed());
    }

    #[test]
    fn paste_allowed_after_esc_and_ctrl_c_give_idle() {
        for key in [KeyKind::Esc, KeyKind::CtrlC] {
            let mut rig = Rig::claude_working();
            rig.hook(HookKind::PreToolUse);
            let at = rig.now + 1_000;
            rig.at(at, Input::OperatorKey(key));
            assert!(!rig.machine.paste_allowed(), "working, {key:?}");
            assert_eq!(rig.at(at + 2_000, Input::Tick), Some(Status::Idle));
            assert!(rig.machine.paste_allowed(), "{key:?}");
        }
    }

    #[test]
    fn paste_allowed_after_a_codex_permission_request_and_interrupt() {
        let mut rig = Rig::codex();
        rig.hook(HookKind::UserPromptSubmit);
        rig.hook(HookKind::PreToolUse);
        rig.hook(HookKind::PermissionRequest);
        assert!(!rig.machine.paste_allowed(), "question");
        assert_eq!(rig.hook(HookKind::Interrupt), Some(Status::Idle));
        assert!(rig.machine.paste_allowed());
    }

    #[test]
    fn paste_refused_when_pre_tool_use_comes_after_a_stop_with_no_submit() {
        // The queued-turn window: Stop gives idle, then the queued turn runs a tool with no
        // UserPromptSubmit. The tool event clears the paste right at once.
        let mut rig = Rig::claude_working();
        rig.hook(HookKind::Stop);
        assert!(rig.machine.paste_allowed(), "idle after Stop");
        assert_eq!(rig.hook(HookKind::PreToolUse), Some(Status::Working));
        assert!(!rig.machine.paste_allowed(), "working");
        assert_eq!(
            rig.hook(HookKind::PermissionRequest),
            Some(Status::Question)
        );
        assert!(!rig.machine.paste_allowed(), "question");
        rig.hook(HookKind::PostToolUse);
        rig.hook(HookKind::Stop);
        assert!(rig.machine.paste_allowed(), "idle after the next Stop");
    }

    #[test]
    fn a_tool_event_after_a_turn_end_with_no_stop_clears_the_paste_again() {
        // Deny, idle (paste allowed), then a tool event with no submit: not allowed.
        let mut rig = Rig::claude_question();
        let at = rig.now + 1_000;
        rig.at(at, Input::OperatorKey(KeyKind::Enter));
        rig.at(at + 2_000, Input::Tick);
        assert!(rig.machine.paste_allowed());
        rig.hook(HookKind::PreToolUse);
        assert!(!rig.machine.paste_allowed());
    }

    #[test]
    fn paste_allowed_in_done() {
        let mut rig = Rig::claude_working();
        rig.after(10, Input::TaskMarkedDone);
        rig.hook(HookKind::Stop);
        assert_eq!(rig.status(), Status::Done);
        assert!(rig.machine.paste_allowed());
    }

    #[test]
    fn paste_refused_for_codex_with_no_prompt_yet() {
        let mut rig = Rig::codex();
        // Artificial: the first hook event is an Interrupt. Idle, but no prompt came.
        rig.hook(HookKind::Interrupt);
        assert_eq!(rig.status(), Status::Idle);
        assert!(!rig.machine.paste_allowed());
        rig.hook(HookKind::UserPromptSubmit);
        rig.hook(HookKind::Stop);
        assert!(rig.machine.paste_allowed());
    }

    #[test]
    fn paste_refused_while_a_blocked_stop_keeps_working() {
        let mut rig = Rig::claude_working();
        rig.after(10, Input::StopBlockedByApp);
        rig.hook(HookKind::Stop);
        assert!(!rig.machine.paste_allowed());
    }

    // ---- misc ------------------------------------------------------------------------------------

    #[test]
    fn the_queued_mark_is_cleared_when_a_quiet_watch_gives_idle() {
        let mut rig = Rig::claude_working();
        rig.hook(HookKind::UserPromptSubmit); // queued mark
        rig.after(10, Input::OperatorKey(KeyKind::Esc));
        assert_eq!(rig.after(2_000, Input::Tick), Some(Status::Idle));
        assert!(!rig.machine.queued_turn());
        // The next turn ends with a plain Stop: idle.
        rig.hook(HookKind::UserPromptSubmit);
        assert_eq!(rig.hook(HookKind::Stop), Some(Status::Idle));
    }

    #[test]
    fn next_deadline_is_the_earliest_rule() {
        let mut rig = Rig::claude_working();
        assert_eq!(rig.machine.next_deadline(), None);
        rig.at(1_000, Input::OperatorKey(KeyKind::Esc));
        assert_eq!(rig.machine.next_deadline(), Some(Millis(3_000)));
        let machine = Machine::new(CliKind::Codex, Millis(500));
        assert_eq!(machine.next_deadline(), Some(Millis(20_501)));
    }

    #[test]
    fn millis_helpers_do_not_overflow() {
        assert_eq!(Millis(u64::MAX).plus(5), Millis(u64::MAX));
        assert_eq!(Millis(3).since(Millis(10)), 0);
        assert_eq!(Millis(10).since(Millis(3)), 7);
    }
}
