//! Session start, status events, hook routing, paste, stop, resume and the one-session rule
//! (tests 2, 3, 4, 5, 7 and 8 of the task). The processes are the real `fake-cli`.

mod app_support;

use app_support::{Rig, marker_path, wait_for};
use std::time::Duration;
use tower_core::app::TowerError;
use tower_core::status::Status;

/// A short wait to check that something does NOT happen.
const QUIET: Duration = Duration::from_millis(400);

#[test]
fn claude_session_goes_starting_idle_working_idle_and_the_database_has_it_all() {
    let rig = Rig::new();
    rig.create_floor();
    let started = rig.start("ceo");
    assert_eq!(started.slug, "ceo");
    assert_eq!(started.status, Status::Starting);
    assert!(!started.can_resume && !started.draft && !started.message_waiting);
    let id = started.session_id.clone();
    rig.wait_first_turn(&id);

    // The observer saw the statuses in order.
    assert_eq!(
        rig.events.statuses(&id),
        [
            Status::Starting,
            Status::Idle,
            Status::Working,
            Status::Idle
        ]
    );

    // What the adapter was asked to build.
    let calls = rig.calls();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(call.app_session_id, id);
    assert_eq!(call.resume, None);
    assert_eq!(call.cwd, rig.cwd, "the CWD of the process is the floor CWD");
    assert_eq!(
        call.first_message.as_deref(),
        Some("You are Ada, the CEO. Say that you are ready in one short sentence.")
    );
    let instructions = call.instructions.as_deref().unwrap();
    assert!(
        instructions.contains("You are Ada, the CEO"),
        "{instructions}"
    );
    assert!(
        instructions.contains("You lead this floor"),
        "{instructions}"
    );
    assert_eq!(call.socket, rig.socket);
    let (log, limit) = call.log.clone().expect("a log config");
    assert_eq!(log, rig.log_path(&id));
    assert_eq!(limit, 4 * 1024 * 1024);

    // The database has the row, the CLI session ID (from the plan) and the status events.
    let db = rig.db();
    let row = db.last_session("ceo").unwrap().expect("the session row");
    assert_eq!(row.app_session_id, id);
    assert_eq!(row.cli_session_id, call.plan_cli_session_id);
    assert!(row.cli_session_id.is_some());
    assert_eq!(row.ended_at, None);
    let events: Vec<String> = db
        .status_events(&id)
        .unwrap()
        .into_iter()
        .map(|(status, _)| status)
        .collect();
    assert_eq!(events, ["starting", "idle", "working", "idle"]);
    assert_eq!(row.last_status, "idle");

    // The process got the message as its first prompt, and the floor view shows the session.
    let view = rig.tower.floor_open(&rig.cwd).unwrap();
    assert_eq!(view.sessions.len(), 1);
    assert_eq!(view.sessions[0].status, Status::Idle);
    rig.stop_and_wait(&id);
}

#[test]
fn a_given_first_message_replaces_the_default() {
    let rig = Rig::new();
    rig.create_floor();
    let started = rig.start_with("pm", "Make a plan for the login page.");
    rig.wait_first_turn(&started.session_id);
    let call = &rig.calls()[0];
    assert_eq!(
        call.first_message.as_deref(),
        Some("Make a plan for the login page.")
    );
    wait_for("the log has the echo", || {
        String::from_utf8_lossy(&rig.log_bytes(&started.session_id))
            .contains("you said: Make a plan for the login page.")
    });
}

#[test]
fn codex_session_stores_the_cli_session_id_from_the_first_hook_event() {
    let rig = Rig::new();
    rig.create_floor();
    rig.make_codex("pm");
    let started = rig.start("pm");
    let id = started.session_id.clone();
    let call = rig.calls().pop().unwrap();
    assert_eq!(
        call.plan_cli_session_id, None,
        "a fresh Codex has no ID yet"
    );
    // The first hook event (SessionStart with the first prompt) carries `session_id`.
    let expected = format!("codex-{}", &id[..8]);
    wait_for("the CLI session ID in the database", || {
        rig.db()
            .last_session("pm")
            .unwrap()
            .is_some_and(|row| row.cli_session_id.as_deref() == Some(expected.as_str()))
    });
    rig.wait_first_turn(&id);
    // Codex stays `starting` until the first hook, so there is no `idle` before `working`.
    assert_eq!(
        rig.events.statuses(&id),
        [Status::Starting, Status::Working, Status::Idle]
    );
    rig.stop_and_wait(&id);
    // `canResume` shows after the stop, because an ID is stored.
    assert!(rig.events.latest(&id).unwrap().can_resume);
}

/// Found live on 2026-11-21: a Codex session sends no hook event until its first prompt, also
/// after a resume. A resume with no message left the session in `starting`, and after 20 s the
/// status became a false `question` (the resumed TUI was idle). So a Codex resume with no text
/// gets a short default message, and the status leaves `starting` with the first hook event.
#[test]
fn a_codex_resume_with_no_message_gets_a_default_message_and_leaves_starting() {
    let rig = Rig::new();
    rig.create_floor();
    rig.make_codex("pm");
    let first = rig.start("pm");
    rig.wait_first_turn(&first.session_id);
    rig.stop_and_wait(&first.session_id);
    assert!(rig.events.latest(&first.session_id).unwrap().can_resume);

    let second = rig
        .tower
        .session_resume(&rig.cwd, "pm", None, 24, 80)
        .unwrap();
    let call = rig.calls().pop().unwrap();
    assert!(call.resume.is_some(), "it is a resume");
    let message = call.first_message.as_deref().expect("a default message");
    assert!(
        message.contains("resumed your session") && message.contains("Maya"),
        "{message}"
    );
    // The session leaves `starting` by its first hook event, and then it ends its turn.
    rig.wait_first_turn(&second.session_id);
    assert_eq!(
        rig.events.statuses(&second.session_id),
        [Status::Starting, Status::Working, Status::Idle]
    );
}

/// A Claude resume sends `SessionStart` by itself, so it needs no message (and none is made up).
#[test]
fn a_claude_resume_with_no_message_gets_no_message() {
    let rig = Rig::new();
    rig.create_floor();
    let first = rig.start("pm");
    rig.wait_first_turn(&first.session_id);
    rig.stop_and_wait(&first.session_id);
    let second = rig
        .tower
        .session_resume(&rig.cwd, "pm", None, 24, 80)
        .unwrap();
    let call = rig.calls().pop().unwrap();
    assert!(call.resume.is_some());
    assert_eq!(call.first_message, None, "Claude gets no made up message");
    rig.events.wait_status(&second.session_id, Status::Idle);
}

#[test]
fn codex_resume_with_no_stored_id_starts_fresh() {
    let rig = Rig::new();
    rig.create_floor();
    rig.make_codex("pm");
    // The startup dialog keeps Codex from its first hook, so no ID is ever stored.
    rig.set_script("pm", "startup-dialog");
    let first = rig.start("pm");
    wait_for("the process runs", || {
        !rig.log_bytes(&first.session_id).is_empty()
    });
    rig.stop_and_wait(&first.session_id);
    assert!(!rig.events.latest(&first.session_id).unwrap().can_resume);

    let second = rig
        .tower
        .session_resume(&rig.cwd, "pm", None, 24, 80)
        .unwrap();
    assert_ne!(second.session_id, first.session_id);
    let calls = rig.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].resume, None, "no ID: this is a fresh start");
    assert!(
        calls[1]
            .first_message
            .as_deref()
            .unwrap()
            .starts_with("You are Maya, the Project Manager."),
    );
    rig.stop_and_wait(&second.session_id);
}

#[test]
fn two_sessions_get_their_own_hook_events() {
    let rig = Rig::new();
    rig.create_floor();
    // The CEO reaches a permission dialog. The PM stays idle after a plain turn.
    rig.set_script("ceo", "perm-dialog");
    rig.set_script("pm", "echo");
    let ceo = rig.start("ceo");
    let pm = rig.start("pm");
    rig.events.wait_status(&ceo.session_id, Status::Question);
    rig.wait_first_turn(&pm.session_id);

    // At the same moment: one is in a dialog, the other is idle.
    let view = rig.tower.floor_open(&rig.cwd).unwrap();
    let status_of = |slug: &str| {
        view.sessions
            .iter()
            .find(|s| s.slug == slug)
            .unwrap()
            .status
    };
    assert_eq!(status_of("ceo"), Status::Question);
    assert_eq!(status_of("pm"), Status::Idle);
    assert!(
        !rig.events
            .statuses(&pm.session_id)
            .contains(&Status::Question)
    );
    assert!(
        !rig.events.statuses(&ceo.session_id).is_empty()
            && rig.events.statuses(&ceo.session_id).last() == Some(&Status::Question)
    );

    // The operator answers the dialog of the CEO. Only the CEO changes.
    let pm_changes = rig.events.views(&pm.session_id).len();
    rig.tower.pty_write(&ceo.session_id, b"1").unwrap();
    wait_for("the marker of the CEO", || {
        marker_path(&rig.cwd, &ceo.session_id).exists()
    });
    rig.events.wait("the CEO is idle again", |events| {
        events.statuses(&ceo.session_id).last() == Some(&Status::Idle)
    });
    assert!(!marker_path(&rig.cwd, &pm.session_id).exists());
    assert_eq!(rig.events.views(&pm.session_id).len(), pm_changes);
    rig.stop_and_wait(&ceo.session_id);
    rig.stop_and_wait(&pm.session_id);
}

#[test]
fn debug_paste_works_in_idle_and_is_refused_in_question_without_a_write() {
    let rig = Rig::new();
    rig.create_floor();
    rig.set_script("ceo", "echo");
    rig.set_script("pm", "perm-dialog");

    // In idle: the paste goes through, in one bracketed paste.
    let ceo = rig.start("ceo");
    rig.wait_first_turn(&ceo.session_id);
    rig.tower
        .debug_paste(&ceo.session_id, "hello from the test")
        .unwrap();
    wait_for("the paste in the record", || {
        rig.record(&ceo.session_id) == b"\x1b[200~hello from the test\x1b[201~\r"
    });

    // In question: refused with the text of the contract. Nothing is written.
    let pm = rig.start("pm");
    rig.events.wait_status(&pm.session_id, Status::Question);
    let error = rig
        .tower
        .debug_paste(&pm.session_id, "approve this")
        .unwrap_err();
    assert!(matches!(error, TowerError::Refused(_)), "{error:?}");
    assert_eq!(error.to_string(), "REFUSED: status question");
    std::thread::sleep(QUIET);
    assert!(
        rig.record(&pm.session_id).is_empty(),
        "the refused paste reached the process: {:?}",
        rig.record(&pm.session_id)
    );
    assert!(
        !marker_path(&rig.cwd, &pm.session_id).exists(),
        "the refused paste approved the action"
    );
    // The tab shows that a message waits.
    assert!(rig.events.latest(&pm.session_id).unwrap().message_waiting);
    rig.stop_and_wait(&ceo.session_id);
    rig.stop_and_wait(&pm.session_id);
}

#[test]
fn a_draft_refuses_the_paste_until_the_operator_clears_it() {
    let rig = Rig::new();
    rig.create_floor();
    let ceo = rig.start("ceo");
    let id = ceo.session_id;
    rig.wait_first_turn(&id);
    rig.tower.pty_write(&id, b"half a sentence").unwrap();
    assert!(
        rig.events.latest(&id).unwrap().draft,
        "the draft flag shows"
    );
    let error = rig.tower.debug_paste(&id, "x").unwrap_err();
    assert_eq!(error.to_string(), "REFUSED: draft");
    let view = rig.events.latest(&id).unwrap();
    assert!(view.draft && view.message_waiting);

    rig.tower.clear_draft(&id).unwrap();
    let view = rig.events.latest(&id).unwrap();
    assert!(!view.draft);
    // D5: the click clears the "message waiting" mark too, and the observer is told.
    assert!(
        !view.message_waiting,
        "clear_draft must clear message_waiting"
    );
    rig.tower.debug_paste(&id, "now it works").unwrap();
    assert!(!rig.events.latest(&id).unwrap().message_waiting);
    rig.stop_and_wait(&id);
}

#[test]
fn stop_sets_stopped_and_the_end_time_and_resume_uses_the_stored_id() {
    let rig = Rig::new();
    rig.create_floor();
    let first = rig.start("ceo");
    let id = first.session_id.clone();
    rig.wait_first_turn(&id);
    rig.tower.session_stop(&id).unwrap();
    rig.events.wait_status(&id, Status::Stopped);
    // The end time is in the database when the stopped event came.
    wait_for("the end time", || {
        rig.db()
            .last_session("ceo")
            .unwrap()
            .is_some_and(|row| row.ended_at.is_some())
    });
    let row = rig.db().last_session("ceo").unwrap().unwrap();
    assert_eq!(row.last_status, "stopped");
    let stored = row.cli_session_id.clone().expect("a stored CLI session ID");
    let events: Vec<String> = rig
        .db()
        .status_events(&id)
        .unwrap()
        .into_iter()
        .map(|(status, _)| status)
        .collect();
    assert_eq!(events.last().map(String::as_str), Some("stopped"));
    let last = rig.events.latest(&id).unwrap();
    assert!(last.can_resume && !last.draft && !last.message_waiting);
    // The floor view shows the stopped session with `can_resume`.
    let view = rig.tower.floor_open(&rig.cwd).unwrap();
    assert_eq!(view.sessions[0].status, Status::Stopped);
    assert!(view.sessions[0].can_resume);

    // Resume: the adapter is asked to resume with the stored ID, and a process starts.
    let second = rig
        .tower
        .session_resume(&rig.cwd, "ceo", Some("Where were we?".into()), 30, 100)
        .unwrap();
    assert_ne!(second.session_id, id, "a resume is a new app session");
    let calls = rig.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].resume.as_deref(), Some(stored.as_str()));
    assert_eq!(calls[1].first_message.as_deref(), Some("Where were we?"));
    assert!(calls[1].args.contains(&stored), "{:?}", calls[1].args);
    rig.wait_first_turn(&second.session_id);
    wait_for("the second echo", || {
        String::from_utf8_lossy(&rig.log_bytes(&second.session_id))
            .contains("you said: Where were we?")
    });
    // The new row has the same CLI session ID.
    let row = rig.db().last_session("ceo").unwrap().unwrap();
    assert_eq!(row.app_session_id, second.session_id);
    assert_eq!(row.cli_session_id.as_deref(), Some(stored.as_str()));
    rig.stop_and_wait(&second.session_id);
}

#[test]
fn a_second_start_and_a_remove_are_refused_while_a_session_lives() {
    let rig = Rig::new();
    rig.create_floor();
    let first = rig.start("pm");
    let id = first.session_id.clone();

    let error = rig.try_start("pm").unwrap_err();
    assert!(matches!(error, TowerError::AlreadyLive { .. }), "{error:?}");
    assert!(error.to_string().contains("already has a live session"));
    let error = rig
        .tower
        .session_resume(&rig.cwd, "pm", None, 24, 80)
        .unwrap_err();
    assert!(matches!(error, TowerError::AlreadyLive { .. }), "{error:?}");
    let error = rig.tower.employee_remove(&rig.cwd, "pm").unwrap_err();
    assert!(
        matches!(error, TowerError::HasLiveSession { .. }),
        "{error:?}"
    );
    assert!(rig.cwd.join(".swarm/employees/pm.md").exists());
    assert_eq!(rig.calls().len(), 1, "no second process was built");

    // Another employee is not affected.
    let other = rig.start("ceo");
    rig.stop_and_wait(&other.session_id);

    // After the stop, both work.
    rig.stop_and_wait(&id);
    rig.tower.employee_remove(&rig.cwd, "pm").unwrap();
    assert!(!rig.cwd.join(".swarm/employees/pm.md").exists());
}

#[test]
fn many_starts_at_the_same_time_make_one_session() {
    let rig = Rig::new();
    rig.create_floor();
    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..6)
            .map(|_| scope.spawn(|| rig.try_start("ceo")))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let ok: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    assert_eq!(ok.len(), 1, "results: {results:?}");
    assert_eq!(rig.calls().len(), 1);
    assert_eq!(rig.tower.floor_open(&rig.cwd).unwrap().sessions.len(), 1);
    rig.stop_and_wait(&ok[0].session_id);
}

#[test]
fn unknown_session_ids_and_a_missing_employee_give_clear_errors() {
    let rig = Rig::new();
    rig.create_floor();
    let error = rig.tower.session_stop("no-such-session").unwrap_err();
    assert!(matches!(error, TowerError::UnknownSession(_)));
    assert!(rig.tower.pty_write("x", b"a").is_err());
    let error = rig.try_start("nobody").unwrap_err();
    assert!(error.to_string().contains("nobody"), "{error}");
    // A refused start leaves nothing behind: the employee can start later.
    assert!(rig.db().last_session("nobody").unwrap().is_none());
}

#[test]
fn ctrl_v_sets_the_draft_flag_and_the_paste_is_refused() {
    // D1: xterm.js sends Ctrl+V as one byte. The CLI then reads the clipboard itself.
    let rig = Rig::new();
    rig.create_floor();
    let id = rig.start("ceo").session_id;
    rig.wait_first_turn(&id);
    rig.tower.pty_write(&id, b"\x16").unwrap();
    assert!(
        rig.events.latest(&id).unwrap().draft,
        "Ctrl+V sets the flag"
    );
    let error = rig.tower.debug_paste(&id, "injected").unwrap_err();
    assert_eq!(error.to_string(), "REFUSED: draft");
    rig.tower.clear_draft(&id).unwrap();
    assert!(!rig.events.latest(&id).unwrap().draft);
    rig.stop_and_wait(&id);
}

#[test]
fn enter_with_no_prompt_event_keeps_the_draft_flag_and_the_paste_stays_refused() {
    // N1: `/help` + Enter fires no UserPromptSubmit and leaves a dialog open. `cat` stands for
    // the CLI: it sends no hook. The test sends the hooks by hand.
    let rig = Rig::new();
    rig.create_floor();
    *app_support::lock(&rig.shared.program) = Some(("/bin/cat".into(), vec![]));
    let id = rig.start("ceo").session_id;
    rig.send_hook(&id, "SessionStart");
    rig.events.wait_status(&id, Status::Idle);
    rig.tower.pty_write(&id, b"/help").unwrap();
    rig.tower.pty_write(&id, b"\r").unwrap();
    // Wait longer than the old 2 s watch (and a few polls of the Tower).
    std::thread::sleep(Duration::from_millis(3000));
    assert!(rig.events.latest(&id).unwrap().draft, "the flag must stay");
    let error = rig
        .tower
        .debug_paste(&id, "lost in the dialog")
        .unwrap_err();
    assert_eq!(error.to_string(), "REFUSED: draft");
    // The operator clears the flag (the DRAFT mark in the UI), and then a paste works.
    rig.tower.clear_draft(&id).unwrap();
    assert!(!rig.events.latest(&id).unwrap().draft);
    rig.tower.debug_paste(&id, "now it works").unwrap();
    rig.stop_and_wait(&id);
}

#[test]
fn a_prompt_event_clears_the_draft_flag_and_the_observer_is_told_with_no_status_change() {
    // In `working`, a queued message fires UserPromptSubmit: the status stays `working`, there
    // is no output, and only the flag changes. The Tower polls the flags for this case.
    let rig = Rig::new();
    rig.create_floor();
    *app_support::lock(&rig.shared.program) = Some(("/bin/cat".into(), vec![]));
    let id = rig.start("ceo").session_id;
    rig.send_hook(&id, "SessionStart");
    rig.events.wait_status(&id, Status::Idle);
    rig.send_hook(&id, "UserPromptSubmit");
    rig.events.wait_status(&id, Status::Working);
    rig.tower.pty_write(&id, b"queued").unwrap();
    assert!(rig.events.latest(&id).unwrap().draft);
    let views = rig.events.views(&id).len();
    rig.send_hook(&id, "UserPromptSubmit");
    rig.events.wait("the draft flag to end", |events| {
        events.latest(&id).is_some_and(|view| !view.draft)
    });
    assert_eq!(rig.events.latest(&id).unwrap().status, Status::Working);
    assert!(rig.events.views(&id).len() > views);
    rig.stop_and_wait(&id);
}

#[test]
fn a_change_of_floor_toml_applies_at_floor_open_and_at_start() {
    let rig = Rig::new();
    rig.create_floor();
    let toml = rig.cwd.join(".swarm/floor.toml");
    let write = |model: &str| {
        std::fs::write(
            &toml,
            format!("version = 1\nname = \"Renamed\"\n\n[defaults]\nmodel = \"{model}\"\n"),
        )
        .unwrap();
    };
    write("model-a");
    // Same Tower, no restart: floor_open reads the file again (name and defaults).
    let view = rig.tower.floor_open(&rig.cwd).unwrap();
    assert_eq!(view.name, "Renamed");
    let first = rig.start("ceo");
    assert_eq!(rig.calls()[0].model.as_deref(), Some("model-a"));
    rig.stop_and_wait(&first.session_id);
    // An edit with no floor_open before the start also applies.
    write("model-b");
    let second = rig.start("ceo");
    assert_eq!(rig.calls()[1].model.as_deref(), Some("model-b"));
    rig.stop_and_wait(&second.session_id);
}

#[test]
fn resume_is_not_offered_for_a_session_that_never_had_a_turn() {
    // D3: `claude --resume` fails for a session with no conversation. The startup dialog keeps
    // the first message from running, so the session never has a `working` status.
    let rig = Rig::new();
    rig.create_floor();
    rig.set_script("ceo", "startup-dialog");
    let first = rig.start("ceo");
    let id = first.session_id.clone();
    wait_for("the process runs", || !rig.log_bytes(&id).is_empty());
    rig.stop_and_wait(&id);
    // The ID is stored (Claude: from the plan), but there was no turn.
    let row = rig.db().last_session("ceo").unwrap().unwrap();
    assert!(row.cli_session_id.is_some());
    assert!(!rig.events.latest(&id).unwrap().can_resume);
    let view = rig.tower.floor_open(&rig.cwd).unwrap();
    assert!(!view.sessions[0].can_resume, "the floor view agrees");
    let error = rig
        .tower
        .session_resume(&rig.cwd, "ceo", None, 24, 80)
        .unwrap_err();
    assert!(error.to_string().contains("never had a turn"), "{error}");
    assert_eq!(rig.calls().len(), 1, "no process was built for the resume");
    // A fresh start is still possible.
    rig.set_script("ceo", "echo");
    let second = rig.start("ceo");
    rig.wait_first_turn(&second.session_id);
    rig.stop_and_wait(&second.session_id);
}

#[test]
fn resume_is_offered_after_a_turn_and_stays_offered_after_a_resume_with_no_turn() {
    let rig = Rig::new();
    rig.create_floor();
    let first = rig.start("ceo");
    rig.wait_first_turn(&first.session_id);
    rig.stop_and_wait(&first.session_id);
    assert!(rig.events.latest(&first.session_id).unwrap().can_resume);
    assert!(rig.tower.floor_open(&rig.cwd).unwrap().sessions[0].can_resume);

    // Resume with no message: the CLI waits at its prompt, with no new turn. The CLI session
    // still has its old turn, so a second resume is still allowed.
    let second = rig
        .tower
        .session_resume(&rig.cwd, "ceo", None, 24, 80)
        .unwrap();
    rig.events.wait_status(&second.session_id, Status::Idle);
    rig.stop_and_wait(&second.session_id);
    assert!(rig.events.latest(&second.session_id).unwrap().can_resume);
    assert!(rig.tower.floor_open(&rig.cwd).unwrap().sessions[0].can_resume);
    let third = rig
        .tower
        .session_resume(&rig.cwd, "ceo", None, 24, 80)
        .unwrap();
    rig.stop_and_wait(&third.session_id);
}

#[test]
fn the_floor_defaults_fill_a_missing_model_and_effort() {
    // Today (before this fix) nothing applied them. Now `[defaults]` of floor.toml gives the
    // model and the effort of an employee that has none. An own value is kept.
    let rig = Rig::new();
    rig.create_floor();
    let toml = rig.cwd.join(".swarm/floor.toml");
    std::fs::write(
        &toml,
        "version = 1\nname = \"Test Floor\"\n\n[defaults]\nmodel = \"sonnet\"\neffort = \"low\"\n",
    )
    .unwrap();
    // A new Tower reads the file. The first Tower has the old floor.toml in memory.
    let socket = rig.root.path().join("second.sock");
    let second = Rig::make_tower(&rig.config, &socket, &rig.shared, |_| {});
    let view = second.floor_open(&rig.cwd).unwrap();
    let mut pm = view
        .employees
        .iter()
        .find(|e| e.slug == "pm")
        .unwrap()
        .clone();
    pm.model = Some("opus".into());
    second.employee_save(&rig.cwd, pm).unwrap();
    let a = second.session_start(&rig.cwd, "ceo", None, 24, 80).unwrap();
    let b = second.session_start(&rig.cwd, "pm", None, 24, 80).unwrap();
    let calls = rig.calls();
    let ceo = calls.iter().find(|c| c.slug == "ceo").unwrap();
    let pm = calls.iter().find(|c| c.slug == "pm").unwrap();
    assert_eq!(ceo.model.as_deref(), Some("sonnet"));
    assert_eq!(ceo.effort.as_deref(), Some("low"));
    assert_eq!(pm.model.as_deref(), Some("opus"), "an own model is kept");
    assert_eq!(pm.effort.as_deref(), Some("low"));
    second.session_stop(&a.session_id).unwrap();
    second.session_stop(&b.session_id).unwrap();
}
