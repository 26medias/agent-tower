//! The settings, the logo generation and the setup session of the `Tower` (contract
//! `docs/ipc-visuals-init.md`). The sessions are the real `fake-cli`. The image API is a local
//! fake server. No test calls the real API and no test starts a real CLI.

mod app_support;
mod fake_api;

use app_support::{Ev, Rig, wait_for};
use fake_api::{FakeApi, png_with_rect};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;
use tower_core::app::{FLOOR_INIT, FloorView, KeySource, SETUP_FIRST_MESSAGE};
use tower_core::model::{CliKind, Employee, PermissionPolicy};
use tower_core::status::Status;

const KEY: &str = "sk-test-SECRET-key-0123";

/// A rig that has the key in the environment and the image API at `api`.
fn rig_with_api(api: &FakeApi, key_in_env: bool) -> Rig {
    let base = api.base.clone();
    Rig::with(move |config| {
        config.image_api_base = base;
        config.logo_timeout = Duration::from_secs(10);
        if key_in_env {
            config
                .parent_env
                .insert("OPENAI_API_KEY".into(), KEY.into());
        }
    })
}

fn view(rig: &Rig) -> FloorView {
    rig.tower.floor_open(&rig.cwd).unwrap()
}

fn wait_idle_logo(rig: &Rig) {
    wait_for("the logo generation to end", || !view(rig).logo_busy);
}

fn floor_events(rig: &Rig) -> usize {
    rig.events
        .all()
        .iter()
        .filter(|ev| matches!(ev, Ev::Floor(_)))
        .count()
}

fn logo_file(rig: &Rig) -> std::path::PathBuf {
    rig.cwd.join(".swarm/assets/logo.png")
}

fn read_toml(path: &Path) -> toml::Table {
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

// ---------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------

#[test]
fn settings_live_in_the_app_config_beside_the_programs_and_never_show_the_key() {
    let rig = Rig::new();
    let first = rig.tower.settings_get();
    assert!(!first.openai_key_set && first.openai_key_source.is_none());
    assert_eq!(first.image_model, "gpt-image-2.6-sunburst");

    // A key and a model. A null argument means no change.
    let set = rig
        .tower
        .settings_set(Some(format!("  {KEY} ")), Some("model-y".into()))
        .unwrap();
    assert!(set.openai_key_set);
    assert_eq!(set.openai_key_source, Some(KeySource::Config));
    assert_eq!(set.image_model, "model-y");
    assert!(!format!("{set:?}").contains(KEY), "the view holds the key");
    let same = rig.tower.settings_set(None, None).unwrap();
    assert_eq!(same, set);

    // The file keeps the program overrides, and a program change keeps the key.
    let file = rig.config.join("programs.toml");
    let table = read_toml(&file);
    assert_eq!(table["openai"]["key"].as_str(), Some(KEY));
    assert_eq!(table["openai"]["image_model"].as_str(), Some("model-y"));
    assert!(table["programs"]["claude"].is_str());
    rig.tower.programs_set("node", "/bin/true").unwrap();
    let table = read_toml(&file);
    assert_eq!(table["openai"]["key"].as_str(), Some(KEY));
    assert!(table["programs"]["node"].is_str());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );

    // An empty text removes the key and resets the model.
    let cleared = rig
        .tower
        .settings_set(Some(String::new()), Some(String::new()))
        .unwrap();
    assert!(!cleared.openai_key_set);
    assert_eq!(cleared.image_model, "gpt-image-2.6-sunburst");
    assert!(read_toml(&file).get("openai").is_none());
    assert!(read_toml(&file)["programs"]["claude"].is_str());

    // A key with a space is refused, and the text of the error has no key.
    let error = rig
        .tower
        .settings_set(Some("sk bad key".into()), None)
        .unwrap_err()
        .to_string();
    assert!(!error.contains("sk bad key"), "{error}");
}

#[test]
fn the_key_comes_from_the_app_config_first_and_then_from_the_environment() {
    let api = FakeApi::start();
    let rig = rig_with_api(&api, true);
    let from_env = rig.tower.settings_get();
    assert_eq!(from_env.openai_key_source, Some(KeySource::Env));
    assert!(from_env.openai_key_set);
    let from_config = rig
        .tower
        .settings_set(Some("sk-from-config".into()), None)
        .unwrap();
    assert_eq!(from_config.openai_key_source, Some(KeySource::Config));
    // The request uses the key of the config file.
    rig.create_floor();
    api.reply_picture(&png_with_rect(20, 20, 2, 2, 9, 9));
    rig.tower.logo_generate(&rig.cwd, None).unwrap();
    wait_idle_logo(&rig);
    let sent = api.requests();
    assert_eq!(
        sent[0].header("authorization"),
        Some("Bearer sk-from-config")
    );
}

// ---------------------------------------------------------------------------------------------
// Logo
// ---------------------------------------------------------------------------------------------

#[test]
fn logo_generate_with_no_key_says_no_key_and_still_saves_the_prompt() {
    let api = FakeApi::start();
    let rig = rig_with_api(&api, false);
    rig.create_floor();
    let error = rig
        .tower
        .logo_generate(&rig.cwd, Some("A bee.".into()))
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("NO_KEY:"), "{error}");
    assert!(api.requests().is_empty());
    let floor = view(&rig);
    assert_eq!(floor.logo_prompt.as_deref(), Some("A bee."));
    assert!(
        !floor.logo_busy,
        "a refused start must not leave the busy flag"
    );
    // The refusal is not a stuck state: with a key, the next call runs.
    rig.tower.settings_set(Some(KEY.into()), None).unwrap();
    api.reply_picture(&png_with_rect(20, 20, 2, 2, 9, 9));
    rig.tower.logo_generate(&rig.cwd, None).unwrap();
    wait_idle_logo(&rig);
    assert!(logo_file(&rig).is_file());
}

#[test]
fn logo_generate_runs_in_the_background_writes_the_file_and_tells_the_observer() {
    let api = FakeApi::start();
    let rig = rig_with_api(&api, true);
    rig.create_floor();
    assert_eq!(rig.tower.floor_logo(&rig.cwd).unwrap(), None);
    assert_eq!(rig.tower.floor_list().unwrap()[0].logo_version, None);
    // A floor.toml with a default, to see that the brand write keeps it.
    let floor_toml = rig.cwd.join(".swarm/floor.toml");
    std::fs::write(
        &floor_toml,
        "version = 1\nname = \"Test Floor\"\n[defaults]\nmodel = \"haiku\"\n",
    )
    .unwrap();

    api.hold();
    api.reply_picture(&png_with_rect(1100, 600, 50, 50, 1049, 549));
    rig.tower
        .logo_generate(&rig.cwd, Some("A golden bee.".into()))
        .unwrap();
    // It returned at once, and the generation is busy.
    wait_for("the request", || api.requests().len() == 1);
    let busy = view(&rig);
    assert!(busy.logo_busy);
    assert_eq!(busy.logo_prompt.as_deref(), Some("A golden bee."));
    assert!(floor_events(&rig) >= 1, "no floor-changed at the start");
    let error = rig
        .tower
        .logo_generate(&rig.cwd, Some("Another prompt.".into()))
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("BUSY:"), "{error}");
    assert_eq!(
        view(&rig).logo_prompt.as_deref(),
        Some("A golden bee."),
        "a refused call must not change the prompt"
    );
    let at_start = floor_events(&rig);

    api.release();
    wait_idle_logo(&rig);
    assert!(floor_events(&rig) > at_start, "no floor-changed at the end");
    let done = view(&rig);
    assert_eq!(done.logo_error, None);
    assert!(done.logo_version.is_some());
    assert_eq!(
        rig.tower.floor_list().unwrap()[0].logo_version,
        done.logo_version
    );
    // The file is the cleaned picture: 1000 x 500 scaled to 512 x 256. No temp file stays.
    let bytes = std::fs::read(logo_file(&rig)).unwrap();
    assert_eq!(rig.tower.floor_logo(&rig.cwd).unwrap(), Some(bytes.clone()));
    let picture = image::load_from_memory(&bytes).unwrap();
    assert_eq!((picture.width(), picture.height()), (512, 256));
    let names: Vec<_> = std::fs::read_dir(rig.cwd.join(".swarm/assets"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["logo.png"]);
    // The request had the prompt, and floor.toml has the brand and the old content.
    let body: Value = serde_json::from_str(&api.requests()[0].body).unwrap();
    assert!(
        body["prompt"]
            .as_str()
            .unwrap()
            .contains(": A golden bee. Centered")
    );
    let table = read_toml(&floor_toml);
    assert_eq!(
        table["brand"]["logo_prompt"].as_str(),
        Some("A golden bee.")
    );
    assert_eq!(table["name"].as_str(), Some("Test Floor"));
    assert_eq!(table["defaults"]["model"].as_str(), Some("haiku"));
}

#[test]
fn logo_generate_without_a_prompt_uses_the_floor_name_and_a_failure_shows_and_clears() {
    let api = FakeApi::start();
    let rig = rig_with_api(&api, true);
    rig.create_floor();
    api.reply(500, format!("{{\"error\":{{\"message\":\"boom {KEY}\"}}}}"));
    rig.tower.logo_generate(&rig.cwd, None).unwrap();
    wait_idle_logo(&rig);
    let body: Value = serde_json::from_str(&api.requests()[0].body).unwrap();
    assert!(
        body["prompt"]
            .as_str()
            .unwrap()
            .contains(": Test Floor. Centered")
    );
    let failed = view(&rig);
    let error = failed.logo_error.clone().expect("an error text");
    assert!(error.contains("500") && error.contains("boom"), "{error}");
    assert!(!error.contains(KEY), "{error}");
    assert!(!logo_file(&rig).exists());
    assert_eq!(rig.tower.floor_logo(&rig.cwd).unwrap(), None);
    // A new start clears the error.
    api.hold();
    api.reply_picture(&png_with_rect(20, 20, 2, 2, 9, 9));
    rig.tower.logo_generate(&rig.cwd, None).unwrap();
    wait_for("the request", || api.requests().len() == 2);
    assert_eq!(view(&rig).logo_error, None);
    api.release();
    wait_idle_logo(&rig);
    assert!(logo_file(&rig).is_file());
}

// ---------------------------------------------------------------------------------------------
// The reserved slug
// ---------------------------------------------------------------------------------------------

fn employee(slug: &str) -> Employee {
    Employee {
        name: "Eli".into(),
        slug: slug.into(),
        role: "Engineer".into(),
        manager: None,
        cli: CliKind::Claude,
        model: None,
        effort: None,
        permission: PermissionPolicy::ReadOnly,
        role_text: String::new(),
    }
}

#[test]
fn the_slug_setup_is_reserved() {
    let rig = Rig::new();
    rig.create_floor();
    let error = rig
        .tower
        .employee_save(&rig.cwd, employee("setup"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("reserved"), "{error}");
    assert!(!rig.cwd.join(".swarm/employees/setup.md").exists());
    // A file with that slug (written by hand) is an error in the view, not an employee.
    std::fs::write(
        rig.cwd.join(".swarm/employees/setup.md"),
        "+++\nname = \"S\"\nrole = \"R\"\ncli = \"claude\"\npermission = \"read-only\"\n+++\n",
    )
    .unwrap();
    let floor = view(&rig);
    assert!(floor.employees.iter().all(|e| e.slug != "setup"));
    assert!(
        floor.employee_errors.iter().any(|e| e.contains("reserved")),
        "{:?}",
        floor.employee_errors
    );
    // The employee commands do not start it either.
    for result in [
        rig.tower.session_start(&rig.cwd, "setup", None, 24, 80),
        rig.tower.session_resume(&rig.cwd, "setup", None, 24, 80),
    ] {
        assert!(result.unwrap_err().to_string().contains("setup_start"));
    }
}

// ---------------------------------------------------------------------------------------------
// The setup session
// ---------------------------------------------------------------------------------------------

#[test]
fn the_instructions_show_the_format_that_the_store_reads() {
    // Each employee example in `floor_init.md` is a file that the real reader accepts, and each
    // value of `cli` and `permission` that the text lists is one that the reader knows.
    let rig = Rig::new();
    rig.create_floor();
    let mut blocks = Vec::new();
    let mut rest = FLOOR_INIT;
    while let Some(start) = rest.find("```\n") {
        let after = &rest[start + 4..];
        let end = after.find("```").expect("a closed code block");
        blocks.push(&after[..end]);
        rest = &after[end + 3..];
    }
    let employees: Vec<&&str> = blocks.iter().filter(|b| b.starts_with("+++\n")).collect();
    assert!(employees.len() >= 3, "{blocks:?}");
    for (index, block) in employees.iter().enumerate() {
        std::fs::write(
            rig.cwd.join(format!(".swarm/employees/example-{index}.md")),
            block,
        )
        .unwrap();
    }
    let floor_toml: Vec<&&str> = blocks.iter().filter(|b| b.starts_with("version")).collect();
    assert_eq!(floor_toml.len(), 1);
    std::fs::write(rig.cwd.join(".swarm/floor.toml"), floor_toml[0]).unwrap();
    let floor = view(&rig);
    assert_eq!(floor.employee_errors, Vec::<String>::new());
    let eli = floor
        .employees
        .iter()
        .find(|e| e.slug == "example-0")
        .unwrap();
    assert_eq!(eli.name, "Eli");
    assert_eq!(eli.permission, PermissionPolicy::EditInWorktree);
    assert_eq!(eli.model.as_deref(), Some("sonnet"));
    assert_eq!(floor.name, "Acme");
    assert_eq!(
        floor.description.as_deref(),
        Some("A web shop for hand-made candles.")
    );
    assert_eq!(
        floor.logo_prompt.as_deref(),
        Some("A golden honeycomb hexagon with a small bee.")
    );
    for word in [
        "read-only",
        "edit-in-worktree",
        "ask-always",
        "claude",
        "codex",
    ] {
        assert!(FLOOR_INIT.contains(word), "{word}");
    }
    // The facts about the task board and the report files (spec 4.4 and 4.10).
    for word in [
        "task_create(title, description, parent?)",
        "task_assign(task, employee)",
        "task_update(task, status, note?, report?)",
        "task_get(task)",
        "task_list(status?, assignee?)",
        "task_comment(task, text)",
        ".swarm/reports/<task-id>.md",
        "[task T-12 from <Name> (<Role>)]",
    ] {
        assert!(FLOOR_INIT.contains(word), "{word}");
    }
}

#[test]
fn setup_start_runs_a_normal_session_with_the_slug_setup_and_the_init_instructions() {
    let rig = Rig::new();
    rig.create_floor();
    std::fs::write(
        rig.cwd.join(".swarm/floor.toml"),
        "version = 1\nname = \"Test Floor\"\n[defaults]\nmodel = \"haiku\"\neffort = \"low\"\n",
    )
    .unwrap();
    let started = rig
        .tower
        .setup_start(&rig.cwd, CliKind::Claude, 24, 80)
        .unwrap();
    assert_eq!(started.slug, "setup");
    assert_eq!(started.status, Status::Starting);
    let id = started.session_id.clone();
    rig.wait_first_turn(&id);

    let calls = rig.calls();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(call.slug, "setup");
    assert_eq!(call.first_message.as_deref(), Some(SETUP_FIRST_MESSAGE));
    assert_eq!(SETUP_FIRST_MESSAGE, "Start the floor setup.");
    assert!(
        call.instructions
            .as_deref()
            .unwrap()
            .contains(FLOOR_INIT.trim())
    );
    assert_eq!(call.permission, PermissionPolicy::EditInWorktree);
    assert_eq!(call.model.as_deref(), Some("haiku"));
    assert_eq!(call.effort.as_deref(), Some("low"));
    assert_eq!(call.cwd, rig.cwd);
    assert_eq!(
        rig.db()
            .last_session("setup")
            .unwrap()
            .unwrap()
            .app_session_id,
        id
    );

    // The view shows it while it is live.
    let floor = view(&rig);
    assert_eq!(floor.setup_session_id.as_deref(), Some(id.as_str()));
    let shown = floor.sessions.iter().find(|s| s.slug == "setup").unwrap();
    assert_eq!(shown.session_id, id);
    // One setup session at a time. The employee commands do not reach it.
    let error = rig
        .tower
        .setup_start(&rig.cwd, CliKind::Codex, 24, 80)
        .unwrap_err()
        .to_string();
    assert!(error.contains("already has a live session"), "{error}");

    // After the stop: a stopped session that cannot resume, and no live ID.
    rig.stop_and_wait(&id);
    wait_for("the floor view to drop the live session", || {
        view(&rig).setup_session_id.is_none()
    });
    let floor = view(&rig);
    let shown = floor.sessions.iter().find(|s| s.slug == "setup").unwrap();
    assert_eq!(shown.status, Status::Stopped);
    assert!(!shown.can_resume);
    // The program of the last session is stopped, so a new setup can start.
    let again = rig
        .tower
        .setup_start(&rig.cwd, CliKind::Codex, 24, 80)
        .unwrap();
    assert_ne!(again.session_id, id);
    rig.stop_and_wait(&again.session_id);
}

#[test]
fn the_floor_is_read_again_after_each_turn_and_at_the_stop() {
    let rig = Rig::new();
    rig.create_floor();
    let started = rig
        .tower
        .setup_start(&rig.cwd, CliKind::Claude, 24, 80)
        .unwrap();
    let id = started.session_id.clone();
    rig.wait_first_turn(&id);
    // The end of the first turn gives one floor-changed. The session start gives none.
    wait_for("the floor-changed of the first turn", || {
        floor_events(&rig) >= 1
    });
    let after_first = floor_events(&rig);

    // The agent writes the files while the session runs: an employee, and the brand.
    std::fs::write(
        rig.cwd.join(".swarm/employees/eng-1.md"),
        "+++\nname = \"Eli\"\nrole = \"Engineer\"\nmanager = \"pm\"\ncli = \"claude\"\npermission = \"edit-in-worktree\"\n+++\nWrite code.\n",
    )
    .unwrap();
    let floor_toml = rig.cwd.join(".swarm/floor.toml");
    let text = std::fs::read_to_string(&floor_toml).unwrap();
    std::fs::write(
        &floor_toml,
        format!("{text}\n[brand]\ndescription = \"A web shop.\"\n"),
    )
    .unwrap();

    // Nothing reads the files before the turn ends. The command `employee_save` gives a view
    // with no read of floor.toml, so it shows what the Tower holds in memory.
    let before = rig.tower.employee_save(&rig.cwd, employee("tmp")).unwrap();
    assert_eq!(before.description, None);
    rig.tower.employee_remove(&rig.cwd, "tmp").unwrap();
    let at_save = floor_events(&rig);

    // A second turn. When it ends, the Tower reads the floor again and tells the observer.
    rig.tower.debug_paste(&id, "next question").unwrap();
    wait_for("the floor-changed of the second turn", || {
        floor_events(&rig) > at_save
    });
    assert!(floor_events(&rig) > after_first);
    let after = rig.tower.employee_save(&rig.cwd, employee("tmp")).unwrap();
    assert_eq!(after.description.as_deref(), Some("A web shop."));
    assert!(after.employees.iter().any(|e| e.slug == "eng-1"));
    rig.tower.employee_remove(&rig.cwd, "tmp").unwrap();

    // The stop gives one more.
    let before_stop = floor_events(&rig);
    rig.stop_and_wait(&id);
    wait_for("the floor-changed of the stop", || {
        floor_events(&rig) > before_stop
    });
}

/// The agent writes `logo_prompt` into `floor.toml` (or replaces it), like in a setup turn.
fn write_logo_prompt(rig: &Rig, prompt: &str) {
    let floor_toml = rig.cwd.join(".swarm/floor.toml");
    let text = std::fs::read_to_string(&floor_toml).unwrap();
    let mut table: toml::Table = toml::from_str(&text).unwrap();
    let brand = table
        .entry("brand")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .unwrap();
    brand.insert("logo_prompt".into(), toml::Value::String(prompt.into()));
    std::fs::write(&floor_toml, toml::to_string(&table).unwrap()).unwrap();
}

/// One more turn of the setup session. Returns after the floor-changed of the turn end.
fn setup_turn(rig: &Rig, id: &str, text: &str) {
    let before = floor_events(rig);
    rig.tower.debug_paste(id, text).unwrap();
    wait_for("the floor-changed of a turn", || floor_events(rig) > before);
}

#[test]
fn the_logo_starts_by_itself_after_a_turn_when_the_conditions_hold_and_is_not_tried_again() {
    let api = FakeApi::start();
    let rig = rig_with_api(&api, true);
    rig.create_floor();
    let id = rig
        .tower
        .setup_start(&rig.cwd, CliKind::Claude, 24, 80)
        .unwrap()
        .session_id;
    rig.wait_first_turn(&id);
    wait_for("the floor-changed of the first turn", || {
        floor_events(&rig) >= 1
    });
    // No prompt yet: nothing starts.
    std::thread::sleep(Duration::from_millis(200));
    assert!(api.requests().is_empty());

    // The agent writes a subject. The next turn end starts the generation, and it fails.
    api.reply(500, "{\"error\":{\"message\":\"no\"}}");
    write_logo_prompt(&rig, "A bee.");
    setup_turn(&rig, &id, "next question");
    wait_for("the automatic request", || api.requests().len() == 1);
    wait_idle_logo(&rig);
    let failed = view(&rig);
    assert!(failed.logo_error.as_deref().unwrap_or("").contains("500"));
    assert!(!logo_file(&rig).exists());

    // A second turn does not try the same prompt again after a failure.
    setup_turn(&rig, &id, "second question");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(api.requests().len(), 1, "the failed prompt was tried again");

    // A new prompt is a new attempt. It works, and then there is a logo.
    api.reply_picture(&png_with_rect(20, 20, 2, 2, 9, 9));
    write_logo_prompt(&rig, "A fox.");
    setup_turn(&rig, &id, "third question");
    wait_for("the second automatic request", || api.requests().len() == 2);
    wait_idle_logo(&rig);
    assert!(logo_file(&rig).is_file());
    let body: Value = serde_json::from_str(&api.requests()[1].body).unwrap();
    assert!(
        body["prompt"]
            .as_str()
            .unwrap()
            .contains(": A fox. Centered")
    );

    // The same prompt with a logo: a new turn starts nothing.
    setup_turn(&rig, &id, "fourth question");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(api.requests().len(), 2);
    rig.stop_and_wait(&id);
}

#[test]
fn an_old_prompt_from_before_the_setup_does_not_start_a_logo() {
    let api = FakeApi::start();
    let rig = rig_with_api(&api, true);
    rig.create_floor();
    // The prompt is in the file before the setup starts (for example a manual attempt that failed).
    write_logo_prompt(&rig, "An old bee.");
    let id = rig
        .tower
        .setup_start(&rig.cwd, CliKind::Claude, 24, 80)
        .unwrap()
        .session_id;
    rig.wait_first_turn(&id);
    wait_for("the floor-changed of the first turn", || {
        floor_events(&rig) >= 1
    });
    setup_turn(&rig, &id, "second question");
    std::thread::sleep(Duration::from_millis(300));
    assert!(api.requests().is_empty(), "the old prompt started a logo");
    assert!(!logo_file(&rig).exists());

    // The prompt that the agent writes later gets a logo, also when no logo exists yet.
    api.reply_picture(&png_with_rect(20, 20, 2, 2, 9, 9));
    write_logo_prompt(&rig, "A new fox.");
    setup_turn(&rig, &id, "third question");
    wait_for("the automatic request", || api.requests().len() == 1);
    wait_idle_logo(&rig);
    assert!(logo_file(&rig).is_file());
    let body: Value = serde_json::from_str(&api.requests()[0].body).unwrap();
    assert!(
        body["prompt"]
            .as_str()
            .unwrap()
            .contains(": A new fox. Centered")
    );
    rig.stop_and_wait(&id);
}

#[test]
fn a_prompt_that_the_agent_changes_replaces_a_logo_that_exists() {
    let api = FakeApi::start();
    let rig = rig_with_api(&api, true);
    rig.create_floor();
    write_logo_prompt(&rig, "An old bee.");
    let old_logo = png_with_rect(20, 20, 2, 2, 9, 9);
    std::fs::create_dir_all(logo_file(&rig).parent().unwrap()).unwrap();
    std::fs::write(logo_file(&rig), &old_logo).unwrap();
    let id = rig
        .tower
        .setup_start(&rig.cwd, CliKind::Claude, 24, 80)
        .unwrap()
        .session_id;
    rig.wait_first_turn(&id);
    wait_for("the floor-changed of the first turn", || {
        floor_events(&rig) >= 1
    });
    setup_turn(&rig, &id, "second question");
    std::thread::sleep(Duration::from_millis(300));
    assert!(api.requests().is_empty());
    assert_eq!(std::fs::read(logo_file(&rig)).unwrap(), old_logo);

    // The agent writes a new subject. The logo exists, and it is made again.
    let new_logo = png_with_rect(30, 30, 4, 4, 20, 20);
    api.reply_picture(&new_logo);
    write_logo_prompt(&rig, "A new fox.");
    setup_turn(&rig, &id, "third question");
    wait_for("the automatic request", || api.requests().len() == 1);
    wait_idle_logo(&rig);
    assert_ne!(std::fs::read(logo_file(&rig)).unwrap(), old_logo);
    rig.stop_and_wait(&id);
}

#[test]
fn the_logo_does_not_start_by_itself_without_a_key() {
    let api = FakeApi::start();
    let rig = rig_with_api(&api, false);
    rig.create_floor();
    let floor_toml = rig.cwd.join(".swarm/floor.toml");
    let text = std::fs::read_to_string(&floor_toml).unwrap();
    std::fs::write(
        &floor_toml,
        format!("{text}\n[brand]\nlogo_prompt = \"A bee.\"\n"),
    )
    .unwrap();
    let id = rig
        .tower
        .setup_start(&rig.cwd, CliKind::Claude, 24, 80)
        .unwrap()
        .session_id;
    rig.wait_first_turn(&id);
    wait_for("the floor-changed of the first turn", || {
        floor_events(&rig) >= 1
    });
    std::thread::sleep(Duration::from_millis(300));
    assert!(api.requests().is_empty());
    assert!(!view(&rig).logo_busy);
    rig.stop_and_wait(&id);
}

// ---------------------------------------------------------------------------------------------
// The setup session: the project listing and the removal mark
// ---------------------------------------------------------------------------------------------

#[test]
fn the_setup_instructions_end_with_the_listing_of_the_project_and_of_the_employees() {
    let rig = Rig::new();
    rig.create_floor();
    std::fs::write(rig.cwd.join("README.md"), "# Demo\n").unwrap();
    std::fs::create_dir_all(rig.cwd.join("crates/demo_sim")).unwrap();
    std::fs::create_dir_all(rig.cwd.join("node_modules/left-pad")).unwrap();
    std::fs::create_dir_all(rig.cwd.join(".git/objects")).unwrap();
    let id = rig
        .tower
        .setup_start(&rig.cwd, CliKind::Codex, 24, 80)
        .unwrap()
        .session_id;
    rig.wait_first_turn(&id);
    let calls = rig.calls();
    let text = calls[0].instructions.as_deref().unwrap();
    let section = &text[text.find("## The project now").expect("the section")..];
    // The section comes after the fixed text.
    assert!(text.find(FLOOR_INIT.trim()).unwrap() < text.find("## The project now").unwrap());
    assert!(section.contains("You run in the CLI `codex`."), "{section}");
    assert!(section.contains("\nREADME.md\n"), "{section}");
    assert!(section.contains("\ncrates/\n    demo_sim/\n"), "{section}");
    // The content of build folders and of hidden folders is not shown.
    assert!(section.contains("\nnode_modules/\n"), "{section}");
    assert!(!section.contains("left-pad"), "{section}");
    assert!(section.contains("\n.git/\n"), "{section}");
    assert!(!section.contains("objects"), "{section}");
    // The two employees of a new floor, with their roles.
    assert!(section.contains("has 2 employee files"), "{section}");
    assert!(section.contains("ceo.md (role: CEO)"), "{section}");
    assert!(
        section.contains("pm.md (role: Project Manager)"),
        "{section}"
    );
}

#[test]
fn a_floor_with_no_employee_file_says_so_in_the_setup_instructions() {
    let rig = Rig::new();
    rig.create_floor();
    for slug in ["ceo", "pm"] {
        std::fs::remove_file(rig.cwd.join(format!(".swarm/employees/{slug}.md"))).unwrap();
    }
    let id = rig
        .tower
        .setup_start(&rig.cwd, CliKind::Claude, 24, 80)
        .unwrap()
        .session_id;
    rig.wait_first_turn(&id);
    let calls = rig.calls();
    let text = calls[0].instructions.as_deref().unwrap();
    assert!(text.contains("has no employee file"), "{text}");
    assert!(text.contains("You run in the CLI `claude`."));
}

#[test]
fn an_employee_file_with_the_delete_mark_is_removed_at_the_end_of_a_setup_turn() {
    let rig = Rig::new();
    rig.create_floor();
    let dir = rig.cwd.join(".swarm/employees");
    let file = |slug: &str, role_text: &str| {
        std::fs::write(
            dir.join(format!("{slug}.md")),
            format!("+++\nname = \"Eli\"\nrole = \"Engineer\"\ncli = \"claude\"\npermission = \"read-only\"\n+++\n{role_text}\n"),
        )
        .unwrap();
    };
    file("eng-1", "Write code.");
    file("eng-2", "DELETE");
    let id = rig
        .tower
        .setup_start(&rig.cwd, CliKind::Claude, 24, 80)
        .unwrap()
        .session_id;
    rig.wait_first_turn(&id);
    wait_for("the floor-changed of the first turn", || {
        floor_events(&rig) >= 1
    });
    assert_eq!(view(&rig).employees.len(), 4);

    // The agent marks one file. A file that only has the word in its role text is not a mark.
    std::fs::write(dir.join("eng-1.md"), "DELETE\n").unwrap();
    setup_turn(&rig, &id, "next");
    let floor = view(&rig);
    let slugs: Vec<&str> = floor.employees.iter().map(|e| e.slug.as_str()).collect();
    assert_eq!(slugs, ["ceo", "eng-2", "pm"]);
    assert_eq!(floor.employee_errors, Vec::<String>::new());
    assert!(!dir.join("eng-1.md").exists());
    assert!(dir.join("eng-2.md").exists());
}

#[test]
fn the_delete_mark_does_nothing_outside_a_setup_session() {
    let rig = Rig::new();
    rig.create_floor();
    let marked = rig.cwd.join(".swarm/employees/eng-1.md");
    std::fs::write(&marked, "DELETE").unwrap();
    let id = rig.start("ceo").session_id;
    rig.wait_first_turn(&id);
    rig.stop_and_wait(&id);
    assert!(marked.exists());
    assert_eq!(view(&rig).employee_errors.len(), 1);
}
