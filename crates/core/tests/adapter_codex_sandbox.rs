//! The Codex adapter with the sandbox switch (`StartContext::codex_sandbox`, decision O7).
//!
//! The exact vectors are snapshots. The last test asks the real `codex` what sandbox it would
//! use for those flags (`codex debug prompt-input` renders the sandbox and starts no model). It
//! skips with a message when `codex` is not installed. The old snapshots of the off case stay in
//! `adapter_snapshot.rs`.

mod adapter_support;

use adapter_support::*;
use std::ffi::OsString;
use tempfile::TempDir;
use tower_core::adapter::{AdapterError, CliAdapter, CodexAdapter, StartContext};
use tower_core::env::current_env;
use tower_core::model::{CliKind, PermissionPolicy};

const OFF_TAIL: [&str; 4] = ["-a", "on-request", "-s", "danger-full-access"];

fn ctx(policy: PermissionPolicy, worktrees: bool, sandbox: bool) -> StartContext {
    let mut ctx = fixed_ctx(CliKind::Codex, policy);
    ctx.worktrees = worktrees;
    ctx.codex_sandbox = sandbox;
    ctx
}

fn args_of(ctx: &StartContext) -> Vec<String> {
    args_text(&CodexAdapter.start(ctx).expect("plan").spec.args)
}

/// The arguments from the first `-a` up to the message (the last argument).
fn permission_part(args: &[String]) -> Vec<String> {
    let from = args.iter().position(|a| a == "-a").expect("an -a flag");
    args[from..args.len() - 1].to_vec()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

const PROFILE: &str = "default_permissions=\"agent_tower\"";
const EXTENDS: &str = "permissions.agent_tower.extends=\":read-only\"";

#[test]
fn with_the_switch_off_each_policy_gets_the_old_flags_for_a_git_floor_and_for_another() {
    // The whole vector, not only the tail: the switch off must change no byte, and `worktrees`
    // must not matter.
    let base = args_of(&ctx(PermissionPolicy::EditInWorktree, false, false));
    for policy in [PermissionPolicy::ReadOnly, PermissionPolicy::EditInWorktree] {
        for worktrees in [false, true] {
            let args = args_of(&ctx(policy, worktrees, false));
            assert_eq!(args, base, "{policy:?} worktrees={worktrees}");
            assert_eq!(permission_part(&args), strings(&OFF_TAIL));
        }
    }
}

#[test]
fn read_only_with_the_sandbox_gets_a_read_only_profile_with_the_reports_folder_writable() {
    for worktrees in [false, true] {
        let args = args_of(&ctx(PermissionPolicy::ReadOnly, worktrees, true));
        assert_eq!(
            permission_part(&args),
            strings(&[
                "-a",
                "on-request",
                "-c",
                PROFILE,
                "-c",
                EXTENDS,
                "-c",
                r#"permissions.agent_tower.filesystem={"/work/floor/wt1/.swarm/reports"="write"}"#,
            ]),
            "worktrees={worktrees}"
        );
        assert!(!args.iter().any(|a| a == "-s" || a == "danger-full-access"));
    }
}

#[test]
fn edit_in_worktree_in_a_git_floor_gets_the_worktrees_reports_and_temp_folders() {
    let args = args_of(&ctx(PermissionPolicy::EditInWorktree, true, true));
    assert_eq!(
        permission_part(&args),
        strings(&[
            "-a",
            "on-request",
            "-c",
            PROFILE,
            "-c",
            EXTENDS,
            "-c",
            r#"permissions.agent_tower.filesystem={"/work/floor/wt1/.swarm/reports"="write","/work/floor/wt1/.swarm/worktrees"="write",":slash_tmp"="write",":tmpdir"="write"}"#,
        ])
    );
    // The main checkout (the working folder) has no write entry.
    assert!(
        !args
            .iter()
            .any(|a| a.contains(r#""/work/floor/wt1"="write""#))
    );
}

#[test]
fn edit_in_worktree_without_a_repository_gets_the_workspace_write_mode() {
    let args = args_of(&ctx(PermissionPolicy::EditInWorktree, false, true));
    assert_eq!(
        permission_part(&args),
        strings(&["-a", "on-request", "-s", "workspace-write"])
    );
}

#[test]
fn the_other_flags_stay_the_same_with_the_sandbox_on() {
    // Hooks, trust, instructions, model, effort and the message are the same as with it off.
    for policy in [PermissionPolicy::ReadOnly, PermissionPolicy::EditInWorktree] {
        let off = args_of(&ctx(policy, true, false));
        let on = args_of(&ctx(policy, true, true));
        let head = |args: &[String]| args[..args.iter().position(|a| a == "-a").unwrap()].to_vec();
        assert_eq!(head(&off), head(&on));
        assert_eq!(off.last(), on.last());
    }
}

#[test]
fn ask_always_stays_refused_with_the_sandbox_on_and_off() {
    for sandbox in [false, true] {
        for worktrees in [false, true] {
            let ctx = ctx(PermissionPolicy::AskAlways, worktrees, sandbox);
            assert!(matches!(
                CodexAdapter.start(&ctx),
                Err(AdapterError::AskAlwaysUnsupported)
            ));
            assert!(matches!(
                CodexAdapter.resume(&ctx, "01a1f6e0-5cda-7fe2-bc90-eef347d035ca"),
                Err(AdapterError::AskAlwaysUnsupported)
            ));
        }
    }
}

#[test]
fn resume_gets_the_same_sandbox_flags_before_the_subcommand() {
    let ctx = ctx(PermissionPolicy::EditInWorktree, true, true);
    let start = args_of(&ctx);
    let resume = args_text(
        &CodexAdapter
            .resume(&ctx, "01a1f6e0-5cda-7fe2-bc90-eef347d035ca")
            .expect("plan")
            .spec
            .args,
    );
    let at = resume.iter().position(|a| a == "resume").expect("resume");
    // The flags of a start, then `resume <id>`, then the message, and nothing else.
    let mut expected = start[..start.len() - 1].to_vec();
    expected.extend(strings(&["resume", "01a1f6e0-5cda-7fe2-bc90-eef347d035ca"]));
    expected.push(start.last().unwrap().clone());
    assert_eq!(at, start.len() - 1);
    assert_eq!(resume, expected);
}

#[test]
fn a_folder_with_special_characters_stays_one_toml_key() {
    let mut ctx = ctx(PermissionPolicy::EditInWorktree, true, true);
    ctx.cwd = "/work/a \"b\" \\c/floor/".into();
    let args = args_of(&ctx);
    let table_arg = args
        .iter()
        .find(|a| a.starts_with("permissions.agent_tower.filesystem="))
        .expect("the filesystem argument");
    let parsed: toml::Table = toml::from_str(table_arg).expect("valid TOML");
    let filesystem = parsed["permissions"]["agent_tower"]["filesystem"]
        .as_table()
        .expect("a table");
    // The trailing slash of the folder does not give a double slash.
    assert_eq!(
        filesystem["/work/a \"b\" \\c/floor/.swarm/worktrees"].as_str(),
        Some("write")
    );
    assert_eq!(filesystem.len(), 4);
}

// ---- the real codex, no model ----

/// Each (access, path or token) entry of the file system that `codex debug prompt-input`
/// renders, in the order of the text.
fn rendered_entries(output: &str) -> Vec<(String, String)> {
    let text = output.replace("\\\"", "\"");
    let start = text
        .find("<file_system")
        .expect("a file_system in the output");
    let end = text[start..].find("</file_system>").expect("its end") + start;
    let mut entries = Vec::new();
    let mut rest = &text[start..end];
    while let Some(at) = rest.find("<entry access=\"") {
        rest = &rest[at + "<entry access=\"".len()..];
        let access = rest[..rest.find('"').unwrap()].to_owned();
        let close = rest.find("</entry>").unwrap();
        let body = &rest[..close];
        let target = body
            .split_once("<path>")
            .and_then(|(_, tail)| tail.split_once("</path>"))
            .map(|(path, _)| path.to_owned())
            .or_else(|| {
                body.split_once("<special>")
                    .and_then(|(_, tail)| tail.split_once("</special>"))
                    .map(|(special, _)| special.to_owned())
            })
            .expect("a path or a special entry");
        entries.push((access, target));
        rest = &rest[close..];
    }
    entries
}

fn rendered_sandbox(codex_ctx: &StartContext) -> Vec<(String, String)> {
    let home = TempDir::new().expect("a Codex home");
    let plan = CodexAdapter.start(codex_ctx).expect("plan");
    let mut args: Vec<OsString> = plan.spec.args.clone();
    args.pop(); // the first message
    args.extend(["debug", "prompt-input", "hi"].map(OsString::from));
    let mut env = plan.spec.env.clone();
    // A Codex home of its own: nothing is read from or written to `~/.codex`.
    env.insert(os("CODEX_HOME"), home.path().as_os_str().to_owned());
    let (ok, stdout, stderr) = run_plan_command(&plan.spec.program, &args, &env, &codex_ctx.cwd);
    assert!(ok, "codex debug prompt-input failed: {stderr}");
    rendered_entries(&stdout)
}

#[test]
fn the_real_codex_renders_the_sandbox_that_each_case_asks_for() {
    let Some(programs) = real_programs() else {
        return;
    };
    let floor = TempDir::new().expect("a floor");
    let root = floor.path().canonicalize().expect("real path");
    let with = |policy, worktrees, sandbox| {
        let mut c = ctx(policy, worktrees, sandbox);
        c.programs = programs.clone();
        c.parent_env = current_env();
        c.cwd = root.clone();
        c.hook_program = hook_bin();
        c.first_message = Some("x".into());
        c.instructions = None;
        rendered_sandbox(&c)
    };
    let path = |sub: &str| root.join(sub).to_string_lossy().into_owned();
    let writes = |entries: &[(String, String)]| -> Vec<String> {
        entries
            .iter()
            .filter(|(access, _)| access == "write")
            .map(|(_, target)| target.clone())
            .collect()
    };
    let has_root_read = |entries: &[(String, String)]| {
        entries
            .iter()
            .any(|(access, target)| access == "read" && target == ":root")
    };

    // read-only: the whole disk is readable, and only the reports folder is writable.
    let read_only = with(PermissionPolicy::ReadOnly, true, true);
    assert_eq!(writes(&read_only), vec![path(".swarm/reports")]);
    assert!(has_root_read(&read_only), "{read_only:?}");

    // edit-in-worktree, git floor: the worktrees, the reports and the temp folders. Not the floor.
    let edit = with(PermissionPolicy::EditInWorktree, true, true);
    let mut expected = vec![
        path(".swarm/reports"),
        path(".swarm/worktrees"),
        ":slash_tmp".to_owned(),
        ":tmpdir".to_owned(),
    ];
    let mut got = writes(&edit);
    got.sort();
    expected.sort();
    assert_eq!(got, expected, "{edit:?}");
    assert!(!writes(&edit).contains(&root.to_string_lossy().into_owned()));
    assert!(has_root_read(&edit), "{edit:?}");

    // edit-in-worktree, no repository: the floor folder is a writable root.
    let plain = with(PermissionPolicy::EditInWorktree, false, true);
    assert!(
        writes(&plain).contains(&root.to_string_lossy().into_owned()),
        "{plain:?}"
    );
    assert!(has_root_read(&plain), "{plain:?}");
}
