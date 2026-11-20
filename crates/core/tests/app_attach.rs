//! Output and attach (test 6 of the task). The oracle is the PTY log file: it has every byte that
//! the process wrote, and `scrollback ++ live output` must become equal to it, with no gap and
//! no duplicate.

mod app_support;

use app_support::{Collector, Rig, Shared, lock, wait_for};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower_core::app::{OutputSink, SCROLLBACK_LIMIT};

type Hook = Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;

fn fast_rig() -> Rig {
    let rig = Rig::new();
    lock(&rig.shared.extra_args).extend(
        ["--work-ms", "0", "--hook-gap-ms", "0"]
            .iter()
            .map(|s| s.to_string()),
    );
    rig.create_floor();
    rig
}

/// Start the CEO and wait until the first turn is over: the process is in raw mode and ready.
/// Input that is written before that is lost (the terminal drops it when the mode changes).
fn start_ready(rig: &Rig) -> String {
    let id = rig.start("ceo").session_id;
    rig.wait_first_turn(&id);
    id
}

/// The numbers N of each `you said: line-N` in the bytes, in order.
fn said_lines(bytes: &[u8]) -> Vec<u32> {
    let text = String::from_utf8_lossy(bytes);
    text.match_indices("you said: line-")
        .filter_map(|(at, marker)| {
            text[at + marker.len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .ok()
        })
        .collect()
}

fn send_lines(rig: &Rig, id: &str, range: std::ops::Range<u32>) {
    for n in range {
        rig.tower
            .pty_write(id, format!("line-{n:03}\r").as_bytes())
            .unwrap();
    }
}

/// Wait until `scrollback ++ live` is the same as the log. A gap or a duplicate never gets there.
fn assert_converges_to_log(rig: &Rig, id: &str, scrollback: &[u8], live: &Collector) {
    let deadline = std::time::Instant::now() + app_support::LIMIT;
    loop {
        let mut joined = scrollback.to_vec();
        joined.extend(live.get());
        let log = rig.log_bytes(id);
        if joined == log {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "scrollback + live ({}) is not the log ({}). Common prefix: {}",
            joined.len(),
            log.len(),
            joined.iter().zip(&log).take_while(|(a, b)| a == b).count()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn live_output_arrives_in_order_and_equals_the_log() {
    let rig = fast_rig();
    // Attach at once, before the process has drawn anything.
    let id = rig.start("ceo").session_id;
    let live = Collector::default();
    let scrollback = rig
        .tower
        .session_attach(&id, Arc::new(live.clone()))
        .unwrap();
    rig.wait_first_turn(&id);
    send_lines(&rig, &id, 0..60);
    wait_for("the last line", || said_lines(&live.get()).ends_with(&[59]));
    // Each numbered line came once, in order.
    assert_eq!(said_lines(&live.get()), (0..60).collect::<Vec<u32>>());
    assert_converges_to_log(&rig, &id, &scrollback, &live);
    rig.stop_and_wait(&id);
}

#[test]
fn attach_after_some_output_gives_scrollback_and_then_continues() {
    let rig = fast_rig();
    let id = start_ready(&rig);
    send_lines(&rig, &id, 0..30);
    wait_for("30 lines in the log", || {
        said_lines(&rig.log_bytes(&id)).ends_with(&[29])
    });

    let live = Collector::default();
    let scrollback = rig
        .tower
        .session_attach(&id, Arc::new(live.clone()))
        .unwrap();
    assert!(
        !said_lines(&scrollback).is_empty(),
        "the scrollback has the output that came before the attach"
    );
    send_lines(&rig, &id, 30..60);
    wait_for("line 59 on the stream", || {
        said_lines(&live.get()).ends_with(&[59])
    });
    let mut all = scrollback.clone();
    all.extend(live.get());
    // The full sequence: 0..60, each once, in order, across the border.
    assert_eq!(said_lines(&all), (0..60).collect::<Vec<u32>>());
    assert_converges_to_log(&rig, &id, &scrollback, &live);
    rig.stop_and_wait(&id);
}

#[test]
fn output_during_the_attach_goes_to_the_new_sink_once() {
    // The pause hook runs inside `attach`, after the copy of the scrollback. It makes the
    // process write more output and waits until that output has reached the Tower. A build
    // that has a gap or a duplicate between the copy and the new sink shows it here.
    let hook: Hook = Arc::new(Mutex::new(None));
    let rig = {
        let hook = Arc::clone(&hook);
        Rig::with(move |cfg| {
            cfg.attach_pause = Some(Arc::new(move || {
                let run = lock(&hook).clone();
                if let Some(run) = run {
                    run();
                }
            }));
        })
    };
    lock(&rig.shared.extra_args).extend(
        ["--work-ms", "0", "--hook-gap-ms", "0"]
            .iter()
            .map(|s| s.to_string()),
    );
    rig.create_floor();
    let id = start_ready(&rig);
    send_lines(&rig, &id, 0..5);
    wait_for("5 lines in the log", || {
        said_lines(&rig.log_bytes(&id)).ends_with(&[4])
    });

    // An old sink is attached first, so that a build with a gap sends the output to it.
    let old = Collector::default();
    rig.tower
        .session_attach(&id, Arc::new(old.clone()))
        .unwrap();
    {
        let tower = Arc::clone(&rig.tower);
        let id = id.clone();
        let log = rig.log_path(&id);
        *lock(&hook) = Some(Arc::new(move || {
            tower.pty_write(&id, b"line-100\r").unwrap();
            // The process writes the answer in a few ms. Wait until it is in the log.
            wait_for("line 100 in the log", || {
                std::fs::read(&log).is_ok_and(|bytes| {
                    String::from_utf8_lossy(&bytes).contains("you said: line-100")
                })
            });
            // The output thread cannot take the buffer while this hook holds it.
            std::thread::sleep(Duration::from_millis(300));
        }));
    }
    let new = Collector::default();
    let scrollback = rig
        .tower
        .session_attach(&id, Arc::new(new.clone()))
        .unwrap();
    *lock(&hook) = None;

    wait_for("line 100 on the new sink or in the scrollback", || {
        let mut all = scrollback.clone();
        all.extend(new.get());
        said_lines(&all).ends_with(&[100])
    });
    let mut all = scrollback.clone();
    all.extend(new.get());
    assert_eq!(said_lines(&all), [0, 1, 2, 3, 4, 100]);
    assert_converges_to_log(&rig, &id, &scrollback, &new);
    rig.stop_and_wait(&id);
}

#[test]
fn scrollback_is_cut_to_the_limit_and_is_the_tail_of_the_log() {
    let rig = fast_rig();
    let id = start_ready(&rig);
    // About 330 KB in one paste: more than the 256 KB limit.
    let mut paste = b"\x1b[200~".to_vec();
    for n in 0..30_000u32 {
        paste.extend_from_slice(format!("{n:09}\n").as_bytes());
    }
    paste.extend_from_slice(b"\x1b[201~\r");
    rig.tower.pty_write(&id, &paste).unwrap();
    wait_for("the last number in the log", || {
        String::from_utf8_lossy(&rig.log_bytes(&id)).contains("000029999")
    });
    // The text after the last number is a CR LF. Wait until the output is quiet.
    let mut size = 0;
    wait_for("the output to settle", || {
        let now = rig.log_bytes(&id).len();
        let same = now == size;
        size = now;
        std::thread::sleep(Duration::from_millis(100));
        same
    });
    let sink = Collector::default();
    let scrollback = rig
        .tower
        .session_attach(&id, Arc::new(sink.clone()))
        .unwrap();
    let log = rig.log_bytes(&id);
    assert!(log.len() > SCROLLBACK_LIMIT, "the test output is too small");
    assert_eq!(scrollback.len(), SCROLLBACK_LIMIT);
    assert_eq!(scrollback[..], log[log.len() - SCROLLBACK_LIMIT..]);
    rig.stop_and_wait(&id);
}

#[test]
fn a_new_attach_replaces_the_old_sink() {
    let rig = fast_rig();
    let id = start_ready(&rig);
    let first = Collector::default();
    rig.tower
        .session_attach(&id, Arc::new(first.clone()))
        .unwrap();
    send_lines(&rig, &id, 0..3);
    wait_for("line 2 on the first sink", || {
        said_lines(&first.get()).ends_with(&[2])
    });
    let second = Collector::default();
    let scrollback = rig
        .tower
        .session_attach(&id, Arc::new(second.clone()))
        .unwrap();
    let first_size = first.get().len();
    send_lines(&rig, &id, 3..6);
    wait_for("line 5 on the second sink", || {
        said_lines(&second.get()).ends_with(&[5])
    });
    // The old sink is quiet after the replacement (output of the in-flight chunk is allowed
    // only before the attach returned, and no output was in flight here).
    assert_eq!(first.get().len(), first_size);
    assert_converges_to_log(&rig, &id, &scrollback, &second);
    rig.stop_and_wait(&id);
}

#[test]
fn the_scrollback_of_an_old_session_comes_from_its_log_file() {
    let rig = fast_rig();
    let id = start_ready(&rig);
    send_lines(&rig, &id, 0..3);
    wait_for("line 2 in the log", || {
        said_lines(&rig.log_bytes(&id)).ends_with(&[2])
    });
    rig.stop_and_wait(&id);
    let log = rig.log_bytes(&id);

    // A new Tower (a new run of the app) shows the session as stopped, and attaches from the log.
    let socket = rig.root.path().join("second.sock");
    let second = Rig::make_tower(&rig.config, &socket, &Arc::new(Shared::default()), |_| {});
    let view = second.floor_open(&rig.cwd).unwrap();
    assert_eq!(view.sessions.len(), 1);
    assert_eq!(view.sessions[0].session_id, id);
    assert_eq!(view.sessions[0].status, tower_core::status::Status::Stopped);
    assert!(view.sessions[0].can_resume);
    let sink = Collector::default();
    let scrollback = second.session_attach(&id, Arc::new(sink.clone())).unwrap();
    assert_eq!(scrollback, log);
    assert!(sink.get().is_empty());
    // A name that is not a session ID is refused, and cannot reach another file.
    assert!(
        second
            .session_attach("../../etc/passwd", Arc::new(sink.clone()))
            .is_err()
    );
    let _: &dyn OutputSink = &sink;
}
