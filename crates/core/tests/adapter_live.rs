//! Live smoke tests: one real session for each CLI, with a cheap model. They call a model, so
//! they are `#[ignore]`. Run them by hand:
//!
//! `cargo test -p tower-core --test adapter_live -- --ignored --nocapture`
//!
//! Each test starts the CLI through the adapter, gives the first message as the prompt argument,
//! and expects the hook events `SessionStart`, `UserPromptSubmit` and `Stop` for the right
//! session. Then it stops the process, resumes the session through the adapter, and expects a
//! `Stop` whose `last_assistant_message` shows that the context was kept.
//!
//! The tests never type into a CLI. A startup dialog (Claude folder trust, Codex "Hooks need
//! review") blocks the session and fires no hook. Then the test stops the session and reports
//! the dialog as the step that the operator must do.

mod adapter_support;

use adapter_support::*;
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::Receiver;
use std::time::Duration;
use tempfile::TempDir;
use tower_core::adapter::{
    ClaudeAdapter, CliAdapter, CodexAdapter, SpawnPlan, StartContext, cli_session_id_from_event,
};
use tower_core::env::current_env;
use tower_core::hookrx::{HookMessage, HookReceiver};
use tower_core::model::{CliKind, PermissionPolicy};

const FIRST: &str = "Reply with only: OK";
const SECOND: &str = "What was my last message? Reply in five words or less.";
const SESSION: &str = "app-session-live-1";
/// A model turn, with the start of the CLI.
const TURN_LIMIT: Duration = Duration::from_secs(120);

fn live_ctx(kind: CliKind, cwd: &Path, sock: &Path) -> Option<StartContext> {
    let mut ctx = fixed_ctx(kind, PermissionPolicy::EditInWorktree);
    ctx.programs = real_programs()?;
    ctx.parent_env = current_env();
    ctx.cwd = cwd.to_path_buf();
    ctx.socket = sock.to_path_buf();
    ctx.hook_program = hook_bin();
    ctx.app_session_id = SESSION.into();
    ctx.instructions = None;
    ctx.first_message = Some(FIRST.into());
    Some(ctx)
}

/// What a finished first turn gives back.
struct FirstTurn {
    cli_session_id: String,
    names: Vec<String>,
    user_prompt_event: Value,
}

/// True when the Codex dialog "Hooks need review" is on the screen.
fn hooks_dialog_shown(output: &[u8]) -> bool {
    // The words of the title can lose letters in `squash`; the options are stable.
    let screen = squash(output);
    screen.contains("reviewhooks") && screen.contains("trustallandcontinue")
}

/// The failure text of a wait that ended.
fn failure(run: &LiveRun, what: &str, name: &str, end: &WaitEnd) -> String {
    format!(
        "{what}: no {name} ({end:?}). Events so far: {:?}. Output tail: {}",
        run.event_names(),
        output_tail(&run.output, 500)
    )
}

/// Wait for the three events of a turn. The text of the error says why a wait ended.
fn expect_turn(
    run: &mut LiveRun,
    messages: &Receiver<HookMessage>,
    what: &str,
) -> Result<(HookMessage, HookMessage, HookMessage), String> {
    // A startup dialog fires no hook. Look for it after a short wait, so that a blocked session
    // does not take the full time limit.
    let mut limit = Duration::from_secs(25);
    let start = loop {
        match run.wait_for(messages, SESSION, limit, hook_name_is("SessionStart"), 0) {
            Ok(message) => break message,
            Err(WaitEnd::TimedOut) if hooks_dialog_shown(&run.output) => {
                return Err(format!(
                    "{what}: the startup dialog \"Hooks need review\" blocks the session"
                ));
            }
            Err(WaitEnd::TimedOut) if limit < TURN_LIMIT => limit = TURN_LIMIT,
            Err(end) => return Err(failure(run, what, "SessionStart", &end)),
        }
    };
    let mut wait = |name: &'static str| {
        run.wait_for(messages, SESSION, TURN_LIMIT, hook_name_is(name), 0)
            .map_err(|end| failure(run, what, name, &end))
    };
    let submit = wait("UserPromptSubmit")?;
    let stop = wait("Stop")?;
    Ok((start, submit, stop))
}

/// A command line that the operator can paste into a terminal, from an argument vector.
fn shell_line(program: &Path, args: &[std::ffi::OsString]) -> String {
    let quote = |text: &str| format!("'{}'", text.replace('\'', "'\\''"));
    let mut parts = vec![quote(&program.to_string_lossy())];
    parts.extend(args.iter().map(|arg| quote(&arg.to_string_lossy())));
    parts.join(" ")
}

fn says_context_was_kept(answer: &str) -> bool {
    let lower = answer.to_lowercase();
    let has_ok = lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| word == "ok");
    has_ok || lower.contains("reply")
}

fn run_turns(kind: CliKind, plan_start: SpawnPlan, ctx: &StartContext) {
    let sock = ctx.socket.clone();
    let (receiver, messages) = HookReceiver::bind(&sock).expect("bind");
    // The operator command that trusts the hooks one time: the same hooks, and no prompt.
    let trust_line = {
        let mut args = plan_start.spec.args.clone();
        args.pop();
        shell_line(&plan_start.spec.program, &args)
    };
    let mut run = LiveRun::start(plan_start.spec);

    let first = match expect_turn(&mut run, &messages, "first turn") {
        Ok(events) => events,
        Err(text) => {
            let blocked = hooks_dialog_shown(&run.output);
            run.stop(&messages, SESSION);
            drop(receiver);
            if blocked {
                panic!(
                    "BLOCKED BY A STARTUP DIALOG (the expected one-time operator step). {text}. \
                     The {kind:?} CLI fires no hook until the operator answers it. To do it: \
                     run this command in a terminal inside a git repository, choose \
                     \"2. Trust all and continue\", then press Ctrl-C twice, and run this test \
                     again:\n{trust_line}"
                );
            }
            panic!("{text}");
        }
    };
    let (start, submit, stop) = first;
    let cli_session_id = match kind {
        CliKind::Claude => plan_cli_id(&start.event),
        CliKind::Codex => {
            cli_session_id_from_event(&start.event).expect("session_id in the first event")
        }
    };
    println!("{kind:?}: first turn events {:?}", run.event_names());
    println!("{kind:?}: CLI session ID {cli_session_id}");
    println!(
        "{kind:?}: UserPromptSubmit permission_mode = {}",
        submit.event["permission_mode"]
    );
    println!(
        "{kind:?}: first answer = {}",
        stop.event["last_assistant_message"]
    );
    let turn = FirstTurn {
        cli_session_id,
        names: run.event_names(),
        user_prompt_event: submit.event.clone(),
    };
    assert_eq!(
        &turn.names[..3],
        ["SessionStart", "UserPromptSubmit", "Stop"]
    );
    assert!(
        turn.user_prompt_event["prompt"]
            .as_str()
            .is_some_and(|p| p.contains(FIRST))
    );
    for message in [&start, &submit, &stop] {
        assert_eq!(
            message.event["session_id"].as_str(),
            Some(turn.cli_session_id.as_str())
        );
    }

    run.stop(&messages, SESSION);
    assert!(run.exit.is_some(), "the first process did not end");
    println!("{kind:?}: events after stop {:?}", &run.event_names()[3..]);
    drop(run);

    // Resume through the adapter, with the second message as the prompt argument.
    let mut resume_ctx = ctx.clone();
    resume_ctx.first_message = Some(SECOND.into());
    let plan = match kind {
        CliKind::Claude => ClaudeAdapter.resume(&resume_ctx, &turn.cli_session_id),
        CliKind::Codex => CodexAdapter.resume(&resume_ctx, &turn.cli_session_id),
    }
    .expect("resume plan");
    assert_eq!(
        plan.cli_session_id.as_deref(),
        Some(turn.cli_session_id.as_str())
    );
    let mut run = LiveRun::start(plan.spec);
    let second = expect_turn(&mut run, &messages, "resumed turn");
    let events = run.event_names();
    run.stop(&messages, SESSION);
    drop(receiver);
    let (start2, submit2, stop2) = second.unwrap_or_else(|text| panic!("{text}"));
    println!("{kind:?}: resumed events {events:?}");
    println!(
        "{kind:?}: SessionStart source on resume = {}",
        start2.event["source"]
    );
    println!(
        "{kind:?}: second answer = {}",
        stop2.event["last_assistant_message"]
    );
    assert_eq!(
        start2.event["session_id"].as_str(),
        Some(turn.cli_session_id.as_str())
    );
    assert_eq!(
        submit2.event["session_id"].as_str(),
        Some(turn.cli_session_id.as_str())
    );
    let answer = stop2.event["last_assistant_message"]
        .as_str()
        .unwrap_or_default();
    assert!(
        says_context_was_kept(answer),
        "the answer does not show the first message: {answer:?}"
    );
}

fn plan_cli_id(event: &Value) -> String {
    event["session_id"].as_str().expect("session_id").to_owned()
}

#[test]
#[ignore = "calls a real model (claude haiku)"]
fn live_claude_start_then_resume_keeps_the_context() {
    let Some(repo) = trusted_worktree() else {
        panic!("no trusted repository: the live Claude test cannot run");
    };
    let sock_dir = TempDir::new().expect("socket folder");
    let sock = sock_dir.path().join("h.sock");
    let Some(mut ctx) = live_ctx(CliKind::Claude, &repo.path(), &sock) else {
        panic!("claude or codex is not installed");
    };
    ctx.employee.model = Some("haiku".into());
    ctx.employee.effort = None;
    // `read-only` gives `--permission-mode manual`: the payload shows what Claude makes of it.
    ctx.employee.permission = PermissionPolicy::ReadOnly;
    let plan = ClaudeAdapter.start(&ctx).expect("plan");
    let chosen = plan.cli_session_id.clone().expect("ID");
    run_turns(CliKind::Claude, plan, &ctx);
    println!("Claude: the app chose the CLI session ID {chosen}");
}

#[test]
#[ignore = "calls a real model (codex gpt-6-luna, low effort); the weekly limit is shared"]
fn live_codex_start_then_resume_keeps_the_context() {
    let dir = TempDir::new().expect("folder");
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(dir.path())
        .status()
        .expect("git init");
    assert!(status.success());
    let sock_dir = TempDir::new().expect("socket folder");
    let sock = sock_dir.path().join("h.sock");
    let Some(mut ctx) = live_ctx(CliKind::Codex, dir.path(), &sock) else {
        panic!("claude or codex is not installed");
    };
    ctx.employee.model = Some("gpt-6-luna".into());
    ctx.employee.effort = Some("low".into());
    let plan = CodexAdapter.start(&ctx).expect("plan");
    assert_eq!(plan.cli_session_id, None);
    run_turns(CliKind::Codex, plan, &ctx);
}
