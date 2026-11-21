//! The rules for commands with no question (milestone M3, moved on 2026-11-21): the operator lists
//! Claude permission rules for a floor. They are in the app config (table `[floor_permissions]`
//! of `programs.toml`), not in the project folder, so an agent of the floor cannot change them.
//! The commands `floor_permissions_get`, `floor_permissions_set` and `floor_permissions_proposed`,
//! the checks, the old table of `floor.toml`, and the rules that reach each Claude session.

mod app_support;

use app_support::Rig;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tower_core::app::FLOOR_INIT;
use tower_core::model::{Brand, CliKind, FloorDefaults, PermissionPolicy};
use tower_core::store::FloorStore;

const WARNING: &str =
    "This rule lets each agent of the floor run the project's own code with no question.";

fn floor_toml(rig: &Rig) -> String {
    fs::read_to_string(rig.cwd.join(".swarm/floor.toml")).unwrap()
}

fn rules(list: &[&str]) -> Vec<String> {
    list.iter().map(|r| (*r).to_owned()).collect()
}

fn config_file(rig: &Rig) -> PathBuf {
    rig.config.join("programs.toml")
}

fn config_text(rig: &Rig) -> String {
    fs::read_to_string(config_file(rig)).unwrap_or_default()
}

/// The `[floor_permissions]` table of the app config, parsed.
fn stored(rig: &Rig) -> toml::Table {
    let table: toml::Table = toml::from_str(&config_text(rig)).unwrap();
    table
        .get("floor_permissions")
        .and_then(|v| v.as_table())
        .cloned()
        .unwrap_or_default()
}

fn stored_rules(rig: &Rig, floor: &Path) -> Option<Vec<String>> {
    stored(rig).get(floor.to_str().unwrap()).map(|v| {
        v.as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_str().unwrap().to_owned())
            .collect()
    })
}

/// A hand edit of `floor.toml`, as an agent can make it: add the old `[permissions]` table.
fn write_old_table(rig: &Rig, allow: &[&str]) {
    let list = allow
        .iter()
        .map(|r| format!("{r:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let edited = format!("{}\n[permissions]\nallow = [{list}]\n", floor_toml(rig));
    fs::write(rig.cwd.join(".swarm/floor.toml"), edited).unwrap();
}

fn call_of(rig: &Rig, slug: &str) -> app_support::Call {
    rig.calls()
        .into_iter()
        .rev()
        .find(|c| c.slug == slug)
        .unwrap()
}

#[test]
fn the_operator_sets_and_reads_the_rules_in_the_app_config() {
    let rig = Rig::new();
    rig.create_floor();
    // A new floor has no entry: the list is empty, and the app config has no file or no entry.
    assert_eq!(
        rig.tower.floor_permissions_get(&rig.cwd).unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(stored_rules(&rig, &rig.cwd), None);

    let before = rig.events.all().len();
    let saved = rig
        .tower
        .floor_permissions_set(
            &rig.cwd,
            rules(&[
                "Bash(rtk cargo test:*)",
                " Bash(git status) ",
                "Read(./src/**)",
                "Bash(rtk cargo test:*)",
            ]),
        )
        .unwrap();
    // The text is trimmed and a duplicate is dropped.
    assert_eq!(
        saved.allow,
        [
            "Bash(rtk cargo test:*)",
            "Bash(git status)",
            "Read(./src/**)"
        ]
    );
    // One warning for each rule that runs the code of the project, and one for the path rule with
    // a wildcard, with the rule in front. The exact rule `Bash(git status)` has none.
    assert_eq!(
        saved.warnings,
        [
            format!("Bash(rtk cargo test:*): {WARNING}"),
            format!("Read(./src/**): {PATH_WILDCARD_WARNING}"),
        ]
    );
    assert_eq!(
        rig.tower.floor_permissions_get(&rig.cwd).unwrap(),
        saved.allow
    );
    // The rules are in the app config, under the real path of the floor. Not in floor.toml.
    assert_eq!(stored_rules(&rig, &rig.cwd).unwrap(), saved.allow);
    assert!(!floor_toml(&rig).contains("permissions"));
    assert!(!floor_toml(&rig).contains("cargo"));
    assert!(rig.events.all().len() > before, "floor_changed is sent");

    // An empty list is an entry too: the operator chose no rule.
    let saved = rig.tower.floor_permissions_set(&rig.cwd, vec![]).unwrap();
    assert!(saved.allow.is_empty() && saved.warnings.is_empty());
    assert_eq!(stored_rules(&rig, &rig.cwd), Some(vec![]));
    assert!(
        rig.tower
            .floor_permissions_get(&rig.cwd)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_app_config_file_keeps_mode_0600_and_the_other_tables() {
    let rig = Rig::new();
    rig.create_floor();
    fs::create_dir_all(&rig.config).unwrap();
    // The tables that the other writers own. The values are fake.
    fs::write(
        config_file(&rig),
        "[programs]\nclaude = \"/opt/fake/claude\"\n\n[openai]\nkey = \"fake-key-for-the-test\"\nimage_model = \"fake-image\"\n\n[models]\nclaude = [\"fake-model\"]\n",
    )
    .unwrap();
    rig.tower
        .floor_permissions_set(&rig.cwd, rules(&["Bash(ls:*)"]))
        .unwrap();
    let table: toml::Table = toml::from_str(&config_text(&rig)).unwrap();
    assert_eq!(
        table["programs"]["claude"].as_str(),
        Some("/opt/fake/claude")
    );
    assert_eq!(table["openai"]["image_model"].as_str(), Some("fake-image"));
    assert_eq!(
        table["openai"]["key"].as_str(),
        Some("fake-key-for-the-test")
    );
    assert_eq!(table["models"]["claude"][0].as_str(), Some("fake-model"));
    assert_eq!(stored_rules(&rig, &rig.cwd).unwrap(), ["Bash(ls:*)"]);
    let mode = |file: &Path| fs::metadata(file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&config_file(&rig)), 0o600);

    // The other writers keep the rules of the floor. The settings writer reads the file again.
    rig.tower
        .settings_set(Some("another-fake-key".to_owned()), None)
        .unwrap();
    assert_eq!(stored_rules(&rig, &rig.cwd).unwrap(), ["Bash(ls:*)"]);
    assert_eq!(mode(&config_file(&rig)), 0o600);
    // And the rules writer keeps the logo key.
    rig.tower
        .floor_permissions_set(&rig.cwd, rules(&["Grep(todo)"]))
        .unwrap();
    let table: toml::Table = toml::from_str(&config_text(&rig)).unwrap();
    assert_eq!(table["openai"]["key"].as_str(), Some("another-fake-key"));
    assert_eq!(mode(&config_file(&rig)), 0o600);
}

#[test]
fn two_floors_keep_separate_lists_and_forget_removes_the_entry() {
    let rig = Rig::new();
    rig.create_floor();
    // A path with a space, dots, a quote and Unicode. A TOML key must hold all of them.
    let other = rig.root.path().join("my project.v2 \u{e9}\u{1f4a1} \"q\"");
    fs::create_dir(&other).unwrap();
    let other = fs::canonicalize(&other).unwrap();
    rig.tower.floor_create(&other, "Other").unwrap();

    rig.tower
        .floor_permissions_set(&rig.cwd, rules(&["Bash(ls:*)"]))
        .unwrap();
    rig.tower
        .floor_permissions_set(&other, rules(&["Grep(todo)", "WebSearch"]))
        .unwrap();
    assert_eq!(
        rig.tower.floor_permissions_get(&rig.cwd).unwrap(),
        ["Bash(ls:*)"]
    );
    assert_eq!(
        rig.tower.floor_permissions_get(&other).unwrap(),
        ["Grep(todo)", "WebSearch"]
    );
    assert_eq!(stored(&rig).len(), 2);
    assert_eq!(
        stored_rules(&rig, &other).unwrap(),
        ["Grep(todo)", "WebSearch"]
    );

    // A path that is a link to the floor gives the same entry (the key is the real path).
    let link = rig.root.path().join("link");
    std::os::unix::fs::symlink(&rig.cwd, &link).unwrap();
    assert_eq!(
        rig.tower.floor_permissions_get(&link).unwrap(),
        ["Bash(ls:*)"]
    );

    // `floor_forget` removes the entry of that floor and no other.
    rig.tower.floor_forget(&other).unwrap();
    assert_eq!(stored_rules(&rig, &other), None);
    assert_eq!(stored_rules(&rig, &rig.cwd).unwrap(), ["Bash(ls:*)"]);
    // A floor that is added again starts with no rule.
    rig.tower.floor_open(&other).unwrap();
    assert!(rig.tower.floor_permissions_get(&other).unwrap().is_empty());
    // The last entry goes, and the table goes with it.
    rig.tower.floor_forget(&rig.cwd).unwrap();
    assert!(!config_text(&rig).contains("floor_permissions"));
}

const PATH_WILDCARD_WARNING: &str = "This rule lets each agent of the floor read every file that the pattern matches, with no question. A pattern with a wildcard can match more files than you expect.";

const WEB_FETCH_WARNING: &str = "This rule lets each agent of the floor get pages from this host with no question. The app does not check where the host name points.";

const WILDCARD_WARNING: &str = "This rule lets each agent of the floor run this program with any arguments and no question. Many programs can write files or run other commands through their options.";

/// The check is informed consent, not a wall: a wildcard rule of a plain program is stored with a
/// warning, and an exact rule is stored with none. A build tool keeps the stronger text.
#[test]
fn a_wildcard_rule_of_a_plain_program_has_a_warning_and_an_exact_rule_has_none() {
    let rig = Rig::new();
    rig.create_floor();
    let saved = rig
        .tower
        .floor_permissions_set(
            &rig.cwd,
            rules(&[
                "Bash(ls:*)",
                "Bash(rtk git status)",
                "Bash(sort *)",
                "Bash(cargo fmt --check)",
                "Bash(rtk cargo test:*)",
                "Bash(cat README.md)",
                "Read(./src/**)",
                "Read(./README.md)",
                "WebFetch(domain:example.com)",
            ]),
        )
        .unwrap();
    assert_eq!(saved.allow.len(), 9);
    assert_eq!(
        saved.warnings,
        [
            format!("Bash(ls:*): {WILDCARD_WARNING}"),
            format!("Bash(sort *): {WILDCARD_WARNING}"),
            format!("Bash(cargo fmt --check): {WARNING}"),
            format!("Bash(rtk cargo test:*): {WARNING}"),
            format!("Read(./src/**): {PATH_WILDCARD_WARNING}"),
            format!("WebFetch(domain:example.com): {WEB_FETCH_WARNING}"),
        ]
    );
    // A shell character is refused, and nothing is stored for it.
    let error = rig
        .tower
        .floor_permissions_set(&rig.cwd, rules(&["Bash(ls && rm)"]))
        .unwrap_err()
        .to_string();
    assert!(error.contains("has the shell character &"), "{error}");
    assert_eq!(
        rig.tower.floor_permissions_get(&rig.cwd).unwrap(),
        saved.allow
    );
    // A git rule with a wildcard is refused, and nothing is stored for it.
    let error = rig
        .tower
        .floor_permissions_set(&rig.cwd, rules(&["Bash(git log:*)"]))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("A git rule must be the exact command"),
        "{error}"
    );
    assert_eq!(
        rig.tower.floor_permissions_get(&rig.cwd).unwrap(),
        saved.allow
    );
}

#[test]
fn a_bad_rule_refuses_the_whole_list_and_writes_nothing() {
    let rig = Rig::new();
    rig.create_floor();
    rig.tower
        .floor_permissions_set(&rig.cwd, rules(&["Bash(ls:*)"]))
        .unwrap();
    let before = (floor_toml(&rig), config_text(&rig));
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
        // Rules that the second review passed (finding H1 again).
        "Bash(rtk proxy:*)",
        "Bash(busybox sh:*)",
        "Bash(find:*)",
        "Bash(git rebase:*)",
        "Bash(git log --output=x:*)",
        // The third review: a git rule with a wildcard, and programs that run commands.
        "Bash(git log:*)",
        "Bash(rtk git diff:*)",
        "Bash(git log --output=x)",
        "Bash(sqlite3:*)",
        "Bash(man:*)",
        "Read(.swarm/state/**)",
        "WebFetch(domain:169.254.169.254)",
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
        assert_eq!((floor_toml(&rig), config_text(&rig)), before, "{rule:?}");
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
    assert_eq!((floor_toml(&rig), config_text(&rig)), before);
}

/// The old table of `floor.toml` is a proposal, never a list that the app uses. `set` removes it
/// and keeps the rest of the file.
#[test]
fn the_old_table_is_only_a_proposal_and_set_removes_it_and_keeps_the_rest() {
    let rig = Rig::new();
    rig.create_floor();
    rig.tower
        .floor_defaults_set(
            &rig.cwd,
            FloorDefaults {
                model: Some("haiku".into()),
                ..FloorDefaults::default()
            },
        )
        .unwrap();
    FloorStore::open(&rig.cwd)
        .unwrap()
        .set_brand(Brand {
            description: Some("A shop.".into()),
            logo_prompt: None,
        })
        .unwrap();
    // No old table: nothing to propose.
    assert!(
        rig.tower
            .floor_permissions_proposed(&rig.cwd)
            .unwrap()
            .is_empty()
    );
    // A hand edit (or an agent) writes the old table, with good rules, bad rules and a duplicate.
    write_old_table(
        &rig,
        &[
            "Bash(cargo test:*)",
            "Bash",
            "Bash(*)",
            "Bash(sh:*)",
            "Bash(rtk proxy:*)",
            "Read(./src/**)",
            "Bash(cargo test:*)",
        ],
    );
    // The app does not use it: the list is empty. It is only proposed, and only the rules that
    // pass the check.
    assert!(
        rig.tower
            .floor_permissions_get(&rig.cwd)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        rig.tower.floor_permissions_proposed(&rig.cwd).unwrap(),
        ["Bash(cargo test:*)", "Read(./src/**)"]
    );
    assert_eq!(stored_rules(&rig, &rig.cwd), None);
    assert!(
        floor_toml(&rig).contains("[permissions]"),
        "the file is not changed"
    );

    // The operator takes the proposal over with one click: the UI calls `set` with the list.
    let proposed = rig.tower.floor_permissions_proposed(&rig.cwd).unwrap();
    let saved = rig.tower.floor_permissions_set(&rig.cwd, proposed).unwrap();
    assert_eq!(saved.allow, ["Bash(cargo test:*)", "Read(./src/**)"]);
    // The build tool rule and the path rule with a wildcard each have a warning.
    assert_eq!(saved.warnings.len(), 2);
    let text = floor_toml(&rig);
    assert!(
        !text.contains("permissions") && !text.contains("cargo"),
        "{text}"
    );
    assert!(
        text.contains("A shop.") && text.contains("haiku") && text.contains("Test Floor"),
        "the rest of the file stays: {text}"
    );
    let reopened = FloorStore::open(&rig.cwd).unwrap();
    assert_eq!(reopened.floor().name, "Test Floor");
    assert_eq!(reopened.floor().defaults.model.as_deref(), Some("haiku"));
    assert_eq!(
        reopened.floor().brand.description.as_deref(),
        Some("A shop.")
    );

    // Once the app config has an entry, a table that an agent writes later is not proposed.
    write_old_table(&rig, &["Bash(npm test:*)"]);
    assert!(
        rig.tower
            .floor_permissions_proposed(&rig.cwd)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        rig.tower.floor_permissions_get(&rig.cwd).unwrap(),
        ["Bash(cargo test:*)", "Read(./src/**)"]
    );
    // An entry with an empty list is a decision too.
    rig.tower.floor_permissions_set(&rig.cwd, vec![]).unwrap();
    write_old_table(&rig, &["Bash(npm test:*)"]);
    assert!(
        rig.tower
            .floor_permissions_proposed(&rig.cwd)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn each_claude_session_gets_the_rules_of_the_app_config_and_never_the_old_table() {
    let rig = Rig::new();
    rig.create_floor();
    rig.add_employee("eng", Some("pm"));
    // The old table has good rules. It changes nothing for the sessions.
    write_old_table(&rig, &["Bash(cargo test:*)", "Read(./src/**)"]);
    let pm = rig.start("pm").session_id;
    assert!(call_of(&rig, "pm").floor_allow.is_empty());

    // The operator sets the rules in Settings: they apply at the next start of a session.
    rig.tower
        .floor_permissions_set(&rig.cwd, rules(&["Bash(rtk cargo test:*)", "Grep(todo)"]))
        .unwrap();
    rig.start("eng");
    assert_eq!(
        call_of(&rig, "eng").floor_allow,
        ["Bash(rtk cargo test:*)", "Grep(todo)"]
    );

    // An agent writes the table while a session runs. The next start has the same rules.
    write_old_table(
        &rig,
        &["Bash(npm publish:*)", "WebFetch(domain:evil.example)"],
    );
    rig.stop_and_wait(&pm);
    rig.start("pm");
    assert_eq!(
        call_of(&rig, "pm").floor_allow,
        ["Bash(rtk cargo test:*)", "Grep(todo)"]
    );

    // A rule in the app config that the check refuses (a hand edit of the config file) is
    // dropped at the start.
    let mut table: toml::Table = toml::from_str(&config_text(&rig)).unwrap();
    table.insert(
        "floor_permissions".into(),
        toml::from_str(&format!(
            "{:?} = [\"Bash(ls:*)\", \"Bash(sh:*)\", \"Bash\", \"Bash(ls:*)\"]",
            rig.cwd.to_str().unwrap()
        ))
        .unwrap(),
    );
    fs::write(config_file(&rig), toml::to_string(&table).unwrap()).unwrap();
    rig.add_employee("qa", Some("pm"));
    rig.start("qa");
    assert_eq!(call_of(&rig, "qa").floor_allow, ["Bash(ls:*)"]);

    // The setup session writes `.swarm/`, has no app server, and gets no rule.
    rig.tower
        .setup_start(&rig.cwd, CliKind::Claude, 24, 80)
        .unwrap();
    let setup = call_of(&rig, "setup");
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
fn the_setup_agent_is_told_that_the_operator_sets_the_rules_in_settings() {
    let flat = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let init = flat(FLOOR_INIT);
    for text in [
        "The operator sets these rules in Settings. They are not in the project folder.",
        "Do NOT write them anywhere",
        "Worktrees and merge",
        "task_merge(task)",
        "`edit-in-worktree`, in a git repository",
        "Add or change only the table `[brand]`.",
    ] {
        assert!(init.contains(&flat(text)), "{text}");
    }
    // The setup agent has no reason to keep or to know the old table.
    assert!(!FLOOR_INIT.contains("[permissions]"));
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

/// The setup agent runs with `acceptEdits` and can write `floor.toml` (finding H1). The rules are
/// not in that file any more, so the agent cannot change them. The app does not put anything back,
/// and it sends no notice.
#[test]
fn a_table_that_the_setup_agent_writes_changes_nothing_at_the_next_start() {
    let rig = Rig::new();
    rig.create_floor();
    rig.add_employee("eng", Some("pm"));
    rig.tower
        .floor_permissions_set(&rig.cwd, rules(&["Bash(ls:*)"]))
        .unwrap();
    let id = rig
        .tower
        .setup_start(&rig.cwd, CliKind::Claude, 24, 80)
        .unwrap()
        .session_id;
    rig.wait_first_turn(&id);
    app_support::wait_for("the floor-changed of the first turn", || {
        floor_events(&rig) >= 2
    });

    // The agent writes the old table, and also a brand (an allowed change).
    let file = rig.cwd.join(".swarm/floor.toml");
    let mut table: toml::Table = toml::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    table.insert(
        "permissions".into(),
        toml::from_str("allow = [\"Bash\", \"Bash(sh:*)\", \"Bash(cargo publish:*)\"]").unwrap(),
    );
    table.insert(
        "brand".into(),
        toml::from_str("description = \"A web shop.\"").unwrap(),
    );
    fs::write(&file, toml::to_string(&table).unwrap()).unwrap();
    setup_turn(&rig, &id);

    // No notice, no put-back: the file is as the agent wrote it.
    assert!(
        app_support::lock(&rig.events.notices)
            .iter()
            .all(|(_, kind, _)| kind != "permissions_restored")
    );
    let text = floor_toml(&rig);
    assert!(
        text.contains("cargo publish") && text.contains("A web shop."),
        "{text}"
    );
    // The list of the operator is the same, and the next start has it.
    assert_eq!(
        rig.tower.floor_permissions_get(&rig.cwd).unwrap(),
        ["Bash(ls:*)"]
    );
    rig.stop_and_wait(&id);
    rig.start("eng");
    assert_eq!(call_of(&rig, "eng").floor_allow, ["Bash(ls:*)"]);
    // The proposal is empty: the app config has an entry, so the operator decided already.
    assert!(
        rig.tower
            .floor_permissions_proposed(&rig.cwd)
            .unwrap()
            .is_empty()
    );
}

/// A floor with no entry in the app config and an old table that an agent wrote: no rule reaches
/// a session, and the table is only a proposal.
#[test]
fn an_old_table_that_the_setup_agent_adds_is_a_proposal_and_no_rule() {
    let rig = Rig::new();
    rig.create_floor();
    rig.add_employee("eng", Some("pm"));
    let id = rig
        .tower
        .setup_start(&rig.cwd, CliKind::Claude, 24, 80)
        .unwrap()
        .session_id;
    rig.wait_first_turn(&id);
    app_support::wait_for("the floor-changed of the first turn", || {
        floor_events(&rig) >= 1
    });
    write_old_table(&rig, &["Bash(cargo test:*)", "Bash(sh:*)"]);
    setup_turn(&rig, &id);
    rig.stop_and_wait(&id);
    rig.start("eng");
    assert!(call_of(&rig, "eng").floor_allow.is_empty());
    assert!(
        rig.tower
            .floor_permissions_get(&rig.cwd)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        rig.tower.floor_permissions_proposed(&rig.cwd).unwrap(),
        ["Bash(cargo test:*)"]
    );
}
