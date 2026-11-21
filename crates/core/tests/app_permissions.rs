//! The `[permissions]` table of `floor.toml` (milestone M3): the rules that the operator lists so
//! that a command needs no question. The commands `floor_permissions_get` and
//! `floor_permissions_set`, the checks, the writers that keep the table, the old file with no
//! table, and the rules that reach each Claude session.

mod app_support;

use app_support::Rig;
use std::fs;
use tower_core::app::FLOOR_INIT;
use tower_core::model::{Brand, FloorDefaults, PermissionPolicy};
use tower_core::store::FloorStore;

fn floor_toml(rig: &Rig) -> String {
    fs::read_to_string(rig.cwd.join(".swarm/floor.toml")).unwrap()
}

fn rules(list: &[&str]) -> Vec<String> {
    list.iter().map(|r| (*r).to_owned()).collect()
}

#[test]
fn the_operator_sets_and_reads_the_rules() {
    let rig = Rig::new();
    rig.create_floor();
    // An old floor file has no table: the list is empty.
    assert!(!floor_toml(&rig).contains("permissions"));
    assert_eq!(
        rig.tower.floor_permissions_get(&rig.cwd).unwrap(),
        Vec::<String>::new()
    );

    let before = rig.events.all().len();
    let stored = rig
        .tower
        .floor_permissions_set(
            &rig.cwd,
            rules(&[
                "Bash(cargo test:*)",
                " Bash(git status:*) ",
                "Read(./src/**)",
                "Bash(cargo test:*)",
            ]),
        )
        .unwrap();
    // The text is trimmed and a duplicate is dropped.
    assert_eq!(
        stored,
        ["Bash(cargo test:*)", "Bash(git status:*)", "Read(./src/**)"]
    );
    assert_eq!(rig.tower.floor_permissions_get(&rig.cwd).unwrap(), stored);
    let text = floor_toml(&rig);
    assert!(text.contains("[permissions]"), "{text}");
    assert!(
        text.contains(r#"allow = ["Bash(cargo test:*)", "Bash(git status:*)", "Read(./src/**)"]"#),
        "{text}"
    );
    assert!(rig.events.all().len() > before, "floor_changed is sent");

    // An empty list removes the table.
    rig.tower.floor_permissions_set(&rig.cwd, vec![]).unwrap();
    assert!(!floor_toml(&rig).contains("permissions"));
    // A hand edit shows at the next read.
    let edited = format!(
        "{}\n[permissions]\nallow = [\"Grep(todo)\"]\n",
        floor_toml(&rig)
    );
    fs::write(rig.cwd.join(".swarm/floor.toml"), edited).unwrap();
    assert_eq!(
        rig.tower.floor_permissions_get(&rig.cwd).unwrap(),
        ["Grep(todo)"]
    );
}

#[test]
fn a_bad_rule_refuses_the_whole_list_and_writes_nothing() {
    let rig = Rig::new();
    rig.create_floor();
    rig.tower
        .floor_permissions_set(&rig.cwd, rules(&["Bash(ls:*)"]))
        .unwrap();
    let before = floor_toml(&rig);
    let long = format!("Bash({})", "x".repeat(200));
    let bad = [
        "Bash",
        "Bash(*)",
        "Bash(:*)",
        "Bash()",
        "Edit(./x)",
        "Bash(a\nb)",
        "Bash(a\u{7}b)",
        // Rules that allow every command (H1).
        "Bash(* *)",
        "Bash( *)",
        "Bash(?*)",
        "Bash(ls) Bash(rm:*)",
        "Bash(ls:*))",
        "Bash(ls\u{2028}rm)",
        "Bash(ls\u{200b}:*)",
        "Read(//**)",
        "Read",
        "WebFetch(*)",
        "WebFetch(domain:*)",
        "Bash(git:*)",
        "Bash(sh:*)",
        "",
        "   ",
        long.as_str(),
    ];
    for rule in bad {
        let error = rig
            .tower
            .floor_permissions_set(&rig.cwd, rules(&["Read(./src/**)", rule]))
            .expect_err(rule)
            .to_string();
        assert!(error.contains("is refused"), "{rule:?}: {error}");
        assert_eq!(floor_toml(&rig), before, "{rule:?} changed the file");
    }
    assert_eq!(
        rig.tower.floor_permissions_get(&rig.cwd).unwrap(),
        ["Bash(ls:*)"]
    );
    // Too many rules.
    let many: Vec<String> = (0..101).map(|i| format!("Bash(cmd{i}:*)")).collect();
    let error = rig
        .tower
        .floor_permissions_set(&rig.cwd, many)
        .unwrap_err()
        .to_string();
    assert!(error.contains("limit is 100"), "{error}");
    assert_eq!(floor_toml(&rig), before);
}

#[test]
fn the_table_stays_when_the_brand_or_the_defaults_are_written() {
    let rig = Rig::new();
    rig.create_floor();
    rig.tower
        .floor_permissions_set(&rig.cwd, rules(&["Bash(cargo test:*)"]))
        .unwrap();
    // The defaults writer (a command of the operator).
    rig.tower
        .floor_defaults_set(
            &rig.cwd,
            FloorDefaults {
                model: Some("haiku".into()),
                ..FloorDefaults::default()
            },
        )
        .unwrap();
    assert_eq!(
        rig.tower.floor_permissions_get(&rig.cwd).unwrap(),
        ["Bash(cargo test:*)"]
    );
    // The brand writer (the logo flow and the setup agent) and a full rewrite of the store.
    let mut store = FloorStore::open(&rig.cwd).unwrap();
    store
        .set_brand(Brand {
            description: Some("A shop.".into()),
            logo_prompt: None,
        })
        .unwrap();
    let text = floor_toml(&rig);
    assert!(
        text.contains("[permissions]") && text.contains("A shop."),
        "{text}"
    );
    assert!(text.contains("haiku"), "{text}");
    // The other way: the rules writer keeps the brand and the defaults.
    store
        .set_permissions(tower_core::model::FloorPermissions {
            allow: rules(&["Read(./src/**)"]),
        })
        .unwrap();
    let text = floor_toml(&rig);
    assert!(
        text.contains("A shop.") && text.contains("haiku") && text.contains("Read(./src/**)"),
        "{text}"
    );
    let reopened = FloorStore::open(&rig.cwd).unwrap();
    assert_eq!(reopened.floor().permissions.allow, ["Read(./src/**)"]);
    assert_eq!(
        reopened.floor().brand.description.as_deref(),
        Some("A shop.")
    );
    assert_eq!(reopened.floor().defaults.model.as_deref(), Some("haiku"));
}

#[test]
fn each_claude_session_gets_the_valid_rules_of_the_floor_and_never_a_hand_written_bad_one() {
    let rig = Rig::new();
    rig.create_floor();
    rig.add_employee("eng", Some("pm"));
    // A hand edit of the file with a good rule and two bad ones.
    let text = format!(
        "{}\n[permissions]\nallow = [\"Bash(cargo test:*)\", \"Bash\", \"Bash(*)\", \"Read(./src/**)\", \"Read\", \"Bash(cargo test:*)\"]\n",
        floor_toml(&rig)
    );
    fs::write(rig.cwd.join(".swarm/floor.toml"), text).unwrap();
    rig.start("pm");
    let call = rig.calls().into_iter().find(|c| c.slug == "pm").unwrap();
    assert_eq!(call.floor_allow, ["Bash(cargo test:*)", "Read(./src/**)"]);
    // A rule that the operator sets in Settings applies at the next start of a session.
    rig.tower
        .floor_permissions_set(&rig.cwd, rules(&["Grep(todo)"]))
        .unwrap();
    rig.start("eng");
    let call = rig.calls().into_iter().find(|c| c.slug == "eng").unwrap();
    assert_eq!(call.floor_allow, ["Grep(todo)"]);
    // The setup session writes `.swarm/`, has no app server, and gets no rule.
    rig.tower
        .setup_start(&rig.cwd, tower_core::model::CliKind::Claude, 24, 80)
        .unwrap();
    let setup = rig.calls().into_iter().find(|c| c.slug == "setup").unwrap();
    assert!(setup.floor_allow.is_empty() && setup.mcp.is_none() && !setup.worktrees);
    assert_eq!(setup.permission, PermissionPolicy::EditInWorktree);
}

#[test]
fn a_session_gets_the_worktree_flag_only_in_a_git_repository_that_has_a_commit() {
    let rig = Rig::new();
    rig.create_floor();
    rig.add_editor("eng", Some("pm"));
    rig.start("eng");
    assert!(
        !rig.calls()
            .iter()
            .find(|c| c.slug == "eng")
            .unwrap()
            .worktrees
    );
    let rig = Rig::new();
    rig.create_floor();
    app_support::init_repo(&rig.cwd);
    rig.add_editor("eng", Some("pm"));
    rig.start("eng");
    assert!(
        rig.calls()
            .iter()
            .find(|c| c.slug == "eng")
            .unwrap()
            .worktrees
    );
}

#[test]
fn the_setup_agent_is_told_not_to_write_the_table() {
    for text in [
        "Do not change the lines `version`, `name` and the tables `[defaults]` and `[permissions]`.",
        "Do NOT write this table",
        "`[permissions]` in `floor.toml`",
        "Worktrees and merge",
        "task_merge(task)",
        "`edit-in-worktree`, in a git repository",
    ] {
        assert!(
            FLOOR_INIT
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .contains(&text.split_whitespace().collect::<Vec<_>>().join(" ")),
            "{text}"
        );
    }
}

fn floor_events(rig: &Rig) -> usize {
    rig.events
        .all()
        .iter()
        .filter(|ev| matches!(ev, app_support::Ev::Floor(_)))
        .count()
}

/// One turn of the setup session. Returns after the `floor_changed` of the turn end.
fn setup_turn(rig: &Rig, id: &str) {
    let before = floor_events(rig);
    rig.tower.debug_paste(id, "next").unwrap();
    app_support::wait_for("the floor-changed of a turn", || floor_events(rig) > before);
}

fn notices(rig: &Rig) -> Vec<String> {
    app_support::lock(&rig.events.notices)
        .iter()
        .filter(|(_, kind, _)| kind == "permissions_restored")
        .map(|(_, _, text)| text.clone())
        .collect()
}

/// The setup agent runs with `acceptEdits` and can write `floor.toml` (finding H1). The app puts
/// the old `[permissions]` value back after the turn and tells the operator.
#[test]
fn the_app_puts_back_the_permissions_that_the_setup_agent_changed() {
    let rig = Rig::new();
    rig.create_floor();
    rig.tower
        .floor_permissions_set(&rig.cwd, rules(&["Bash(cargo test:*)"]))
        .unwrap();
    let id = rig
        .tower
        .setup_start(&rig.cwd, tower_core::model::CliKind::Claude, 24, 80)
        .unwrap()
        .session_id;
    rig.wait_first_turn(&id);
    // One floor-changed is from `floor_permissions_set`, and one is from the end of the first turn.
    app_support::wait_for("the floor-changed of the first turn", || {
        floor_events(&rig) >= 2
    });
    assert!(notices(&rig).is_empty());

    // The agent changes the table, and it also writes the brand (an allowed change).
    let file = rig.cwd.join(".swarm/floor.toml");
    let mut table: toml::Table = toml::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    table.insert(
        "permissions".into(),
        toml::from_str("allow = [\"Bash\", \"Bash(sh:*)\"]").unwrap(),
    );
    table.insert(
        "brand".into(),
        toml::from_str("description = \"A web shop.\"").unwrap(),
    );
    fs::write(&file, toml::to_string(&table).unwrap()).unwrap();
    setup_turn(&rig, &id);

    assert_eq!(
        notices(&rig),
        [
            "The setup agent tried to change the commands with no question. The app put the old list back."
        ]
    );
    assert_eq!(
        rig.tower.floor_permissions_get(&rig.cwd).unwrap(),
        ["Bash(cargo test:*)"]
    );
    let text = floor_toml(&rig);
    assert!(
        text.contains("Bash(cargo test:*)") && !text.contains("sh:*"),
        "{text}"
    );
    assert!(text.contains("A web shop."), "the brand stays: {text}");

    // The agent deletes the table: the old value comes back too.
    let mut table: toml::Table = toml::from_str(&floor_toml(&rig)).unwrap();
    table.remove("permissions");
    fs::write(&file, toml::to_string(&table).unwrap()).unwrap();
    setup_turn(&rig, &id);
    assert_eq!(notices(&rig).len(), 2);
    assert!(floor_toml(&rig).contains("Bash(cargo test:*)"));

    // A turn with no change gives no notice.
    setup_turn(&rig, &id);
    assert_eq!(notices(&rig).len(), 2);

    // The operator changes the list in Settings while the setup runs: that is kept.
    rig.tower
        .floor_permissions_set(&rig.cwd, rules(&["Bash(ls:*)"]))
        .unwrap();
    setup_turn(&rig, &id);
    assert_eq!(notices(&rig).len(), 2);
    assert_eq!(
        rig.tower.floor_permissions_get(&rig.cwd).unwrap(),
        ["Bash(ls:*)"]
    );
    rig.stop_and_wait(&id);
}

/// A table that the agent adds to a floor that had none is removed again.
#[test]
fn the_app_removes_a_permissions_table_that_the_setup_agent_added() {
    let rig = Rig::new();
    rig.create_floor();
    let id = rig
        .tower
        .setup_start(&rig.cwd, tower_core::model::CliKind::Claude, 24, 80)
        .unwrap()
        .session_id;
    rig.wait_first_turn(&id);
    app_support::wait_for("the floor-changed of the first turn", || {
        floor_events(&rig) >= 1
    });
    let edited = format!("{}\n[permissions]\nallow = [\"Read\"]\n", floor_toml(&rig));
    fs::write(rig.cwd.join(".swarm/floor.toml"), edited).unwrap();
    setup_turn(&rig, &id);
    assert_eq!(notices(&rig).len(), 1);
    assert!(!floor_toml(&rig).contains("permissions"));
    assert!(
        rig.tower
            .floor_permissions_get(&rig.cwd)
            .unwrap()
            .is_empty()
    );
    rig.stop_and_wait(&id);
}
