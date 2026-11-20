//! The model lists, the custom models, the floor defaults and the model name check of the `Tower`
//! (contract `docs/ipc-settings.md`). The sessions are the real `fake-cli`. The Codex home is
//! always a temp folder. No test reads `~/.codex`, and no test starts a real CLI.

mod app_support;

use app_support::{Ev, Rig};
use std::path::{Path, PathBuf};
use tower_core::app::{CodexSource, ModelCatalog, ModelOption};
use tower_core::model::{CliKind, Employee, FloorDefaults, PermissionPolicy};

/// The Codex home of a rig.
fn codex_home(rig: &Rig) -> PathBuf {
    rig.root.path().join("codex-home")
}

fn write_cache(rig: &Rig, json: &str) {
    let home = codex_home(rig);
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("models_cache.json"), json).unwrap();
}

fn ids(list: &[ModelOption]) -> Vec<&str> {
    list.iter().map(|m| m.id.as_str()).collect()
}

fn read_toml(path: &Path) -> toml::Table {
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn config_file(rig: &Rig) -> PathBuf {
    rig.config.join("programs.toml")
}

fn floor_file(rig: &Rig) -> PathBuf {
    rig.cwd.join(".swarm/floor.toml")
}

fn engineer(slug: &str, cli: CliKind) -> Employee {
    Employee {
        name: "Eli".into(),
        slug: slug.into(),
        role: "Engineer".into(),
        manager: None,
        cli,
        model: None,
        effort: None,
        permission: PermissionPolicy::ReadOnly,
        role_text: String::new(),
    }
}

/// A cache file like the real one: extra keys, a hidden model, and a model with many extra fields.
const CACHE: &str = r#"{
    "fetched_at": "2026-11-20T10:00:00Z",
    "etag": "abc",
    "client_version": "9.9.9",
    "identity": {"a": 1},
    "models": [
        {"slug": "gpt-a", "display_name": "GPT A", "visibility": "list",
         "default_reasoning_level": "low",
         "supported_reasoning_levels": [
             {"effort": "low", "description": "x"}, {"effort": "high", "description": "y"},
             {"effort": "ultra"}],
         "base_instructions": "long text", "extra": {"deep": [1, 2, 3]}},
        {"slug": "gpt-hidden", "display_name": "Hidden", "visibility": "hide",
         "default_reasoning_level": "medium",
         "supported_reasoning_levels": [{"effort": "medium"}]},
        {"slug": "gpt-b", "display_name": "GPT B", "visibility": "list",
         "default_reasoning_level": "medium",
         "supported_reasoning_levels": [{"effort": "medium"}]}
    ]
}"#;

// ---------------------------------------------------------------------------------------------
// The lists
// ---------------------------------------------------------------------------------------------

#[test]
fn with_no_codex_file_the_lists_are_the_built_in_ones() {
    let rig = Rig::new();
    let catalog = rig.tower.models_get();
    assert_eq!(catalog.codex_source, CodexSource::Builtin);
    assert_eq!(
        ids(&catalog.claude),
        ["fable", "opus", "sonnet", "haiku"],
        "the Claude aliases"
    );
    for model in &catalog.claude {
        assert_eq!(model.efforts, ["low", "medium", "high", "xhigh", "max"]);
        assert!(!model.custom);
    }
    assert_eq!(
        ids(&catalog.image),
        ["gpt-image-2.6-sunburst", "gpt-image-2.6-flare"]
    );
    assert!(catalog.image.iter().all(|m| m.efforts.is_empty()));
    assert!(!catalog.codex.is_empty());
    assert!(catalog.codex.iter().any(|m| m.id == "gpt-6-luna"));
}

#[test]
fn the_codex_list_comes_from_the_cache_file_in_the_file_order_and_is_read_again_each_call() {
    let rig = Rig::new();
    write_cache(&rig, CACHE);
    let catalog = rig.tower.models_get();
    assert_eq!(catalog.codex_source, CodexSource::Cache);
    assert_eq!(
        ids(&catalog.codex),
        ["gpt-a", "gpt-b"],
        "hidden is left out"
    );
    let first = &catalog.codex[0];
    assert_eq!(first.label, "GPT A");
    assert_eq!(first.efforts, ["low", "high", "ultra"]);
    assert_eq!(first.default_effort.as_deref(), Some("low"));
    assert!(!first.custom);
    assert_eq!(catalog.codex[1].default_effort.as_deref(), Some("medium"));
    // The other lists do not change.
    assert_eq!(ids(&catalog.claude).len(), 4);

    // A new file gives a new list on the next call.
    write_cache(
        &rig,
        r#"{"models": [{"slug": "gpt-z", "visibility": "list"}]}"#,
    );
    let again = rig.tower.models_get();
    assert_eq!(ids(&again.codex), ["gpt-z"]);
    assert_eq!(again.codex[0].label, "gpt-z", "no display name: the slug");
    assert!(again.codex[0].efforts.is_empty());
    assert_eq!(again.codex[0].default_effort, None);
}

#[test]
fn a_missing_or_bad_cache_file_gives_the_built_in_codex_list() {
    let rig = Rig::new();
    for bad in [
        "not json at all",
        "{}",
        r#"{"models": "x"}"#,
        r#"{"models": []}"#,
        // Only hidden models: nothing to show.
        r#"{"models": [{"slug": "a", "visibility": "hide"}]}"#,
        // No model has a valid slug.
        r#"{"models": [{"slug": "--foo", "visibility": "list"}]}"#,
    ] {
        write_cache(&rig, bad);
        let catalog = rig.tower.models_get();
        assert_eq!(catalog.codex_source, CodexSource::Builtin, "{bad}");
        assert!(catalog.codex.iter().any(|m| m.id == "gpt-6-luna"), "{bad}");
    }
    // The folder has no file at all.
    std::fs::remove_file(codex_home(&rig).join("models_cache.json")).unwrap();
    assert_eq!(rig.tower.models_get().codex_source, CodexSource::Builtin);
}

#[test]
fn a_cache_slug_or_effort_that_is_not_valid_never_reaches_a_list() {
    let rig = Rig::new();
    write_cache(
        &rig,
        r#"{"models": [
            {"slug": "--evil", "visibility": "list"},
            {"slug": "good", "visibility": "list", "default_reasoning_level": "--x",
             "supported_reasoning_levels": [{"effort": "low"}, {"effort": "--bad"}, {"effort": "Hi"}]},
            {"slug": "good", "visibility": "list"}
        ]}"#,
    );
    let catalog = rig.tower.models_get();
    assert_eq!(catalog.codex_source, CodexSource::Cache);
    assert_eq!(
        ids(&catalog.codex),
        ["good"],
        "bad and duplicate slugs are out"
    );
    assert_eq!(catalog.codex[0].efforts, ["low"]);
    assert_eq!(catalog.codex[0].default_effort, None);
}

#[test]
fn the_codex_home_comes_from_codex_home_and_then_from_home() {
    // `CODEX_HOME` of the environment of the app.
    let rig = Rig::new();
    let home = rig.root.path().join("elsewhere");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("models_cache.json"), CACHE).unwrap();
    let socket = rig.root.path().join("a.sock");
    let value = home.clone();
    let tower = Rig::make_tower(&rig.config, &socket, &rig.shared, move |cfg| {
        cfg.codex_home = None;
        cfg.parent_env
            .insert("CODEX_HOME".into(), value.into_os_string());
    });
    assert_eq!(tower.models_get().codex_source, CodexSource::Cache);

    // No `CODEX_HOME`: `$HOME/.codex`.
    let fake_home = rig.root.path().join("home");
    std::fs::create_dir_all(fake_home.join(".codex")).unwrap();
    std::fs::write(fake_home.join(".codex/models_cache.json"), CACHE).unwrap();
    let socket = rig.root.path().join("b.sock");
    let value = fake_home.clone();
    let tower = Rig::make_tower(&rig.config, &socket, &rig.shared, move |cfg| {
        cfg.codex_home = None;
        cfg.parent_env.insert("HOME".into(), value.into_os_string());
    });
    assert_eq!(ids(&tower.models_get().codex), ["gpt-a", "gpt-b"]);

    // Neither is set: the built-in list.
    let socket = rig.root.path().join("c.sock");
    let tower = Rig::make_tower(&rig.config, &socket, &rig.shared, |cfg| {
        cfg.codex_home = None;
        cfg.parent_env.remove(std::ffi::OsStr::new("HOME"));
    });
    assert_eq!(tower.models_get().codex_source, CodexSource::Builtin);
}

// ---------------------------------------------------------------------------------------------
// Custom models
// ---------------------------------------------------------------------------------------------

#[test]
fn a_custom_model_comes_after_the_others_and_is_stored_in_the_app_config() {
    let rig = Rig::new();
    write_cache(&rig, CACHE);
    // Dups, a name that is in the list already, and spaces around a name.
    let catalog = rig
        .tower
        .models_set_custom(
            "codex",
            &[
                "my-model".into(),
                " other.v2 ".into(),
                "my-model".into(),
                "gpt-a".into(),
            ],
        )
        .unwrap();
    assert_eq!(
        ids(&catalog.codex),
        ["gpt-a", "gpt-b", "my-model", "other.v2"]
    );
    let custom = &catalog.codex[2];
    assert!(custom.custom);
    assert_eq!(custom.label, "my-model");
    assert_eq!(custom.efforts, ["low", "medium", "high", "xhigh", "max"]);
    assert!(!catalog.codex[0].custom);

    let table = read_toml(&config_file(&rig));
    let stored: Vec<&str> = table["models"]["codex"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        stored,
        ["my-model", "other.v2"],
        "a list name is not stored"
    );

    // The other tables of the file stay, and a later program change keeps `[models]`.
    assert!(table["programs"]["claude"].is_str());
    rig.tower.programs_set("node", "/bin/true").unwrap();
    rig.tower.settings_set(Some("sk-x".into()), None).unwrap();
    let table = read_toml(&config_file(&rig));
    assert!(table["models"]["codex"].is_array());
    assert_eq!(table["openai"]["key"].as_str(), Some("sk-x"));

    // Claude and image lists take their own custom models. An image model has no efforts.
    let catalog = rig
        .tower
        .models_set_custom("claude", &["claude-opus-4-1[1m]".into()])
        .unwrap();
    assert_eq!(catalog.claude.last().unwrap().id, "claude-opus-4-1[1m]");
    assert!(catalog.claude.last().unwrap().custom);
    let catalog = rig
        .tower
        .models_set_custom("image", &["img-x".into(), "gpt-image-2.6-flare".into()])
        .unwrap();
    assert_eq!(
        ids(&catalog.image),
        ["gpt-image-2.6-sunburst", "gpt-image-2.6-flare", "img-x"]
    );
    assert!(catalog.image.last().unwrap().efforts.is_empty());
    // The codex custom models stay.
    assert!(ids(&catalog.codex).contains(&"my-model"));

    // The list is replaced, not added to. An empty list removes the key, and the table.
    let catalog = rig
        .tower
        .models_set_custom("codex", &["only-one".into()])
        .unwrap();
    assert_eq!(ids(&catalog.codex), ["gpt-a", "gpt-b", "only-one"]);
    rig.tower.models_set_custom("codex", &[]).unwrap();
    rig.tower.models_set_custom("claude", &[]).unwrap();
    let catalog = rig.tower.models_set_custom("image", &[]).unwrap();
    assert!(
        catalog
            .codex
            .iter()
            .chain(&catalog.claude)
            .chain(&catalog.image)
            .all(|m| !m.custom)
    );
    assert!(read_toml(&config_file(&rig)).get("models").is_none());
}

#[test]
fn models_set_custom_refuses_a_bad_name_and_writes_nothing() {
    let rig = Rig::new();
    rig.tower
        .models_set_custom("claude", &["fine-one".into()])
        .unwrap();
    let before = std::fs::read_to_string(config_file(&rig)).unwrap();
    for bad in [
        "--foo",
        "-x",
        "",
        "   ",
        "a b",
        "a\nb",
        "x;y",
        "é",
        &"a".repeat(101),
    ] {
        let error = rig
            .tower
            .models_set_custom("claude", &["new-ok".into(), bad.into()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a valid model name"), "{bad:?}: {error}");
    }
    assert_eq!(
        std::fs::read_to_string(config_file(&rig)).unwrap(),
        before,
        "the file did not change"
    );
    let error = rig
        .tower
        .models_set_custom("gemini", &["x".into()])
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown model list"), "{error}");
    // A name of exactly 100 characters is fine.
    rig.tower
        .models_set_custom("claude", &["a".repeat(100)])
        .unwrap();
}

#[test]
fn a_hand_written_custom_entry_that_is_not_valid_is_left_out_of_the_list() {
    let rig = Rig::new();
    std::fs::create_dir_all(&rig.config).unwrap();
    std::fs::write(
        config_file(&rig),
        "[models]\nclaude = [\"--foo\", \"ok-1\", 5, \"ok-1\", \"opus\"]\ncodex = \"text\"\nimage = [\"-i\"]\n",
    )
    .unwrap();
    let catalog = rig.tower.models_get();
    assert_eq!(
        ids(&catalog.claude),
        ["fable", "opus", "sonnet", "haiku", "ok-1"]
    );
    assert!(catalog.codex.iter().all(|m| !m.custom));
    assert!(catalog.image.iter().all(|m| !m.custom));
    // A `models` value that is not a table is replaced by a write.
    std::fs::write(config_file(&rig), "models = 3\n").unwrap();
    rig.tower
        .models_set_custom("claude", &["fine".into()])
        .unwrap();
    assert_eq!(
        read_toml(&config_file(&rig))["models"]["claude"][0].as_str(),
        Some("fine")
    );
}

// ---------------------------------------------------------------------------------------------
// The model name check at the four doors
// ---------------------------------------------------------------------------------------------

#[test]
fn employee_save_refuses_a_bad_model_or_effort_and_stores_nothing() {
    let rig = Rig::new();
    rig.create_floor();
    for model in ["--foo", "-m", "a b", "x\ny", "é"] {
        let mut e = engineer("eli", CliKind::Claude);
        e.model = Some(model.into());
        let error = rig
            .tower
            .employee_save(&rig.cwd, e)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("not a valid model name"),
            "{model:?}: {error}"
        );
    }
    for effort in ["--x", "High", "low high", "a1", &"a".repeat(21)] {
        let mut e = engineer("eli", CliKind::Codex);
        e.effort = Some(effort.into());
        let error = rig
            .tower
            .employee_save(&rig.cwd, e)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a valid effort"), "{effort:?}: {error}");
    }
    assert!(!rig.cwd.join(".swarm/employees/eli.md").exists());
    // Valid values, and the not-set values, are saved.
    let mut e = engineer("eli", CliKind::Claude);
    e.model = Some("claude-opus-4-1[1m]".into());
    e.effort = Some("xhigh".into());
    rig.tower.employee_save(&rig.cwd, e).unwrap();
    let mut e = engineer("eli", CliKind::Claude);
    e.model = Some(String::new());
    rig.tower.employee_save(&rig.cwd, e).unwrap();
}

#[test]
fn floor_defaults_set_refuses_a_bad_model_or_effort() {
    let rig = Rig::new();
    rig.create_floor();
    let before = std::fs::read_to_string(floor_file(&rig)).unwrap();
    for model in ["--foo", "-m", "a b"] {
        let error = rig
            .tower
            .floor_defaults_set(
                &rig.cwd,
                FloorDefaults {
                    model: Some(model.into()),
                    ..FloorDefaults::default()
                },
            )
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("not a valid model name"),
            "{model:?}: {error}"
        );
    }
    let error = rig
        .tower
        .floor_defaults_set(
            &rig.cwd,
            FloorDefaults {
                effort: Some("--x".into()),
                ..FloorDefaults::default()
            },
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("not a valid effort"), "{error}");
    assert_eq!(std::fs::read_to_string(floor_file(&rig)).unwrap(), before);
}

#[test]
fn settings_set_refuses_a_bad_image_model_and_still_takes_a_good_one_or_an_empty_one() {
    let rig = Rig::new();
    for bad in ["--foo", "-x", "a b", "a\nb", "é"] {
        let error = rig
            .tower
            .settings_set(None, Some(bad.into()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a valid model name"), "{bad:?}: {error}");
    }
    assert_eq!(
        rig.tower.settings_get().image_model,
        "gpt-image-2.6-sunburst"
    );
    assert!(!config_file(&rig).exists() || read_toml(&config_file(&rig)).get("openai").is_none());
    let set = rig
        .tower
        .settings_set(None, Some("gpt-image-2.6-flare".into()))
        .unwrap();
    assert_eq!(set.image_model, "gpt-image-2.6-flare");
    let set = rig.tower.settings_set(None, Some("  ".into())).unwrap();
    assert_eq!(set.image_model, "gpt-image-2.6-sunburst", "empty resets");
}

// ---------------------------------------------------------------------------------------------
// The floor defaults
// ---------------------------------------------------------------------------------------------

#[test]
fn floor_defaults_set_writes_the_defaults_table_and_keeps_the_rest_of_the_file() {
    let rig = Rig::new();
    rig.create_floor();
    // Brand and a key that this build does not know must stay.
    std::fs::write(
        floor_file(&rig),
        "version = 1\nname = \"Test Floor\"\nfuture = \"keep\"\n\n[brand]\ndescription = \"A shop.\"\n\n[defaults]\nmodel = \"old\"\n",
    )
    .unwrap();
    let floor = rig.tower.floor_open(&rig.cwd).unwrap();
    assert_eq!(floor.defaults.model.as_deref(), Some("old"));
    let events_before = rig
        .events
        .all()
        .iter()
        .filter(|ev| matches!(ev, Ev::Floor(_)))
        .count();

    let defaults = FloorDefaults {
        cli: Some(CliKind::Codex),
        model: Some("gpt-6-luna".into()),
        effort: Some("high".into()),
        permission: Some(PermissionPolicy::EditInWorktree),
    };
    let view = rig
        .tower
        .floor_defaults_set(&rig.cwd, defaults.clone())
        .unwrap();
    assert_eq!(view.defaults, defaults);
    let table = read_toml(&floor_file(&rig));
    assert_eq!(table["future"].as_str(), Some("keep"));
    assert_eq!(table["name"].as_str(), Some("Test Floor"));
    assert_eq!(table["brand"]["description"].as_str(), Some("A shop."));
    assert_eq!(table["defaults"]["cli"].as_str(), Some("codex"));
    assert_eq!(table["defaults"]["model"].as_str(), Some("gpt-6-luna"));
    assert_eq!(table["defaults"]["effort"].as_str(), Some("high"));
    assert_eq!(
        table["defaults"]["permission"].as_str(),
        Some("edit-in-worktree")
    );
    let events_after = rig
        .events
        .all()
        .iter()
        .filter(|ev| matches!(ev, Ev::Floor(_)))
        .count();
    assert_eq!(events_after, events_before + 1, "one floor-changed");
    // A fresh read of the folder gives the same defaults.
    assert_eq!(rig.tower.floor_open(&rig.cwd).unwrap().defaults, defaults);

    // A null field goes out of the file. The other fields stay.
    let view = rig
        .tower
        .floor_defaults_set(
            &rig.cwd,
            FloorDefaults {
                cli: Some(CliKind::Claude),
                ..FloorDefaults::default()
            },
        )
        .unwrap();
    assert_eq!(view.defaults.model, None);
    let table = read_toml(&floor_file(&rig));
    let section = table["defaults"].as_table().unwrap();
    assert_eq!(section.len(), 1);
    assert_eq!(section["cli"].as_str(), Some("claude"));

    // All null: the table goes. The rest stays.
    rig.tower
        .floor_defaults_set(&rig.cwd, FloorDefaults::default())
        .unwrap();
    let table = read_toml(&floor_file(&rig));
    assert!(table.get("defaults").is_none());
    assert_eq!(table["future"].as_str(), Some("keep"));
    assert_eq!(table["brand"]["description"].as_str(), Some("A shop."));
}

// ---------------------------------------------------------------------------------------------
// Section 5: which defaults go to which employee
// ---------------------------------------------------------------------------------------------

fn set_defaults(rig: &Rig, cli: Option<CliKind>, model: Option<&str>, effort: Option<&str>) {
    rig.tower
        .floor_defaults_set(
            &rig.cwd,
            FloorDefaults {
                cli,
                model: model.map(str::to_owned),
                effort: effort.map(str::to_owned),
                permission: None,
            },
        )
        .unwrap();
}

/// Start an employee, read what the adapter got, and stop the session.
fn start_and_call(rig: &Rig, slug: &str) -> app_support::Call {
    let session = rig.start(slug);
    let call = rig
        .calls()
        .into_iter()
        .rfind(|c| c.slug == slug)
        .expect("a call");
    rig.stop_and_wait(&session.session_id);
    call
}

#[test]
fn the_default_model_does_not_go_to_an_employee_of_another_cli() {
    let rig = Rig::new();
    rig.create_floor();
    rig.make_codex("pm");
    // The default CLI is codex: the Codex employee gets both, the Claude employee gets none.
    set_defaults(&rig, Some(CliKind::Codex), Some("gpt-6-luna"), Some("high"));
    let pm = start_and_call(&rig, "pm");
    assert_eq!(pm.model.as_deref(), Some("gpt-6-luna"));
    assert_eq!(pm.effort.as_deref(), Some("high"));
    let ceo = start_and_call(&rig, "ceo");
    assert_eq!(ceo.model, None, "a Codex model must not go to Claude");
    assert_eq!(ceo.effort, None);

    // The default CLI is claude: the other way round.
    set_defaults(&rig, Some(CliKind::Claude), Some("haiku"), Some("low"));
    let ceo = start_and_call(&rig, "ceo");
    assert_eq!(ceo.model.as_deref(), Some("haiku"));
    assert_eq!(ceo.effort.as_deref(), Some("low"));
    let pm = start_and_call(&rig, "pm");
    assert_eq!((pm.model, pm.effort), (None, None));

    // No default CLI: the defaults go to both (the old rule).
    set_defaults(&rig, None, Some("m-x"), Some("medium"));
    for slug in ["ceo", "pm"] {
        let call = start_and_call(&rig, slug);
        assert_eq!(call.model.as_deref(), Some("m-x"), "{slug}");
        assert_eq!(call.effort.as_deref(), Some("medium"), "{slug}");
    }
}

#[test]
fn an_own_model_blocks_the_default_model_and_the_default_effort_follows_the_cli_rule() {
    let rig = Rig::new();
    rig.create_floor();
    let mut pm = rig
        .tower
        .floor_open(&rig.cwd)
        .unwrap()
        .employees
        .into_iter()
        .find(|e| e.slug == "pm")
        .unwrap();
    pm.model = Some("opus".into());
    rig.tower.employee_save(&rig.cwd, pm).unwrap();
    // Same CLI: the own model stays and the effort comes from the defaults.
    set_defaults(&rig, Some(CliKind::Claude), Some("haiku"), Some("low"));
    let call = start_and_call(&rig, "pm");
    assert_eq!(call.model.as_deref(), Some("opus"));
    assert_eq!(call.effort.as_deref(), Some("low"));
    // Other CLI: the own model stays and no default effort comes.
    set_defaults(&rig, Some(CliKind::Codex), Some("gpt-6-luna"), Some("high"));
    let call = start_and_call(&rig, "pm");
    assert_eq!(call.model.as_deref(), Some("opus"));
    assert_eq!(call.effort, None);
}

#[test]
fn the_setup_session_follows_the_same_rule() {
    let rig = Rig::new();
    rig.create_floor();
    set_defaults(&rig, Some(CliKind::Codex), Some("gpt-6-luna"), Some("high"));
    let session = rig
        .tower
        .setup_start(&rig.cwd, CliKind::Claude, 24, 80)
        .unwrap();
    let call = rig.calls().pop().unwrap();
    assert_eq!(call.slug, "setup");
    assert_eq!((call.model.clone(), call.effort.clone()), (None, None));
    rig.stop_and_wait(&session.session_id);

    let session = rig
        .tower
        .setup_start(&rig.cwd, CliKind::Codex, 24, 80)
        .unwrap();
    let call = rig.calls().pop().unwrap();
    assert_eq!(call.model.as_deref(), Some("gpt-6-luna"));
    assert_eq!(call.effort.as_deref(), Some("high"));
    rig.stop_and_wait(&session.session_id);
}

// ---------------------------------------------------------------------------------------------
// A hand-written file with a bad model must not start a session
// ---------------------------------------------------------------------------------------------

fn write_employee_file(rig: &Rig, slug: &str, model: Option<&str>, effort: Option<&str>) {
    let mut text = String::from("+++\nname = \"Hand\"\nrole = \"Engineer\"\ncli = \"claude\"\n");
    if let Some(model) = model {
        text.push_str(&format!("model = {model:?}\n"));
    }
    if let Some(effort) = effort {
        text.push_str(&format!("effort = {effort:?}\n"));
    }
    text.push_str("permission = \"read-only\"\n+++\nWork.\n");
    std::fs::write(rig.cwd.join(format!(".swarm/employees/{slug}.md")), text).unwrap();
}

#[test]
fn a_hand_written_employee_with_a_flag_as_model_or_effort_does_not_start() {
    let rig = Rig::new();
    rig.create_floor();
    write_employee_file(&rig, "hand", Some("--foo"), None);
    let error = rig.try_start("hand").unwrap_err().to_string();
    assert!(error.contains("not a valid model name"), "{error}");
    assert!(error.contains("--foo"), "{error}");
    assert!(rig.calls().is_empty(), "no process was prepared");

    write_employee_file(&rig, "hand", None, Some("--bar"));
    let error = rig.try_start("hand").unwrap_err().to_string();
    assert!(error.contains("not a valid effort"), "{error}");
    assert!(rig.calls().is_empty());

    // The same file with good values starts.
    write_employee_file(&rig, "hand", Some("haiku"), Some("low"));
    let call = start_and_call(&rig, "hand");
    assert_eq!(call.model.as_deref(), Some("haiku"));
    assert_eq!(call.effort.as_deref(), Some("low"));
    // The refused starts left no reservation: the employee started after the fix.
}

#[test]
fn a_hand_written_floor_default_with_a_flag_does_not_start_an_employee_or_the_setup() {
    let rig = Rig::new();
    rig.create_floor();
    std::fs::write(
        floor_file(&rig),
        "version = 1\nname = \"Test Floor\"\n\n[defaults]\nmodel = \"--foo\"\n",
    )
    .unwrap();
    let error = rig.try_start("ceo").unwrap_err().to_string();
    assert!(error.contains("not a valid model name"), "{error}");
    let error = rig
        .tower
        .setup_start(&rig.cwd, CliKind::Claude, 24, 80)
        .unwrap_err()
        .to_string();
    assert!(error.contains("not a valid model name"), "{error}");
    assert!(rig.calls().is_empty());
    // An employee with an own good model still would take the default effort, but the bad
    // default model does not matter for it: it has its own.
    write_employee_file(&rig, "own", Some("haiku"), None);
    let call = start_and_call(&rig, "own");
    assert_eq!(call.model.as_deref(), Some("haiku"));
}

#[test]
fn the_view_of_a_new_floor_has_empty_defaults() {
    let rig = Rig::new();
    let view = rig.create_floor();
    assert_eq!(view.defaults, FloorDefaults::default());
    let _: ModelCatalog = rig.tower.models_get();
}
