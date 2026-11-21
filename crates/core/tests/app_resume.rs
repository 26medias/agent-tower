//! The restore of the last team (milestone M3): the list that `shutdown` stores before it kills
//! the sessions, the list that a manual stop clears, the list that survives a restart of the
//! `Tower`, and the background resume with a pause and with a failure of one start.

mod app_support;

use app_support::{Rig, Shared, wait_for};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower_core::app::Tower;

/// Close the first `Tower` of the rig with `ceo`, `pm` and `eng` live, and open a second one on the
/// same config and the same floor, with the pause of the resume set to `pause`. The second `Tower`
/// writes its calls to `shared`.
fn restart(rig: &Rig, pause: Duration, shared: &Arc<Shared>) -> Tower {
    let socket = rig
        .root
        .path()
        .join(format!("restart-{:?}.sock", Instant::now()));
    Rig::make_tower(&rig.config, &socket, shared, move |cfg| {
        cfg.resume_pause = pause;
    })
}

/// A rig whose team (`ceo`, `pm`, `eng`) was live when the app closed.
fn closed_rig() -> Rig {
    let rig = Rig::new();
    rig.create_floor();
    rig.add_employee("eng", Some("pm"));
    for slug in ["ceo", "pm", "eng"] {
        rig.start(slug);
        rig.live_idle(slug);
    }
    rig.tower.shutdown();
    rig
}

#[test]
fn shutdown_stores_the_live_employees_before_it_kills_them_and_the_list_survives_a_restart() {
    let rig = Rig::new();
    rig.create_floor();
    rig.add_employee("eng", Some("pm"));
    rig.add_employee("qa", Some("pm"));
    // `qa` has no session. `eng` has one that the operator stopped before the close.
    for slug in ["ceo", "pm", "eng"] {
        rig.start(slug);
        rig.live_idle(slug);
    }
    // A session puts its mark when it starts, so the list is there before the close.
    let mut before = rig.db().resume_marks().unwrap();
    before.sort();
    assert_eq!(before, ["ceo", "eng", "pm"]);
    let eng = rig.live_idle("eng");
    rig.stop_and_wait(&eng);
    // The manual stop takes `eng` out of the list.
    let mut after_stop = rig.db().resume_marks().unwrap();
    after_stop.sort();
    assert_eq!(after_stop, ["ceo", "pm"]);

    rig.tower.shutdown();
    // The marks are the sessions that were live when the app closed.
    let mut marks = rig.db().resume_marks().unwrap();
    marks.sort();
    assert_eq!(marks, ["ceo", "pm"]);

    // A new Tower (a new run of the app) shows the list, managers first. It resumes nothing.
    let shared = Arc::new(Shared::default());
    let second = restart(&rig, Duration::from_millis(1), &shared);
    let view = second.floor_open(&rig.cwd).unwrap();
    assert_eq!(view.resumable, ["ceo", "pm"]);
    assert_eq!(view.resume_progress, None);
    assert!(
        shared.calls.lock().unwrap().is_empty(),
        "no start at the app start"
    );
    // A manual stop is not part of the team: `eng` is not in the list.
    assert!(!view.resumable.contains(&"eng".to_owned()));
}

#[test]
fn a_floor_with_no_live_session_at_the_close_keeps_its_old_list() {
    let rig = closed_rig();
    let first = rig.db().resume_marks().unwrap().len();
    assert_eq!(first, 3);
    // A second run that starts nothing and closes: the list stays.
    let shared = Arc::new(Shared::default());
    let second = restart(&rig, Duration::from_millis(1), &shared);
    second.floor_open(&rig.cwd).unwrap();
    second.shutdown();
    assert_eq!(rig.db().resume_marks().unwrap().len(), 3);
}

#[test]
fn a_manual_stop_takes_the_employee_out_of_the_list() {
    let rig = closed_rig();
    let shared = Arc::new(Shared::default());
    let second = restart(&rig, Duration::from_millis(1), &shared);
    let view = second.floor_open(&rig.cwd).unwrap();
    assert_eq!(view.resumable, ["ceo", "pm", "eng"]);
    let started = second.session_resume(&rig.cwd, "pm", None, 24, 80).unwrap();
    // `pm` is live now, so it leaves the visible list. It is still stored.
    assert_eq!(
        second.floor_open(&rig.cwd).unwrap().resumable,
        ["ceo", "eng"]
    );
    assert_eq!(rig.db().resume_marks().unwrap().len(), 3);
    second.session_stop(&started.session_id).unwrap();
    wait_for("the stop", || {
        rig.db().resume_marks().unwrap() == ["ceo".to_owned(), "eng".to_owned()]
    });
    assert_eq!(rig.db().resume_marks().unwrap(), ["ceo", "eng"]);
    // A third run sees the list without `pm`.
    second.shutdown();
    let third = restart(&rig, Duration::from_millis(1), &Arc::new(Shared::default()));
    assert_eq!(
        third.floor_open(&rig.cwd).unwrap().resumable,
        ["ceo", "eng"]
    );
}

#[test]
fn the_team_resumes_in_order_with_a_pause_and_a_failed_start_does_not_stop_the_others() {
    let rig = closed_rig();
    let shared = Arc::new(Shared::default());
    let second = restart(&rig, Duration::from_millis(300), &shared);
    let events = Arc::new(app_support::Events::default());
    second.set_observer(events.clone());
    // `pm` cannot start (the adapter refuses). `ceo` and `eng` must still resume.
    shared.fail_slugs.lock().unwrap().insert("pm".to_owned());
    assert_eq!(
        second.floor_open(&rig.cwd).unwrap().resumable,
        ["ceo", "pm", "eng"]
    );

    let started_at = Instant::now();
    let progress = second.team_resume(&rig.cwd).unwrap();
    assert_eq!((progress.done, progress.total), (0, 3));
    // The call returned at once, and the progress shows in the floor view.
    assert!(started_at.elapsed() < Duration::from_millis(250));
    let shown = second.floor_open(&rig.cwd).unwrap();
    assert_eq!(shown.resume_progress.as_ref().map(|p| p.total), Some(3));
    assert!(
        shown.resumable.is_empty(),
        "the button shows the progress, not a list"
    );
    // A second call while it runs is refused.
    let again = second.team_resume(&rig.cwd).unwrap_err().to_string();
    assert!(again.contains("resuming now"), "{again}");

    wait_for("the resume ends", || {
        second
            .floor_open(&rig.cwd)
            .unwrap()
            .resume_progress
            .is_none()
    });
    // Two pauses of 300 ms lie between three starts.
    assert!(
        started_at.elapsed() >= Duration::from_millis(600),
        "{:?}",
        started_at.elapsed()
    );
    // The order is the order of the list. `pm` failed, the others live, with a resume of the
    // last CLI session (the plan has the old CLI session ID).
    let calls = shared.calls.lock().unwrap().clone();
    let slugs: Vec<&str> = calls.iter().map(|c| c.slug.as_str()).collect();
    assert_eq!(slugs, ["ceo", "eng"]);
    assert!(calls.iter().all(|c| c.resume.is_some()), "{calls:?}");
    // The failure came as a notice with the name of the employee and the error.
    let notices = events.notices.lock().unwrap().clone();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].1, "resume_failed");
    assert!(notices[0].2.contains("Could not resume pm"), "{notices:?}");
    assert!(notices[0].2.contains("forced test failure"), "{notices:?}");
    // The list is empty for the live ones; `pm` stays in the list for a new try.
    let view = second.floor_open(&rig.cwd).unwrap();
    assert_eq!(view.resumable, ["pm"]);
    // With nothing to resume the call is an error.
    shared.fail_slugs.lock().unwrap().clear();
    second.team_resume(&rig.cwd).unwrap();
    wait_for("the second resume ends", || {
        second
            .floor_open(&rig.cwd)
            .unwrap()
            .resume_progress
            .is_none()
    });
    let error = second.team_resume(&rig.cwd).unwrap_err().to_string();
    assert!(error.contains("no session of the last team"), "{error}");
}

#[test]
fn an_employee_that_never_had_a_turn_or_is_gone_is_not_in_the_list() {
    let rig = Rig::new();
    rig.create_floor();
    rig.add_employee("eng", Some("pm"));
    rig.start("ceo");
    rig.live_idle("ceo");
    // `pm` starts but never gets a turn (a script that does nothing is not needed: the session is
    // stopped at once, before its first turn).
    let pm = rig.start("pm");
    rig.tower.shutdown();
    let marks = rig.db().resume_marks().unwrap();
    assert!(marks.contains(&"ceo".to_owned()));
    let _ = pm;
    // `eng` is removed from the team after the close.
    rig.db()
        .replace_resume_marks(&["ceo".to_owned(), "eng".to_owned(), "ghost".to_owned()], 1)
        .unwrap();
    let second = restart(&rig, Duration::from_millis(1), &Arc::new(Shared::default()));
    second.employee_remove(&rig.cwd, "eng").unwrap();
    let view = second.floor_open(&rig.cwd).unwrap();
    // `ghost` was never an employee. `eng` is removed. `eng` never started: it has no session.
    assert_eq!(view.resumable, ["ceo"]);
}

/// Finding M7: the list was written only at `shutdown`, so a crash lost it. Now a session puts its
/// mark when it starts. Two sessions start, the first Tower is NOT closed (a crash: the
/// `Drop` that calls `shutdown` does not run, the first Tower stays alive here), and a new Tower
/// on the same floor shows both.
#[test]
fn a_crash_of_the_app_keeps_the_sessions_that_were_live() {
    let rig = Rig::new();
    rig.create_floor();
    for slug in ["ceo", "pm"] {
        rig.start(slug);
        rig.live_idle(slug);
    }
    // No `shutdown`. The marks are in the database already.
    let mut marks = rig.db().resume_marks().unwrap();
    marks.sort();
    assert_eq!(marks, ["ceo", "pm"]);
    let shared = Arc::new(Shared::default());
    let second = restart(&rig, Duration::from_millis(1), &shared);
    let view = second.floor_open(&rig.cwd).unwrap();
    assert_eq!(view.resumable, ["ceo", "pm"]);
}

/// A session that ends by itself (the CLI exits) leaves the list. The setup session never enters.
#[test]
fn a_session_that_ends_by_itself_leaves_the_list_and_the_setup_session_never_enters() {
    let rig = Rig::new();
    rig.create_floor();
    for slug in ["ceo", "pm"] {
        rig.start(slug);
        rig.live_idle(slug);
    }
    let mut marks = rig.db().resume_marks().unwrap();
    marks.sort();
    assert_eq!(marks, ["ceo", "pm"]);
    // The CLI of `pm` ends: the fake CLI exits when it reads `/exit`.
    let pm = rig.live_idle("pm");
    rig.tower.pty_write(&pm, b"/exit 3\r").unwrap();
    wait_for("the end of the session of pm", || {
        rig.db().resume_marks().unwrap() == ["ceo"]
    });
    // The setup session has no mark, at the start and at the end.
    let setup = rig
        .tower
        .setup_start(&rig.cwd, tower_core::model::CliKind::Claude, 24, 80)
        .unwrap()
        .session_id;
    assert_eq!(rig.db().resume_marks().unwrap(), ["ceo"]);
    rig.stop_and_wait(&setup);
    assert_eq!(rig.db().resume_marks().unwrap(), ["ceo"]);
}
