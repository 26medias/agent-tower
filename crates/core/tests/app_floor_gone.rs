//! The path check of `floor_open` and `floor_create` (D5), and the floor whose folder is deleted
//! or moved while the app runs (D4). See `docs/ipc-final.md`. Temporary folders only. The
//! sessions are the real `fake-cli`.

mod app_support;

use app_support::{Ev, Rig, lock, wait_for};
use serde_json::json;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tower_core::app::TowerError;
use tower_core::env::EnvMap;
use tower_core::store::Database;

/// A pause that is long for the watcher: it has many ticks in it.
const MANY_TICKS: Duration = Duration::from_millis(500);

const SHORT_WATCH: Duration = Duration::from_millis(25);

fn text(error: TowerError) -> String {
    error.to_string()
}

/// True when this process can write in a folder of mode 0555 (the root user can). The tests of
/// the mode skip themselves then.
fn mode_is_ignored() -> bool {
    unsafe { libc::geteuid() == 0 }
}

fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

/// A rig whose environment has a fake home folder (the folder `home` of the returned temp
/// folder, which the caller keeps alive).
fn rig_with_home() -> (Rig, PathBuf, tempfile::TempDir) {
    let root = tempfile::TempDir::new().unwrap();
    let home = fs::canonicalize(root.path()).unwrap().join("home");
    fs::create_dir(&home).unwrap();
    let home_value = home.clone();
    let rig = Rig::with(move |config| {
        config.parent_env = [
            ("HOME", home_value.as_os_str()),
            ("LANG", "C.UTF-8".as_ref()),
        ]
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect::<EnvMap>();
    });
    (rig, home, root)
}

// ---------------------------------------------------------------------------------------------
// D5: the path check
// ---------------------------------------------------------------------------------------------

#[test]
fn a_relative_path_gives_not_absolute_before_any_other_answer() {
    let rig = Rig::new();
    let expected = "NOT_ABSOLUTE: Type the full path of the folder, for example /home/you/project.";
    for path in ["project", "./project", "../x/y", "", "~user/x"] {
        let error = rig.tower.floor_open(Path::new(path)).unwrap_err();
        assert_eq!(text(error), expected, "floor_open of {path:?}");
        let error = rig.tower.floor_create(Path::new(path), "Name").unwrap_err();
        assert_eq!(text(error), expected, "floor_create of {path:?}");
    }
    // The path check is before the check of the name.
    let error = rig.tower.floor_create(Path::new("rel"), "  ").unwrap_err();
    assert!(text(error).starts_with("NOT_ABSOLUTE:"));
}

#[test]
fn a_path_that_does_not_exist_gives_not_found() {
    let rig = Rig::new();
    let missing = rig.cwd.join("nope");
    let expected = format!(
        "NOT_FOUND: The folder {} does not exist.",
        missing.display()
    );
    assert_eq!(text(rig.tower.floor_open(&missing).unwrap_err()), expected);
    assert_eq!(
        text(rig.tower.floor_create(&missing, "Name").unwrap_err()),
        expected
    );
    // A path below a file does not exist either.
    let file = rig.cwd.join("a-file");
    fs::write(&file, "x").unwrap();
    let below = file.join("child");
    assert_eq!(
        text(rig.tower.floor_open(&below).unwrap_err()),
        format!("NOT_FOUND: The folder {} does not exist.", below.display())
    );
    assert!(!missing.exists(), "the check makes no folder");
    assert!(
        rig.tower.floor_list().unwrap().is_empty(),
        "a failed open adds nothing to the list"
    );
}

#[test]
fn a_file_gives_not_a_folder() {
    let rig = Rig::new();
    let file = rig.cwd.join("a-file");
    fs::write(&file, "x").unwrap();
    let expected = format!("NOT_A_FOLDER: {} is a file, not a folder.", file.display());
    assert_eq!(text(rig.tower.floor_open(&file).unwrap_err()), expected);
    assert_eq!(
        text(rig.tower.floor_create(&file, "Name").unwrap_err()),
        expected
    );
}

#[test]
fn a_folder_that_cannot_be_written_gives_no_write() {
    if mode_is_ignored() {
        return;
    }
    let rig = Rig::new();
    set_mode(&rig.cwd, 0o555);
    let expected = format!(
        "NO_WRITE: You cannot write in the folder {}.",
        rig.cwd.display()
    );
    let open = rig.tower.floor_open(&rig.cwd);
    let create = rig.tower.floor_create(&rig.cwd, "Name");
    set_mode(&rig.cwd, 0o755);
    assert_eq!(text(open.unwrap_err()), expected);
    assert_eq!(text(create.unwrap_err()), expected);
    assert!(!rig.cwd.join(".swarm").exists(), "nothing was written");
    // The same folder with write rights gives the normal answers.
    assert!(text(rig.tower.floor_open(&rig.cwd).unwrap_err()).starts_with("NO_FLOOR:"));
}

#[test]
fn the_write_check_is_real_and_looks_at_swarm_when_there_is_one() {
    if mode_is_ignored() {
        return;
    }
    let rig = Rig::new();
    rig.create_floor();
    // A second Tower (a fresh start of the app) opens the floor in a folder that is read-only
    // but has a writable `.swarm/`: the app only writes in `.swarm/`, so this works.
    let socket = rig.root.path().join("second.sock");
    let shared = std::sync::Arc::new(app_support::Shared::default());
    let second = Rig::make_tower(&rig.root.path().join("config2"), &socket, &shared, |_| {});
    set_mode(&rig.cwd, 0o555);
    let view = second.floor_open(&rig.cwd);
    set_mode(&rig.cwd, 0o755);
    assert!(!view.unwrap().gone);

    // A read-only `.swarm/` is refused, whatever the folder allows.
    let second_socket = rig.root.path().join("third.sock");
    let third = Rig::make_tower(
        &rig.root.path().join("config3"),
        &second_socket,
        &shared,
        |_| {},
    );
    set_mode(&rig.cwd.join(".swarm"), 0o555);
    let result = third.floor_open(&rig.cwd);
    set_mode(&rig.cwd.join(".swarm"), 0o755);
    assert_eq!(
        text(result.unwrap_err()),
        format!(
            "NO_WRITE: You cannot write in the folder {}.",
            rig.cwd.display()
        )
    );
}

#[test]
fn a_good_folder_keeps_its_answers_and_the_check_leaves_no_file() {
    let rig = Rig::new();
    // No `.swarm/`: the old answer, which the UI uses to offer "create".
    let error = rig.tower.floor_open(&rig.cwd).unwrap_err();
    assert!(matches!(error, TowerError::NoFloor(_)), "{error}");
    assert!(
        text(error).starts_with("NO_FLOOR: there is no floor at"),
        "the text is the same as before"
    );
    assert!(rig.tower.floor_list().unwrap().is_empty());
    let names = |dir: &Path| -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    };
    assert!(names(&rig.cwd).is_empty(), "no probe file stays");

    // With `.swarm/`: the view, and `gone` is false.
    rig.create_floor();
    let view = rig.tower.floor_open(&rig.cwd).unwrap();
    assert!(!view.gone);
    assert_eq!(view.name, "Test Floor");
    assert_eq!(names(&rig.cwd), vec![".swarm".to_owned()]);
    assert!(
        !names(&rig.cwd.join(".swarm"))
            .iter()
            .any(|n| n.starts_with(".agent-tower-write-check"))
    );
}

#[test]
fn a_tilde_path_means_the_home_folder_of_the_environment() {
    let (rig, home, _keep) = rig_with_home();
    let project = home.join("proj");
    fs::create_dir(&project).unwrap();

    // `~/proj` is the folder under the home folder. It has no floor yet.
    let error = rig.tower.floor_open(Path::new("~/proj")).unwrap_err();
    assert!(
        text(error).starts_with(&format!(
            "NO_FLOOR: there is no floor at {}.",
            project.display()
        )),
        "the error names the expanded path"
    );
    assert_eq!(
        text(rig.tower.floor_open(Path::new("~/missing")).unwrap_err()),
        format!(
            "NOT_FOUND: The folder {} does not exist.",
            home.join("missing").display()
        )
    );
    // Create through `~`: the floor is at the real path.
    let view = rig.tower.floor_create(Path::new("~/proj"), "Home").unwrap();
    assert_eq!(view.path, project);
    assert!(project.join(".swarm/floor.toml").is_file());
    assert_eq!(
        rig.tower.floor_open(Path::new("~/proj")).unwrap().name,
        "Home"
    );
    // A bare `~` is the home folder itself.
    assert!(text(rig.tower.floor_open(Path::new("~")).unwrap_err()).starts_with("NO_FLOOR:"));
}

// ---------------------------------------------------------------------------------------------
// D4: a floor folder that is deleted or moved
// ---------------------------------------------------------------------------------------------

fn gone_text(path: &Path) -> String {
    format!(
        "FLOOR_GONE: The folder of this floor is gone: {}.",
        path.display()
    )
}

/// The rows of a database that the commands write. A write after the move changes one of them.
fn counts(db: &Database) -> (usize, usize, usize) {
    (
        db.list_tasks(None, None).unwrap().len(),
        db.all_messages().unwrap().len(),
        db.list_sessions("eng").unwrap().len(),
    )
}

/// The row count of each table of a database file, by table name. The test reads the file with
/// its own connection, so it does not depend on a handle of the app.
fn table_counts(file: &Path) -> std::collections::BTreeMap<String, i64> {
    let conn = rusqlite::Connection::open(file).unwrap();
    let names: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    names
        .into_iter()
        .map(|name| {
            let count = conn
                .query_row(&format!("SELECT count(*) FROM \"{name}\""), [], |row| {
                    row.get(0)
                })
                .unwrap();
            (name, count)
        })
        .collect()
}

fn move_to(rig: &Rig, name: &str) -> PathBuf {
    let moved = rig.root.path().join(name);
    fs::rename(&rig.cwd, &moved).unwrap();
    moved
}

/// `cp -a`: the folder with all its files (the database has a `-wal` file).
fn copy_folder(from: &Path, to: &Path) {
    let status = std::process::Command::new("cp")
        .arg("-a")
        .arg(from)
        .arg(to)
        .status()
        .unwrap();
    assert!(status.success());
}

/// A rig with a floor, the employee `eng`, one task, and a live idle `ceo`.
fn live_rig() -> (Rig, String) {
    let rig = Rig::new();
    rig.create_floor();
    rig.add_employee("eng", Some("ceo"));
    rig.tower.task_create(&rig.cwd, "First", "A task.").unwrap();
    rig.start("ceo");
    let ceo = rig.live_idle("ceo");
    (rig, ceo)
}

#[test]
fn a_moved_floor_is_gone_for_each_command_and_nothing_is_written() {
    let (rig, ceo) = live_rig();
    let before = counts(&Database::open(&rig.cwd.join(".swarm/state/floor.db")).unwrap());
    assert_eq!(before, (1, 0, 0));
    let table_before = table_counts(&rig.cwd.join(".swarm/state/floor.db"));
    assert!(!rig.tower.floor_list().unwrap()[0].missing);
    assert!(!rig.tower.floor_open(&rig.cwd).unwrap().gone);

    let moved = move_to(&rig, "moved");

    // The list shows it at once (it is computed at each call).
    let list = rig.tower.floor_list().unwrap();
    assert!(list[0].missing, "{list:?}");
    // The view: the last known one, with `gone: true`. No failure.
    let view = rig.tower.floor_open(&rig.cwd).unwrap();
    assert!(view.gone);
    assert_eq!(view.name, "Test Floor");
    assert_eq!(view.employees.len(), 3);
    let ceo_view = view.sessions.iter().find(|s| s.slug == "ceo").unwrap();
    assert_eq!(ceo_view.session_id, ceo, "the live session is still there");
    assert_ne!(
        ceo_view.status,
        tower_core::status::Status::Stopped,
        "its process runs"
    );

    // The operator commands.
    let expected = gone_text(&rig.cwd);
    let employee = view.employees[0].clone();
    for (what, error) in [
        (
            "task_create",
            rig.tower.task_create(&rig.cwd, "Second", "x").err(),
        ),
        (
            "employee_save",
            rig.tower.employee_save(&rig.cwd, employee).err(),
        ),
        (
            "session_start",
            rig.tower.session_start(&rig.cwd, "eng", None, 24, 80).err(),
        ),
        ("task_list", rig.tower.task_list(&rig.cwd).err()),
        (
            "employee_remove",
            rig.tower.employee_remove(&rig.cwd, "eng").err(),
        ),
        ("floor_logo", rig.tower.floor_logo(&rig.cwd).err()),
        ("team_resume", rig.tower.team_resume(&rig.cwd).err()),
    ] {
        let error = error.unwrap_or_else(|| panic!("{what} must fail"));
        assert_eq!(text(error), expected, "{what}");
    }

    // The MCP tools of the live session. Each one answers FLOOR_GONE first.
    for (tool, args) in [
        (
            "task_create",
            json!({"title": "From the tool", "description": "x"}),
        ),
        ("task_list", json!({})),
        ("org_chart", json!({})),
        ("send", json!({"to": "eng", "text": "hello"})),
        (
            "employee_start",
            json!({"employee": "eng", "message": "start"}),
        ),
        ("inbox", json!({})),
        ("wait", json!({"timeout_s": 0})),
        ("task_get", json!({"task": 1})),
    ] {
        let result = rig.tool("ceo", tool, args);
        assert!(result.is_error, "{tool}: {}", result.text);
        assert_eq!(result.text, expected, "{tool}");
    }

    // The stop of a session still works, and so does `floor_forget` after it.
    let marks_before = Database::open(&moved.join(".swarm/state/floor.db"))
        .unwrap()
        .resume_marks()
        .unwrap();
    assert!(
        !marks_before.is_empty(),
        "the live session has a resume mark"
    );
    rig.stop_and_wait(&ceo);
    assert_eq!(
        Database::open(&moved.join(".swarm/state/floor.db"))
            .unwrap()
            .resume_marks()
            .unwrap(),
        marks_before,
        "the stop did not clear the mark: the app wrote nothing through its handle"
    );

    // The commands wrote nothing: the database of the moved folder has the same rows. One table
    // is the exception. A live session still holds its database handle, and the handle follows
    // the moved file (`live.rs`, `record_status`). So the status events of that session (the
    // stop above made one) are written into the moved file. No other table can grow: all the
    // other tables must have the same row count as before the move.
    let after = counts(&Database::open(&moved.join(".swarm/state/floor.db")).unwrap());
    assert_eq!(after, before);
    let mut table_after = table_counts(&moved.join(".swarm/state/floor.db"));
    let events_after = table_after.remove("status_events").expect("status_events");
    let mut table_before = table_before;
    let events_before = table_before.remove("status_events").expect("status_events");
    assert_eq!(
        table_after, table_before,
        "only status_events may grow after the move"
    );
    assert!(
        events_after > events_before,
        "the live session writes its status events into the moved file: {events_before} then {events_after}"
    );
    assert_eq!(rig.calls().len(), 1, "no session of `eng` started");

    rig.tower.floor_forget(&rig.cwd).unwrap();
    assert!(rig.tower.floor_list().unwrap().is_empty());
}

/// A `.swarm` that is a link to a folder with no `floor.toml` is not a floor. The answer is
/// `NO_FLOOR:`, as for a folder with no `.swarm` (and as for a link that points nowhere). It is
/// not a raw I/O error.
#[test]
fn a_swarm_link_to_a_folder_with_no_floor_file_is_no_floor() {
    let rig = Rig::new();
    let target = tempfile::TempDir::new().unwrap();
    std::os::unix::fs::symlink(target.path(), rig.cwd.join(".swarm")).unwrap();
    let error = rig.tower.floor_open(&rig.cwd).unwrap_err();
    assert!(matches!(error, TowerError::NoFloor(_)), "{error}");
    let message = text(error);
    assert!(
        message.starts_with("NO_FLOOR: there is no floor at"),
        "{message}"
    );
    assert!(!message.contains("I/O error"), "{message}");
    assert!(rig.tower.floor_list().unwrap().is_empty());
}

#[test]
fn a_deleted_floor_folder_is_gone_too_and_forget_works() {
    let rig = Rig::new();
    rig.create_floor();
    fs::remove_dir_all(&rig.cwd).unwrap();
    assert!(rig.tower.floor_list().unwrap()[0].missing);
    let view = rig.tower.floor_open(&rig.cwd).unwrap();
    assert!(view.gone);
    assert_eq!(view.employees.len(), 2);
    assert_eq!(
        text(rig.tower.task_create(&rig.cwd, "x", "y").unwrap_err()),
        gone_text(&rig.cwd)
    );
    rig.tower.floor_forget(&rig.cwd).unwrap();
    assert!(rig.tower.floor_list().unwrap().is_empty());
    // After the forget, the Tower holds no floor: the answer is the path check.
    assert!(text(rig.tower.floor_open(&rig.cwd).unwrap_err()).starts_with("NOT_FOUND:"));
}

#[test]
fn a_floor_with_no_swarm_folder_or_no_floor_file_is_gone() {
    let rig = Rig::new();
    rig.create_floor();
    let floor_file = rig.cwd.join(".swarm/floor.toml");
    let saved = fs::read(&floor_file).unwrap();
    fs::remove_file(&floor_file).unwrap();
    assert!(rig.tower.floor_list().unwrap()[0].missing);
    assert!(rig.tower.floor_open(&rig.cwd).unwrap().gone);
    assert_eq!(
        text(rig.tower.task_create(&rig.cwd, "x", "y").unwrap_err()),
        gone_text(&rig.cwd)
    );
    fs::write(&floor_file, saved).unwrap();
    assert!(!rig.tower.floor_list().unwrap()[0].missing);
    rig.tower.task_create(&rig.cwd, "back", "y").unwrap();
}

#[test]
fn a_new_empty_folder_at_the_old_path_gets_no_write_from_the_moved_database() {
    let rig = Rig::new();
    rig.create_floor();
    rig.add_employee("eng", Some("ceo"));
    rig.tower.task_create(&rig.cwd, "First", "A task.").unwrap();
    let moved = move_to(&rig, "moved");
    fs::create_dir(&rig.cwd).unwrap();

    assert!(rig.tower.floor_list().unwrap()[0].missing);
    assert!(rig.tower.floor_open(&rig.cwd).unwrap().gone);
    assert_eq!(
        text(rig.tower.task_create(&rig.cwd, "Second", "x").unwrap_err()),
        gone_text(&rig.cwd)
    );
    assert!(
        rig.tower
            .employee_save(
                &rig.cwd,
                rig.tower.floor_open(&rig.cwd).unwrap().employees[0].clone()
            )
            .is_err()
    );
    let moved_db = Database::open(&moved.join(".swarm/state/floor.db")).unwrap();
    assert_eq!(
        counts(&moved_db),
        (1, 0, 0),
        "the moved database has no new row"
    );
    assert!(
        !rig.cwd.join(".swarm").exists(),
        "and the new folder got nothing"
    );
}

#[test]
fn a_floor_that_comes_back_opens_its_database_again_from_the_path() {
    let rig = Rig::new();
    rig.create_floor();
    rig.tower.task_create(&rig.cwd, "First", "A task.").unwrap();

    // The folder moves, a command sees it gone, and then a COPY of the floor is put at the old
    // path (a restore from a backup). The open handle points at the moved file. A write
    // through it would land in the moved folder, not at the path.
    let moved = move_to(&rig, "moved");
    assert_eq!(
        text(rig.tower.task_create(&rig.cwd, "Lost", "x").unwrap_err()),
        gone_text(&rig.cwd)
    );
    copy_folder(&moved, &rig.cwd);

    let task = rig.tower.task_create(&rig.cwd, "Second", "x").unwrap();
    let at_path = Database::open(&rig.cwd.join(".swarm/state/floor.db")).unwrap();
    let titles: Vec<String> = at_path
        .list_tasks(None, None)
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(
        titles,
        vec!["First", "Second"],
        "the new task is at the path"
    );
    assert_eq!(task.title, "Second");
    let moved_db = Database::open(&moved.join(".swarm/state/floor.db")).unwrap();
    assert_eq!(
        moved_db.list_tasks(None, None).unwrap().len(),
        1,
        "and not in the moved folder"
    );
}

#[test]
fn a_floor_moved_away_and_back_works_again_at_once() {
    let rig = Rig::new();
    rig.create_floor();
    let moved = move_to(&rig, "moved");
    assert!(rig.tower.task_create(&rig.cwd, "x", "y").is_err());
    fs::rename(&moved, &rig.cwd).unwrap();
    assert!(!rig.tower.floor_list().unwrap()[0].missing);
    let view = rig.tower.floor_open(&rig.cwd).unwrap();
    assert!(!view.gone);
    rig.tower.task_create(&rig.cwd, "Back", "x").unwrap();
    let db = Database::open(&rig.cwd.join(".swarm/state/floor.db")).unwrap();
    assert_eq!(db.list_tasks(None, None).unwrap().len(), 1);
}

// ---------------------------------------------------------------------------------------------
// D4: the watcher
// ---------------------------------------------------------------------------------------------

fn floor_events(rig: &Rig) -> usize {
    rig.events
        .all()
        .iter()
        .filter(|ev| matches!(ev, Ev::Floor(path) if *path == rig.cwd))
        .count()
}

fn notices(rig: &Rig, kind: &str) -> Vec<String> {
    lock(&rig.events.notices)
        .iter()
        .filter(|(_, k, _)| k == kind)
        .map(|(_, _, text)| text.clone())
        .collect()
}

#[test]
fn the_watcher_tells_one_time_when_a_floor_is_gone_and_one_time_when_it_is_back() {
    let rig = Rig::with(|config| config.floor_watch = Some(SHORT_WATCH));
    rig.create_floor();
    rig.start("ceo");
    rig.live_idle("ceo");
    let before = floor_events(&rig);

    let moved = move_to(&rig, "moved");
    wait_for("the notice floor_gone", || {
        !notices(&rig, "floor_gone").is_empty()
    });
    // Many ticks pass. The watcher says nothing more.
    std::thread::sleep(MANY_TICKS);
    assert_eq!(
        notices(&rig, "floor_gone"),
        vec![format!(
            "The folder of the floor Test Floor is gone: {}. 1 session of it still runs.",
            rig.cwd.display()
        )]
    );
    assert_eq!(floor_events(&rig), before + 1, "one floor_changed");
    assert!(notices(&rig, "floor_back").is_empty());
    // The session still runs: the watcher does not kill it.
    let view = rig.tower.floor_open(&rig.cwd).unwrap();
    assert!(view.gone);
    assert_ne!(view.sessions[0].status, tower_core::status::Status::Stopped);

    // The folder comes back at the same path.
    fs::rename(&moved, &rig.cwd).unwrap();
    wait_for("the notice floor_back", || {
        !notices(&rig, "floor_back").is_empty()
    });
    std::thread::sleep(MANY_TICKS);
    assert_eq!(
        notices(&rig, "floor_back"),
        vec![format!(
            "The folder of the floor Test Floor is back: {}.",
            rig.cwd.display()
        )]
    );
    assert_eq!(notices(&rig, "floor_gone").len(), 1);
    assert_eq!(floor_events(&rig), before + 2, "one more floor_changed");
    assert!(!rig.tower.floor_open(&rig.cwd).unwrap().gone);

    // A task create works and lands in the database at the path.
    rig.tower.task_create(&rig.cwd, "After", "x").unwrap();
    let db = Database::open(&rig.cwd.join(".swarm/state/floor.db")).unwrap();
    assert_eq!(db.list_tasks(None, None).unwrap().len(), 1);
}

#[test]
fn the_notice_names_the_sessions_only_when_some_run() {
    let rig = Rig::with(|config| config.floor_watch = Some(SHORT_WATCH));
    rig.create_floor();
    move_to(&rig, "moved");
    wait_for("the notice floor_gone", || {
        !notices(&rig, "floor_gone").is_empty()
    });
    assert_eq!(
        notices(&rig, "floor_gone"),
        vec![format!(
            "The folder of the floor Test Floor is gone: {}.",
            rig.cwd.display()
        )],
        "no live session: no sentence about sessions"
    );
}

#[test]
fn the_watcher_counts_each_live_session() {
    let rig = Rig::with(|config| config.floor_watch = None);
    rig.create_floor();
    rig.start("ceo");
    rig.start("pm");
    rig.live_idle("ceo");
    rig.live_idle("pm");
    move_to(&rig, "moved");
    rig.tower.check_floors();
    assert_eq!(
        notices(&rig, "floor_gone"),
        vec![format!(
            "The folder of the floor Test Floor is gone: {}. 2 sessions of it still run.",
            rig.cwd.display()
        )]
    );
    // A second check by hand says nothing new.
    rig.tower.check_floors();
    assert_eq!(notices(&rig, "floor_gone").len(), 1);
}

#[test]
fn a_back_floor_that_cannot_be_read_stays_gone_and_says_nothing_until_it_works() {
    let rig = Rig::with(|config| config.floor_watch = None);
    rig.create_floor();
    let moved = move_to(&rig, "moved");
    rig.tower.check_floors();
    assert_eq!(notices(&rig, "floor_gone").len(), 1);
    // The folder is back, but `floor.toml` is broken. The database cannot be opened again by
    // the app because `floor.toml` does not load.
    fs::rename(&moved, &rig.cwd).unwrap();
    let good = fs::read(rig.cwd.join(".swarm/floor.toml")).unwrap();
    fs::write(rig.cwd.join(".swarm/floor.toml"), "version = [").unwrap();
    rig.tower.check_floors();
    rig.tower.check_floors();
    assert!(notices(&rig, "floor_back").is_empty());
    assert!(rig.tower.task_create(&rig.cwd, "x", "y").is_err());
    // The file is repaired: the next check opens the floor again, and says so one time.
    fs::write(rig.cwd.join(".swarm/floor.toml"), good).unwrap();
    rig.tower.check_floors();
    rig.tower.check_floors();
    assert_eq!(notices(&rig, "floor_back").len(), 1);
    rig.tower.task_create(&rig.cwd, "x", "y").unwrap();
}

#[test]
fn the_watcher_ends_at_shutdown_and_a_tower_with_no_watcher_sends_nothing() {
    let rig = Rig::with(|config| config.floor_watch = Some(SHORT_WATCH));
    rig.create_floor();
    let started = std::time::Instant::now();
    rig.tower.shutdown();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "shutdown joins the watcher quickly"
    );
    let before = floor_events(&rig);
    move_to(&rig, "moved");
    std::thread::sleep(MANY_TICKS);
    assert!(notices(&rig, "floor_gone").is_empty());
    assert_eq!(floor_events(&rig), before);

    // The turned-off watcher (`None`, the default of the test rig) sends nothing either.
    let quiet = Rig::new();
    quiet.create_floor();
    let before = floor_events(&quiet);
    move_to(&quiet, "moved");
    std::thread::sleep(MANY_TICKS);
    assert!(notices(&quiet, "floor_gone").is_empty());
    assert_eq!(floor_events(&quiet), before);
}

#[test]
fn the_default_config_has_a_watcher_of_three_seconds() {
    let config = tower_core::app::TowerConfig::new(std::env::temp_dir().join("unused-config"));
    assert_eq!(config.floor_watch, Some(Duration::from_secs(3)));
    assert_eq!(tower_core::app::FLOOR_WATCH_PAUSE, Duration::from_secs(3));
}
