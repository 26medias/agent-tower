//! Clean environment for a CLI process (spec 4.2, "Clean environment").
//!
//! The app never passes its own environment to a CLI. `build_env` makes a new map from an
//! allow-list of the parent environment, plus the values that the caller gives. It is a pure
//! function: it reads no process state, so tests give it maps.
//!
//! An inherited `CLAUDE_CODE_CHILD_SESSION` turns transcript saving off in Claude, and then
//! resume has nothing to resume. The names `CLAUDECODE` and `CLAUDE_CODE_*` are therefore
//! never in the result.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

/// An environment: variable name to value.
pub type EnvMap = BTreeMap<OsString, OsString>;

/// The first folders of the PATH of each session.
pub const BASE_PATH_DIRS: [&str; 2] = ["/usr/bin", "/bin"];

/// The value of `TERM` when the parent has none.
pub const DEFAULT_TERM: &str = "xterm-256color";

/// Names that are copied from the parent when they are present.
const ALLOWED_NAMES: [&str; 10] = [
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "TERM",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "DBUS_SESSION_BUS_ADDRESS",
    "SSH_AUTH_SOCK",
];

/// Prefixes of names that are copied from the parent.
const ALLOWED_PREFIXES: [&str; 2] = ["LC_", "XDG_"];

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

fn is_allowed_name(name: &OsStr) -> bool {
    let bytes = name.as_bytes();
    ALLOWED_NAMES
        .iter()
        .any(|allowed| allowed.as_bytes() == bytes)
        || ALLOWED_PREFIXES
            .iter()
            .any(|prefix| bytes.starts_with(prefix.as_bytes()))
}

/// Make the environment of a CLI process.
///
/// - Allow-list from `parent`: `HOME USER LOGNAME SHELL LANG LC_* TERM DISPLAY WAYLAND_DISPLAY
///   XDG_* DBUS_SESSION_BUS_ADDRESS SSH_AUTH_SOCK`. Nothing else is copied.
/// - `TERM` is `xterm-256color` when the parent has none (or an empty one).
/// - `PATH` is `/usr/bin:/bin` and then `path_dirs`, in order, with no duplicates. The parent
///   `PATH` is not copied.
/// - `extra` is added last and wins over a parent value. It carries `TOWER_SESSION`,
///   `TOWER_SOCKET` and the values of the adapter.
///
/// Errors (and no partial result): a name in `extra` that is `CLAUDECODE` or starts with
/// `CLAUDE_CODE_`; `PATH` in `extra`; an invalid name or value; a bad PATH folder. An error is
/// better than a silent drop: such a value in `extra` is a bug of the caller.
pub fn build_env(
    parent: &EnvMap,
    extra: &[(OsString, OsString)],
    path_dirs: &[PathBuf],
) -> Result<EnvMap, EnvError> {
    let mut env = EnvMap::new();

    for (name, value) in parent {
        if is_allowed_name(name) && !is_forbidden_name(name) {
            env.insert(name.clone(), value.clone());
        }
    }
    if env
        .get(OsStr::new("TERM"))
        .is_none_or(|term| term.is_empty())
    {
        env.insert("TERM".into(), DEFAULT_TERM.into());
    }

    let mut dirs: Vec<PathBuf> = BASE_PATH_DIRS.iter().map(PathBuf::from).collect();
    for dir in path_dirs {
        let bytes = dir.as_os_str().as_bytes();
        if !dir.is_absolute() || bytes.contains(&b':') || bytes.contains(&0) {
            return Err(EnvError::InvalidPathDir(dir.clone()));
        }
        if !dirs.contains(dir) {
            dirs.push(dir.clone());
        }
    }
    let path = std::env::join_paths(&dirs)
        .map_err(|_| EnvError::InvalidPathDir(dirs.last().cloned().unwrap_or_default()))?;
    env.insert("PATH".into(), path);

    for (name, value) in extra {
        let bytes = name.as_bytes();
        if bytes.is_empty() || bytes.contains(&b'=') || bytes.contains(&0) {
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

/// The environment of this process as a map. The app calls this once and gives the result to
/// `build_env`. Tests do not use it, except the test that runs a real CLI.
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
    fn copies_only_the_allow_list() {
        let parent = map(&[
            ("HOME", "/home/a"),
            ("USER", "a"),
            ("LOGNAME", "a"),
            ("SHELL", "/bin/zsh"),
            ("LANG", "fr_FR.UTF-8"),
            ("LC_ALL", "C"),
            ("LC_TIME", "en_GB"),
            ("TERM", "screen"),
            ("DISPLAY", ":0"),
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ("XDG_CONFIG_HOME", "/home/a/.config"),
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
            ("SSH_AUTH_SOCK", "/run/user/1000/ssh"),
            // Not allowed:
            ("PATH", "/home/a/.nvm/bin:/usr/bin"),
            ("AWS_SECRET_ACCESS_KEY", "secret"),
            ("OPENAI_API_KEY", "secret"),
            ("ANTHROPIC_API_KEY", "secret"),
            ("PWD", "/tmp"),
            ("LCX", "not a LC_ name"),
            ("XDG", "not an XDG_ name"),
            ("TOWER_SESSION", "from the parent"),
        ]);
        let env = build_env(&parent, &[], &[]).unwrap();
        let names: Vec<&str> = env.keys().filter_map(|key| key.to_str()).collect();
        assert_eq!(
            names,
            vec![
                "DBUS_SESSION_BUS_ADDRESS",
                "DISPLAY",
                "HOME",
                "LANG",
                "LC_ALL",
                "LC_TIME",
                "LOGNAME",
                "PATH",
                "SHELL",
                "SSH_AUTH_SOCK",
                "TERM",
                "USER",
                "WAYLAND_DISPLAY",
                "XDG_CONFIG_HOME",
                "XDG_RUNTIME_DIR",
            ]
        );
        assert_eq!(get(&env, "TERM"), Some("screen"));
        assert_eq!(get(&env, "HOME"), Some("/home/a"));
    }

    #[test]
    fn term_has_a_default() {
        let env = build_env(&map(&[]), &[], &[]).unwrap();
        assert_eq!(get(&env, "TERM"), Some("xterm-256color"));
        let env = build_env(&map(&[("TERM", "")]), &[], &[]).unwrap();
        assert_eq!(get(&env, "TERM"), Some("xterm-256color"));
    }

    #[test]
    fn claude_variables_in_the_parent_are_dropped() {
        // None of these is in the allow-list. The test also shows that a future change of the
        // allow-list (for example to a prefix) cannot let them through.
        let parent = map(&[
            ("HOME", "/home/a"),
            ("CLAUDE_CODE_CHILD_SESSION", "1"),
            ("CLAUDE_CODE_ENTRYPOINT", "cli"),
            ("CLAUDECODE", "1"),
        ]);
        let env = build_env(&parent, &[], &[]).unwrap();
        assert!(env.keys().all(|key| !is_forbidden_name(key)));
        assert_eq!(get(&env, "HOME"), Some("/home/a"));
        assert!(!is_allowed_name(OsStr::new("CLAUDE_CODE_CHILD_SESSION")));
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
    fn path_is_built_and_never_copied() {
        let parent = map(&[("PATH", "/evil:/usr/bin")]);
        let dirs = [
            PathBuf::from("/home/a/.local/bin"),
            PathBuf::from("/home/a/.nvm/versions/node/v24/bin"),
            PathBuf::from("/home/a/.local/bin"),
            PathBuf::from("/usr/bin"),
        ];
        let env = build_env(&parent, &[], &dirs).unwrap();
        assert_eq!(
            get(&env, "PATH"),
            Some("/usr/bin:/bin:/home/a/.local/bin:/home/a/.nvm/versions/node/v24/bin")
        );
        let env = build_env(&parent, &[], &[]).unwrap();
        assert_eq!(get(&env, "PATH"), Some("/usr/bin:/bin"));
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
