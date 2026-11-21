//! Data types of the store. Other steps add to this file.
//!
//! The status state machine is not here. `status.rs` owns it. A session record keeps the last
//! status as a plain string.

use serde::{Deserialize, Serialize};

/// Which CLI an employee runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CliKind {
    Claude,
    Codex,
}

/// What an employee may do without asking (spec 4.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionPolicy {
    ReadOnly,
    EditInWorktree,
    AskAlways,
}

/// Default values for new employees of a floor. A field that is `None` is not in the file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FloorDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli: Option<CliKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<PermissionPolicy>,
}

/// The `[brand]` table of `floor.toml`. A field that is `None` is not in the file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Brand {
    /// One sentence that says what the project is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The subject of the logo, with no style words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_prompt: Option<String>,
}

/// The `[permissions]` table of `floor.toml` (changed in M3, 2026-11-21): rules that the app adds
/// to the allow list of each Claude employee, so that a command needs no question. The operator
/// sets it in Settings. An employee cannot change it (`floor.toml` is a denied file). A rule
/// must match the command as Claude sees it after the hooks of the operator. On a machine with a
/// hook that rewrites commands (for example RTK), `cargo test` becomes `rtk cargo test`, so the
/// rule names that form.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FloorPermissions {
    /// Claude permission rules, for example `Bash(cargo test:*)`. See `check_allow_rule`.
    pub allow: Vec<String>,
}

/// The data of `.swarm/floor.toml`, without the file version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Floor {
    pub name: String,
    pub defaults: FloorDefaults,
    pub brand: Brand,
    pub permissions: FloorPermissions,
}

/// The most rules in `[permissions] allow`.
pub const ALLOW_RULES_MAX: usize = 100;

/// The most characters in one rule.
pub const ALLOW_RULE_MAX_CHARS: usize = 200;

/// The tools that a rule of `[permissions] allow` can name with a pattern in brackets.
const RULE_TOOLS: [&str; 6] = ["Bash", "Read", "Glob", "Grep", "WebFetch", "WebSearch"];

/// The tools whose pattern is a path in the project folder.
const PATH_TOOLS: [&str; 3] = ["Read", "Glob", "Grep"];

/// Commands that run other commands, or that destroy data or send data out. A rule that allows
/// one of these with no question gives the agent a way around every other rule. So the app does
/// not accept a rule for them (checked on the name of the program, with no folder in front).
/// `git` is a special case: see `GIT_BLOCKED_SUBCOMMANDS`.
const BASH_BLOCKED_PROGRAMS: [&str; 32] = [
    "sh", "bash", "zsh", "dash", "fish", "env", "xargs", "eval", "exec", "sudo", "su", "doas",
    "nohup", "time", "timeout", "nice", "python", "python3", "node", "perl", "ruby", "php", "rm",
    "dd", "mkfs", "chmod", "chown", "curl", "wget", "ssh", "scp", "nc",
];

/// The `git` subcommands that change or destroy work. A rule for `git` needs a subcommand
/// (`Bash(git status:*)`), and it is not one of these.
const GIT_BLOCKED_SUBCOMMANDS: [&str; 5] = ["push", "reset", "clean", "checkout", "config"];

/// Check one rule of `[permissions] allow`. `Err` is the text for the operator.
///
/// The whole text of a rule is 1 to 200 printable ASCII characters (0x20 to 0x7E). It is one of:
/// - `WebSearch` alone;
/// - `Tool(pattern)` with `Tool` one of `Bash`, `Read`, `Glob`, `Grep`, `WebFetch`. The pattern has
///   no `(` and no `)`, so one entry is always one rule.
///
/// The pattern depends on the tool:
/// - `Bash`: the first word (up to the first space or `:`) is a plain program name of
///   `[A-Za-z0-9._/+-]`. It is not a program of `BASH_BLOCKED_PROGRAMS`, and for `git` the
///   subcommand is named and is not one of `GIT_BLOCKED_SUBCOMMANDS`. The pattern does not start
///   with a space.
/// - `Read`, `Glob`, `Grep`: a path that is relative to the project (it does not start with `/`
///   or `~`) and has no `..` part. These tools alone (no pattern) are refused: they open the
///   whole disk.
/// - `WebFetch`: only `domain:<host>`, with a host of `[A-Za-z0-9.-]` that has a dot.
/// - `WebSearch`: no pattern.
///
/// A rule must match the command as Claude sees it after the hooks of the operator. On a machine
/// with a hook that rewrites commands (for example RTK), `cargo test` becomes `rtk cargo test`,
/// so the rule is `Bash(rtk cargo test:*)`.
pub fn check_allow_rule(rule: &str) -> Result<(), String> {
    let count = rule.chars().count();
    if count == 0 {
        return Err("the rule is empty".to_owned());
    }
    if count > ALLOW_RULE_MAX_CHARS {
        return Err(format!(
            "the rule is too long: {count} characters. The limit is {ALLOW_RULE_MAX_CHARS}"
        ));
    }
    if rule.chars().any(char::is_control) {
        return Err("the rule has a control character".to_owned());
    }
    if rule.chars().any(|c| !(' '..='~').contains(&c)) {
        return Err(
            "the rule has a character that is not printable ASCII (0x20 to 0x7E)".to_owned(),
        );
    }
    let form = "Use WebSearch, or Tool(pattern) with Tool one of Bash, Read, Glob, Grep, WebFetch";
    let Some((tool, rest)) = rule.split_once('(') else {
        if rule == "WebSearch" {
            return Ok(());
        }
        if PATH_TOOLS.contains(&rule) {
            return Err(format!(
                "\"{rule}\" with no pattern would allow the whole disk. Write a relative path, for example {rule}(./src/**)"
            ));
        }
        if RULE_TOOLS.contains(&rule) {
            return Err(format!(
                "\"{rule}\" with no pattern would allow too much. Write a pattern, for example {rule}(name:*)"
            ));
        }
        return Err(format!("\"{rule}\" is not a rule. {form}"));
    };
    let Some(pattern) = rest.strip_suffix(')') else {
        return Err(format!("\"{rule}\" is not a rule. {form}"));
    };
    if pattern.contains('(') || pattern.contains(')') {
        return Err(format!(
            "\"{rule}\" has a bracket inside the pattern. One entry is one rule: Tool(pattern)"
        ));
    }
    if !RULE_TOOLS.contains(&tool) {
        return Err(format!(
            "\"{tool}\" is not a tool that a rule can name. {form}"
        ));
    }
    if pattern.trim().is_empty() {
        return Err(format!("\"{rule}\" has an empty pattern"));
    }
    match tool {
        "Bash" => check_bash_pattern(rule, pattern),
        "WebFetch" => check_web_fetch_pattern(rule, pattern),
        "WebSearch" => Err(format!("\"{rule}\" has a pattern. Write WebSearch alone")),
        _ => check_path_pattern(rule, pattern),
    }
}

/// The part of a `Bash` rule: see `check_allow_rule`.
fn check_bash_pattern(rule: &str, pattern: &str) -> Result<(), String> {
    if pattern.starts_with(' ') {
        return Err(format!(
            "\"{rule}\" starts with a space, so the command name is empty and every shell command could match"
        ));
    }
    let word_end = pattern.find([' ', ':']).unwrap_or(pattern.len());
    let word = &pattern[..word_end];
    if word.is_empty() {
        return Err(format!(
            "\"{rule}\" has no command name. A rule with no name would allow every shell command. Name a command, for example Bash(cargo test:*)"
        ));
    }
    if !word
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '+' | '-'))
    {
        return Err(format!(
            "\"{rule}\" has a first word that is not a plain command name. Only letters, digits and . _ / + - are allowed there, so a wildcard in the name cannot allow every shell command. Name a command, for example Bash(cargo test:*)"
        ));
    }
    let program = word.rsplit('/').next().unwrap_or(word).to_ascii_lowercase();
    let is_python_version = program
        .strip_prefix("python")
        .is_some_and(|v| v.chars().all(|c| c.is_ascii_digit() || c == '.'));
    if program.is_empty() || BASH_BLOCKED_PROGRAMS.contains(&program.as_str()) {
        return Err(format!(
            "\"{rule}\" names {word}. That command runs other commands, or it destroys data or sends data out, so a rule for it would allow too much"
        ));
    }
    if is_python_version {
        return Err(format!(
            "\"{rule}\" names {word}. That command runs other commands, so a rule for it would allow too much"
        ));
    }
    if program == "git" {
        let after = pattern[word_end..].trim_start_matches(' ');
        let sub_end = after.find([' ', ':']).unwrap_or(after.len());
        let sub = &after[..sub_end];
        if sub.is_empty() {
            return Err(format!(
                "\"{rule}\" names git with no subcommand, so it would allow git push and others that change or destroy work. Name a subcommand, for example Bash(git status:*)"
            ));
        }
        if !sub.starts_with(|c: char| c.is_ascii_lowercase()) {
            return Err(format!(
                "\"{rule}\" has git with an option before the subcommand. Name the subcommand first, for example Bash(git status:*)"
            ));
        }
        if GIT_BLOCKED_SUBCOMMANDS.contains(&sub) {
            return Err(format!(
                "\"{rule}\" allows git {sub}. That subcommand changes or destroys work, or it sets options that run programs, so a rule for it would allow too much"
            ));
        }
    }
    Ok(())
}

/// The part of a `Read`, `Glob` or `Grep` rule: a relative path with no `..`.
fn check_path_pattern(rule: &str, pattern: &str) -> Result<(), String> {
    if pattern.starts_with('/') || pattern.starts_with('~') {
        return Err(format!(
            "\"{rule}\" is an absolute path (or a path of the home folder). Use a path relative to the project, for example ./src/**"
        ));
    }
    if pattern.split(['/', '\\']).any(|part| part == "..") {
        return Err(format!(
            "\"{rule}\" has a .. part, so it can leave the project folder. Use a path inside the project"
        ));
    }
    Ok(())
}

/// The part of a `WebFetch` rule: `domain:<host>` with a host that has a dot.
fn check_web_fetch_pattern(rule: &str, pattern: &str) -> Result<(), String> {
    let Some(host) = pattern.strip_prefix("domain:") else {
        return Err(format!(
            "\"{rule}\" is not allowed. WebFetch takes only a domain, for example WebFetch(domain:example.com)"
        ));
    };
    let plain = host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    let labels_ok = host.contains('.') && host.split('.').all(|label| !label.is_empty());
    if !plain || !labels_ok {
        return Err(format!(
            "\"{rule}\" has a host that is not a plain domain name. Use letters, digits, . and - only, with at least one dot and no wildcard, for example WebFetch(domain:example.com)"
        ));
    }
    Ok(())
}

/// The valid rules of a list, in order, with no duplicate. The app uses this for each session
/// start, so a hand-written bad rule in `floor.toml` never reaches the CLI.
pub fn valid_allow_rules(rules: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for rule in rules {
        if check_allow_rule(rule).is_ok() && !out.contains(rule) {
            out.push(rule.clone());
        }
    }
    out
}

/// One employee. The file name `<slug>.md` gives the slug. The role text is the file body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Employee {
    pub name: String,
    pub slug: String,
    pub role: String,
    /// Slug of the manager. `None` means the employee answers to the operator.
    pub manager: Option<String>,
    pub cli: CliKind,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission: PermissionPolicy,
    pub role_text: String,
}

/// One row of the `sessions` table. Times are Unix milliseconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    /// ID that the app gives to the session. The caller makes it.
    pub app_session_id: String,
    pub employee_slug: String,
    pub cli_session_id: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub last_status: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_rules_pass() {
        for rule in [
            "Bash(cargo test:*)",
            "Bash(git status:*)",
            "Bash(git diff)",
            "Bash(npm run build)",
            "Bash(rtk cargo test:*)",
            "Bash(./scripts/check.sh:*)",
            "Bash(ls:*)",
            "Bash(a)",
            "Read(./src/**)",
            "Read(src/**)",
            "Glob(**/*.rs)",
            "Grep(todo)",
            "WebFetch(domain:example.com)",
            "WebFetch(domain:docs.rs)",
            "WebSearch",
        ] {
            assert_eq!(check_allow_rule(rule), Ok(()), "{rule}");
        }
        assert_eq!(
            check_allow_rule(&format!("Bash({})", "x".repeat(191))),
            Ok(())
        );
    }

    #[test]
    fn bad_rules_are_refused_with_a_reason() {
        let long = format!("Bash({})", "x".repeat(200));
        let cases: Vec<(&str, &str)> = vec![
            ("", "empty"),
            ("Bash", "no pattern"),
            ("WebFetch", "no pattern"),
            ("Read", "whole disk"),
            ("Glob", "whole disk"),
            ("Grep", "whole disk"),
            ("Bash()", "empty pattern"),
            ("Bash(  )", "empty pattern"),
            ("Read()", "empty pattern"),
            ("Edit(./x)", "not a tool"),
            ("Write", "not a rule"),
            ("mcp__tower", "not a rule"),
            ("Bash(cargo test:*", "not a rule"),
            ("Bash cargo", "not a rule"),
            ("bash(ls)", "not a tool"),
            ("Bash(a\nb)", "control character"),
            ("Bash(a\u{1b}b)", "control character"),
            ("Bash(a\u{0}b)", "control character"),
            ("Read\t", "control character"),
            (long.as_str(), "too long"),
        ];
        for (rule, reason) in cases {
            let error = check_allow_rule(rule).expect_err(rule);
            assert!(error.contains(reason), "{rule:?}: {error}");
        }
    }

    /// The rules that a reviewer showed would allow every command (finding H1).
    #[test]
    fn rules_that_allow_every_command_are_refused() {
        let cases: Vec<(&str, &str)> = vec![
            ("Bash(*)", "plain command name"),
            ("Bash(:*)", "no command name"),
            ("Bash(**)", "plain command name"),
            ("Bash(*:*)", "plain command name"),
            ("Bash(* *)", "plain command name"),
            ("Bash( *)", "starts with a space"),
            ("Bash(?*)", "plain command name"),
            ("Bash(*.sh)", "plain command name"),
            ("Bash(ls) Bash(rm:*)", "bracket"),
            ("Bash(ls:*))", "bracket"),
            ("Bash(a)(b)", "bracket"),
            ("Bash(ls\u{2028}rm)", "not printable ASCII"),
            ("Bash(ls\u{200b}:*)", "not printable ASCII"),
            ("Bash(\u{e9}:*)", "not printable ASCII"),
            ("Read(//**)", "absolute path"),
            ("Read(/etc/**)", "absolute path"),
            ("Read(~/.ssh/**)", "absolute path"),
            ("Grep(../x)", ".. part"),
            ("Glob(src/../../x)", ".. part"),
            ("WebFetch(*)", "only a domain"),
            ("WebFetch(domain:*)", "plain domain name"),
            ("WebFetch(domain:localhost)", "plain domain name"),
            ("WebFetch(domain:*.example.com)", "plain domain name"),
            ("WebFetch(domain:example.com.)", "plain domain name"),
            ("WebSearch(rust)", "WebSearch alone"),
            ("Bash(git:*)", "no subcommand"),
            ("Bash(git)", "no subcommand"),
            ("Bash(git push:*)", "git push"),
            ("Bash(git reset --hard)", "git reset"),
            ("Bash(git clean:*)", "git clean"),
            ("Bash(git checkout:*)", "git checkout"),
            ("Bash(git config:*)", "git config"),
            ("Bash(git -c x=y status)", "option before"),
        ];
        for (rule, reason) in cases {
            let error = check_allow_rule(rule).expect_err(rule);
            assert!(error.contains(reason), "{rule:?}: {error}");
        }
    }

    #[test]
    fn programs_that_run_commands_or_destroy_data_are_refused() {
        for program in BASH_BLOCKED_PROGRAMS {
            for rule in [
                format!("Bash({program}:*)"),
                format!("Bash({program} -c:*)"),
                format!("Bash(/usr/bin/{program}:*)"),
                format!("Bash({})", program.to_ascii_uppercase()),
            ] {
                let error = check_allow_rule(&rule).expect_err(&rule);
                assert!(error.contains("would allow too much"), "{rule:?}: {error}");
            }
        }
        for rule in ["Bash(python3.12:*)", "Bash(python2:*)"] {
            assert!(check_allow_rule(rule).is_err(), "{rule}");
        }
        assert!(check_allow_rule("Bash(pythonista:*)").is_ok());
    }

    #[test]
    fn valid_rules_keep_the_order_and_drop_bad_rules_and_duplicates() {
        let rules: Vec<String> = [
            "Bash(ls:*)",
            "Bash",
            "Bash(ls:*)",
            "Read(./src/**)",
            "Bash(*)",
            "Read",
            "Grep(todo)",
        ]
        .iter()
        .map(|r| (*r).to_owned())
        .collect();
        assert_eq!(
            valid_allow_rules(&rules),
            ["Bash(ls:*)", "Read(./src/**)", "Grep(todo)"]
        );
    }
}
