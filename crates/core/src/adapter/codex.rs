//! The Codex adapter.
//!
//! Hooks come with `-c 'hooks.<Event>=[...]'`; the trust of the working folder comes with
//! `-c 'projects={"<cwd>"={trust_level="trusted"}}'` (the inline-table form; the dotted form
//! does not work). Codex has no flag for the session ID: it comes in the first hook event.
//!
//! Permission mapping (decision O7). Two cases, chosen by `StartContext::codex_sandbox` (the
//! switch "Use the Codex sandbox" in Settings, off by default):
//!
//! - Off (option B): each policy that exists gets `-a on-request -s danger-full-access`. The CLI
//!   does not enforce `read-only` or `edit-in-worktree`; only the instructions limit the agent.
//! - On (option A, needs the AppArmor profile for `bwrap`; NOT run live, see `docs/spec.md` 4.7):
//!   `-a on-request` and a sandbox that the CLI enforces. An action outside the sandbox asks the
//!   operator (`PermissionRequest`).
//!
//!   | Policy | Sandbox flags |
//!   |---|---|
//!   | `read-only` | a permission profile that extends `:read-only`, and `write` only for `.swarm/reports` |
//!   | `edit-in-worktree`, git floor with worktrees | the same profile, with `write` for `.swarm/worktrees`, `.swarm/reports`, `:slash_tmp` and `:tmpdir` |
//!   | `edit-in-worktree`, no git repository | `-s workspace-write` (the floor folder is the workspace) |
//!
//!   The mode `-s workspace-write` always makes the working folder writable, and the session runs
//!   in the main checkout, so a git floor needs the profile: it is the only way to keep the main
//!   checkout read-only (PROBED with `codex debug prompt-input`, which renders the sandbox that
//!   Codex would use and starts no model). The profile is a beta feature of Codex.
//!
//! `ask-always` does not exist for Codex in either case: the adapter returns an error. `-a`
//! has `on-request` and `never` only. `approval_policy = "untrusted"` ends with "no longer
//! supported". `granular` only chooses which prompts may show; it does not ask before each action.
//!
//! Each `-c` value is TOML. The CLI uses the raw text when TOML parsing fails, so a wrong
//! escape does not always give an error. Text values use `toml_string`.

use super::{
    AdapterError, CliAdapter, MCP_SERVER_NAME, MCP_TOKEN_ENV, Result, SpawnPlan, StartContext,
    check_cli_session_id, check_no_nul, hook_command_text, make_spec, non_empty, path_str,
    toml_string,
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
    if let Some(mcp) = &ctx.mcp {
        check_no_nul("the MCP url", &mcp.url)?;
        // The token is not in the arguments: Codex reads it from the environment variable.
        // `default_tools_approval_mode="approve"` stops the question before each tool call.
        push_config(
            &mut args,
            format!(
                "mcp_servers.{MCP_SERVER_NAME}.url={}",
                toml_string(&mcp.url)
            ),
        );
        push_config(
            &mut args,
            format!(
                "mcp_servers.{MCP_SERVER_NAME}.bearer_token_env_var={}",
                toml_string(MCP_TOKEN_ENV)
            ),
        );
        push_config(
            &mut args,
            format!("mcp_servers.{MCP_SERVER_NAME}.default_tools_approval_mode=\"approve\""),
        );
    }
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
    push_permissions(&mut args, ctx, cwd);
    Ok(args)
}

/// The name of the permission profile that the adapter defines with `-c`. A profile of the
/// operator in `~/.codex/config.toml` with the same name would be replaced for the session.
pub const PROFILE_NAME: &str = "agent_tower";

/// The approval flags and the sandbox flags of the policy (see the module text). `cwd` is the
/// working folder, which is the floor folder.
fn push_permissions(args: &mut Vec<OsString>, ctx: &StartContext, cwd: &str) {
    args.push("-a".into());
    args.push("on-request".into());
    if !ctx.codex_sandbox {
        args.push("-s".into());
        args.push("danger-full-access".into());
        return;
    }
    let cwd = cwd.trim_end_matches('/');
    let reports = format!("{cwd}/.swarm/reports");
    let worktrees = format!("{cwd}/.swarm/worktrees");
    match ctx.employee.permission {
        PermissionPolicy::ReadOnly => push_profile(args, &[(&reports, "write")]),
        PermissionPolicy::EditInWorktree if ctx.worktrees => push_profile(
            args,
            &[
                (&reports, "write"),
                (&worktrees, "write"),
                (":slash_tmp", "write"),
                (":tmpdir", "write"),
            ],
        ),
        PermissionPolicy::EditInWorktree => {
            args.push("-s".into());
            args.push("workspace-write".into());
        }
        // `build_args` refuses `ask-always` before it gets here.
        PermissionPolicy::AskAlways => {}
    }
}

/// A permission profile that starts from `:read-only` (the filesystem is readable) and gives
/// `write` to the listed paths. A path or a special token is a key of the `filesystem` table.
/// Without `extends`, Codex does not read the project files through the sandbox.
fn push_profile(args: &mut Vec<OsString>, writes: &[(&str, &str)]) {
    push_config(
        args,
        format!("default_permissions={}", toml_string(PROFILE_NAME)),
    );
    push_config(
        args,
        format!("permissions.{PROFILE_NAME}.extends=\":read-only\""),
    );
    let entries: Vec<String> = writes
        .iter()
        .map(|(path, access)| format!("{}={}", toml_string(path), toml_string(access)))
        .collect();
    push_config(
        args,
        format!(
            "permissions.{PROFILE_NAME}.filesystem={{{}}}",
            entries.join(",")
        ),
    );
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
