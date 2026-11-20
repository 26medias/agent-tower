//! Snapshot tests of the adapters: exact argument vectors and environments, with fixed IDs and
//! paths. A snapshot does not prove that a flag exists. `adapter_cli.rs` checks the flags
//! against the real CLIs.

mod adapter_support;

use adapter_support::*;
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use tower_core::adapter::{
    AdapterError, ClaudeAdapter, CliAdapter, CodexAdapter, SpawnPlan, StartContext, adapter_for,
    claude_permission_mode, cli_session_id_from_event,
};
use tower_core::env::EnvMap;
use tower_core::model::{CliKind, PermissionPolicy};
use tower_core::status::HookEvent;

const UUID_A: &str = "11111111-2222-4333-8444-555555555501";
const UUID_B: &str = "11111111-2222-4333-8444-555555555502";
const HOOK: &str = "/opt/tower/bin/tower-hook";

const CLAUDE_EVENTS: [&str; 8] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Notification",
    "Stop",
    "SessionEnd",
];

const CODEX_EVENTS: [&str; 8] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Stop",
    "Interrupt",
    "SessionEnd",
];

fn claude_settings(hook: &str) -> String {
    let plain = |event: &str| {
        format!(r#""{event}":[{{"hooks":[{{"type":"command","command":"{hook}"}}]}}]"#)
    };
    let tool = |event: &str| {
        format!(
            r#""{event}":[{{"matcher":"*","hooks":[{{"type":"command","command":"{hook}"}}]}}]"#
        )
    };
    format!(
        "{{\"hooks\":{{{}}}}}",
        [
            plain("SessionStart"),
            plain("UserPromptSubmit"),
            tool("PreToolUse"),
            tool("PostToolUse"),
            tool("PermissionRequest"),
            plain("Notification"),
            plain("Stop"),
            plain("SessionEnd"),
        ]
        .join(",")
    )
}

fn codex_hook_arg(event: &str, hook: &str) -> String {
    format!(r#"hooks.{event}=[{{hooks=[{{type="command",command="{hook}"}}]}}]"#)
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn text(plan: &SpawnPlan) -> Vec<String> {
    args_text(&plan.spec.args)
}

fn expected_env(session: &str) -> EnvMap {
    [
        ("HOME", "/home/test"),
        ("LANG", "C.UTF-8"),
        ("TERM", "xterm"),
        ("PATH", "/usr/bin:/bin:/opt/cli/bin:/opt/node/bin"),
        ("TOWER_SESSION", session),
        ("TOWER_SOCKET", "/run/tower/hooks.sock"),
    ]
    .iter()
    .map(|(name, value)| (os(name), os(value)))
    .collect()
}

fn claude_ctx(policy: PermissionPolicy) -> StartContext {
    fixed_ctx(CliKind::Claude, policy)
}

fn codex_ctx(policy: PermissionPolicy) -> StartContext {
    fixed_ctx(CliKind::Codex, policy)
}

// ---- exact vectors ----

#[test]
fn claude_start_argv_and_env_are_exact() {
    let ctx = claude_ctx(PermissionPolicy::EditInWorktree);
    let plan = ClaudeAdapter.start_with_id(&ctx, UUID_A).expect("plan");

    assert_eq!(plan.cli_session_id.as_deref(), Some(UUID_A));
    assert_eq!(plan.spec.program, PathBuf::from("/opt/cli/bin/claude"));
    assert_eq!(plan.spec.cwd, PathBuf::from("/work/floor/wt1"));
    assert_eq!((plan.spec.rows, plan.spec.cols), (40, 120));
    assert_eq!(
        text(&plan),
        vec![
            "--session-id".to_owned(),
            UUID_A.to_owned(),
            "--settings".to_owned(),
            claude_settings(HOOK),
            "--permission-mode".to_owned(),
            "acceptEdits".to_owned(),
            "--model".to_owned(),
            "haiku".to_owned(),
            "--effort".to_owned(),
            "low".to_owned(),
            "--append-system-prompt".to_owned(),
            "Be brief.".to_owned(),
            "Reply with only: OK".to_owned(),
        ]
    );
    assert_eq!(plan.spec.env, expected_env("app-session-A"));
}

#[test]
fn claude_resume_argv_and_env_are_exact() {
    let mut ctx = claude_ctx(PermissionPolicy::ReadOnly);
    ctx.first_message = Some("What was my last message?".into());
    let plan = ClaudeAdapter.resume(&ctx, UUID_A).expect("plan");

    assert_eq!(plan.cli_session_id.as_deref(), Some(UUID_A));
    assert_eq!(
        text(&plan),
        vec![
            "--resume".to_owned(),
            UUID_A.to_owned(),
            "--settings".to_owned(),
            claude_settings(HOOK),
            "--permission-mode".to_owned(),
            "manual".to_owned(),
            "--model".to_owned(),
            "haiku".to_owned(),
            "--effort".to_owned(),
            "low".to_owned(),
            "--append-system-prompt".to_owned(),
            "Be brief.".to_owned(),
            "What was my last message?".to_owned(),
        ]
    );
    assert_eq!(plan.spec.env, expected_env("app-session-A"));
}

#[test]
fn claude_start_makes_a_fresh_uuid_each_time() {
    let ctx = claude_ctx(PermissionPolicy::EditInWorktree);
    let first = ClaudeAdapter.start(&ctx).expect("plan");
    let second = ClaudeAdapter.start(&ctx).expect("plan");
    let id_first = first.cli_session_id.clone().expect("id");
    let id_second = second.cli_session_id.clone().expect("id");
    assert_ne!(id_first, id_second);
    assert!(uuid_like(&id_first), "{id_first}");
    let args = text(&first);
    assert_eq!(args[0], "--session-id");
    assert_eq!(args[1], id_first);
}

fn uuid_like(id: &str) -> bool {
    let parts: Vec<&str> = id.split('-').collect();
    parts.iter().map(|part| part.len()).collect::<Vec<_>>() == [8, 4, 4, 4, 12]
        && id.chars().all(|c| c == '-' || c.is_ascii_hexdigit())
}

#[test]
fn codex_start_argv_and_env_are_exact() {
    let ctx = codex_ctx(PermissionPolicy::EditInWorktree);
    let plan = CodexAdapter.start(&ctx).expect("plan");

    assert_eq!(plan.cli_session_id, None);
    assert_eq!(plan.spec.program, PathBuf::from("/opt/node/bin/codex"));
    assert_eq!(plan.spec.cwd, PathBuf::from("/work/floor/wt1"));
    let mut expected = vec!["--no-daemon".to_owned()];
    for event in CODEX_EVENTS {
        expected.push("-c".to_owned());
        expected.push(codex_hook_arg(event, HOOK));
    }
    expected.extend(strings(&[
        "-c",
        r#"projects={"/work/floor/wt1"={trust_level="trusted"}}"#,
        "-c",
        "check_for_update_on_startup=false",
        "-c",
        r#"developer_instructions="Be brief.""#,
        "-m",
        "haiku",
        "-c",
        r#"model_reasoning_effort="low""#,
        "-a",
        "on-request",
        "-s",
        "danger-full-access",
        "Reply with only: OK",
    ]));
    assert_eq!(text(&plan), expected);
    assert_eq!(plan.spec.env, expected_env("app-session-A"));
}

#[test]
fn codex_resume_argv_is_exact() {
    let mut ctx = codex_ctx(PermissionPolicy::ReadOnly);
    ctx.first_message = Some("What was my last message?".into());
    let plan = CodexAdapter
        .resume(&ctx, "01a1f6e0-5cda-7fe2-bc90-eef347d035ca")
        .expect("plan");

    assert_eq!(
        plan.cli_session_id.as_deref(),
        Some("01a1f6e0-5cda-7fe2-bc90-eef347d035ca")
    );
    let mut expected = vec!["--no-daemon".to_owned()];
    for event in CODEX_EVENTS {
        expected.push("-c".to_owned());
        expected.push(codex_hook_arg(event, HOOK));
    }
    expected.extend(strings(&[
        "-c",
        r#"projects={"/work/floor/wt1"={trust_level="trusted"}}"#,
        "-c",
        "check_for_update_on_startup=false",
        "-c",
        r#"developer_instructions="Be brief.""#,
        "-m",
        "haiku",
        "-c",
        r#"model_reasoning_effort="low""#,
        "-a",
        "on-request",
        "-s",
        "danger-full-access",
        "resume",
        "01a1f6e0-5cda-7fe2-bc90-eef347d035ca",
        "What was my last message?",
    ]));
    assert_eq!(text(&plan), expected);
}

#[test]
fn options_that_are_not_set_are_not_in_the_argv() {
    let mut claude = claude_ctx(PermissionPolicy::EditInWorktree);
    claude.employee.model = None;
    claude.employee.effort = None;
    claude.instructions = None;
    claude.first_message = None;
    let plan = ClaudeAdapter.start_with_id(&claude, UUID_A).expect("plan");
    assert_eq!(
        text(&plan),
        vec![
            "--session-id".to_owned(),
            UUID_A.to_owned(),
            "--settings".to_owned(),
            claude_settings(HOOK),
            "--permission-mode".to_owned(),
            "acceptEdits".to_owned(),
        ]
    );

    let mut codex = codex_ctx(PermissionPolicy::EditInWorktree);
    codex.employee.model = Some(String::new());
    codex.employee.effort = None;
    codex.instructions = Some(String::new());
    codex.first_message = None;
    let args = text(&CodexAdapter.start(&codex).expect("plan"));
    assert!(!args.contains(&"-m".to_owned()));
    assert!(
        !args
            .iter()
            .any(|arg| arg.starts_with("model_reasoning_effort"))
    );
    assert!(
        !args
            .iter()
            .any(|arg| arg.starts_with("developer_instructions"))
    );
    assert_eq!(args.last().map(String::as_str), Some("danger-full-access"));
}

// ---- hook text ----

fn claude_hook_commands(args: &[String]) -> Vec<String> {
    let at = args
        .iter()
        .position(|arg| arg == "--settings")
        .expect("--settings");
    let settings: Value = serde_json::from_str(&args[at + 1]).expect("settings JSON");
    let hooks = settings["hooks"].as_object().expect("hooks object");
    let mut commands = Vec::new();
    for (_, groups) in hooks {
        for group in groups.as_array().expect("array") {
            for hook in group["hooks"].as_array().expect("array") {
                assert_eq!(hook["type"], "command");
                commands.push(hook["command"].as_str().expect("command").to_owned());
            }
        }
    }
    commands
}

fn codex_hook_commands(args: &[String]) -> Vec<String> {
    let mut commands = Vec::new();
    for arg in args.iter().filter(|arg| arg.starts_with("hooks.")) {
        let table: toml::Table = toml::from_str(arg).expect("the -c value is TOML");
        for (_, groups) in table["hooks"].as_table().expect("hooks table") {
            for group in groups.as_array().expect("array") {
                for hook in group["hooks"].as_array().expect("array") {
                    commands.push(hook["command"].as_str().expect("command").to_owned());
                }
            }
        }
    }
    commands
}

#[test]
fn claude_settings_have_the_expected_events_and_one_command_text() {
    let ctx = claude_ctx(PermissionPolicy::EditInWorktree);
    let args = text(&ClaudeAdapter.start_with_id(&ctx, UUID_A).expect("plan"));
    let at = args
        .iter()
        .position(|arg| arg == "--settings")
        .expect("--settings");
    let settings: Value = serde_json::from_str(&args[at + 1]).expect("settings JSON");
    let mut names: Vec<&str> = settings["hooks"]
        .as_object()
        .expect("hooks")
        .keys()
        .map(String::as_str)
        .collect();
    names.sort_unstable();
    let mut expected = CLAUDE_EVENTS.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected, "SubagentStop must not be in the list");
    let commands = claude_hook_commands(&args);
    assert_eq!(commands.len(), 8);
    assert!(
        commands.iter().all(|command| command == HOOK),
        "{commands:?}"
    );
}

#[test]
fn hook_command_text_is_the_same_for_two_sessions_claude() {
    let mut ctx_a = claude_ctx(PermissionPolicy::EditInWorktree);
    ctx_a.app_session_id = "app-session-A".into();
    let mut ctx_b = claude_ctx(PermissionPolicy::EditInWorktree);
    ctx_b.app_session_id = "app-session-B".into();
    let plan_a = ClaudeAdapter.start_with_id(&ctx_a, UUID_A).expect("plan");
    let plan_b = ClaudeAdapter.start_with_id(&ctx_b, UUID_B).expect("plan");
    let args_a = text(&plan_a);
    let args_b = text(&plan_b);

    // The settings argument is byte for byte the same.
    let settings = |args: &[String]| {
        let at = args
            .iter()
            .position(|arg| arg == "--settings")
            .expect("--settings");
        args[at + 1].clone()
    };
    assert_eq!(settings(&args_a), settings(&args_b));
    assert_eq!(claude_hook_commands(&args_a), claude_hook_commands(&args_b));

    // No session data in the hook settings: neither the app ID nor the CLI ID.
    for (args, app, cli) in [
        (&args_a, "app-session-A", UUID_A),
        (&args_b, "app-session-B", UUID_B),
    ] {
        let settings = settings(args);
        assert!(
            !settings.contains(app) && !settings.contains(cli),
            "{settings}"
        );
    }
    // The identity is in the environment, and it differs.
    assert_eq!(plan_a.spec.env[&os("TOWER_SESSION")], os("app-session-A"));
    assert_eq!(plan_b.spec.env[&os("TOWER_SESSION")], os("app-session-B"));
}

#[test]
fn hook_command_text_is_the_same_for_two_sessions_codex() {
    let mut ctx_a = codex_ctx(PermissionPolicy::EditInWorktree);
    ctx_a.app_session_id = "app-session-A".into();
    let mut ctx_b = codex_ctx(PermissionPolicy::EditInWorktree);
    ctx_b.app_session_id = "app-session-B".into();
    let plan_a = CodexAdapter.start(&ctx_a).expect("plan");
    let plan_b = CodexAdapter.start(&ctx_b).expect("plan");
    let resume_b = CodexAdapter
        .resume(&ctx_b, "01a1f6e0-5cda-7fe2-bc90-eef347d035ca")
        .expect("plan");

    // The whole argument vector is the same for the two sessions: nothing in it is per session.
    assert_eq!(text(&plan_a), text(&plan_b));
    let commands = codex_hook_commands(&text(&plan_a));
    assert_eq!(commands.len(), 8);
    assert!(
        commands.iter().all(|command| command == HOOK),
        "{commands:?}"
    );
    // The hooks of a resume have the same text as the hooks of a start (one trust step only).
    assert_eq!(codex_hook_commands(&text(&resume_b)), commands);
    for arg in text(&plan_a) {
        assert!(!arg.contains("app-session"), "{arg}");
    }
    assert_eq!(plan_a.spec.env[&os("TOWER_SESSION")], os("app-session-A"));
    assert_eq!(plan_b.spec.env[&os("TOWER_SESSION")], os("app-session-B"));
}

#[test]
fn a_hook_path_with_special_characters_is_quoted_for_a_shell() {
    let mut ctx = claude_ctx(PermissionPolicy::EditInWorktree);
    ctx.hook_program = PathBuf::from("/opt/my tools/it's/tower-hook");
    let args = text(&ClaudeAdapter.start_with_id(&ctx, UUID_A).expect("plan"));
    let commands = claude_hook_commands(&args);
    assert!(
        commands
            .iter()
            .all(|c| c == r"'/opt/my tools/it'\''s/tower-hook'"),
        "{commands:?}"
    );

    let mut ctx = codex_ctx(PermissionPolicy::EditInWorktree);
    ctx.hook_program = PathBuf::from("/opt/my tools/it's/tower-hook");
    let commands = codex_hook_commands(&text(&CodexAdapter.start(&ctx).expect("plan")));
    assert!(
        commands
            .iter()
            .all(|c| c == r"'/opt/my tools/it'\''s/tower-hook'"),
        "{commands:?}"
    );
}

// ---- hard text ----

const HARD_CWD: &str = "/work/my floor/it's \"x\" \\ dir";
const HARD_TEXT: &str =
    "Say \"hi\" and 'bye'.\nBackslash: C:\\dir\\n (not a newline)\r\n\tTab end \\\\ \"";

#[test]
fn a_working_folder_with_a_space_and_quotes_is_exact() {
    let mut claude = claude_ctx(PermissionPolicy::EditInWorktree);
    claude.cwd = PathBuf::from(HARD_CWD);
    let plan = ClaudeAdapter.start_with_id(&claude, UUID_A).expect("plan");
    assert_eq!(plan.spec.cwd, PathBuf::from(HARD_CWD));

    let mut codex = codex_ctx(PermissionPolicy::EditInWorktree);
    codex.cwd = PathBuf::from(HARD_CWD);
    let plan = CodexAdapter.start(&codex).expect("plan");
    assert_eq!(plan.spec.cwd, PathBuf::from(HARD_CWD));
    let args = text(&plan);
    let arg = args
        .iter()
        .find(|arg| arg.starts_with("projects="))
        .expect("projects");
    let table: toml::Table = toml::from_str(arg).expect("TOML");
    assert_eq!(
        table["projects"][HARD_CWD]["trust_level"].as_str(),
        Some("trusted")
    );
    assert_eq!(table["projects"].as_table().expect("table").len(), 1);
}

#[test]
fn instructions_with_quotes_backslashes_and_newlines_are_exact() {
    let mut claude = claude_ctx(PermissionPolicy::EditInWorktree);
    claude.instructions = Some(HARD_TEXT.into());
    claude.first_message = Some(HARD_TEXT.into());
    let args = text(&ClaudeAdapter.start_with_id(&claude, UUID_A).expect("plan"));
    let at = args
        .iter()
        .position(|arg| arg == "--append-system-prompt")
        .expect("flag");
    assert_eq!(args[at + 1], HARD_TEXT);
    assert_eq!(args.last().map(String::as_str), Some(HARD_TEXT));

    let mut codex = codex_ctx(PermissionPolicy::EditInWorktree);
    codex.instructions = Some(HARD_TEXT.into());
    codex.first_message = Some(HARD_TEXT.into());
    let args = text(&CodexAdapter.start(&codex).expect("plan"));
    let arg = args
        .iter()
        .find(|arg| arg.starts_with("developer_instructions="))
        .expect("arg");
    assert!(
        !arg.contains('\n') && !arg.contains('\r'),
        "one line: {arg:?}"
    );
    let table: toml::Table = toml::from_str(arg).expect("TOML");
    assert_eq!(table["developer_instructions"].as_str(), Some(HARD_TEXT));
    assert_eq!(args.last().map(String::as_str), Some(HARD_TEXT));
}

#[test]
fn a_message_that_starts_with_a_dash_follows_a_double_dash() {
    let mut claude = claude_ctx(PermissionPolicy::EditInWorktree);
    claude.first_message = Some("--version please".into());
    let args = text(&ClaudeAdapter.start_with_id(&claude, UUID_A).expect("plan"));
    assert_eq!(&args[args.len() - 2..], ["--", "--version please"]);

    let mut codex = codex_ctx(PermissionPolicy::EditInWorktree);
    codex.first_message = Some("-m evil".into());
    let args = text(&CodexAdapter.start(&codex).expect("plan"));
    assert_eq!(&args[args.len() - 2..], ["--", "-m evil"]);
}

// ---- policy ----

#[test]
fn claude_permission_mapping() {
    assert_eq!(claude_permission_mode(PermissionPolicy::ReadOnly), "manual");
    assert_eq!(
        claude_permission_mode(PermissionPolicy::AskAlways),
        "manual"
    );
    assert_eq!(
        claude_permission_mode(PermissionPolicy::EditInWorktree),
        "acceptEdits"
    );
    // The flag is always explicit, for each policy.
    for policy in [
        PermissionPolicy::ReadOnly,
        PermissionPolicy::EditInWorktree,
        PermissionPolicy::AskAlways,
    ] {
        let args = text(
            &ClaudeAdapter
                .start_with_id(&claude_ctx(policy), UUID_A)
                .expect("plan"),
        );
        let at = args
            .iter()
            .position(|arg| arg == "--permission-mode")
            .expect("flag");
        assert_eq!(args[at + 1], claude_permission_mode(policy));
    }
}

#[test]
fn codex_gives_the_same_approval_flags_to_each_policy_and_refuses_ask_always() {
    for policy in [PermissionPolicy::ReadOnly, PermissionPolicy::EditInWorktree] {
        let args = text(&CodexAdapter.start(&codex_ctx(policy)).expect("plan"));
        let at = args.iter().position(|arg| arg == "-a").expect("-a");
        assert_eq!(
            &args[at..at + 4],
            ["-a", "on-request", "-s", "danger-full-access"]
        );
    }
    let ctx = codex_ctx(PermissionPolicy::AskAlways);
    assert!(matches!(
        CodexAdapter.start(&ctx),
        Err(AdapterError::AskAlwaysUnsupported)
    ));
    assert!(matches!(
        CodexAdapter.resume(&ctx, "abc"),
        Err(AdapterError::AskAlwaysUnsupported)
    ));
}

// ---- environment ----

#[test]
fn the_environment_is_clean_and_the_tower_values_are_set() {
    for kind in [CliKind::Claude, CliKind::Codex] {
        let ctx = fixed_ctx(kind, PermissionPolicy::EditInWorktree);
        let plan = adapter_for(kind).start(&ctx).expect("plan");
        let env: &BTreeMap<OsString, OsString> = &plan.spec.env;
        assert!(!env.contains_key(&os("CLAUDE_CODE_CHILD_SESSION")));
        assert!(!env.contains_key(&os("SECRET_TOKEN")));
        assert_eq!(
            env[&os("PATH")],
            os("/usr/bin:/bin:/opt/cli/bin:/opt/node/bin")
        );
        assert_eq!(env[&os("TOWER_SESSION")], os("app-session-A"));
        assert_eq!(env[&os("TOWER_SOCKET")], os("/run/tower/hooks.sock"));
    }
}

#[test]
fn a_bad_context_is_an_error() {
    let mut ctx = claude_ctx(PermissionPolicy::EditInWorktree);
    ctx.hook_program = PathBuf::from("tower-hook");
    assert!(matches!(
        ClaudeAdapter.start(&ctx),
        Err(AdapterError::NotAbsolute { .. })
    ));

    let mut ctx = claude_ctx(PermissionPolicy::EditInWorktree);
    ctx.cwd = PathBuf::from("relative/dir");
    assert!(matches!(
        CodexAdapter.start(&ctx),
        Err(AdapterError::NotAbsolute { .. })
    ));

    let mut ctx = claude_ctx(PermissionPolicy::EditInWorktree);
    ctx.app_session_id = String::new();
    assert!(matches!(
        ClaudeAdapter.start(&ctx),
        Err(AdapterError::EmptySessionId)
    ));

    let ctx = claude_ctx(PermissionPolicy::EditInWorktree);
    for id in ["", "--model", "a b", "x;y"] {
        assert!(matches!(
            ClaudeAdapter.resume(&ctx, id),
            Err(AdapterError::BadCliSessionId(_))
        ));
        assert!(matches!(
            CodexAdapter.resume(&ctx, id),
            Err(AdapterError::BadCliSessionId(_))
        ));
    }
    assert!(matches!(
        ClaudeAdapter.start_with_id(&ctx, "not-a-uuid"),
        Err(AdapterError::BadCliSessionId(_))
    ));
}

// ---- CLI session ID and payloads ----

#[test]
fn cli_session_id_comes_from_the_session_id_field() {
    let event = serde_json::json!({"hook_event_name":"SessionStart","session_id":"abc-123"});
    assert_eq!(
        cli_session_id_from_event(&event).as_deref(),
        Some("abc-123")
    );
    assert_eq!(cli_session_id_from_event(&serde_json::json!({})), None);
    assert_eq!(
        cli_session_id_from_event(&serde_json::json!({"session_id": ""})),
        None
    );
    assert_eq!(
        cli_session_id_from_event(&serde_json::json!({"session_id": 7})),
        None
    );
}

/// The payloads of the probe logs: `data` of each line.
fn logged_payloads(file: &str) -> Vec<Value> {
    let path = format!("{}/../../probes/{file}", env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    content
        .lines()
        .map(|line| {
            let value: Value = serde_json::from_str(line).expect("log line");
            value["data"].clone()
        })
        .collect()
}

#[test]
fn codex_session_id_is_in_the_first_event_of_the_probe_logs() {
    for file in ["logs/codex-hooks.jsonl", "step0/logs/codex-hooks.jsonl"] {
        let payloads = logged_payloads(file);
        assert!(!payloads.is_empty());
        assert_eq!(payloads[0]["hook_event_name"], "SessionStart");
        let first = cli_session_id_from_event(&payloads[0]).expect("first event has an ID");
        assert!(first.len() > 30, "{first}");
        for payload in &payloads {
            let id = cli_session_id_from_event(payload).expect("each Codex event has an ID");
            assert_eq!(
                HookEvent::from_json(payload).session_id.as_deref(),
                Some(id.as_str())
            );
        }
    }
}

#[test]
fn claude_payloads_of_the_probe_logs_carry_the_session_id() {
    let payloads = logged_payloads("logs/claude-hooks.jsonl");
    assert!(!payloads.is_empty());
    for payload in &payloads {
        let id = payload["session_id"].as_str().expect("session_id");
        assert_eq!(
            HookEvent::from_json(payload).session_id.as_deref(),
            Some(id)
        );
    }
}
