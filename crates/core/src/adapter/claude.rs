//! The Claude Code adapter.
//!
//! Hooks come with `--settings <json>` (PROBED at gate 1 and step 0). Each hook command is the
//! text of the hook program, the same for all sessions and events. The event name is inside the
//! payload as `hook_event_name`.
//!
//! The permission mapping is deliberately simple for M1 (spec item N12 is open):
//!
//! | Policy | `--permission-mode` |
//! |---|---|
//! | `read-only` | `manual` |
//! | `ask-always` | `manual` |
//! | `edit-in-worktree` | `acceptEdits` |
//!
//! `plan` is not used for `read-only`. In `plan` mode the agent writes a plan and then asks the
//! operator to leave the mode, so each CEO or Project Manager turn would end in a prompt. Also,
//! whether `plan` mode allows the MCP tools of the app is NOT TRACED. So `read-only` is only
//! as strong as the operator who answers each prompt in `manual` mode. `acceptEdits` accepts
//! file edits in any folder that the session can reach, not only in its worktree; other
//! actions still prompt.

use super::{
    AdapterError, CliAdapter, MCP_SERVER_NAME, MCP_TOKEN_ENV, McpAccess, Result, SpawnPlan,
    StartContext, check_cli_session_id, check_no_nul, hook_command_text, make_spec, non_empty,
};
use crate::model::PermissionPolicy;
use std::ffi::OsString;
use std::path::Path;
use uuid::Uuid;

/// The events that the app listens to. `SubagentStop` is not here: spec 4.3 ignores it.
/// The tool events get the matcher `*`, as in the probes.
const HOOK_EVENTS: [(&str, bool); 8] = [
    ("SessionStart", false),
    ("UserPromptSubmit", false),
    ("PreToolUse", true),
    ("PostToolUse", true),
    ("PermissionRequest", true),
    ("Notification", false),
    ("Stop", false),
    ("SessionEnd", false),
];

pub struct ClaudeAdapter;

/// The `--permission-mode` value of a policy. See the module text for the limits.
pub fn permission_mode(policy: PermissionPolicy) -> &'static str {
    match policy {
        PermissionPolicy::ReadOnly | PermissionPolicy::AskAlways => "manual",
        PermissionPolicy::EditInWorktree => "acceptEdits",
    }
}

/// The JSON text for `--mcp-config`. The token is NOT in the text: Claude expands
/// `${TOWER_MCP_TOKEN}` in a header from the environment of the session (PROBED, Claude 2.1.330).
/// So the token is not in the argument list, which any process of the same user can read.
fn mcp_config_json(mcp: &McpAccess) -> String {
    let url = serde_json::Value::String(mcp.url.clone());
    format!(
        r#"{{"mcpServers":{{"{MCP_SERVER_NAME}":{{"type":"http","url":{url},"headers":{{"Authorization":"Bearer ${{{MCP_TOKEN_ENV}}}"}}}}}}}}"#
    )
}

/// The files that the app reads under `.swarm/` of the floor, as the path part of a rule. An
/// employee must not edit them: the start rule reads `employees/*.md` (the `manager` field
/// gives authority), and `floor.toml` holds the defaults. `reports/` and `worktrees/` are not
/// here: the employees write there (spec 4.9, 4.10).
const PROTECTED_SWARM_PATHS: [&str; 5] = [
    "floor.toml",
    ".gitignore",
    "employees/**",
    "assets/**",
    "state/**",
];

/// The deny rules for an employee session: the file tools of Claude (`Edit` is the rule for
/// every tool that edits a file: Edit, Write, MultiEdit, NotebookEdit; PROBED with `claude -p`,
/// 2.1.330) cannot change the protected files. Each file has two rules: the absolute path
/// (`//` is the root of the file system in a rule), and the path relative to the working
/// folder. The relative rule still holds when the absolute path has a character that a rule
/// reads as a pattern (`[`, `*`, `?`). `cwd` is the floor folder: a session starts in it.
///
/// Limits (NOT closed here): a shell command can still write these files. In the modes `manual`
/// and `acceptEdits` a shell command asks the operator first.
fn deny_rules(cwd: &Path) -> Vec<String> {
    let cwd = cwd.to_string_lossy();
    let root = cwd.trim_start_matches('/').trim_end_matches('/');
    PROTECTED_SWARM_PATHS
        .iter()
        .flat_map(|path| {
            [
                format!("Edit(//{root}/.swarm/{path})"),
                format!("Edit(./.swarm/{path})"),
            ]
        })
        .collect()
}

/// The JSON text for `--settings`: the hooks, and, with an MCP server, the permission to use the
/// tools of that one server with no prompt (`mcp__tower` is the rule for a whole server) and the
/// deny rules for the files under `.swarm/` (`deny_rules`). No other tool is allowed. Key order
/// is fixed, so snapshots are exact.
fn settings_json(hook_command: &str, with_mcp: bool, cwd: &Path) -> String {
    let command = serde_json::Value::String(hook_command.to_owned()).to_string();
    let entries: Vec<String> = HOOK_EVENTS
        .iter()
        .map(|(event, matcher)| {
            let matcher = if *matcher { r#""matcher":"*","# } else { "" };
            format!(
                r#""{event}":[{{{matcher}"hooks":[{{"type":"command","command":{command}}}]}}]"#
            )
        })
        .collect();
    let permissions = if with_mcp {
        let deny = serde_json::Value::from(deny_rules(cwd)).to_string();
        format!(r#","permissions":{{"allow":["mcp__{MCP_SERVER_NAME}"],"deny":{deny}}}"#)
    } else {
        String::new()
    };
    format!(r#"{{"hooks":{{{}}}{permissions}}}"#, entries.join(","))
}

impl ClaudeAdapter {
    /// Start with a session ID that the caller chose. `start` uses a new random UUID.
    pub fn start_with_id(&self, ctx: &StartContext, session_id: &str) -> Result<SpawnPlan> {
        let uuid = Uuid::parse_str(session_id)
            .map_err(|_| AdapterError::BadCliSessionId(session_id.to_owned()))?;
        let id = uuid.hyphenated().to_string();
        let args = build_args(ctx, &["--session-id", &id])?;
        Ok(SpawnPlan {
            spec: make_spec(ctx, &ctx.programs.claude, args)?,
            cli_session_id: Some(id),
        })
    }
}

impl CliAdapter for ClaudeAdapter {
    fn start(&self, ctx: &StartContext) -> Result<SpawnPlan> {
        self.start_with_id(ctx, &Uuid::new_v4().to_string())
    }

    fn resume(&self, ctx: &StartContext, cli_session_id: &str) -> Result<SpawnPlan> {
        check_cli_session_id(cli_session_id)?;
        let args = build_args(ctx, &["--resume", cli_session_id])?;
        Ok(SpawnPlan {
            spec: make_spec(ctx, &ctx.programs.claude, args)?,
            cli_session_id: Some(cli_session_id.to_owned()),
        })
    }
}

fn build_args(ctx: &StartContext, session_args: &[&str]) -> Result<Vec<OsString>> {
    let hook_command = hook_command_text(&ctx.hook_program)?;
    let mut args: Vec<OsString> = session_args.iter().map(OsString::from).collect();
    if let Some(mcp) = &ctx.mcp {
        check_no_nul("the MCP url", &mcp.url)?;
        // `--mcp-config` takes a list of values. A flag must follow it, or the next positional
        // argument (the first message) would become a config. `--settings` always follows.
        args.push("--mcp-config".into());
        args.push(mcp_config_json(mcp).into());
    }
    args.push("--settings".into());
    args.push(settings_json(&hook_command, ctx.mcp.is_some(), &ctx.cwd).into());
    args.push("--permission-mode".into());
    args.push(permission_mode(ctx.employee.permission).into());
    if let Some(model) = non_empty(&ctx.employee.model) {
        check_no_nul("the model", model)?;
        args.push("--model".into());
        args.push(model.into());
    }
    if let Some(effort) = non_empty(&ctx.employee.effort) {
        check_no_nul("the effort", effort)?;
        args.push("--effort".into());
        args.push(effort.into());
    }
    if let Some(instructions) = non_empty(&ctx.instructions) {
        check_no_nul("the instructions", instructions)?;
        args.push("--append-system-prompt".into());
        args.push(instructions.into());
    }
    if let Some(message) = non_empty(&ctx.first_message) {
        check_no_nul("the first message", message)?;
        // `--` stops flag parsing, so a message that starts with `-` stays a message.
        if message.starts_with('-') {
            args.push("--".into());
        }
        args.push(message.into());
    }
    Ok(args)
}
