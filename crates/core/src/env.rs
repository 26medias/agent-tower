//! Environment of a CLI process (spec 4.2, "Clean environment").
//!
//! `build_env` makes a new map for each CLI process. It copies all variables of the parent
//! environment, except a short list of names (see below). The parent is the environment of the
//! login shell of the operator, laid over the environment of the app (`login_env`). So a session
//! sees what the operator sees in a terminal: `cargo`, `pnpm`, `NVM_DIR`, `JAVA_HOME`, a proxy.
//! The operator asked for this rule on 2026-11-20. Before that, the app copied an allow-list only.
//!
//! These names never reach a session:
//! - `CLAUDECODE` and `CLAUDE_CODE_*`. An inherited `CLAUDE_CODE_CHILD_SESSION` turns transcript
//!   saving off in Claude, and then resume has nothing to resume.
//! - `PATH` (built by `build_env`), `PWD`, `OLDPWD`, `SHLVL`, `_`: they describe the shell or the
//!   process of the app, not the session.
//! - `TOWER_*`, `BASH_FUNC_*`, `TAURI_*`: they belong to the app, to exported shell functions,
//!   or to the Tauri runtime. `TOWER_SESSION` and `TOWER_SOCKET` come from the caller in `extra`.
//! - The API keys and base URLs in `CREDENTIAL_NAMES`.
//!
//! It is a pure function: it reads no process state, so tests give it maps.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

/// An environment: variable name to value.
pub type EnvMap = BTreeMap<OsString, OsString>;

/// The last folders of the PATH of each session.
pub const BASE_PATH_DIRS: [&str; 2] = ["/usr/bin", "/bin"];

/// The value of `TERM` when the parent has none.
pub const DEFAULT_TERM: &str = "xterm-256color";

/// API keys and base URLs that a session does not inherit.
///
/// Reason: a key in the environment changes how the CLI logs in, and who pays. Claude Code uses
/// `ANTHROPIC_API_KEY` instead of the subscription of the operator. A base URL sends the traffic
/// to another server. The operator can change this rule here. (`CODEX_HOME` is not in the list:
/// it only says where the Codex files are.) The `extra` values of the caller can still set them.
const CREDENTIAL_NAMES: [&str; 5] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
];

/// Other names that are not copied from the parent.
const EXCLUDED_NAMES: [&str; 7] = [
    "PATH",
    "PWD",
    "OLDPWD",
    "SHLVL",
    "_",
    "TOWER_SESSION",
    "TOWER_SOCKET",
];

/// Prefixes of names that are not copied from the parent.
const EXCLUDED_PREFIXES: [&str; 3] = ["TOWER_", "BASH_FUNC_", "TAURI_"];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnvError {
    /// A caller value has a name that must never reach a CLI.
    #[error("the variable {0:?} must never be given to a CLI process")]
    ForbiddenName(OsString),
    /// The caller tried to set PATH. `build_env` builds it from the folders.
    #[error("PATH is built by build_env; do not give it in the extra values")]
    PathInExtra,
    /// A name that no process can have (empty, or with `=` or a NUL byte).
    #[error("{0:?} is not a valid variable name")]
    InvalidName(OsString),
    /// A value with a NUL byte.
    #[error("the value of {0:?} has a NUL byte")]
    InvalidValue(OsString),
    /// A PATH folder that is not absolute, or that has a `:` or a NUL byte.
    #[error("{0:?} is not usable as a PATH folder (it must be absolute, with no ':')")]
    InvalidPathDir(PathBuf),
}

/// True for the names that a CLI process must never inherit or get.
pub fn is_forbidden_name(name: &OsStr) -> bool {
    let bytes = name.as_bytes();
    bytes == b"CLAUDECODE" || bytes.starts_with(b"CLAUDE_CODE_")
}

/// True for the names that `build_env` does not copy from the parent (the forbidden names, the
/// names that the app sets or builds, the shell state, and the credentials).
fn is_excluded_from_parent(name: &OsStr) -> bool {
    let bytes = name.as_bytes();
    is_forbidden_name(name)
        || EXCLUDED_NAMES
            .iter()
            .chain(CREDENTIAL_NAMES.iter())
            .any(|excluded| excluded.as_bytes() == bytes)
        || EXCLUDED_PREFIXES
            .iter()
            .any(|prefix| bytes.starts_with(prefix.as_bytes()))
}

/// A name that no process can have: empty, or with `=` or a NUL byte.
fn is_invalid_name(name: &OsStr) -> bool {
    let bytes = name.as_bytes();
    bytes.is_empty() || bytes.contains(&b'=') || bytes.contains(&0)
}

/// Make the environment of a CLI process.
///
/// - All variables of `parent` are copied, except: the forbidden names (`CLAUDECODE`,
///   `CLAUDE_CODE_*`); `PATH`, `PWD`, `OLDPWD`, `SHLVL`, `_`; `TOWER_*`, `BASH_FUNC_*`,
///   `TAURI_*`; and the names in `CREDENTIAL_NAMES`. A name or a value that no process can have
///   is skipped with no error: the parent is the environment of a shell, not a caller value.
/// - `TERM` is `xterm-256color` when the parent has none (or an empty one).
/// - `PATH` is, in this order and with no folder two times: `path_dirs` (the folders of the
///   programs that the app resolved, so the session runs the same `claude`, `codex` and
///   `node`), the folders of the `PATH` of `parent`, then `/usr/bin` and `/bin`. A folder of
///   the parent `PATH` that is not absolute, or has a NUL byte, is skipped.
/// - `extra` is added last and wins over a parent value. It carries `TOWER_SESSION`,
///   `TOWER_SOCKET` and the values of the adapter.
///
/// Errors (and no partial result): a name in `extra` that is `CLAUDECODE` or starts with
/// `CLAUDE_CODE_`; `PATH` in `extra`; an invalid name or value in `extra`; a bad folder in
/// `path_dirs`. An error is better than a silent drop: such a value is a bug of the caller.
pub fn build_env(
    parent: &EnvMap,
    extra: &[(OsString, OsString)],
    path_dirs: &[PathBuf],
) -> Result<EnvMap, EnvError> {
    let mut env = EnvMap::new();

    for (name, value) in parent {
        if is_invalid_name(name) || value.as_bytes().contains(&0) || is_excluded_from_parent(name) {
            continue;
        }
        env.insert(name.clone(), value.clone());
    }
    if env
        .get(OsStr::new("TERM"))
        .is_none_or(|term| term.is_empty())
    {
        env.insert("TERM".into(), DEFAULT_TERM.into());
    }

    let mut dirs: Vec<PathBuf> = Vec::new();
    for dir in path_dirs {
        let bytes = dir.as_os_str().as_bytes();
        if !dir.is_absolute() || bytes.contains(&b':') || bytes.contains(&0) {
            return Err(EnvError::InvalidPathDir(dir.clone()));
        }
        if !dirs.contains(dir) {
            dirs.push(dir.clone());
        }
    }
    if let Some(parent_path) = parent.get(OsStr::new("PATH")) {
        // `split_paths` splits at `:`, so a folder has no `:`. An empty part is not absolute.
        for dir in std::env::split_paths(parent_path) {
            if dir.is_absolute() && !dir.as_os_str().as_bytes().contains(&0) && !dirs.contains(&dir)
            {
                dirs.push(dir);
            }
        }
    }
    for base in BASE_PATH_DIRS {
        let base = PathBuf::from(base);
        if !dirs.contains(&base) {
            dirs.push(base);
        }
    }
    let path = std::env::join_paths(&dirs)
        .map_err(|_| EnvError::InvalidPathDir(dirs.last().cloned().unwrap_or_default()))?;
    env.insert("PATH".into(), path);

    for (name, value) in extra {
        let bytes = name.as_bytes();
        if is_invalid_name(name) {
            return Err(EnvError::InvalidName(name.clone()));
        }
        if is_forbidden_name(name) {
            return Err(EnvError::ForbiddenName(name.clone()));
        }
        if bytes == b"PATH" {
            return Err(EnvError::PathInExtra);
        }
        if value.as_bytes().contains(&0) {
            return Err(EnvError::InvalidValue(name.clone()));
        }
        env.insert(name.clone(), value.clone());
    }
    Ok(env)
}

/// The environment of this process as a map. The app calls this once. It is the base of the
/// parent environment of the sessions (see `login_env`). Tests do not use it, except the test
/// that runs a real CLI.
pub fn current_env() -> EnvMap {
    std::env::vars_os().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> EnvMap {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    fn extra(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    fn get<'a>(env: &'a EnvMap, name: &str) -> Option<&'a str> {
        env.get(OsStr::new(name)).and_then(|value| value.to_str())
    }

    #[test]
    fn normal_parent_variables_are_copied() {
        let parent = map(&[
            ("HOME", "/home/a"),
            ("USER", "a"),
            ("SHELL", "/bin/zsh"),
            ("LANG", "fr_FR.UTF-8"),
            ("LC_ALL", "C"),
            ("TERM", "screen"),
            ("DISPLAY", ":0"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ("SSH_AUTH_SOCK", "/run/user/1000/ssh"),
            ("CARGO_HOME", "/home/a/.cargo"),
            ("HTTPS_PROXY", "http://proxy.example:3128"),
            ("NVM_DIR", "/home/a/.nvm"),
            ("JAVA_HOME", "/usr/lib/jvm/java-21"),
            ("CODEX_HOME", "/home/a/.codex"),
            ("AWS_PROFILE", "dev"),
            ("LCX", "not a LC_ name"),
            ("TOWERISH", "not a TOWER_ name"),
        ]);
        let env = build_env(&parent, &[], &[]).unwrap();
        for (name, value) in &parent {
            assert_eq!(env.get(name), Some(value), "{name:?}");
        }
        // The copy is complete, and PATH is the only name that is added.
        assert_eq!(env.len(), parent.len() + 1);
        assert_eq!(get(&env, "TERM"), Some("screen"));
    }

    #[test]
    fn the_excluded_names_are_not_copied() {
        let excluded = [
            "PWD",
            "OLDPWD",
            "SHLVL",
            "_",
            "TOWER_SESSION",
            "TOWER_SOCKET",
            "TOWER_ANYTHING",
            "BASH_FUNC_ll%%",
            "BASH_FUNC_x()",
            "TAURI_ENV_DEBUG",
            "TAURI_DEV",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
            "OPENAI_API_KEY",
            "OPENAI_BASE_URL",
        ];
        let mut parent = map(&[("HOME", "/home/a")]);
        for name in excluded {
            parent.insert(name.into(), "from the parent".into());
        }
        let env = build_env(&parent, &[], &[]).unwrap();
        for name in excluded {
            assert!(!env.contains_key(OsStr::new(name)), "{name} was copied");
        }
        assert_eq!(get(&env, "HOME"), Some("/home/a"));
        // Each name of the two constants is excluded (the test list cannot drift from them).
        for name in EXCLUDED_NAMES.iter().chain(CREDENTIAL_NAMES.iter()) {
            assert!(is_excluded_from_parent(OsStr::new(name)), "{name}");
        }
    }

    #[test]
    fn codex_home_is_not_a_credential() {
        let env = build_env(&map(&[("CODEX_HOME", "/home/a/.codex")]), &[], &[]).unwrap();
        assert_eq!(get(&env, "CODEX_HOME"), Some("/home/a/.codex"));
    }

    #[test]
    fn extra_can_set_a_credential_name_and_a_tower_name() {
        let env = build_env(
            &map(&[("OPENAI_API_KEY", "from the parent")]),
            &extra(&[("OPENAI_API_KEY", "made-up"), ("TOWER_SESSION", "s-1")]),
            &[],
        )
        .unwrap();
        assert_eq!(get(&env, "OPENAI_API_KEY"), Some("made-up"));
        assert_eq!(get(&env, "TOWER_SESSION"), Some("s-1"));
    }

    #[test]
    fn term_has_a_default() {
        let env = build_env(&map(&[]), &[], &[]).unwrap();
        assert_eq!(get(&env, "TERM"), Some("xterm-256color"));
        let env = build_env(&map(&[("TERM", "")]), &[], &[]).unwrap();
        assert_eq!(get(&env, "TERM"), Some("xterm-256color"));
    }

    #[test]
    fn invalid_parent_names_and_values_are_skipped_without_an_error() {
        let mut parent = map(&[("HOME", "/home/a"), ("OK", "1")]);
        parent.insert("".into(), "empty name".into());
        parent.insert("A=B".into(), "equals in the name".into());
        parent.insert(OsString::from("A\0B"), "NUL in the name".into());
        parent.insert("NUL_VALUE".into(), OsString::from("x\0y"));
        let env = build_env(&parent, &[], &[]).unwrap();
        let names: Vec<&str> = env.keys().filter_map(|key| key.to_str()).collect();
        assert_eq!(names, vec!["HOME", "OK", "PATH", "TERM"]);
    }

    #[test]
    fn claude_variables_in_the_parent_are_dropped() {
        let parent = map(&[
            ("HOME", "/home/a"),
            ("CLAUDE_CODE_CHILD_SESSION", "1"),
            ("CLAUDE_CODE_ENTRYPOINT", "cli"),
            ("CLAUDECODE", "1"),
            ("CLAUDE_CODEX", "looks alike"),
        ]);
        let env = build_env(&parent, &[], &[]).unwrap();
        assert!(env.keys().all(|key| !is_forbidden_name(key)));
        assert_eq!(get(&env, "HOME"), Some("/home/a"));
        // A name that only looks alike is copied.
        assert_eq!(get(&env, "CLAUDE_CODEX"), Some("looks alike"));
    }

    #[test]
    fn claude_variables_in_extra_are_an_error() {
        for name in ["CLAUDE_CODE_CHILD_SESSION", "CLAUDECODE", "CLAUDE_CODE_"] {
            let result = build_env(&map(&[]), &extra(&[(name, "1")]), &[]);
            assert_eq!(result, Err(EnvError::ForbiddenName(name.into())), "{name}");
        }
        // Names that only look alike are fine.
        let env = build_env(
            &map(&[]),
            &extra(&[("CLAUDE_CODEX", "1"), ("MY_CLAUDECODE", "1")]),
            &[],
        )
        .unwrap();
        assert_eq!(get(&env, "CLAUDE_CODEX"), Some("1"));
    }

    #[test]
    fn extra_is_added_and_wins() {
        let parent = map(&[("HOME", "/home/a"), ("LANG", "C")]);
        let env = build_env(
            &parent,
            &extra(&[
                ("TOWER_SESSION", "s-1"),
                ("TOWER_SOCKET", "/run/x.sock"),
                ("HOME", "/other"),
            ]),
            &[],
        )
        .unwrap();
        assert_eq!(get(&env, "TOWER_SESSION"), Some("s-1"));
        assert_eq!(get(&env, "TOWER_SOCKET"), Some("/run/x.sock"));
        assert_eq!(get(&env, "HOME"), Some("/other"));
        assert_eq!(get(&env, "LANG"), Some("C"));
    }

    #[test]
    fn path_is_the_program_folders_then_the_parent_path_then_the_base_folders() {
        let parent = map(&[(
            "PATH",
            // A relative folder, an empty part, a repeat, and the base folders in the middle.
            "/home/a/.cargo/bin:relative/bin::/home/a/.local/bin:/usr/bin:/opt/x/bin:/home/a/.cargo/bin/",
        )]);
        let dirs = [
            PathBuf::from("/home/a/.local/bin"),
            PathBuf::from("/home/a/.nvm/versions/node/v24/bin"),
            PathBuf::from("/home/a/.local/bin"),
            PathBuf::from("/bin"),
        ];
        let env = build_env(&parent, &[], &dirs).unwrap();
        assert_eq!(
            get(&env, "PATH"),
            Some(
                "/home/a/.local/bin:/home/a/.nvm/versions/node/v24/bin:/bin:/home/a/.cargo/bin:/usr/bin:/opt/x/bin"
            )
        );
    }

    #[test]
    fn path_has_the_base_folders_last_when_nothing_else_is_there() {
        let env = build_env(&map(&[]), &[], &[]).unwrap();
        assert_eq!(get(&env, "PATH"), Some("/usr/bin:/bin"));
        let env = build_env(&map(&[("PATH", "/opt/a")]), &[], &[]).unwrap();
        assert_eq!(get(&env, "PATH"), Some("/opt/a:/usr/bin:/bin"));
        let dirs = [PathBuf::from("/opt/b")];
        let env = build_env(&map(&[("PATH", "/opt/a")]), &[], &dirs).unwrap();
        assert_eq!(get(&env, "PATH"), Some("/opt/b:/opt/a:/usr/bin:/bin"));
    }

    #[test]
    fn a_parent_path_folder_with_a_nul_byte_is_skipped() {
        let mut path = OsString::from("/opt/a:/opt/b");
        path.push(OsStr::from_bytes(b"\0x"));
        path.push(":/opt/c");
        let mut parent = EnvMap::new();
        parent.insert("PATH".into(), path);
        let env = build_env(&parent, &[], &[]).unwrap();
        assert_eq!(get(&env, "PATH"), Some("/opt/a:/opt/c:/usr/bin:/bin"));
    }

    #[test]
    fn bad_input_is_an_error() {
        assert_eq!(
            build_env(&map(&[]), &extra(&[("PATH", "/x")]), &[]),
            Err(EnvError::PathInExtra)
        );
        assert!(matches!(
            build_env(&map(&[]), &extra(&[("A=B", "x")]), &[]),
            Err(EnvError::InvalidName(_))
        ));
        assert!(matches!(
            build_env(&map(&[]), &extra(&[("", "x")]), &[]),
            Err(EnvError::InvalidName(_))
        ));
        assert!(matches!(
            build_env(&map(&[]), &extra(&[("A", "x\0y")]), &[]),
            Err(EnvError::InvalidValue(_))
        ));
        assert!(matches!(
            build_env(&map(&[]), &[], &[PathBuf::from("relative/bin")]),
            Err(EnvError::InvalidPathDir(_))
        ));
        assert!(matches!(
            build_env(&map(&[]), &[], &[PathBuf::from("/a:/b")]),
            Err(EnvError::InvalidPathDir(_))
        ));
    }
}
