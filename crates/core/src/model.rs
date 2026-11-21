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

/// The data of `.swarm/floor.toml`, without the file version. The `[permissions]` table is not
/// here: since 2026-11-21 the rules for commands with no question live in the app config, not in
/// the project folder (`docs/ipc-m3.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Floor {
    pub name: String,
    pub defaults: FloorDefaults,
    pub brand: Brand,
}

/// The most rules in the list of one floor.
pub const ALLOW_RULES_MAX: usize = 100;

/// The most characters in one rule.
pub const ALLOW_RULE_MAX_CHARS: usize = 200;

/// The tools that a rule can name with a pattern in brackets.
const RULE_TOOLS: [&str; 6] = ["Bash", "Read", "Glob", "Grep", "WebFetch", "WebSearch"];

/// The tools whose pattern is a path in the project folder.
const PATH_TOOLS: [&str; 3] = ["Read", "Glob", "Grep"];

/// Programs that run other commands, that change or destroy data, or that send data out. A rule
/// that allows one of these with no question gives the agent a way around every other rule, so
/// the app does not accept a rule for them. The check is on the name of the program, with no
/// folder in front, with no regard to letter case, and also for a versioned name (`python3.12`,
/// `lua5.4`). The list is a guard against a mistake of the operator. No list of programs can be
/// complete. `git` is a special case (`GIT_ALLOWED_SUBCOMMANDS`). `rtk` is a wrapper
/// (`RTK_BLOCKED_SUBCOMMANDS`).
const BASH_BLOCKED_PROGRAMS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "dash",
    "fish",
    "env",
    "xargs",
    "eval",
    "exec",
    "sudo",
    "su",
    "doas",
    "nohup",
    "time",
    "timeout",
    "nice",
    "python",
    "python3",
    "node",
    "perl",
    "ruby",
    "php",
    "rm",
    "dd",
    "mkfs",
    "chmod",
    "chown",
    "curl",
    "wget",
    "ssh",
    "scp",
    "nc",
    "busybox",
    "ksh",
    "csh",
    "tcsh",
    "pwsh",
    "powershell",
    "lua",
    "deno",
    "bun",
    "pypy",
    "pypy3",
    ".",
    "source",
    "command",
    "builtin",
    "setsid",
    "stdbuf",
    "flock",
    "unshare",
    "script",
    "watch",
    "strace",
    "ltrace",
    "gdb",
    "parallel",
    "pkexec",
    "docker",
    "podman",
    "kubectl",
    "find",
    "awk",
    "gawk",
    "mawk",
    "sed",
    "tar",
    "tee",
    "cp",
    "mv",
    "ln",
    "rsync",
    "socat",
    "ncat",
    "telnet",
    "ftp",
    "vim",
    "vi",
    "nano",
    "emacs",
    "shred",
    "truncate",
    "unlink",
    "kill",
    "pkill",
    "systemctl",
    "mount",
    "umount",
    "crontab",
    "at",
    // Names that the operator may not expect. Each one runs a command that is its argument.
    "ssh-agent",
    "sshpass",
    "expect",
    "chroot",
    "nsenter",
    "ionice",
    "taskset",
    "numactl",
    "runuser",
    "setpriv",
    "start-stop-daemon",
    "run-parts",
    "systemd-run",
    "screen",
    "tmux",
];

/// The `git` subcommands that a rule can allow. They only read. A rule for `git` needs one of
/// them as the first word after `git` (`Bash(git status:*)`). Every other subcommand is refused.
const GIT_ALLOWED_SUBCOMMANDS: &[&str] = &[
    "status",
    "log",
    "diff",
    "show",
    "blame",
    "ls-files",
    "rev-parse",
    "describe",
];

/// The words after `rtk` that run a command that is their argument, or that change the
/// configuration of `rtk`. A rule for them would allow any command.
const RTK_BLOCKED_SUBCOMMANDS: &[&str] = &[
    "proxy", "run", "err", "test", "summary", "pipe", "rewrite", "hook", "init", "trust",
    "untrust", "config",
];

/// The build and package tools. A rule for them is allowed, because the operator wants for
/// example `cargo test` with no question. They run the code of the project, so
/// `floor_permissions_set` returns a warning for each such rule (`allow_rule_warning`).
const BUILD_TOOLS: &[&str] = &[
    "cargo", "npm", "npx", "pnpm", "yarn", "make", "just", "uv", "pip", "go", "gradle", "mvn",
    "pytest", "tox", "bundle", "rake", "composer", "dotnet",
];

/// The warning text for a rule that runs the code of the project.
pub const PROJECT_CODE_WARNING: &str =
    "This rule lets each agent of the floor run the project's own code with no question.";

/// Check one rule of the list of a floor. `Err` is the text for the operator.
///
/// The whole text of a rule is 1 to 200 printable ASCII characters (0x20 to 0x7E). It is one of:
/// - `WebSearch` alone;
/// - `Tool(pattern)` with `Tool` one of `Bash`, `Read`, `Glob`, `Grep`, `WebFetch`. The pattern has
///   no `(` and no `)`, so one entry is always one rule.
///
/// The pattern depends on the tool:
/// - `Bash`: the first word (up to the first space or `:`) is a plain program name of
///   `[A-Za-z0-9._/+-]`. The check unwraps `rtk` first: it applies to the words after `rtk`,
///   and `rtk` alone, or `rtk` with a word of `RTK_BLOCKED_SUBCOMMANDS` or an option, is
///   refused. The program is not one of `BASH_BLOCKED_PROGRAMS`. For `git` the subcommand is one
///   of `GIT_ALLOWED_SUBCOMMANDS`, and the pattern has no `--output`, `-c` or `--exec`. Each
///   other program is allowed, with a warning for the build tools (`allow_rule_warning`). The
///   pattern does not start with a space.
/// - `Read`, `Glob`, `Grep`: a path that is relative to the project (it does not start with `/`
///   or `~`), has no `..` part, and does not name `.swarm/state`. These tools alone (no pattern)
///   are refused: they open the whole disk.
/// - `WebFetch`: only `domain:<host>`, with a host of `[A-Za-z0-9.-]` that has a dot. An IP
///   address, `localhost`, and a host that ends with `.local` or `.internal` are refused.
/// - `WebSearch`: no pattern.
///
/// A rule must match the command as Claude sees it after the hooks of the operator. On a machine
/// with a hook that rewrites commands (for example RTK), `cargo test` becomes `rtk cargo test`,
/// so the rule is `Bash(rtk cargo test:*)`.
///
/// This check is a guard against the mistakes of the operator. The list is the choice of the
/// operator, and it is in the app config, so an agent of the floor cannot change it.
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
        "Bash" => check_bash_pattern(rule, pattern).map(|_| ()),
        "WebFetch" => check_web_fetch_pattern(rule, pattern),
        "WebSearch" => Err(format!("\"{rule}\" has a pattern. Write WebSearch alone")),
        _ => check_path_pattern(rule, pattern),
    }
}

/// The warning for a rule that is allowed but that runs the code of the project: a build or
/// package tool (`cargo`, `npm`, `make` and others, also after `rtk`), or a file of the project
/// that is named by a relative path (`./scripts/check.sh`). `None` for any other rule, and for a
/// rule that `check_allow_rule` refuses. The text starts with the rule.
pub fn allow_rule_warning(rule: &str) -> Option<String> {
    let pattern = rule.strip_prefix("Bash(")?.strip_suffix(')')?;
    let program = check_bash_pattern(rule, pattern).ok()?;
    (BUILD_TOOLS.contains(&program.name.as_str()) || program.runs_project_file)
        .then(|| format!("{rule}: {PROJECT_CODE_WARNING}"))
}

/// The warnings for a list of rules, in the order of the rules. A rule that has none adds none.
pub fn allow_rule_warnings(rules: &[String]) -> Vec<String> {
    rules.iter().filter_map(|r| allow_rule_warning(r)).collect()
}

/// What `check_bash_pattern` found: the program that really runs.
struct BashProgram {
    /// The name of the program, with no folder, in lower case. It is the word after `rtk`
    /// when the rule starts with `rtk`.
    name: String,
    /// The program is a relative path of a file (for example `./scripts/check.sh`).
    runs_project_file: bool,
}

/// Is `name` the program `blocked`, or a versioned name of it (`python3.12`, `lua5.4`)? The
/// version is only digits and dots. The program `.` has no versioned name.
fn is_blocked_name(name: &str, blocked: &str) -> bool {
    if name == blocked {
        return true;
    }
    blocked != "."
        && name
            .strip_prefix(blocked)
            .is_some_and(|v| v.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

/// The part of a `Bash` rule: see `check_allow_rule`.
fn check_bash_pattern(rule: &str, pattern: &str) -> Result<BashProgram, String> {
    if pattern.starts_with(' ') {
        return Err(format!(
            "\"{rule}\" starts with a space, so the command name is empty and every shell command could match"
        ));
    }
    // The words of the pattern. `rest` starts at the program that really runs: it moves past
    // each `rtk`.
    let mut rest = pattern;
    let mut after_rtk = false;
    let (word, name) = loop {
        let word_end = rest.find([' ', ':']).unwrap_or(rest.len());
        let word = &rest[..word_end];
        if word.is_empty() {
            return Err(if after_rtk {
                format!(
                    "\"{rule}\" names rtk with no command after it. A rule for rtk alone would allow every command. Name a command, for example Bash(rtk cargo test:*)"
                )
            } else {
                format!(
                    "\"{rule}\" has no command name. A rule with no name would allow every shell command. Name a command, for example Bash(cargo test:*)"
                )
            });
        }
        if !word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '+' | '-'))
        {
            return Err(format!(
                "\"{rule}\" has a first word that is not a plain command name. Only letters, digits and . _ / + - are allowed there, so a wildcard in the name cannot allow every shell command. Name a command, for example Bash(cargo test:*)"
            ));
        }
        let name = word.rsplit('/').next().unwrap_or(word).to_ascii_lowercase();
        if after_rtk {
            if word.starts_with('-') {
                return Err(format!(
                    "\"{rule}\" has an option after rtk. Name the command first, for example Bash(rtk cargo test:*)"
                ));
            }
            if RTK_BLOCKED_SUBCOMMANDS.contains(&name.as_str()) {
                return Err(format!(
                    "\"{rule}\" allows rtk {name}. That rtk command runs any command, or it changes the settings of rtk, so a rule for it would allow too much"
                ));
            }
        }
        if name == "rtk" {
            rest = rest[word_end..].trim_start_matches(' ');
            after_rtk = true;
            continue;
        }
        break (word, name);
    };
    if name.is_empty()
        || BASH_BLOCKED_PROGRAMS
            .iter()
            .any(|blocked| is_blocked_name(&name, blocked))
    {
        return Err(format!(
            "\"{rule}\" names {word}. That command runs other commands, or it changes or destroys data, or it sends data out, so a rule for it would allow too much"
        ));
    }
    if name == "git" {
        check_git_pattern(rule, pattern, &rest[word.len()..])?;
    }
    Ok(BashProgram {
        name,
        runs_project_file: word.contains('/') && !word.starts_with('/'),
    })
}

/// The `git` part of a `Bash` rule. `after` is the text after the word `git`.
fn check_git_pattern(rule: &str, pattern: &str, after: &str) -> Result<(), String> {
    let after = after.trim_start_matches(' ');
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
    if !GIT_ALLOWED_SUBCOMMANDS.contains(&sub) {
        return Err(format!(
            "\"{rule}\" allows git {sub}. A rule for git can name only the subcommands that read: {}. Another subcommand can change or destroy work, or run a program, so a rule for it would allow too much",
            GIT_ALLOWED_SUBCOMMANDS.join(", ")
        ));
    }
    let bad_option = pattern.contains("--output")
        || pattern.contains("--exec")
        || pattern.contains("-c ")
        || pattern.contains("-c:")
        || pattern.ends_with("-c");
    if bad_option {
        return Err(format!(
            "\"{rule}\" has the option --output, --exec or -c. Each of them can write a file or run a program, so a rule with it would allow too much"
        ));
    }
    Ok(())
}

/// The part of a `Read`, `Glob` or `Grep` rule: a relative path with no `..`, and not the
/// state folder of the floor.
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
    let plain = pattern.strip_prefix("./").unwrap_or(pattern);
    let in_swarm = plain
        .strip_prefix(".swarm/")
        .is_some_and(|inside| inside.starts_with("state") || inside.starts_with('*'));
    if in_swarm || pattern.contains("**/.swarm/state") {
        return Err(format!(
            "\"{rule}\" reaches .swarm/state. That folder has the database and the logs of the floor, so a rule for it would allow too much"
        ));
    }
    Ok(())
}

/// The part of a `WebFetch` rule: `domain:<host>` with a host that has a dot, that is not an IP
/// address, and that is not a local name.
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
    let lower = host.to_ascii_lowercase();
    let last = lower.rsplit('.').next().unwrap_or_default();
    // A real top-level name never starts with a digit. So this refuses an IP address, also in the
    // forms that a resolver reads as one (`127.1`, `0x7f.0.0.1`, `127.0.0.0x1`).
    let is_address = lower.chars().all(|c| c.is_ascii_digit() || c == '.')
        || last.starts_with(|c: char| c.is_ascii_digit());
    let is_local = lower == "localhost"
        || [".localhost", ".local", ".internal", ".localdomain"]
            .iter()
            .any(|end| lower.ends_with(end));
    if is_address || is_local {
        return Err(format!(
            "\"{rule}\" names an IP address or a local host. A rule for it would let an agent reach services of this machine or of the local network. Use a public domain name"
        ));
    }
    Ok(())
}

/// The valid rules of a list, in order, with no duplicate. The app uses this for each session
/// start, so that a rule that the check refuses now (the check can change in a new version) never
/// reaches the CLI.
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
            ("Bash(git push:*)", "allows git push"),
            ("Bash(git reset --hard)", "allows git reset"),
            ("Bash(git clean:*)", "allows git clean"),
            ("Bash(git checkout:*)", "allows git checkout"),
            ("Bash(git config:*)", "allows git config"),
            ("Bash(git -c x=y status)", "option before"),
            ("Bash(rtk:*)", "no command after it"),
            ("Bash(rtk)", "no command after it"),
            ("Bash(rtk proxy:*)", "allows rtk proxy"),
            ("Bash(rtk -v proxy touch x)", "option after rtk"),
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
                format!("Bash(rtk {program}:*)"),
            ] {
                let error = check_allow_rule(&rule).expect_err(&rule);
                assert!(error.contains("would allow too much"), "{rule:?}: {error}");
            }
        }
        // A versioned name is the same program.
        for rule in [
            "Bash(python3.12:*)",
            "Bash(python2:*)",
            "Bash(lua5.4:*)",
            "Bash(pypy3.10:*)",
            "Bash(/usr/bin/python3.11:*)",
        ] {
            assert!(check_allow_rule(rule).is_err(), "{rule}");
        }
        assert!(check_allow_rule("Bash(pythonista:*)").is_ok());
        assert!(check_allow_rule("Bash(python3-config:*)").is_ok());
    }

    /// What a rule gets under the design of 2026-11-21.
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Expect {
        /// Allowed, with no warning.
        Pass,
        /// Allowed, with the warning about the code of the project.
        Warn,
        /// Refused.
        No,
    }
    use Expect::{No, Pass, Warn};

    fn outcome(rule: &str) -> Expect {
        match (check_allow_rule(rule), allow_rule_warning(rule)) {
            (Err(_), warning) => {
                assert_eq!(warning, None, "{rule}: a refused rule has no warning");
                No
            }
            (Ok(()), None) => Pass,
            (Ok(()), Some(_)) => Warn,
        }
    }

    /// The rules of the reviewer (about 110, from the probe `zz_rules.rs`), each with the result
    /// that the new design gives. A rule that the old check let through, and that is refused now,
    /// has `No` and a comment.
    #[test]
    fn the_rules_of_the_review_get_the_result_of_the_new_design() {
        let table: &[(&str, Expect)] = &[
            // The wrappers and the shells.
            ("Bash(/usr/bin/env:*)", No),
            ("Bash(busybox sh:*)", No),
            ("Bash(busybox:*)", No),
            ("Bash(sh)", No),
            ("Bash(SH:*)", No),
            ("Bash(/bin/Bash:*)", No),
            ("Bash(../../bin/sh:*)", No),
            ("Bash(.:*)", No),
            ("Bash(source:*)", No),
            ("Bash(command:*)", No),
            ("Bash(builtin:*)", No),
            ("Bash(ksh:*)", No),
            ("Bash(tcsh:*)", No),
            ("Bash(csh:*)", No),
            ("Bash(pwsh:*)", No),
            ("Bash(lua:*)", No),
            ("Bash(deno:*)", No),
            ("Bash(bun:*)", No),
            ("Bash(pypy3:*)", No),
            ("Bash(script:*)", No),
            ("Bash(watch:*)", No),
            ("Bash(strace:*)", No),
            ("Bash(setsid:*)", No),
            ("Bash(stdbuf:*)", No),
            ("Bash(flock:*)", No),
            ("Bash(unshare:*)", No),
            ("Bash(parallel:*)", No),
            ("Bash(pkexec:*)", No),
            ("Bash(docker:*)", No),
            // A name that no program has (not blocked).
            ("Bash(python3-x:*)", Pass),
            // Programs that run a command, write a file or send data out.
            ("Bash(find:*)", No),
            ("Bash(awk:*)", No),
            ("Bash(gawk:*)", No),
            ("Bash(sed:*)", No),
            ("Bash(tar:*)", No),
            ("Bash(tee:*)", No),
            ("Bash(cp:*)", No),
            ("Bash(mv:*)", No),
            ("Bash(ln:*)", No),
            ("Bash(rsync:*)", No),
            ("Bash(socat:*)", No),
            ("Bash(ncat:*)", No),
            ("Bash(shred:*)", No),
            ("Bash(truncate:*)", No),
            ("Bash(unlink:*)", No),
            ("Bash(vim:*)", No),
            ("Bash(systemctl:*)", No),
            ("Bash(crontab:*)", No),
            ("Bash(rm -rf:*)", No),
            // The build and package tools: allowed, with a warning.
            ("Bash(make:*)", Warn),
            ("Bash(npm:*)", Warn),
            ("Bash(npx:*)", Warn),
            ("Bash(cargo:*)", Warn),
            ("Bash(cargo run:*)", Warn),
            ("Bash(cargo test:*)", Warn),
            ("Bash(cargo test *)", Warn),
            ("Bash(cargo *)", Warn),
            ("Bash(cargo:* *)", Warn),
            ("Bash(cargo test;*)", Warn),
            ("Bash(cargo test && *)", Warn),
            ("Bash(uv run:*)", Warn),
            ("Bash(rtk cargo test:*)", Warn),
            ("Bash(rtk npm run build)", Warn),
            ("Bash(./run.sh:*)", Warn),
            ("Bash(../x:*)", Warn),
            // `rtk` is a wrapper: the check applies to the words after it.
            ("Bash(rtk:*)", No),
            ("Bash(rtk proxy:*)", No),
            ("Bash(rtk git push:*)", No),
            ("Bash(rtk run:*)", No),
            ("Bash(rtk rtk proxy:*)", No),
            ("Bash(rtk -v cargo:*)", No),
            ("Bash(rtk find:*)", No),
            ("Bash(rtk git status:*)", Pass),
            ("Bash(rtk ls:*)", Pass),
            // `git`: only the subcommands that read.
            ("Bash(git status:*)", Pass),
            ("Bash(git log:*)", Pass),
            ("Bash(git diff:*)", Pass),
            ("Bash(git show:*)", Pass),
            ("Bash(git blame:*)", Pass),
            ("Bash(git ls-files:*)", Pass),
            ("Bash(git rev-parse:*)", Pass),
            ("Bash(git describe:*)", Pass),
            ("Bash(git -c:*)", No),
            ("Bash(git grep:*)", No),
            ("Bash(git rebase:*)", No),
            ("Bash(git submodule:*)", No),
            ("Bash(git branch:*)", No),
            ("Bash(git stash:*)", No),
            ("Bash(git commit:*)", No),
            ("Bash(git rm:*)", No),
            ("Bash(git restore:*)", No),
            ("Bash(git switch:*)", No),
            ("Bash(git worktree:*)", No),
            ("Bash(git bisect:*)", No),
            ("Bash(git filter-branch:*)", No),
            ("Bash(git difftool:*)", No),
            ("Bash(git gc:*)", No),
            ("Bash(git update-ref:*)", No),
            ("Bash(git Push:*)", No),
            ("Bash(git push --force)", No),
            ("Bash(git log --output=x:*)", No),
            ("Bash(git diff --output x)", No),
            ("Bash(git log --exec=x:*)", No),
            ("Bash(git log -c x=y)", No),
            ("Bash(git log -c:*)", No),
            ("Bash(git log -c)", No),
            ("Bash(git log --oneline:*)", Pass),
            // Versioned names.
            ("Bash(python3.12:*)", No),
            ("Bash(lua5.4:*)", No),
            // Each other program is the choice of the operator.
            ("Bash(ls:*)", Pass),
            ("Bash(/usr/bin/ls:*)", Pass),
            ("Bash(pythonista:*)", Pass),
            // The tools with a path.
            ("Read(./../x)", No),
            ("Read(src/../../x)", No),
            ("Read(src\\..\\x)", No),
            ("Read(.swarm/state/**)", No),
            ("Read(./.swarm/state/**)", No),
            ("Read(**/.swarm/state/**)", No),
            ("Glob(.swarm/state/*.db)", No),
            ("Grep(./.swarm/state)", No),
            ("Read(.swarm/**)", No),
            ("Read(.swarm/*)", No),
            ("Read(.swarm/reports/**)", Pass),
            ("Read(**)", Pass),
            ("Read(*)", Pass),
            ("Read(.ssh/**)", Pass),
            ("Read(.)", Pass),
            ("Grep(**)", Pass),
            ("Glob(**/*)", Pass),
            // The web.
            ("WebFetch(domain:a.b)", Pass),
            ("WebFetch(domain:docs.rs)", Pass),
            ("WebFetch(domain:com.)", No),
            ("WebFetch(domain:localhost.localdomain)", No),
            ("WebFetch(domain:169.254.169.254)", No),
            ("WebFetch(domain:127.1)", No),
            ("WebFetch(domain:0x7f.0.0.1)", No),
            ("WebFetch(domain:127.0.0.0x1)", No),
            ("WebFetch(domain:localhost)", No),
            ("WebFetch(domain:printer.local)", No),
            ("WebFetch(domain:Wiki.INTERNAL)", No),
            ("WebFetch(domain:a.localhost)", No),
            ("WebFetch(domain:1password.com)", Pass),
            ("WebSearch", Pass),
            // Other tools.
            ("Edit(./x)", No),
            ("Write(./x)", No),
        ];
        let mut wrong = Vec::new();
        for (rule, expected) in table {
            let got = outcome(rule);
            if got != *expected {
                wrong.push(format!("{rule}: expected {expected:?}, got {got:?}"));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn a_warning_names_the_rule_and_says_why() {
        let rules: Vec<String> = ["Bash(ls:*)", "Bash(rtk cargo test:*)", "Bash(git log:*)"]
            .iter()
            .map(|r| (*r).to_owned())
            .collect();
        assert_eq!(
            allow_rule_warnings(&rules),
            [
                "Bash(rtk cargo test:*): This rule lets each agent of the floor run the project's own code with no question."
            ]
        );
        assert_eq!(allow_rule_warning("Bash(sh:*)"), None);
        assert_eq!(allow_rule_warning("WebSearch"), None);
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
            "Bash(sh:*)",
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
