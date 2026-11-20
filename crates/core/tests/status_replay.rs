//! Replay tests of the status machine. They feed real hook logs of the probes (copies in
//! `tests/fixtures/`) and compare the status after each event with a table written by hand.
//!
//! Each log file is split by `session_id` and then at each `SessionStart`: one session ID can
//! cover two processes after a resume.

use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tower_core::model::CliKind;
use tower_core::status::{HookEvent, Input, KeyKind, Machine, Millis, Status};

use Status::{Idle, Question, Starting, Stopped, Working};

// ---- loading and splitting ---------------------------------------------------------------------

#[derive(Debug, Clone)]
struct LogEvent {
    t_ms: u64,
    run: Option<String>,
    data: Value,
}

impl LogEvent {
    fn name(&self) -> &str {
        self.data["hook_event_name"].as_str().unwrap_or("")
    }

    /// `Name` or `Notification/<type>`.
    fn describe(&self) -> String {
        match self.data["notification_type"].as_str() {
            Some(kind) => format!("{}/{kind}", self.name()),
            None => self.name().to_owned(),
        }
    }

    fn session_id(&self) -> &str {
        self.data["session_id"].as_str().unwrap_or("")
    }

    fn stop_hook_active(&self) -> bool {
        self.data["stop_hook_active"].as_bool().unwrap_or(false)
    }
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn to_ms(seconds: f64) -> u64 {
    (seconds * 1000.0).round() as u64
}

fn load(name: &str) -> Vec<LogEvent> {
    let text = fs::read_to_string(fixture(name)).expect("fixture file");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: Value = serde_json::from_str(line).expect("fixture line is JSON");
            LogEvent {
                t_ms: to_ms(value["t"].as_f64().expect("t")),
                run: value["env"]["AT_RUN"].as_str().map(str::to_owned),
                data: value["data"].clone(),
            }
        })
        .collect()
}

/// Split by session ID (sorted by time), then at each `SessionStart`.
fn segments(events: &[LogEvent]) -> Vec<Vec<LogEvent>> {
    let mut by_session: Vec<(String, Vec<LogEvent>)> = Vec::new();
    for event in events {
        let id = event.session_id().to_owned();
        match by_session.iter_mut().find(|(known, _)| *known == id) {
            Some((_, list)) => list.push(event.clone()),
            None => by_session.push((id, vec![event.clone()])),
        }
    }
    let mut result = Vec::new();
    for (_, mut list) in by_session {
        list.sort_by_key(|event| event.t_ms);
        let mut current: Vec<LogEvent> = Vec::new();
        for event in list {
            if event.name() == "SessionStart" && !current.is_empty() {
                result.push(std::mem::take(&mut current));
            }
            current.push(event);
        }
        if !current.is_empty() {
            result.push(current);
        }
    }
    result.sort_by_key(|segment| segment[0].t_ms);
    result
}

/// The segment whose first event has the time `first_t` (seconds, as in the log).
fn segment_at(segments: &[Vec<LogEvent>], first_t: f64) -> Vec<LogEvent> {
    let wanted = to_ms(first_t);
    segments
        .iter()
        .find(|segment| segment[0].t_ms == wanted)
        .unwrap_or_else(|| panic!("no segment starts at {first_t}"))
        .clone()
}

// ---- driver ------------------------------------------------------------------------------------

/// Apply one input at `now`. First fire each timed rule that is due, at its own time.
/// Returns the status changes with their times.
fn feed(machine: &mut Machine, now: u64, input: Input) -> Vec<(u64, Status)> {
    let mut changes = Vec::new();
    while let Some(deadline) = machine.next_deadline() {
        if deadline.0 > now {
            break;
        }
        if let Some(status) = machine.apply(Input::Tick, deadline) {
            changes.push((deadline.0, status));
        }
    }
    if let Some(status) = machine.apply(input, Millis(now)) {
        changes.push((now, status));
    }
    changes
}

/// Replay hook events only. Returns `(event description, status after the event)`.
/// `inject_blocked_stops`: a `Stop` with `stop_hook_active == false` that the next event
/// (a `Stop` with `stop_hook_active == true`) follows was blocked by the Stop hook of the probe.
/// The app would send `StopBlockedByApp` before that `Stop`.
fn replay_hooks(
    cli: CliKind,
    segment: &[LogEvent],
    inject_blocked_stops: bool,
    tick_ms: Option<u64>,
) -> Vec<(String, Status)> {
    let mut machine = Machine::new(cli, Millis(segment[0].t_ms));
    let mut result = Vec::new();
    let mut last_t = segment[0].t_ms;
    for (index, event) in segment.iter().enumerate() {
        if let Some(step) = tick_ms {
            let mut tick = last_t + step;
            while tick < event.t_ms {
                machine.apply(Input::Tick, Millis(tick));
                tick += step;
            }
        }
        last_t = event.t_ms;
        let blocked = inject_blocked_stops
            && event.name() == "Stop"
            && !event.stop_hook_active()
            && segment
                .get(index + 1)
                .is_some_and(|next| next.name() == "Stop" && next.stop_hook_active());
        if blocked {
            machine.apply(Input::StopBlockedByApp, Millis(event.t_ms));
        }
        machine.apply(
            Input::Hook(HookEvent::from_json(&event.data)),
            Millis(event.t_ms),
        );
        result.push((event.describe(), machine.status()));
    }
    result
}

fn table(rows: &[(&str, Status)]) -> Vec<(String, Status)> {
    rows.iter()
        .map(|(name, status)| ((*name).to_owned(), *status))
        .collect()
}

const SUBAGENT_STOP: &str = "SubagentStop";
const PROMPT: &str = "UserPromptSubmit";

// ---- Claude, gate 1 log -----------------------------------------------------------------------

#[test]
fn claude_gate1_log_splits_into_four_processes() {
    let segs = segments(&load("claude-gate1.jsonl"));
    assert_eq!(segs.len(), 4);
    let lengths: Vec<usize> = segs.iter().map(Vec::len).collect();
    // ...502 (28 events + its SessionEnd), ...503 startup (7), ...503 resume (6), f2de2a6b (2).
    assert_eq!(lengths, vec![29, 7, 6, 2]);
}

#[test]
fn claude_gate1_queued_turn_permission_and_idle_prompt() {
    let segs = segments(&load("claude-gate1.jsonl"));
    let segment = segment_at(&segs, 1795140397.113);
    let got = replay_hooks(CliKind::Claude, &segment, false, None);
    let expected = table(&[
        ("SessionStart", Idle),
        // Five plain turns: each prompt gives working, each Stop gives idle.
        (PROMPT, Working),
        ("Stop", Idle),
        (PROMPT, Working),
        ("Stop", Idle),
        (PROMPT, Working),
        ("Stop", Idle),
        (SUBAGENT_STOP, Idle),
        (PROMPT, Working),
        ("Stop", Idle),
        (SUBAGENT_STOP, Idle),
        (PROMPT, Working),
        ("Stop", Idle),
        (SUBAGENT_STOP, Idle),
        // Queued turn run (t = ...520.67 and ...522.17).
        (PROMPT, Working),
        (PROMPT, Working), // submitted during the turn: sets the queued mark
        ("Stop", Working), // ...524.59: the queued turn runs. Must stay working.
        ("Stop", Idle),    // ...525.92
        (SUBAGENT_STOP, Idle),
        (SUBAGENT_STOP, Idle),
        // Permission run with approval (...559.45 to ...585.26).
        (PROMPT, Working),
        ("PreToolUse", Working),
        ("PermissionRequest", Question), // 40 ms after PreToolUse
        ("Notification/permission_prompt", Question), // 6 s later: no change
        ("PostToolUse", Working),        // the operator approved
        ("Stop", Idle),
        (SUBAGENT_STOP, Idle),
        (PROMPT, Working), // ...618.26, no Stop follows
        ("SessionEnd", Stopped),
    ]);
    assert_eq!(got, expected);
}

#[test]
fn claude_gate1_stop_blocked_by_the_hook_keeps_working_once() {
    // ...652.65: the Stop hook of the probe blocked the first Stop (stop_hook_active false),
    // and the CLI continued (second Stop, stop_hook_active true).
    let segs = segments(&load("claude-gate1.jsonl"));
    let segment = segment_at(&segs, 1795140652.651);
    let got = replay_hooks(CliKind::Claude, &segment, true, None);
    let expected = table(&[
        ("SessionStart", Idle),
        (PROMPT, Working),
        ("Stop", Working), // blocked by the app: keeps working
        ("Stop", Idle),
        (SUBAGENT_STOP, Idle),
        ("Notification/idle_prompt", Idle), // ...724.83: must not give question
        ("SessionEnd", Stopped),
    ]);
    assert_eq!(got, expected);

    // Without the input from the app the first Stop gives idle. This shows what the input does.
    let got = replay_hooks(CliKind::Claude, &segment, false, None);
    assert_eq!(got[2], ("Stop".to_owned(), Idle));
}

#[test]
fn claude_gate1_resume_process() {
    let segs = segments(&load("claude-gate1.jsonl"));
    let segment = segment_at(&segs, 1795141440.202);
    let got = replay_hooks(CliKind::Claude, &segment, false, None);
    let expected = table(&[
        ("SessionStart", Idle), // source: resume
        (PROMPT, Working),
        ("Stop", Idle),
        (SUBAGENT_STOP, Idle),
        ("Notification/idle_prompt", Idle), // ...510.46: must not give question
        ("SessionEnd", Stopped),
    ]);
    assert_eq!(got, expected);
}

#[test]
fn claude_gate1_short_process() {
    let segs = segments(&load("claude-gate1.jsonl"));
    let segment = segment_at(&segs, 1795141651.952);
    let got = replay_hooks(CliKind::Claude, &segment, false, None);
    assert_eq!(
        got,
        table(&[("SessionStart", Idle), ("SessionEnd", Stopped)])
    );
}

// ---- Codex, gate 1 log ------------------------------------------------------------------------

#[test]
fn codex_gate1_start_steering_permission_and_interrupt() {
    let segs = segments(&load("codex-gate1.jsonl"));
    assert_eq!(segs.len(), 3);
    let segment = segment_at(&segs, 1795141076.598);
    let got = replay_hooks(CliKind::Codex, &segment, false, None);
    let expected = table(&[
        ("SessionStart", Working), // ...076.60: must not give idle
        (PROMPT, Working),         // ...076.63
        ("Stop", Idle),
        (PROMPT, Working),
        ("Stop", Idle),
        // Steering run: 2 submits, 1 Stop.
        (PROMPT, Working), // ...123.64
        (PROMPT, Working), // ...125.88
        ("Stop", Idle),    // ...135.72: one Stop ends the turn
        // Permission run with approval.
        (PROMPT, Working),
        ("PreToolUse", Working),
        ("PostToolUse", Working),
        ("PreToolUse", Working),
        ("PermissionRequest", Question),
        ("PostToolUse", Working),
        ("Stop", Idle),
        // Esc during a turn: the Interrupt hook gives idle.
        (PROMPT, Working),
        ("Interrupt", Idle),
    ]);
    assert_eq!(got, expected);
}

#[test]
fn codex_gate1_resume_process() {
    let segs = segments(&load("codex-gate1.jsonl"));
    let segment = segment_at(&segs, 1795141490.515);
    let got = replay_hooks(CliKind::Codex, &segment, false, None);
    let expected = table(&[("SessionStart", Working), (PROMPT, Working), ("Stop", Idle)]);
    assert_eq!(got, expected);
}

#[test]
fn codex_gate1_stop_blocked_by_the_hook() {
    let segs = segments(&load("codex-gate1.jsonl"));
    let segment = segment_at(&segs, 1795141286.422);
    let got = replay_hooks(CliKind::Codex, &segment, true, None);
    let expected = table(&[
        ("SessionStart", Working),
        (PROMPT, Working),
        ("Stop", Idle),
        (PROMPT, Working),
        ("Stop", Working), // blocked by the app
        ("Stop", Idle),
    ]);
    assert_eq!(got, expected);
}

// ---- Codex deny, step 0 run x2 ------------------------------------------------------------------

#[test]
fn codex_step0_x2_deny_interrupt_gives_idle() {
    let events: Vec<LogEvent> = load("codex-step0.jsonl")
        .into_iter()
        .filter(|event| event.run.as_deref() == Some("x2"))
        .collect();
    let segs = segments(&events);
    assert_eq!(segs.len(), 1);
    let got = replay_hooks(CliKind::Codex, &segs[0], false, None);
    let expected = table(&[
        ("SessionStart", Working),
        (PROMPT, Working),
        ("PreToolUse", Working),
        ("PostToolUse", Working),
        ("PreToolUse", Working),
        ("PermissionRequest", Question),
        ("Interrupt", Idle), // ...281.44: the operator denied
        (PROMPT, Working),
        ("Stop", Idle),
        ("SessionEnd", Stopped),
    ]);
    assert_eq!(got, expected);
}

// ---- Claude deny, step 0 run c4, with PTY times ---------------------------------------------------

/// The key of a `SEND` line of the `.tim` file.
fn key_of(payload: &str) -> KeyKind {
    match payload {
        "\\r" => KeyKind::Enter,
        "\\x03" => KeyKind::CtrlC,
        "\\x1b" => KeyKind::Esc,
        digit if digit.len() == 1 && digit.chars().all(|c| c.is_ascii_digit()) => KeyKind::Digit,
        // Arrows (`\x1b[B`) and the bracketed paste of the probe prompt.
        _ => KeyKind::Other,
    }
}

struct Timed {
    t_ms: u64,
    input: Input,
}

/// Hook events of run c4 plus the START / OUT / SEND / EXIT lines of `c4.tim`, in time order.
/// Returns the start time and the inputs.
fn c4_timeline() -> (u64, Vec<Timed>) {
    let mut timeline: Vec<Timed> = Vec::new();
    for event in load("claude-step0.jsonl")
        .into_iter()
        .filter(|event| event.run.as_deref() == Some("c4"))
    {
        timeline.push(Timed {
            t_ms: event.t_ms,
            input: Input::Hook(HookEvent::from_json(&event.data)),
        });
    }
    let text = fs::read_to_string(fixture("c4.tim")).expect("c4.tim");
    let mut start = None;
    for line in text.lines() {
        let mut parts = line.splitn(3, ' ');
        let (Some(time), Some(kind)) = (parts.next(), parts.next()) else {
            continue;
        };
        let t_ms = to_ms(time.parse::<f64>().expect("time"));
        let rest = parts.next().unwrap_or("");
        match kind {
            "START" => start = Some(t_ms),
            "OUT" => timeline.push(Timed {
                t_ms,
                input: Input::PtyOutput,
            }),
            "SEND" => {
                let payload = rest.strip_prefix("b'").and_then(|r| r.strip_suffix('\''));
                // A multi-byte paste keeps its text: it is `Other`.
                timeline.push(Timed {
                    t_ms,
                    input: Input::OperatorKey(key_of(payload.expect("SEND payload"))),
                });
            }
            "EXIT" => timeline.push(Timed {
                t_ms,
                input: Input::ProcessExit,
            }),
            _ => {}
        }
    }
    // Stable sort: at the same millisecond the hooks (pushed first) come before the PTY lines.
    timeline.sort_by_key(|item| item.t_ms);
    (start.expect("START line"), timeline)
}

/// Status changes of run c4, with their times in milliseconds of the log.
fn c4_changes() -> Vec<(u64, Status)> {
    let (start, timeline) = c4_timeline();
    let mut machine = Machine::new(CliKind::Claude, Millis(start));
    let mut changes = Vec::new();
    for item in timeline {
        changes.extend(feed(&mut machine, item.t_ms, item.input));
    }
    changes
}

#[test]
fn claude_step0_c4_deny_gives_idle_two_seconds_after_the_last_output() {
    let changes = c4_changes();
    let expected = vec![
        // Trust dialog answered in 16 s: under the 20 s limit, no question.
        (1795142726423, Idle),     // SessionStart
        (1795142741419, Working),  // UserPromptSubmit
        (1795142743969, Question), // PermissionRequest (PreToolUse 743.94: no change)
        // Notification permission_prompt at 749.98: no change. Down, Down, Down (arrows): no watch.
        // Enter (deny) at 759.765, last output 759.798: idle at 759.798 + 2000.
        (1795142761798, Idle),
        (1795142869108, Working), // UserPromptSubmit (Enter at 869.078 in idle: no watch)
        (1795142874516, Idle),    // Stop (SubagentStop 876.12: no change)
        (1795142886954, Working), // UserPromptSubmit
        (1795142889143, Question),
        // Enter (approve) at 897.984: the tool runs, the output goes on each 0.1 s, no hook.
        // The last output is less than 1000 ms old at 897.984 + 2000: working.
        (1795142899984, Working),
        // Ctrl-C at 904.530 in working, last output 904.555: idle at 904.555 + 2000.
        (1795142906555, Idle),
        // Ctrl-C at 926.98, 934.20, 934.53 in idle: no change. SessionEnd at 934.80.
        (1795142934799, Stopped),
    ];
    assert_eq!(changes, expected);
}

#[test]
fn claude_step0_c4_status_at_checkpoints() {
    let changes = c4_changes();
    let status_at = |t: f64| -> Status {
        let ms = to_ms(t);
        changes
            .iter()
            .take_while(|(time, _)| *time <= ms)
            .last()
            .map_or(Starting, |(_, status)| *status)
    };
    let base = 1795142000.0;
    assert_eq!(status_at(base + 710.5), Starting);
    assert_eq!(status_at(base + 726.0), Starting); // dialog answered, no hook yet
    assert_eq!(status_at(base + 750.0), Question); // after the late Notification
    assert_eq!(status_at(base + 759.7), Question); // before the deny key
    assert_eq!(status_at(base + 761.7), Question); // 2 s not passed since the last output
    assert_eq!(status_at(base + 761.9), Idle);
    assert_eq!(status_at(base + 899.9), Question); // approved at 897.984, 2 s not passed
    assert_eq!(status_at(base + 900.0), Working); // output goes on: working
    assert_eq!(status_at(base + 906.4), Working);
    assert_eq!(status_at(base + 906.6), Idle);
    assert_eq!(status_at(base + 930.0), Idle); // Ctrl-C in idle changes nothing
    assert_eq!(status_at(base + 935.0), Stopped);
}

// ---- Ticks do not change the result of a hook-only replay -------------------------------------

#[test]
fn ticks_between_events_do_not_change_any_replay() {
    let files = [
        ("claude-gate1.jsonl", CliKind::Claude),
        ("claude-step0.jsonl", CliKind::Claude),
        ("codex-gate1.jsonl", CliKind::Codex),
        ("codex-step0.jsonl", CliKind::Codex),
    ];
    let mut compared = 0;
    for (name, cli) in files {
        let events = load(name);
        // The step 0 files hold several runs: split them by run first.
        let mut runs: HashMap<String, Vec<LogEvent>> = HashMap::new();
        for event in events {
            runs.entry(event.run.clone().unwrap_or_default())
                .or_default()
                .push(event);
        }
        for run in runs.values() {
            for segment in segments(run) {
                // A segment that does not start with SessionStart (a lone SessionEnd) has no
                // start time that means anything.
                if segment[0].name() != "SessionStart" {
                    continue;
                }
                let plain = replay_hooks(cli, &segment, false, None);
                let ticked = replay_hooks(cli, &segment, false, Some(250));
                assert_eq!(plain, ticked, "{name} segment at {}", segment[0].t_ms);
                compared += 1;
            }
        }
    }
    assert!(compared >= 14, "compared only {compared} segments");
}
