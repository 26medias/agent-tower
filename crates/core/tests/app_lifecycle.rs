//! Shutdown (test 9 of the task): dropping a `Tower` with live sessions ends every process, does
//! not hang, writes the end of each session, and removes the socket.

mod app_support;

use app_support::{Rig, pids_with, wait_for};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower_core::status::Status;

#[test]
fn drop_with_live_sessions_leaves_no_process_and_no_socket() {
    let rig = Rig::new();
    rig.create_floor();
    let ceo = rig.start("ceo");
    let pm = rig.start("pm");
    rig.wait_first_turn(&ceo.session_id);
    rig.wait_first_turn(&pm.session_id);
    assert!(
        rig.socket.exists(),
        "the hook socket exists while the Tower lives"
    );

    // The processes carry the temp folder in their command line (the `--record` path).
    let needle = rig.cwd.display().to_string();
    let before = pids_with(&needle);
    assert_eq!(before.len(), 2, "two fake-cli processes: {before:?}");

    let Rig {
        tower,
        socket,
        cwd,
        events,
        root,
        ..
    } = rig;
    let tower = Arc::try_unwrap(tower).unwrap_or_else(|_| panic!("the test holds the only Arc"));
    let started = Instant::now();
    let done = std::thread::spawn(move || drop(tower));
    wait_for("the drop to end", || done.is_finished());
    let took = started.elapsed();
    assert!(took < Duration::from_secs(5), "the drop took {took:?}");
    done.join().unwrap();

    assert_eq!(pids_with(&needle), Vec::<u32>::new(), "processes are left");
    assert!(!socket.exists(), "the socket file is left");
    // The end of each session is in the database.
    let db = tower_core::store::Database::open(&cwd.join(".swarm/state/floor.db")).unwrap();
    for (slug, id) in [("ceo", &ceo.session_id), ("pm", &pm.session_id)] {
        let row = db.last_session(slug).unwrap().unwrap();
        assert_eq!(&row.app_session_id, id);
        assert!(row.ended_at.is_some(), "{slug} has no end time");
        assert_eq!(row.last_status, "stopped");
    }
    assert_eq!(
        events.latest(&ceo.session_id).unwrap().status,
        Status::Stopped
    );
    drop(root);
}

#[test]
fn drop_kills_a_process_that_ignores_sighup() {
    let rig = Rig::new();
    rig.create_floor();
    // The shell ignores SIGHUP.
    *app_support::lock(&rig.shared.program) = Some((
        "/bin/sh".into(),
        vec![
            "-c".into(),
            format!(
                "trap '' HUP; while :; do sleep 1; done # {}",
                rig.cwd.display()
            ),
        ],
    ));
    let session = rig.start("ceo");
    let needle = rig.cwd.display().to_string();
    wait_for("the process", || !pids_with(&needle).is_empty());

    let Rig { tower, cwd, .. } = rig;
    let tower = Arc::try_unwrap(tower).unwrap_or_else(|_| panic!("the test holds the only Arc"));
    let started = Instant::now();
    drop(tower);
    let took = started.elapsed();
    assert!(took < Duration::from_secs(10), "the drop took {took:?}");
    assert_eq!(pids_with(&needle), Vec::<u32>::new(), "the shell is left");
    let db = tower_core::store::Database::open(&cwd.join(".swarm/state/floor.db")).unwrap();
    let row = db.last_session("ceo").unwrap().unwrap();
    assert_eq!(row.app_session_id, session.session_id);
    assert!(row.ended_at.is_some());
}

#[test]
fn stop_kills_a_process_that_ignores_sighup() {
    let rig = Rig::new();
    rig.create_floor();
    *app_support::lock(&rig.shared.program) = Some((
        "/bin/sh".into(),
        vec![
            "-c".into(),
            format!(
                "trap '' HUP; while :; do sleep 1; done # {}",
                rig.cwd.display()
            ),
        ],
    ));
    let session = rig.start("ceo");
    let needle = rig.cwd.display().to_string();
    wait_for("the process", || !pids_with(&needle).is_empty());
    let started = Instant::now();
    rig.tower.session_stop(&session.session_id).unwrap();
    rig.events.wait_status(&session.session_id, Status::Stopped);
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(pids_with(&needle), Vec::<u32>::new());
}

#[test]
fn nothing_starts_after_shutdown() {
    let rig = Rig::new();
    rig.create_floor();
    rig.tower.shutdown();
    let error = rig.try_start("ceo").unwrap_err();
    assert!(error.to_string().contains("closing"), "{error}");
    assert!(!rig.socket.exists());
    assert_eq!(rig.calls().len(), 0);
}
