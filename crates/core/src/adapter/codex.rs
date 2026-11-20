//! The Codex adapter.
//!
//! Hooks come with `-c 'hooks.<Event>=[...]'`; the trust of the working folder comes with
//! `-c 'projects={"<cwd>"={trust_level="trusted"}}'` (the inline-table form; the dotted form
//! does not work). Codex has no flag for the session ID: it comes in the first hook event.
//!
//! Permission mapping (decision O7, option B): the Codex sandbox does not work on this machine,
//! so each policy that exists gets `-a on-request -s danger-full-access`. The CLI does not
//! enforce `read-only` or `edit-in-worktree`; only the instructions limit the agent. `ask-always`
//! does not exist for Codex: the adapter returns an error.
//!
//! Each `-c` value is TOML. The CLI uses the raw text when TOML parsing fails, so a wrong
//! escape does not always give an error. Text values use `toml_string`.

use super::{
    AdapterError, CliAdapter, Result, SpawnPlan, StartContext, check_cli_session_id, check_no_nul,
    hook_command_text, make_spec, non_empty, path_str, toml_string,
};
use crate::model::PermissionPolicy;
use serde_json::Value;
use std::ffi::OsString;

/// The events that Codex has. `SessionEnd` and `Interrupt` fired in the step 0 probes.
const HOOK_EVENTS: [&str; 8] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Stop",
    "Interrupt",
    "SessionEnd",
];

pub struct CodexAdapter;

/// The session ID of a Codex hook payload (`session_id`). Each Codex hook event has it; the
/// first one (`SessionStart`, at the first prompt) gives the ID that resume needs.
pub fn cli_session_id_from_event(event: &Value) -> Option<String> {
    event
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

/// One `-c` value for the hooks of one event.
fn hook_config(event: &str, hook_command: &str) -> String {
    format!(
        "hooks.{event}=[{{hooks=[{{type=\"command\",command={}}}]}}]",
        toml_string(hook_command)
    )
}

impl CliAdapter for CodexAdapter {
    fn start(&self, ctx: &StartContext) -> Result<SpawnPlan> {
        let mut args = build_args(ctx)?;
        push_message(&mut args, ctx)?;
        Ok(SpawnPlan {
            spec: make_spec(ctx, &ctx.programs.codex, args)?,
            cli_session_id: None,
        })
    }

    fn resume(&self, ctx: &StartContext, cli_session_id: &str) -> Result<SpawnPlan> {
        check_cli_session_id(cli_session_id)?;
        let mut args = build_args(ctx)?;
        args.push("resume".into());
        args.push(cli_session_id.into());
        push_message(&mut args, ctx)?;
        Ok(SpawnPlan {
            spec: make_spec(ctx, &ctx.programs.codex, args)?,
            cli_session_id: Some(cli_session_id.to_owned()),
        })
    }
}

/// The arguments before the subcommand and the prompt.
fn build_args(ctx: &StartContext) -> Result<Vec<OsString>> {
    if ctx.employee.permission == PermissionPolicy::AskAlways {
        return Err(AdapterError::AskAlwaysUnsupported);
    }
    let hook_command = hook_command_text(&ctx.hook_program)?;
    let cwd = path_str("the working folder", &ctx.cwd)?;
    check_no_nul("the working folder", cwd)?;

    let mut args: Vec<OsString> = vec!["--no-daemon".into()];
    for event in HOOK_EVENTS {
        push_config(&mut args, hook_config(event, &hook_command));
    }
    push_config(
        &mut args,
        format!(
            "projects={{{}={{trust_level=\"trusted\"}}}}",
            toml_string(cwd)
        ),
    );
    push_config(&mut args, "check_for_update_on_startup=false".to_owned());
    if let Some(instructions) = non_empty(&ctx.instructions) {
        check_no_nul("the instructions", instructions)?;
        push_config(
            &mut args,
            format!("developer_instructions={}", toml_string(instructions)),
        );
    }
    if let Some(model) = non_empty(&ctx.employee.model) {
        check_no_nul("the model", model)?;
        args.push("-m".into());
        args.push(model.into());
    }
    if let Some(effort) = non_empty(&ctx.employee.effort) {
        check_no_nul("the effort", effort)?;
        push_config(
            &mut args,
            format!("model_reasoning_effort={}", toml_string(effort)),
        );
    }
    for arg in ["-a", "on-request", "-s", "danger-full-access"] {
        args.push(arg.into());
    }
    Ok(args)
}

fn push_config(args: &mut Vec<OsString>, value: String) {
    args.push("-c".into());
    args.push(value.into());
}

fn push_message(args: &mut Vec<OsString>, ctx: &StartContext) -> Result<()> {
    if let Some(message) = non_empty(&ctx.first_message) {
        check_no_nul("the first message", message)?;
        // `--` stops flag parsing, so a message that starts with `-` stays a message.
        if message.starts_with('-') {
            args.push("--".into());
        }
        args.push(message.into());
    }
    Ok(())
}
