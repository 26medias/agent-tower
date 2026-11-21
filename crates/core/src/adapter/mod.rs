//! CLI adapters (spec 4.2 start rules, 4.3 hooks, 4.6 instructions, 4.7 permission policy).
//!
//! An adapter turns an employee and a start context into a [`SpawnPlan`]: the absolute program,
//! the argument vector (no shell), the exact environment and the working folder. The adapter
//! does no I/O and reads no process state, so tests compare exact vectors.
//!
//! Rules that both adapters follow:
//! - The program is an absolute path. The environment comes from `build_env` with `TOWER_SESSION`
//!   and `TOWER_SOCKET`. The PATH is the folders of the programs, then the parent PATH, then
//!   `/usr/bin:/bin`.
//! - The hook command text is the absolute path of `tower-hook` and nothing more. It is the same
//!   text for each session and each event. The identity of the session goes in the environment
//!   (spec 4.2, "Codex hook trust"). A path with a character that a shell would change is put in
//!   single quotes; the text is still the same for each session.
//! - The first message is the last positional argument of the CLI, never typed afterwards.

mod claude;
mod codex;

use crate::env::{EnvError, EnvMap, build_env};
use crate::model::{CliKind, Employee};
use crate::pty::{LogConfig, SpawnSpec};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub use claude::{ClaudeAdapter, permission_mode as claude_permission_mode};
pub use codex::{CodexAdapter, cli_session_id_from_event};

pub type Result<T> = std::result::Result<T, AdapterError>;

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    /// Codex has two approval modes only (`on-request` and `never`), so `ask-always` does not
    /// exist there (spec 4.7).
    #[error("the policy ask-always does not exist for Codex (spec 4.7)")]
    AskAlwaysUnsupported,
    /// A path that must be absolute is not.
    #[error("{what} must be an absolute path: {}", path.display())]
    NotAbsolute { what: &'static str, path: PathBuf },
    /// A path that goes into the settings text is not valid UTF-8.
    #[error("{what} is not valid UTF-8: {}", path.display())]
    NotUtf8 { what: &'static str, path: PathBuf },
    /// A text with a NUL byte cannot be an argument.
    #[error("{what} has a NUL byte")]
    NulByte { what: &'static str },
    /// The app session ID is empty.
    #[error("the app session ID is empty")]
    EmptySessionId,
    /// A CLI session ID that is not usable as a command line argument.
    #[error("{0:?} is not a usable CLI session ID")]
    BadCliSessionId(String),
    #[error(transparent)]
    Env(#[from] EnvError),
}

/// Absolute paths of the programs that the app starts (from `programs::Resolver`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Programs {
    pub claude: PathBuf,
    pub codex: PathBuf,
    /// `codex` is a script that needs `node` on the PATH.
    pub node: Option<PathBuf>,
}

/// The name of the MCP server of the app, in the configuration of each CLI. The tools show as
/// `mcp__tower__<tool>` in Claude.
pub const MCP_SERVER_NAME: &str = "tower";

/// The environment variable that holds the token of a session. Neither CLI gets the token in an
/// argument: Claude expands `${TOWER_MCP_TOKEN}` in the header, and Codex reads the variable
/// named by `bearer_token_env_var`.
pub const MCP_TOKEN_ENV: &str = "TOWER_MCP_TOKEN";

/// How a session reaches the MCP server of the app (spec 4.4).
#[derive(Clone, PartialEq, Eq)]
pub struct McpAccess {
    /// For example `http://127.0.0.1:41234/mcp`.
    pub url: String,
    /// The secret of this one session. It is never part of a log text or an error text.
    pub token: String,
}

impl std::fmt::Debug for McpAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The token stays out of every debug text.
        f.debug_struct("McpAccess")
            .field("url", &self.url)
            .field("token", &"<hidden>")
            .finish()
    }
}

/// All that an adapter needs to start or resume one session.
#[derive(Debug, Clone)]
pub struct StartContext {
    pub employee: Employee,
    /// The working folder of the process. It must be absolute.
    pub cwd: PathBuf,
    /// The ID that the app gives to the session. It goes in `TOWER_SESSION`.
    pub app_session_id: String,
    /// The Unix socket of the hook receiver. It goes in `TOWER_SOCKET`.
    pub socket: PathBuf,
    /// Absolute path of `tower-hook`.
    pub hook_program: PathBuf,
    pub programs: Programs,
    /// The parent environment of the session. `build_env` copies it, minus the names that stay
    /// out. The app gives the login shell environment over its own; tests give a fixed map.
    pub parent_env: EnvMap,
    /// Becomes the last positional argument of the CLI.
    pub first_message: Option<String>,
    /// The instructions of the session (spec 4.6). The caller builds the text, role included.
    pub instructions: Option<String>,
    pub rows: u16,
    pub cols: u16,
    pub log: Option<LogConfig>,
    /// The MCP server of the app for this session. `None` for the setup session.
    pub mcp: Option<McpAccess>,
    /// The floor folder is the top of a git repository, so the app makes a worktree for each task
    /// that it gives to an employee with the policy `edit-in-worktree` (spec 4.8). Claude then
    /// asks the operator for an edit in the main checkout. With `false` (no repository), the
    /// mode `acceptEdits` stays for that policy, as in M1.
    pub worktrees: bool,
    /// The rules of the floor for commands with no question (from the app config). The caller has checked them
    /// (`model::valid_allow_rules`). Claude only. They are not used for the policy `ask-always`.
    pub floor_allow: Vec<String>,
}

/// What to start, and what the app learns at once.
#[derive(Debug, Clone)]
pub struct SpawnPlan {
    pub spec: SpawnSpec,
    /// The CLI session ID when the app chooses it (Claude). Codex gives `None`: the ID comes in
    /// the first hook event (see `cli_session_id_from_event`).
    pub cli_session_id: Option<String>,
}

/// Starts and resumes sessions of one CLI.
pub trait CliAdapter {
    fn start(&self, ctx: &StartContext) -> Result<SpawnPlan>;
    fn resume(&self, ctx: &StartContext, cli_session_id: &str) -> Result<SpawnPlan>;
}

/// The adapter of a CLI kind.
pub fn adapter_for(kind: CliKind) -> &'static dyn CliAdapter {
    match kind {
        CliKind::Claude => &ClaudeAdapter,
        CliKind::Codex => &CodexAdapter,
    }
}

// ---- helpers for both adapters ----

/// `Some(text)` when the option holds a text that is not empty.
fn non_empty(text: &Option<String>) -> Option<&str> {
    text.as_deref().filter(|text| !text.is_empty())
}

fn check_no_nul(what: &'static str, text: &str) -> Result<()> {
    if text.contains('\0') {
        return Err(AdapterError::NulByte { what });
    }
    Ok(())
}

fn require_absolute(what: &'static str, path: &Path) -> Result<()> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(AdapterError::NotAbsolute {
            what,
            path: path.to_path_buf(),
        })
    }
}

fn path_str<'a>(what: &'static str, path: &'a Path) -> Result<&'a str> {
    path.to_str().ok_or_else(|| AdapterError::NotUtf8 {
        what,
        path: path.to_path_buf(),
    })
}

/// A CLI session ID goes into the argument list. It must not look like a flag.
fn check_cli_session_id(id: &str) -> Result<()> {
    let good = !id.is_empty()
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if good {
        Ok(())
    } else {
        Err(AdapterError::BadCliSessionId(id.to_owned()))
    }
}

/// The command text of each hook: the absolute path of `tower-hook`, and nothing more.
/// A path with a character that a shell would change gets single quotes.
fn hook_command_text(hook_program: &Path) -> Result<String> {
    require_absolute("the hook program", hook_program)?;
    let text = path_str("the hook program", hook_program)?;
    check_no_nul("the hook program path", text)?;
    Ok(shell_word(text))
}

fn shell_word(text: &str) -> String {
    let safe = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._+-:@%,=~".contains(c));
    if safe {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

/// A TOML basic string (with the quotes). Escapes `"`, `\` and each control character.
pub(crate) fn toml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The checks and the environment that both adapters share, then the spec.
fn make_spec(ctx: &StartContext, program: &Path, args: Vec<OsString>) -> Result<SpawnSpec> {
    require_absolute("the program", program)?;
    require_absolute("the working folder", &ctx.cwd)?;
    require_absolute("the socket path", &ctx.socket)?;
    if ctx.app_session_id.is_empty() {
        return Err(AdapterError::EmptySessionId);
    }

    let mut path_dirs: Vec<PathBuf> = Vec::new();
    let tools = [
        Some(&ctx.programs.claude),
        Some(&ctx.programs.codex),
        ctx.programs.node.as_ref(),
    ];
    for tool in tools.into_iter().flatten() {
        if let Some(dir) = tool.parent() {
            path_dirs.push(dir.to_path_buf());
        }
    }
    let extra = [
        (
            OsString::from("TOWER_SESSION"),
            OsString::from(&ctx.app_session_id),
        ),
        (
            OsString::from("TOWER_SOCKET"),
            ctx.socket.clone().into_os_string(),
        ),
    ];
    let mut extra = extra.to_vec();
    if let Some(mcp) = &ctx.mcp {
        check_no_nul("the MCP token", &mcp.token)?;
        extra.push((OsString::from(MCP_TOKEN_ENV), OsString::from(&mcp.token)));
    }
    let env = build_env(&ctx.parent_env, &extra, &path_dirs)?;
    Ok(SpawnSpec {
        program: program.to_path_buf(),
        args,
        env,
        cwd: ctx.cwd.clone(),
        rows: ctx.rows,
        cols: ctx.cols,
        log: ctx.log.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(value: &str) -> toml::Value {
        let table: toml::Table = toml::from_str(&format!("x = {value}")).expect("valid TOML");
        table["x"].clone()
    }

    #[test]
    fn toml_string_round_trips() {
        let samples = [
            "plain",
            "with \"quotes\"",
            "back\\slash",
            "line one\nline two\r\n\ttab",
            "bell\u{7} nul-less \u{1} \u{1f} del\u{7f}",
            "unicode: é 日本 🙂",
            "\\\"",
            "",
        ];
        for sample in samples {
            let parsed = parse(&toml_string(sample));
            assert_eq!(parsed.as_str(), Some(sample), "sample {sample:?}");
        }
    }

    #[test]
    fn shell_word_is_plain_for_a_safe_path() {
        assert_eq!(
            shell_word("/opt/tower/bin/tower-hook"),
            "/opt/tower/bin/tower-hook"
        );
        assert_eq!(shell_word("/a b/hook"), "'/a b/hook'");
        assert_eq!(shell_word("/it's/hook"), "'/it'\\''s/hook'");
    }
}
