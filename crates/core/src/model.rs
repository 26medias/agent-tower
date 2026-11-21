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
/// sets it in Settings. An employee cannot change it (`floor.toml` is a denied file).
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

/// The tools that a rule can name with no pattern (the tool is allowed for every argument).
/// These three only read. `Bash`, `WebFetch` and `WebSearch` with no pattern are refused.
const BARE_RULES: [&str; 3] = ["Read", "Glob", "Grep"];

/// Check one rule of `[permissions] allow`. `Err` is the text for the operator.
///
/// A rule is 1 to 200 characters, has no control character, and is one of:
/// - `Read`, `Glob` or `Grep` alone;
/// - `Tool(pattern)` with `Tool` one of `Bash`, `Read`, `Glob`, `Grep`, `WebFetch`, `WebSearch`.
///
/// The pattern must not be empty, and for `Bash` it must not allow every command: `Bash(*)`,
/// `Bash(:*)` and any pattern that has only `*` and `:` are refused. `Bash` alone is refused.
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
    if BARE_RULES.contains(&rule) {
        return Ok(());
    }
    let form = "Use Read, Glob, Grep, or Tool(pattern) with Tool one of Bash, Read, Glob, Grep, WebFetch, WebSearch";
    if RULE_TOOLS.contains(&rule) {
        return Err(format!(
            "\"{rule}\" with no pattern would allow too much. Write a pattern, for example {rule}(name:*)"
        ));
    }
    let Some((tool, rest)) = rule.split_once('(') else {
        return Err(format!("\"{rule}\" is not a rule. {form}"));
    };
    let Some(pattern) = rest.strip_suffix(')') else {
        return Err(format!("\"{rule}\" is not a rule. {form}"));
    };
    if !RULE_TOOLS.contains(&tool) {
        return Err(format!(
            "\"{tool}\" is not a tool that a rule can name. {form}"
        ));
    }
    if pattern.trim().is_empty() {
        return Err(format!("\"{rule}\" has an empty pattern"));
    }
    if tool == "Bash" && pattern.chars().all(|c| c == '*' || c == ':') {
        return Err(format!(
            "\"{rule}\" would allow every shell command. Name a command, for example Bash(cargo test:*)"
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
            "Bash(npm run build)",
            "Read",
            "Glob",
            "Grep",
            "Read(./src/**)",
            "Glob(**/*.rs)",
            "Grep(todo)",
            "WebFetch(domain:example.com)",
            "WebSearch(rust)",
            "Bash(a)",
            "Bash(*.sh)",
            "Bash(ls:*)",
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
            ("WebSearch", "no pattern"),
            ("Bash(*)", "every shell command"),
            ("Bash(:*)", "every shell command"),
            ("Bash(**)", "every shell command"),
            ("Bash(*:*)", "every shell command"),
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

    #[test]
    fn valid_rules_keep_the_order_and_drop_bad_rules_and_duplicates() {
        let rules: Vec<String> = [
            "Bash(ls:*)",
            "Bash",
            "Bash(ls:*)",
            "Read",
            "Bash(*)",
            "Grep",
        ]
        .iter()
        .map(|r| (*r).to_owned())
        .collect();
        assert_eq!(valid_allow_rules(&rules), ["Bash(ls:*)", "Read", "Grep"]);
    }
}
