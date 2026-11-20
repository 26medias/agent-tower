//! Tests of the adapters against the real CLIs, with NO model call.
//!
//! A snapshot test cannot show that a flag or a config key exists. These tests ask the real
//! `claude` and `codex` of this machine. A test skips with a message on stderr when its CLI is
//! not installed. Nothing here sends a prompt or types into a CLI.

mod adapter_support;

use adapter_support::*;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;
use tower_core::adapter::{
    ClaudeAdapter, CliAdapter, CodexAdapter, SpawnPlan, StartContext, claude_permission_mode,
};
use tower_core::env::current_env;
use tower_core::hookrx::HookReceiver;
use tower_core::model::{CliKind, PermissionPolicy};

const LIMIT: Duration = Duration::from_secs(30);

const POLICIES: [PermissionPolicy; 3] = [
    PermissionPolicy::ReadOnly,
    PermissionPolicy::EditInWorktree,
    PermissionPolicy::AskAlways,
];

/// A context with the real programs and the real environment of this machine.
fn real_ctx(kind: CliKind, cwd: &Path, socket: &Path) -> Option<StartContext> {
    let programs = real_programs()?;
    let mut ctx = fixed_ctx(kind, PermissionPolicy::EditInWorktree);
    ctx.programs = programs;
    ctx.parent_env = current_env();
    ctx.cwd = cwd.to_path_buf();
    ctx.socket = socket.to_path_buf();
    ctx.hook_program = hook_bin();
    ctx.first_message = None;
    ctx.instructions = None;
    Some(ctx)
}

fn help_of(program: &Path, args: &[&str], plan: &SpawnPlan) -> String {
    let args: Vec<_> = args.iter().map(|arg| os(arg)).collect();
    let tmp = std::env::temp_dir();
    let (ok, stdout, stderr) = run_plan_command(program, &args, &plan.spec.env, &tmp);
    assert!(ok, "{} {args:?} failed: {stderr}", program.display());
    format!("{stdout}{stderr}")
}

// ---- help text ----

#[test]
fn every_claude_flag_is_in_the_help_text() {
    let tmp = TempDir::new().expect("tmp");
    let Some(ctx) = real_ctx(CliKind::Claude, tmp.path(), &tmp.path().join("s")) else {
        return;
    };
    let mut full = ctx.clone();
    full.first_message = Some("Hello there".into());
    full.instructions = Some("Be brief.".into());
    let plan = ClaudeAdapter.start(&full).expect("plan");
    let resume = ClaudeAdapter
        .resume(&full, "11111111-2222-4333-8444-555555555501")
        .expect("plan");
    let help = help_of(&ctx.programs.claude, &["--help"], &plan);

    // The checker itself: a whole word matches, a prefix or a misspelling does not.
    assert!(help_has_flag(&help, "--session-id"));
    assert!(!help_has_flag(&help, "--session-i"));
    assert!(!help_has_flag(&help, "--sesion-id"));

    let mut flags = flags_of(&plan.spec.args);
    flags.extend(flags_of(&resume.spec.args));
    flags.sort();
    flags.dedup();
    assert!(flags.len() >= 7, "too few flags found: {flags:?}");
    for flag in &flags {
        assert!(
            help_has_flag(&help, flag),
            "{flag} is not in `claude --help`"
        );
    }
}

#[test]
fn every_permission_mode_value_is_accepted_by_claude() {
    let tmp = TempDir::new().expect("tmp");
    let Some(ctx) = real_ctx(CliKind::Claude, tmp.path(), &tmp.path().join("s")) else {
        return;
    };
    let plan = ClaudeAdapter.start(&ctx).expect("plan");
    let run = |mode: &str| {
        let args = [os("--permission-mode"), os(mode), os("--version")];
        run_plan_command(&ctx.programs.claude, &args, &plan.spec.env, tmp.path())
    };
    // The check can fail: Claude rejects a value that is not in its list.
    let (ok, _, stderr) = run("bogus-mode");
    assert!(!ok, "claude accepted a bogus mode");
    assert!(stderr.contains("Allowed choices"), "{stderr}");
    for policy in POLICIES {
        let mode = claude_permission_mode(policy);
        let (ok, _, stderr) = run(mode);
        assert!(ok, "claude rejects --permission-mode {mode}: {stderr}");
    }
}

#[test]
fn every_codex_flag_and_subcommand_is_in_the_help_text() {
    let tmp = TempDir::new().expect("tmp");
    let Some(ctx) = real_ctx(CliKind::Codex, tmp.path(), &tmp.path().join("s")) else {
        return;
    };
    let mut full = ctx.clone();
    full.first_message = Some("Hello there".into());
    full.instructions = Some("Be brief.".into());
    let plan = CodexAdapter.start(&full).expect("plan");
    let resume = CodexAdapter
        .resume(&full, "01a1f6e0-5cda-7fe2-bc90-eef347d035ca")
        .expect("plan");
    let program = &ctx.programs.codex;
    let top_help = help_of(program, &["--help"], &plan);
    let resume_help = help_of(program, &["resume", "--help"], &plan);

    assert!(help_has_flag(&top_help, "--no-daemon"));
    assert!(!help_has_flag(&top_help, "--no-daemo"));
    assert!(
        top_help
            .lines()
            .any(|line| line.trim_start().starts_with("resume ")),
        "`resume` is not a subcommand in `codex --help`"
    );

    let mut flags = flags_of(&plan.spec.args);
    flags.sort();
    flags.dedup();
    for flag in &flags {
        assert!(
            help_has_flag(&top_help, flag),
            "{flag} is not in `codex --help`"
        );
    }
    let mut resume_flags = flags_of(&resume.spec.args);
    resume_flags.sort();
    resume_flags.dedup();
    assert_eq!(flags, resume_flags);
    for flag in &resume_flags {
        assert!(
            help_has_flag(&resume_help, flag),
            "{flag} is not in `codex resume --help`"
        );
    }
    // The positional form that the adapter builds: `resume <id> <prompt>`.
    assert!(
        resume_help.contains("[SESSION_ID] [PROMPT]"),
        "{resume_help}"
    );
}

#[test]
fn both_clis_print_their_version_with_the_session_path() {
    let tmp = TempDir::new().expect("tmp");
    for kind in [CliKind::Claude, CliKind::Codex] {
        let Some(ctx) = real_ctx(kind, tmp.path(), &tmp.path().join("s")) else {
            return;
        };
        let (plan, program) = match kind {
            CliKind::Claude => (ClaudeAdapter.start(&ctx), &ctx.programs.claude),
            CliKind::Codex => (CodexAdapter.start(&ctx), &ctx.programs.codex),
        };
        let plan = plan.expect("plan");
        let (ok, stdout, stderr) =
            run_plan_command(program, &[os("--version")], &plan.spec.env, tmp.path());
        assert!(ok, "{kind:?} --version failed: {stderr}");
        assert!(stdout.chars().any(|c| c.is_ascii_digit()), "{stdout}");
    }
}

// ---- Codex config ----

const HARD_INSTRUCTIONS: &str =
    "ZMARKZ \"quoted\" and 'single'\nsecond line \\ backslash \\n literal\n\ttab";

/// The arguments of a Codex plan without the prompt, and a real command after them.
fn codex_command(plan: &SpawnPlan, tail: &[&str]) -> Vec<std::ffi::OsString> {
    let mut args = plan.spec.args.clone();
    args.extend(tail.iter().map(|arg| os(arg)));
    args
}

fn hard_codex_ctx(tmp: &Path) -> Option<StartContext> {
    let mut ctx = real_ctx(CliKind::Codex, tmp, &tmp.join("s"))?;
    // A folder and a hook path with a space, a quote and a backslash: the TOML must be right.
    ctx.cwd = tmp.join("my \"floor\" \\ dir");
    std::fs::create_dir_all(&ctx.cwd).expect("create cwd");
    ctx.hook_program = PathBuf::from("/opt/my \"tools\"/tower \\ hook");
    ctx.instructions = Some(HARD_INSTRUCTIONS.into());
    ctx.employee.model = Some("gpt-6-luna".into());
    ctx.employee.effort = Some("low".into());
    Some(ctx)
}

#[test]
fn codex_loads_a_config_that_has_all_the_adapter_values() {
    let tmp = TempDir::new().expect("tmp");
    let Some(ctx) = hard_codex_ctx(tmp.path()) else {
        return;
    };
    let plan = CodexAdapter.start(&ctx).expect("plan");
    // `features list` loads the whole config, with each `-c` value, and prints no model text.
    let args = codex_command(&plan, &["features", "list"]);
    let (ok, stdout, stderr) =
        run_plan_command(&ctx.programs.codex, &args, &plan.spec.env, tmp.path());
    assert!(ok, "codex failed:\n{stderr}\n{stdout}");
    assert!(!stderr.contains("Error"), "config error text: {stderr}");
    assert!(stdout.contains("hooks"), "no feature list: {stdout}");

    // The check can fail: a value with a wrong TOML shape makes Codex stop with a config error.
    let mut bad = plan.clone();
    bad.spec.args.extend([os("-c"), os("hooks.Stop=garbage")]);
    let args = codex_command(&bad, &["features", "list"]);
    let (ok, _, stderr) = run_plan_command(&ctx.programs.codex, &args, &bad.spec.env, tmp.path());
    assert!(!ok, "codex accepted a bad config value");
    assert!(stderr.contains("hooks"), "{stderr}");
}

#[test]
fn codex_receives_the_developer_instructions_exactly() {
    let tmp = TempDir::new().expect("tmp");
    let Some(ctx) = hard_codex_ctx(tmp.path()) else {
        return;
    };
    let plan = CodexAdapter.start(&ctx).expect("plan");
    let args = codex_command(&plan, &["debug", "prompt-input", "hi"]);
    let (ok, stdout, stderr) =
        run_plan_command(&ctx.programs.codex, &args, &plan.spec.env, tmp.path());
    assert!(ok, "codex debug prompt-input failed:\n{stderr}");
    let items: Value = serde_json::from_str(&stdout).expect("prompt-input prints JSON");
    assert!(
        contains_string(&items, HARD_INSTRUCTIONS),
        "the instructions text is not in the prompt that Codex builds:\n{stdout}"
    );
    // Gate 2: a misspelled key prints the marker 0 times. This count must be at least 1.
    assert!(stdout.matches("ZMARKZ").count() >= 1);
}

fn contains_string(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(text) => text == needle,
        Value::Array(items) => items.iter().any(|item| contains_string(item, needle)),
        Value::Object(map) => map.values().any(|item| contains_string(item, needle)),
        _ => false,
    }
}

// ---- Claude start: SessionStart reaches the receiver ----

#[test]
fn claude_started_by_the_adapter_fires_session_start_for_the_right_session() {
    let Some(repo) = trusted_worktree() else {
        return;
    };
    let sock_dir = TempDir::new().expect("socket folder");
    let sock = sock_dir.path().join("h.sock");
    let Some(mut ctx) = real_ctx(CliKind::Claude, &repo.path(), &sock) else {
        return;
    };
    ctx.app_session_id = "app-session-claude-1".into();
    ctx.employee.model = Some("haiku".into());
    ctx.employee.effort = None;
    let plan = ClaudeAdapter.start(&ctx).expect("plan");
    let chosen = plan.cli_session_id.clone().expect("Claude plan has an ID");

    // No first message: the CLI makes no model call. Nothing is typed into it.
    let (receiver, messages) = HookReceiver::bind(&sock).expect("bind");
    let mut run = LiveRun::start(plan.spec);
    let result = run.wait_for(
        &messages,
        "app-session-claude-1",
        LIMIT,
        hook_name_is("SessionStart"),
        0,
    );
    run.stop(&messages, "app-session-claude-1");
    drop(receiver);

    let message = match result {
        Ok(message) => message,
        Err(end) => {
            if matches!(end, WaitEnd::TimedOut) && squash(&run.output).contains("trust") {
                eprintln!("SKIP: Claude shows a folder trust dialog; no hook can fire.");
                return;
            }
            panic!(
                "no SessionStart within {LIMIT:?} ({end:?}). Output tail: {}",
                output_tail(&run.output, 600)
            );
        }
    };
    assert_eq!(message.session, "app-session-claude-1");
    assert_eq!(
        message.event["session_id"].as_str(),
        Some(chosen.as_str()),
        "the session_id of the hook must be the UUID that the adapter chose"
    );
}
