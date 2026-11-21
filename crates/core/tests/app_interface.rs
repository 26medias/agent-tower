//! Interface features of 2026-11-21: the program test and reset (Settings, "Programs"), the
//! switch for the desktop notification (`[interface]` of the app config) and the event
//! `question_asked` that the shell uses to show the notification.

mod app_support;

use app_support::Rig;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tower_core::app::{NOTIFICATION_TITLE, question_body};
use tower_core::env::EnvMap;
use tower_core::login_env::LoginEnvSource;
use tower_core::status::Status;

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn login_env(pairs: &[(&str, &str)]) -> LoginEnvSource {
    let map: EnvMap = pairs
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect();
    LoginEnvSource::Fixed(Some(map))
}

// ---------------------------------------------------------------------------------------------
// program_test, and the reset through programs_set
// ---------------------------------------------------------------------------------------------

#[test]
fn a_program_test_runs_version_and_gives_the_first_line() {
    let rig = Rig::new();
    let program = script(
        rig.root.path(),
        "claude-ok",
        "echo \"$1 2.1.330 (Claude Code)\"; echo more",
    );
    rig.tower
        .programs_set("claude", program.to_str().unwrap())
        .unwrap();
    let result = rig.tower.program_test("claude").unwrap();
    assert!(result.ok, "{result:?}");
    assert_eq!(result.output, "--version 2.1.330 (Claude Code)");
    assert_eq!(result.path, Some(program));
}

#[test]
fn a_program_test_uses_the_environment_of_a_session() {
    let rig = Rig::with(|config| {
        config.login_env = login_env(&[
            ("MARK", "from-the-login-shell"),
            ("PATH", "/usr/bin:/bin"),
            ("CLAUDECODE", "1"),
            ("CLAUDE_CODE_SSE_PORT", "9"),
        ]);
    });
    let program = script(
        rig.root.path(),
        "claude-env",
        "echo \"$MARK|${CLAUDECODE:-unset}|${CLAUDE_CODE_SSE_PORT:-unset}|$PATH\"",
    );
    rig.tower
        .programs_set("claude", program.to_str().unwrap())
        .unwrap();
    let result = rig.tower.program_test("claude").unwrap();
    assert!(result.ok, "{result:?}");
    let dir = program.parent().unwrap().display().to_string();
    // The login variable is there. The variables of Claude Code are not (`build_env`). The
    // folder of the program is first in the PATH, as in a session.
    assert!(
        result
            .output
            .starts_with(&format!("from-the-login-shell|unset|unset|{dir}:")),
        "{}",
        result.output
    );
}

#[test]
fn a_program_test_of_a_bad_program_gives_the_error() {
    let rig = Rig::new();
    // The rig set `claude` to `fake-cli`, which refuses the flag `--version`.
    let result = rig.tower.program_test("claude").unwrap();
    assert!(!result.ok, "{result:?}");
    assert!(
        result.output.starts_with("exit code 2:"),
        "{}",
        result.output
    );
    assert!(result.path.is_some());
}

#[test]
fn a_program_test_has_a_time_limit_and_kills_the_program() {
    let rig = Rig::new();
    let program = script(rig.root.path(), "claude-slow", "sleep 30");
    rig.tower
        .programs_set("claude", program.to_str().unwrap())
        .unwrap();
    let started = std::time::Instant::now();
    let result = rig
        .tower
        .program_test_within("claude", Duration::from_millis(300))
        .unwrap();
    assert!(!result.ok);
    assert_eq!(result.output, "it did not end in 1 s");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_program_that_is_not_found_gives_ok_false_with_no_path_and_a_bad_name_is_an_error() {
    let rig = Rig::new();
    rig.tower.programs_set("codex", "").unwrap();
    let result = rig.tower.program_test("codex").unwrap();
    assert!(!result.ok);
    assert_eq!(result.path, None);
    assert!(result.output.contains("codex"), "{}", result.output);
    assert!(result.output.chars().count() <= 200);
    let error = rig.tower.program_test("rm").unwrap_err().to_string();
    assert!(error.contains("unknown program"), "{error}");
    assert!(rig.tower.program_test("tower-hook").is_err());
}

#[test]
fn programs_set_with_an_empty_path_removes_the_override() {
    let rig = Rig::new();
    let program = script(rig.root.path(), "node-mine", "echo v1");
    let before = rig.tower.programs_get();
    assert!(
        before.overrides.iter().all(Option::is_some),
        "the rig sets all three"
    );
    let set = rig
        .tower
        .programs_set("node", program.to_str().unwrap())
        .unwrap();
    assert_eq!(set.node, Some(program.clone()));
    assert_eq!(set.overrides[2], Some(program));
    // The reset: an empty path. The override goes, and the app shows what it finds (here the
    // resolver of the rig has no folder and no shell, so it finds nothing).
    let reset = rig.tower.programs_set("node", "").unwrap();
    assert_eq!(reset.overrides[2], None);
    assert_eq!(reset.node, None);
    // The other overrides stay, and the file has no entry for `node`.
    assert!(reset.overrides[0].is_some() && reset.overrides[1].is_some());
    let text = fs::read_to_string(rig.config.join("programs.toml")).unwrap();
    assert!(!text.contains("node"), "{text}");
    assert!(text.contains("claude"), "{text}");
}

// ---------------------------------------------------------------------------------------------
// The switch in `[interface]`
// ---------------------------------------------------------------------------------------------

#[test]
fn the_notification_switch_is_on_by_default_and_is_stored_in_the_interface_table() {
    let rig = Rig::new();
    assert!(rig.tower.settings_get().notify_on_question);
    let file = rig.config.join("programs.toml");

    let off = rig.tower.settings_set_interface(Some(false)).unwrap();
    assert!(!off.notify_on_question);
    assert!(!rig.tower.settings_get().notify_on_question);
    let text = fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("[interface]") && text.contains("notify_on_question = false"),
        "{text}"
    );
    // The other tables stay.
    assert!(text.contains("[programs]"), "{text}");

    // `None` changes nothing.
    assert!(
        !rig.tower
            .settings_set_interface(None)
            .unwrap()
            .notify_on_question
    );
    // The other settings do not touch the switch, and the switch does not touch them.
    let with_key = rig
        .tower
        .settings_set(Some("sk-test".into()), Some("model-y".into()))
        .unwrap();
    assert!(!with_key.notify_on_question);
    assert_eq!(with_key.image_model, "model-y");

    // On again: the default, so the table goes out of the file.
    let on = rig.tower.settings_set_interface(Some(true)).unwrap();
    assert!(on.notify_on_question);
    let text = fs::read_to_string(&file).unwrap();
    assert!(!text.contains("interface"), "{text}");
    assert!(text.contains("[openai]"), "{text}");
}

#[test]
fn a_bad_value_in_the_file_gives_the_default() {
    let rig = Rig::new();
    let file = rig.config.join("programs.toml");
    let mut text = fs::read_to_string(&file).unwrap();
    text.push_str("\n[interface]\nnotify_on_question = \"no\"\n");
    fs::write(&file, text).unwrap();
    assert!(rig.tower.settings_get().notify_on_question);
}

// ---------------------------------------------------------------------------------------------
// The event question_asked
// ---------------------------------------------------------------------------------------------

fn questions(rig: &Rig) -> Vec<tower_core::app::QuestionAsked> {
    app_support::lock(&rig.events.questions)
        .iter()
        .map(|(_, event)| event.clone())
        .collect()
}

#[test]
fn a_change_to_question_sends_one_event_with_the_name_the_role_and_the_floor() {
    let rig = Rig::new();
    let view = rig.create_floor();
    rig.add_employee("eng", Some("ceo"));
    rig.set_script("eng", "perm-dialog");
    let eng = rig.start("eng").session_id;
    rig.events.wait_status(&eng, Status::Question);
    app_support::wait_for("the event", || !questions(&rig).is_empty());
    let events = questions(&rig);
    assert_eq!(events.len(), 1, "{events:?}");
    let event = &events[0];
    assert_eq!(event.session_id, eng);
    assert_eq!(event.slug, "eng");
    assert_eq!(event.name, "ENG");
    assert_eq!(event.role, "Role of eng");
    assert_eq!(event.floor_name, view.name);
    assert_eq!(event.new, Status::Question);
    assert_ne!(event.old, Status::Question);
    assert!(event.notifications_on, "the switch is on by default");
    assert_eq!(
        question_body(&event.name, &event.role, &event.floor_name),
        format!("ENG (Role of eng) asks a question in {}", view.name)
    );
    assert_eq!(NOTIFICATION_TITLE, "Agent Tower");

    // A change of the draft flag in `question` is not a new question.
    rig.tower.pty_write(&eng, b"x").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(questions(&rig).len(), 1);

    // The operator answers. The turn ends. No event for these statuses.
    rig.tower.pty_write(&eng, b"1").unwrap();
    rig.events.wait("idle again", |e| {
        e.statuses(&eng).last() == Some(&Status::Idle)
    });
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(questions(&rig).len(), 1);
}

#[test]
fn the_event_carries_the_state_of_the_switch() {
    let rig = Rig::new();
    rig.create_floor();
    rig.add_employee("eng", Some("ceo"));
    rig.set_script("eng", "perm-dialog");
    rig.tower.settings_set_interface(Some(false)).unwrap();
    let eng = rig.start("eng").session_id;
    rig.events.wait_status(&eng, Status::Question);
    app_support::wait_for("the event", || !questions(&rig).is_empty());
    assert!(!questions(&rig)[0].notifications_on);
}

#[test]
fn each_question_gets_its_own_event() {
    let rig = Rig::new();
    rig.create_floor();
    rig.add_employee("eng", Some("ceo"));
    rig.set_script("eng", "perm-dialog");
    let eng = rig.start("eng").session_id;
    rig.events.wait_status(&eng, Status::Question);
    app_support::wait_for("the first event", || questions(&rig).len() == 1);
    rig.tower.pty_write(&eng, b"1").unwrap();
    rig.events
        .wait("idle", |e| e.statuses(&eng).last() == Some(&Status::Idle));
    // A second prompt: a new question.
    rig.tower
        .pty_write(&eng, b"\x1b[200~go again\x1b[201~\r")
        .unwrap();
    rig.events.wait("question again", |e| {
        e.statuses(&eng)
            .iter()
            .filter(|s| **s == Status::Question)
            .count()
            == 2
    });
    app_support::wait_for("the second event", || questions(&rig).len() == 2);
}

#[test]
fn the_setup_session_names_the_setup_agent() {
    let rig = Rig::new();
    rig.create_floor();
    rig.set_script("setup", "perm-dialog");
    let setup = rig
        .tower
        .setup_start(&rig.cwd, tower_core::model::CliKind::Claude, 24, 80)
        .unwrap()
        .session_id;
    rig.events.wait_status(&setup, Status::Question);
    app_support::wait_for("the event", || !questions(&rig).is_empty());
    let events = questions(&rig);
    assert_eq!(events[0].name, "Setup agent");
    assert_eq!(events[0].role, "Setup");
    assert_eq!(events[0].slug, "setup");
}
