//! The Codex sandbox switch (decision O7, option A): the probe, the setting `[codex] sandbox`,
//! the start of a session, and the instruction texts. `bwrap` is a fake script in a folder of
//! the test, so no test depends on the kernel setting of the machine. The sessions are the real
//! `fake-cli`.

mod app_support;

use app_support::{Rig, Shared, wait_for};
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tower_core::app::{SandboxState, Tower, TowerError};
use tower_core::model::{CliKind, PermissionPolicy};

const UID_ERROR: &str = "bwrap: setting up uid map: Permission denied";

/// A rig whose session PATH finds the `bwrap` of the folder `tools` first. The test sets `node`
/// to a file in `tools`, so the folder is in the PATH before `/usr/bin` (the real `bwrap`).
fn rig_with_bwrap(tools: &Path, body: &str) -> Rig {
    let rig = Rig::new();
    write_exe(&tools.join("node"), "#!/bin/sh\nexit 0\n");
    write_exe(&tools.join("bwrap"), &format!("#!/bin/sh\n{body}\n"));
    rig.tower
        .programs_set("node", tools.join("node").to_str().unwrap())
        .unwrap();
    rig
}

fn write_exe(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn works() -> String {
    "exit 0".to_owned()
}

fn fails() -> String {
    format!("echo '{UID_ERROR}' >&2; exit 1")
}

fn config_text(rig: &Rig) -> String {
    fs::read_to_string(rig.config.join("programs.toml")).unwrap_or_default()
}

fn last_call(rig: &Rig, slug: &str) -> app_support::Call {
    rig.calls()
        .into_iter()
        .rev()
        .find(|c| c.slug == slug)
        .unwrap()
}

// ---------------------------------------------------------------------------------------------
// The probe
// ---------------------------------------------------------------------------------------------

#[test]
fn the_probe_says_works_fails_with_the_first_error_line_or_times_out() {
    let tools = TempDir::new().unwrap();
    let rig = rig_with_bwrap(tools.path(), &works());
    let probe = rig.tower.codex_sandbox_probe().unwrap();
    assert_eq!(probe.state, SandboxState::Works, "{probe:?}");

    write_exe(
        &tools.path().join("bwrap"),
        &format!("#!/bin/sh\n{}\n", fails()),
    );
    let probe = rig.tower.codex_sandbox_probe().unwrap();
    assert_eq!(probe.state, SandboxState::Fails);
    assert_eq!(probe.detail, UID_ERROR);

    write_exe(&tools.path().join("bwrap"), "#!/bin/sh\nsleep 30\n");
    let start = std::time::Instant::now();
    let probe = rig
        .tower
        .codex_sandbox_probe_within(Duration::from_millis(300))
        .unwrap();
    assert_eq!(probe.state, SandboxState::Fails);
    assert!(probe.detail.contains("did not end"), "{probe:?}");
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn the_probe_gets_the_arguments_and_the_environment_of_a_session() {
    let tools = TempDir::new().unwrap();
    let log = tools.path().join("probe.log");
    let rig = rig_with_bwrap(
        tools.path(),
        &format!("echo \"$@|$LANG\" > {}", log.display()),
    );
    assert_eq!(
        rig.tower.codex_sandbox_probe().unwrap().state,
        SandboxState::Works
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap().trim(),
        "--unshare-user --ro-bind / / true|C.UTF-8"
    );
}

// ---------------------------------------------------------------------------------------------
// The setting
// ---------------------------------------------------------------------------------------------

#[test]
fn the_switch_is_off_by_default_and_a_failing_probe_refuses_it() {
    let tools = TempDir::new().unwrap();
    let rig = rig_with_bwrap(tools.path(), &fails());
    assert!(!rig.tower.settings_get().codex_sandbox);
    assert!(!config_text(&rig).contains("[codex]"));

    let error = rig.tower.settings_set_codex(Some(true)).unwrap_err();
    let text = error.to_string();
    assert!(
        text.contains("does not work on this machine") && text.contains(UID_ERROR),
        "{text}"
    );
    assert!(!rig.tower.settings_get().codex_sandbox);
    assert!(
        !config_text(&rig).contains("sandbox"),
        "{}",
        config_text(&rig)
    );

    // `None` changes nothing, and turning it off needs no probe.
    assert!(!rig.tower.settings_set_codex(None).unwrap().codex_sandbox);
    assert!(
        !rig.tower
            .settings_set_codex(Some(false))
            .unwrap()
            .codex_sandbox
    );
}

#[test]
fn the_switch_is_stored_in_the_codex_table_and_the_other_tables_stay() {
    let tools = TempDir::new().unwrap();
    let rig = rig_with_bwrap(tools.path(), &works());
    rig.tower
        .settings_set(Some("sk-test".into()), Some("model-y".into()))
        .unwrap();
    rig.tower.settings_set_interface(Some(false)).unwrap();
    rig.create_floor();
    rig.tower
        .floor_permissions_set(&rig.cwd, vec!["Bash(ls:*)".to_owned()])
        .unwrap();

    let on = rig.tower.settings_set_codex(Some(true)).unwrap();
    assert!(on.codex_sandbox);
    assert!(rig.tower.settings_get().codex_sandbox);
    let table: toml::Table = toml::from_str(&config_text(&rig)).unwrap();
    assert_eq!(table["codex"]["sandbox"].as_bool(), Some(true));
    // The other tables and values stay.
    assert_eq!(table["openai"]["key"].as_str(), Some("sk-test"));
    assert_eq!(table["openai"]["image_model"].as_str(), Some("model-y"));
    assert_eq!(
        table["interface"]["notify_on_question"].as_bool(),
        Some(false)
    );
    assert!(table.contains_key("programs"));
    assert!(table.contains_key("floor_permissions"));
    // The other settings do not change the switch.
    let same = rig
        .tower
        .settings_set(None, Some("model-z".into()))
        .unwrap();
    assert!(same.codex_sandbox);
    assert!(
        rig.tower
            .settings_set_interface(Some(false))
            .unwrap()
            .codex_sandbox
    );
    assert!(rig.tower.settings_get().codex_sandbox);

    // Off again: the default, so the key and its empty table go. A second key of `[codex]` stays.
    let off = rig.tower.settings_set_codex(Some(false)).unwrap();
    assert!(!off.codex_sandbox);
    assert!(
        !config_text(&rig).contains("[codex]"),
        "{}",
        config_text(&rig)
    );
    let mut text = config_text(&rig);
    text.push_str("\n[codex]\nother = 5\n");
    fs::write(rig.config.join("programs.toml"), text).unwrap();
    rig.tower.settings_set_codex(Some(true)).unwrap();
    rig.tower.settings_set_codex(Some(false)).unwrap();
    let table: toml::Table = toml::from_str(&config_text(&rig)).unwrap();
    assert_eq!(table["codex"]["other"].as_integer(), Some(5));
    assert!(table["codex"].get("sandbox").is_none());
}

#[test]
fn a_value_in_the_file_that_is_not_a_boolean_gives_the_default() {
    let rig = Rig::new();
    fs::write(
        rig.config.join("programs.toml"),
        "[codex]\nsandbox = \"yes\"\n",
    )
    .unwrap();
    assert!(!rig.tower.settings_get().codex_sandbox);
}

// ---------------------------------------------------------------------------------------------
// The start of a session
// ---------------------------------------------------------------------------------------------

/// A git floor with three employees: a Codex Project Manager (`read-only`), a Codex engineer
/// (`edit-in-worktree`) and a Claude engineer.
fn git_floor(rig: &Rig) {
    rig.create_floor();
    app_support::init_repo(&rig.cwd);
    rig.make_codex("pm");
    rig.add_editor("eng", Some("pm"));
    rig.add_editor("cl", Some("pm"));
    let mut view = rig.tower.floor_open(&rig.cwd).unwrap();
    let mut eng = view.employees.drain(..).find(|e| e.slug == "eng").unwrap();
    eng.cli = CliKind::Codex;
    rig.tower.employee_save(&rig.cwd, eng).unwrap();
}

#[test]
fn a_session_start_uses_the_switch_for_codex_only() {
    let tools = TempDir::new().unwrap();
    let rig = rig_with_bwrap(tools.path(), &works());
    git_floor(&rig);

    // Off (the default): nothing is enabled, and no folder is made for the sandbox.
    rig.start("eng");
    assert!(!last_call(&rig, "eng").codex_sandbox);
    assert!(!rig.cwd.join(".swarm/worktrees").exists());

    rig.tower.settings_set_codex(Some(true)).unwrap();
    // A running session keeps what it got. The next start reads the switch.
    assert!(!last_call(&rig, "eng").codex_sandbox);
    for slug in ["pm", "cl"] {
        rig.start(slug);
    }
    let pm = last_call(&rig, "pm");
    assert!(pm.codex_sandbox);
    assert_eq!(pm.permission, PermissionPolicy::ReadOnly);
    // The adapter learns that the floor has worktrees: a Codex employee needs it for the profile.
    assert!(pm.worktrees);
    // A Claude employee never gets the Codex switch.
    assert!(!last_call(&rig, "cl").codex_sandbox);
    // The folders that the sandbox names exist before the CLI starts.
    assert!(rig.cwd.join(".swarm/worktrees").is_dir());
    assert!(rig.cwd.join(".swarm/reports").is_dir());

    // The setup session never gets the sandbox, also with a Codex CLI.
    rig.tower
        .setup_start(&rig.cwd, CliKind::Codex, 24, 80)
        .unwrap();
    let setup = last_call(&rig, "setup");
    assert!(!setup.codex_sandbox && !setup.worktrees);
}

#[test]
fn a_floor_that_is_not_a_repository_gets_the_flag_with_no_worktrees() {
    let tools = TempDir::new().unwrap();
    let rig = rig_with_bwrap(tools.path(), &works());
    rig.create_floor();
    rig.make_codex("pm");
    rig.tower.settings_set_codex(Some(true)).unwrap();
    rig.start("pm");
    let pm = last_call(&rig, "pm");
    assert!(pm.codex_sandbox && !pm.worktrees);
}

// ---------------------------------------------------------------------------------------------
// A stored "on" with a sandbox that does not work (defect D1)
// ---------------------------------------------------------------------------------------------

/// Write `[codex] sandbox = true` in the app config, as if the operator had turned it on in an
/// earlier run of the app. No probe runs.
fn store_on(config: &Path) {
    let file = config.join("programs.toml");
    let mut text = fs::read_to_string(&file).unwrap_or_default();
    text.push_str("\n[codex]\nsandbox = true\n");
    fs::write(file, text).unwrap();
}

/// A rig with the sandbox stored as on, and a floor with the Codex employees `pm`, `qa` and
/// `eng`, and the Claude employees `ceo` and `cl`.
fn stored_on_rig(tools: &Path, body: &str, window: Option<Duration>) -> Rig {
    let rig = Rig::with(|cfg| {
        if let Some(window) = window {
            cfg.sandbox_window = window;
        }
    });
    write_exe(&tools.join("node"), "#!/bin/sh\nexit 0\n");
    write_exe(&tools.join("bwrap"), &format!("#!/bin/sh\n{body}\n"));
    rig.tower
        .programs_set("node", tools.join("node").to_str().unwrap())
        .unwrap();
    rig.create_floor();
    rig.add_employee("eng", Some("pm"));
    rig.add_employee("cl", Some("pm"));
    rig.add_employee("qa", Some("pm"));
    for slug in ["pm", "qa", "eng"] {
        rig.make_codex(slug);
    }
    store_on(&rig.config);
    rig
}

fn set_bwrap(tools: &Path, body: &str) {
    write_exe(&tools.join("bwrap"), &format!("#!/bin/sh\n{body}\n"));
}

/// The number of times that the fake `bwrap` ran (it writes one line in `runs.log`).
fn runs(tools: &Path) -> usize {
    fs::read_to_string(tools.join("runs.log")).map_or(0, |text| text.lines().count())
}

fn counting(tools: &Path, tail: &str) -> String {
    format!("echo run >> {}; {tail}", tools.join("runs.log").display())
}

fn sandbox_notices(rig: &Rig) -> Vec<String> {
    rig.events
        .notices
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, kind, _)| kind == "sandbox_down")
        .map(|(_, _, text)| text.clone())
        .collect()
}

fn name_of(rig: &Rig, slug: &str) -> String {
    rig.tower
        .floor_open(&rig.cwd)
        .unwrap()
        .employees
        .into_iter()
        .find(|e| e.slug == slug)
        .unwrap()
        .name
}

#[test]
fn a_stored_on_with_a_failing_sandbox_refuses_a_codex_start_and_changes_nothing() {
    let tools = TempDir::new().unwrap();
    let rig = stored_on_rig(tools.path(), &fails(), None);
    assert!(
        rig.tower.settings_get().codex_sandbox,
        "the stored value is on"
    );
    let config_before = config_text(&rig);
    let name = name_of(&rig, "pm");

    // The start by the operator.
    let error = rig.try_start("pm").unwrap_err();
    assert!(matches!(error, TowerError::SandboxDown { .. }), "{error:?}");
    assert_eq!(
        error.to_string(),
        format!(
            "SANDBOX_DOWN: The Codex sandbox does not work on this machine now ({UID_ERROR}). The start of {name} is refused, because the setting \"Use the Codex sandbox\" is on. Repair the sandbox, or turn the setting off in Settings > Codex sandbox to start Codex with no sandbox."
        )
    );
    // The resume by the operator.
    let error = rig
        .tower
        .session_resume(&rig.cwd, "pm", None, 24, 80)
        .unwrap_err();
    assert!(error.to_string().starts_with("SANDBOX_DOWN:"), "{error}");
    // Nothing started, and no silent fall back to full access.
    assert!(rig.calls().is_empty(), "{:?}", rig.calls());
    assert!(rig.tower.floor_open(&rig.cwd).unwrap().sessions.is_empty());
    // The failing probe did not change the stored value. The app config is the same text.
    assert!(rig.tower.settings_get().codex_sandbox);
    assert_eq!(config_text(&rig), config_before);
    // The operator can turn it off, and then the start has no sandbox and works.
    assert!(
        !rig.tower
            .settings_set_codex(Some(false))
            .unwrap()
            .codex_sandbox
    );
    rig.start("pm");
    assert!(!last_call(&rig, "pm").codex_sandbox);
}

#[test]
fn a_claude_employee_and_the_setup_session_start_with_a_failing_sandbox() {
    let tools = TempDir::new().unwrap();
    let rig = stored_on_rig(tools.path(), &fails(), None);
    rig.start("cl");
    assert!(!last_call(&rig, "cl").codex_sandbox);
    // The setup session with a Codex CLI is not a Codex employee.
    rig.tower
        .setup_start(&rig.cwd, CliKind::Codex, 24, 80)
        .unwrap();
    assert!(!last_call(&rig, "setup").codex_sandbox);
    assert!(sandbox_notices(&rig).is_empty());
}

#[test]
fn a_tool_start_is_refused_with_one_line_and_one_notice_for_each_window() {
    let tools = TempDir::new().unwrap();
    let rig = stored_on_rig(tools.path(), &fails(), None);
    rig.start("ceo");
    rig.live_idle("ceo");

    let first = rig.tool(
        "ceo",
        "employee_start",
        json!({"employee": "pm", "message": "Hello."}),
    );
    assert!(
        first.text.starts_with("SANDBOX_DOWN:")
            && first.text.contains(UID_ERROR)
            && first.text.contains("Do not send it again."),
        "{}",
        first.text
    );
    assert!(!first.text.contains('\n'), "one line");
    // Two more refused starts inside the window: the tool answer says it each time, the UI one time.
    let second = rig.tool(
        "ceo",
        "employee_start",
        json!({"employee": "eng", "message": "Hello."}),
    );
    assert!(second.text.starts_with("SANDBOX_DOWN:"), "{}", second.text);
    let assign = rig.tool("ceo", "send", json!({"to": "qa", "text": "Hello again."}));
    assert!(assign.text.starts_with("SANDBOX_DOWN:"), "{}", assign.text);

    // Nothing started.
    let started: Vec<String> = rig.calls().into_iter().map(|c| c.slug).collect();
    assert_eq!(started, ["ceo"]);
    let notices = sandbox_notices(&rig);
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].starts_with("SANDBOX_DOWN:"), "{notices:?}");
    assert!(notices[0].contains(UID_ERROR), "{notices:?}");
}

#[test]
fn the_notice_comes_again_after_the_window_and_a_repair_lets_the_start_pass() {
    let tools = TempDir::new().unwrap();
    let rig = stored_on_rig(tools.path(), &fails(), Some(Duration::from_millis(400)));
    assert!(rig.try_start("pm").is_err());
    // The operator code path does not send a notice (its error is the answer). A tool start does.
    rig.start("ceo");
    rig.live_idle("ceo");
    rig.tool(
        "ceo",
        "employee_start",
        json!({"employee": "pm", "message": "a"}),
    );
    assert_eq!(sandbox_notices(&rig).len(), 1);
    std::thread::sleep(Duration::from_millis(500));
    rig.tool(
        "ceo",
        "employee_start",
        json!({"employee": "pm", "message": "b"}),
    );
    assert_eq!(sandbox_notices(&rig).len(), 2);

    // A repair: the next start passes, with the profile.
    set_bwrap(tools.path(), &works());
    rig.start("pm");
    assert!(last_call(&rig, "pm").codex_sandbox);
}

#[test]
fn the_team_resume_refuses_each_codex_start_and_sends_one_notice() {
    let tools = TempDir::new().unwrap();
    let rig = Rig::new();
    write_exe(&tools.path().join("node"), "#!/bin/sh\nexit 0\n");
    set_bwrap(tools.path(), &works());
    rig.tower
        .programs_set("node", tools.path().join("node").to_str().unwrap())
        .unwrap();
    rig.create_floor();
    rig.add_employee("eng", Some("pm"));
    for slug in ["pm", "eng"] {
        rig.make_codex(slug);
    }
    // The first run of the app: the switch is off, the team is live, the app closes.
    for slug in ["ceo", "pm", "eng"] {
        rig.start(slug);
        rig.live_idle(slug);
    }
    rig.tower.shutdown();

    // The second run: the switch is on (stored) and `bwrap` fails.
    store_on(&rig.config);
    set_bwrap(tools.path(), &fails());
    let shared = Arc::new(Shared::default());
    let socket = rig.root.path().join("second.sock");
    let second: Tower = Rig::make_tower(&rig.config, &socket, &shared, |cfg| {
        cfg.resume_pause = Duration::from_millis(20);
    });
    second
        .programs_set("node", tools.path().join("node").to_str().unwrap())
        .unwrap();
    let events = Arc::new(app_support::Events::default());
    second.set_observer(events.clone());
    second.team_resume(&rig.cwd).unwrap();
    wait_for("the resume ends", || {
        second
            .floor_open(&rig.cwd)
            .unwrap()
            .resume_progress
            .is_none()
    });
    // The Claude employee `ceo` resumed. The two Codex employees were refused.
    let slugs: Vec<String> = shared
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.slug.clone())
        .collect();
    assert_eq!(slugs, ["ceo"]);
    let notices = events.notices.lock().unwrap().clone();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].1, "sandbox_down");
    assert!(notices[0].2.starts_with("SANDBOX_DOWN:"), "{notices:?}");
    // The refused employees stay in the list for a new try.
    let mut left = second.floor_open(&rig.cwd).unwrap().resumable;
    left.sort();
    assert_eq!(left, ["eng", "pm"]);
}

#[test]
fn a_works_result_is_reused_for_the_window_and_a_failure_is_probed_again() {
    let tools = TempDir::new().unwrap();
    let rig = stored_on_rig(tools.path(), &counting(tools.path(), "exit 0"), None);
    // Three Codex starts, one probe.
    for slug in ["pm", "qa", "eng"] {
        rig.start(slug);
        assert!(last_call(&rig, slug).codex_sandbox);
    }
    assert_eq!(runs(tools.path()), 1);
    // The Claude start and the setup start run no probe.
    rig.start("cl");
    assert_eq!(runs(tools.path()), 1);

    // `bwrap` breaks now. The `works` result still counts inside the window.
    set_bwrap(tools.path(), &counting(tools.path(), &fails()));
    rig.tower.session_stop(&rig.live_idle("pm")).unwrap();
    let limit = std::time::Instant::now() + Duration::from_secs(10);
    while rig.try_start("pm").is_err() {
        assert!(std::time::Instant::now() < limit, "pm does not restart");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(runs(tools.path()), 1);

    // The button "Test again" runs the probe. A failure clears the result that was kept.
    assert_eq!(
        rig.tower.codex_sandbox_probe().unwrap().state,
        SandboxState::Fails
    );
    assert_eq!(runs(tools.path()), 2);
    rig.tower.session_stop(&rig.live_idle("qa")).unwrap();
    let mut refused = 0;
    let limit = std::time::Instant::now() + Duration::from_secs(10);
    // The old process needs a short time to end. A start before that gives another error.
    while refused < 2 {
        assert!(std::time::Instant::now() < limit, "no refusal");
        match rig.try_start("qa") {
            Err(TowerError::SandboxDown { .. }) => refused += 1,
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
            Ok(_) => panic!("a start passed with a failing sandbox"),
        }
    }
    // A failure is probed again at each start: 2 more runs for the 2 refusals.
    assert_eq!(runs(tools.path()), 4);
    // A repair works at the next start: the failure was not kept.
    set_bwrap(tools.path(), &counting(tools.path(), "exit 0"));
    rig.start("qa");
    assert_eq!(runs(tools.path()), 5);
}

#[test]
fn a_works_result_that_is_older_than_the_window_is_probed_again() {
    let tools = TempDir::new().unwrap();
    let rig = stored_on_rig(
        tools.path(),
        &counting(tools.path(), "exit 0"),
        Some(Duration::from_millis(300)),
    );
    rig.start("pm");
    assert_eq!(runs(tools.path()), 1);
    std::thread::sleep(Duration::from_millis(400));
    rig.start("qa");
    assert_eq!(runs(tools.path()), 2);
}

// ---------------------------------------------------------------------------------------------
// The instruction texts
// ---------------------------------------------------------------------------------------------

/// Stop the live session `id` of `slug`, and start the employee again. The old process needs a
/// short time to end, so the start is tried again until it is accepted.
fn restart(rig: &Rig, slug: &str, id: &str) {
    rig.tower.session_stop(id).unwrap();
    let limit = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match rig.try_start(slug) {
            Ok(_) => return,
            Err(error) if std::time::Instant::now() < limit => {
                let _ = error;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => panic!("cannot start {slug} again: {error}"),
        }
    }
}

fn instructions(rig: &Rig, slug: &str) -> String {
    last_call(rig, slug).instructions.unwrap()
}

#[test]
fn the_codex_instructions_follow_the_switch_the_policy_and_the_repository() {
    let tools = TempDir::new().unwrap();
    let rig = rig_with_bwrap(tools.path(), &works());
    git_floor(&rig);
    let reports = rig.cwd.join(".swarm/reports");
    let worktrees = rig.cwd.join(".swarm/worktrees");

    // Off: only the text limits the employee.
    let pm_id = rig.start("pm").session_id;
    let eng_id = rig.start("eng").session_id;
    let pm_off = instructions(&rig, "pm");
    assert!(pm_off.contains("## Your limits"), "{pm_off}");
    assert!(
        pm_off.contains("does not enforce your permission policy")
            && pm_off.contains("only this text limits you"),
        "{pm_off}"
    );
    assert!(!pm_off.contains("enforces your permission policy with a sandbox"));
    assert!(pm_off.contains("Your policy is read-only. Do not edit or create a file"));
    assert!(pm_off.contains(&reports.display().to_string()));
    let eng_off = instructions(&rig, "eng");
    assert!(eng_off.contains("Edit files only inside the worktree of your task"));
    assert!(eng_off.contains("Never change a file of the main checkout"));

    // On: the CLI enforces the policy and asks the operator.
    rig.tower.settings_set_codex(Some(true)).unwrap();
    restart(&rig, "pm", &pm_id);
    restart(&rig, "eng", &eng_id);
    let pm_on = instructions(&rig, "pm");
    assert!(
        pm_on.contains("enforces your permission policy with a sandbox")
            && pm_on.contains("asks the operator to approve it"),
        "{pm_on}"
    );
    assert!(!pm_on.contains("does not enforce your permission policy"));
    assert!(pm_on.contains("You can write only in the reports folder"));
    assert!(pm_on.contains(&reports.display().to_string()));
    let eng_on = instructions(&rig, "eng");
    assert!(
        eng_on.contains("The main checkout is read-only"),
        "{eng_on}"
    );
    assert!(eng_on.contains(&worktrees.display().to_string()));
    assert!(eng_on.contains("Your commands have no network access"));
    // The rest of the instructions is the same text in both cases.
    assert!(
        eng_on.contains("## Your place in the team")
            && eng_off.contains("## Your place in the team")
    );

    // A Claude employee gets neither text.
    rig.start("cl");
    assert!(!instructions(&rig, "cl").contains("## Your limits"));
}

#[test]
fn the_codex_instructions_for_a_floor_with_no_repository_say_so() {
    let tools = TempDir::new().unwrap();
    let rig = rig_with_bwrap(tools.path(), &works());
    rig.create_floor();
    rig.make_codex("pm");
    let mut view = rig.tower.floor_open(&rig.cwd).unwrap();
    let mut pm = view.employees.drain(..).find(|e| e.slug == "pm").unwrap();
    pm.permission = PermissionPolicy::EditInWorktree;
    rig.tower.employee_save(&rig.cwd, pm).unwrap();

    let pm_id = rig.start("pm").session_id;
    let off = instructions(&rig, "pm");
    assert!(off.contains("This floor is not a git repository"), "{off}");
    assert!(off.contains("only this text limits you"));
    rig.tower.settings_set_codex(Some(true)).unwrap();
    restart(&rig, "pm", &pm_id);
    let on = instructions(&rig, "pm");
    assert!(on.contains("This floor is not a git repository"), "{on}");
    assert!(on.contains("enforces your permission policy with a sandbox"));
}

// ---------------------------------------------------------------------------------------------
// The setup text (a static file: it must be true for both cases)
// ---------------------------------------------------------------------------------------------

#[test]
fn the_setup_text_for_codex_is_true_with_the_switch_on_and_off() {
    let text = tower_core::app::FLOOR_INIT;
    assert!(
        text.contains(
            "If the operator turned the Codex sandbox on in Settings, the CLI enforces the policy."
        ),
        "the text must say what the switch on gives"
    );
    assert!(
        text.contains("If the switch is off, the CLI enforces no policy: only the role text limits the employee."),
        "the text must say what the switch off gives"
    );
    // The old sentence, true for the switch off only, is gone.
    assert!(!text.contains("does not enforce `read-only` or"));
    assert!(text.contains("`ask-always` does not work with `codex`"));
}
