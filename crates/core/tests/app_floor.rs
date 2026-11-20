//! Floors, employees and programs of the `Tower` (test 1 of the task, and the config).

mod app_support;

use app_support::{Rig, Shared, bin};
use std::path::Path;
use std::sync::Arc;
use tower_core::app::{Tower, TowerError};
use tower_core::model::{CliKind, PermissionPolicy};
use tower_core::store::FloorStore;

#[test]
fn create_writes_two_default_employees_and_a_new_tower_opens_the_floor() {
    let rig = Rig::new();
    let view = rig.create_floor();
    assert_eq!(view.name, "Test Floor");
    assert_eq!(view.path, rig.cwd);
    let slugs: Vec<&str> = view.employees.iter().map(|e| e.slug.as_str()).collect();
    assert_eq!(slugs, ["ceo", "pm"]);
    assert!(view.sessions.is_empty());
    assert!(view.employee_errors.is_empty());

    // The files are on disk.
    for file in ["ceo.md", "pm.md"] {
        let text = std::fs::read_to_string(rig.cwd.join(".swarm/employees").join(file)).unwrap();
        assert!(text.starts_with("+++\n"), "{file}: {text}");
    }
    let store = FloorStore::open(&rig.cwd).unwrap();
    let loaded = store.read_employees().unwrap();
    assert!(loaded.errors.is_empty());
    let ceo = &loaded.employees[0];
    assert_eq!((ceo.role.as_str(), ceo.cli), ("CEO", CliKind::Claude));
    assert_eq!(ceo.permission, PermissionPolicy::ReadOnly);
    assert!(!ceo.role_text.trim().is_empty());
    let pm = &loaded.employees[1];
    assert_eq!(pm.manager.as_deref(), Some("ceo"));
    assert_eq!(pm.permission, PermissionPolicy::ReadOnly);
    drop(store);

    // The list has the floor.
    let list = rig.tower.floor_list().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].path, rig.cwd);
    assert_eq!(list[0].name.as_deref(), Some("Test Floor"));
    assert!(!list[0].missing);

    // A second Tower on the same config opens the same floor and has it in its list.
    let socket = rig.root.path().join("second.sock");
    let second = Rig::make_tower(&rig.config, &socket, &Arc::new(Shared::default()), |_| {});
    assert_eq!(second.floor_list().unwrap().len(), 1);
    let again = second.floor_open(&rig.cwd).unwrap();
    assert_eq!(again.name, "Test Floor");
    assert_eq!(again.employees, view.employees);
}

#[test]
fn open_without_a_floor_says_no_floor_and_create_twice_is_refused() {
    let rig = Rig::new();
    let error = rig.tower.floor_open(&rig.cwd).unwrap_err();
    assert!(
        matches!(error, TowerError::NoFloor(_)),
        "wrong error: {error:?}"
    );
    assert!(error.to_string().starts_with("NO_FLOOR:"), "{error}");
    // A failed open does not put the folder in the list.
    assert!(rig.tower.floor_list().unwrap().is_empty());

    rig.create_floor();
    let error = rig.tower.floor_create(&rig.cwd, "Again").unwrap_err();
    assert!(error.to_string().contains("already exists"), "{error}");
    let error = rig
        .tower
        .floor_create(&rig.cwd.join("no-such-folder"), "X")
        .unwrap_err();
    assert!(error.to_string().contains("does not exist"), "{error}");
}

#[test]
fn save_and_remove_employee_tell_the_observer_and_forget_keeps_the_files() {
    let rig = Rig::new();
    let mut view = rig.create_floor();
    let mut employee = view.employees[1].clone();
    employee.slug = "eng-1".into();
    employee.name = "Eli".into();
    employee.role = "Engineer".into();
    employee.cli = CliKind::Codex;
    employee.role_text = "Write code.\n".into();
    view = rig.tower.employee_save(&rig.cwd, employee.clone()).unwrap();
    assert_eq!(view.employees.len(), 3);
    assert!(view.employees.contains(&employee));
    assert!(
        rig.events
            .all()
            .iter()
            .any(|ev| matches!(ev, app_support::Ev::Floor(p) if *p == rig.cwd))
    );

    let error = rig.tower.employee_save(&rig.cwd, {
        let mut bad = employee.clone();
        bad.slug = "Bad Slug".into();
        bad
    });
    assert!(error.unwrap_err().to_string().contains("not a valid slug"));

    view = rig.tower.employee_remove(&rig.cwd, "eng-1").unwrap();
    assert_eq!(view.employees.len(), 2);
    assert!(!rig.cwd.join(".swarm/employees/eng-1.md").exists());

    rig.tower.floor_forget(&rig.cwd).unwrap();
    assert!(rig.tower.floor_list().unwrap().is_empty());
    assert!(
        rig.cwd.join(".swarm/floor.toml").exists(),
        "forget must not delete files"
    );
}

#[test]
fn programs_overrides_are_checked_stored_and_read_by_a_new_tower() {
    let rig = Rig::new();
    let fake = bin("fake-cli");
    let got = rig.tower.programs_get();
    assert_eq!(got.claude.as_deref(), Some(fake.as_path()));
    assert_eq!(got.node.as_deref(), Some(Path::new("/bin/true")));
    assert_eq!(got.hook.as_deref(), Some(bin("tower-hook").as_path()));

    // A path that is not an executable file is refused, and nothing changes.
    let error = rig
        .tower
        .programs_set("codex", "/no/such/program")
        .unwrap_err();
    assert!(
        error.to_string().contains("not an executable file"),
        "{error}"
    );
    assert!(rig.tower.programs_set("rm", "/bin/true").is_err());
    assert_eq!(
        rig.tower.programs_get().codex.as_deref(),
        Some(fake.as_path())
    );

    // A new Tower with the same config folder has the overrides (they are in programs.toml).
    let socket = rig.root.path().join("second.sock");
    let mut cfg = tower_core::app::TowerConfig::new(&rig.config);
    cfg.socket_path = Some(socket);
    cfg.hook_program = Some(bin("tower-hook"));
    cfg.resolver = tower_core::programs::Resolver {
        shell: None,
        known_dirs: vec![],
        shell_timeout: std::time::Duration::from_secs(1),
    };
    let second = Tower::new(cfg).unwrap();
    assert_eq!(
        second.programs_get().claude.as_deref(),
        Some(fake.as_path())
    );
    // An empty path removes the override. With no shell and no folder, the program is then missing.
    let after = second.programs_set("claude", "").unwrap();
    assert_eq!(after.claude, None);
}

#[test]
fn the_hook_program_is_found_next_to_the_workspace_build_when_not_given() {
    // The test binary is in `target/<profile>/deps`, so "next to the exe" fails and the
    // workspace `target/debug` (or `release`) is used.
    let hook = bin("tower-hook");
    let rig = Rig::with(|cfg| cfg.hook_program = None);
    let found = rig.tower.programs_get().hook.expect("the hook is found");
    assert_eq!(
        std::fs::canonicalize(found).unwrap(),
        std::fs::canonicalize(hook).unwrap()
    );
}
