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
                "Read",
                "Bash(cargo test:*)",
            ]),
        )
        .unwrap();
    // The text is trimmed and a duplicate is dropped.
    assert_eq!(stored, ["Bash(cargo test:*)", "Bash(git status:*)", "Read"]);
    assert_eq!(rig.tower.floor_permissions_get(&rig.cwd).unwrap(), stored);
    let text = floor_toml(&rig);
    assert!(text.contains("[permissions]"), "{text}");
    assert!(
        text.contains(r#"allow = ["Bash(cargo test:*)", "Bash(git status:*)", "Read"]"#),
        "{text}"
    );
    assert!(rig.events.all().len() > before, "floor_changed is sent");

    // An empty list removes the table.
    rig.tower.floor_permissions_set(&rig.cwd, vec![]).unwrap();
    assert!(!floor_toml(&rig).contains("permissions"));
    // A hand edit shows at the next read.
    let edited = format!("{}\n[permissions]\nallow = [\"Grep\"]\n", floor_toml(&rig));
    fs::write(rig.cwd.join(".swarm/floor.toml"), edited).unwrap();
    assert_eq!(rig.tower.floor_permissions_get(&rig.cwd).unwrap(), ["Grep"]);
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
        "",
        "   ",
        long.as_str(),
    ];
    for rule in bad {
        let error = rig
            .tower
            .floor_permissions_set(&rig.cwd, rules(&["Read", rule]))
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
            allow: rules(&["Read"]),
        })
        .unwrap();
    let text = floor_toml(&rig);
    assert!(
        text.contains("A shop.") && text.contains("haiku") && text.contains("Read"),
        "{text}"
    );
    let reopened = FloorStore::open(&rig.cwd).unwrap();
    assert_eq!(reopened.floor().permissions.allow, ["Read"]);
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
        "{}\n[permissions]\nallow = [\"Bash(cargo test:*)\", \"Bash\", \"Bash(*)\", \"Read\", \"Bash(cargo test:*)\"]\n",
        floor_toml(&rig)
    );
    fs::write(rig.cwd.join(".swarm/floor.toml"), text).unwrap();
    rig.start("pm");
    let call = rig.calls().into_iter().find(|c| c.slug == "pm").unwrap();
    assert_eq!(call.floor_allow, ["Bash(cargo test:*)", "Read"]);
    // A rule that the operator sets in Settings applies at the next start of a session.
    rig.tower
        .floor_permissions_set(&rig.cwd, rules(&["Grep"]))
        .unwrap();
    rig.start("eng");
    let call = rig.calls().into_iter().find(|c| c.slug == "eng").unwrap();
    assert_eq!(call.floor_allow, ["Grep"]);
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
