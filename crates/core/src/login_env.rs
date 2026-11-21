//! The environment of the login shell of the operator (spec 4.2, "Clean environment").
//!
//! A desktop app starts with a small environment: no `~/.cargo/bin`, no `NVM_DIR`, no proxy.
//! A terminal has them, because the login shell reads the profile and the rc files. So the app
//! runs `$SHELL -lic '<print the environment>'` one time, and uses the result as the base of the
//! parent environment of each session (`SessionEnv`). `env::build_env` then filters it.
//!
//! The shell runs with the safety rules of `programs::run_login_shell`: no stdin, its own
//! process group, a reader thread, a time limit, and a kill of the group on a timeout.

use crate::env::EnvMap;
use crate::programs::run_login_shell;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStringExt;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// Time limit of the capture. An rc file that loads nvm and other tools needs some seconds.
pub const LOGIN_ENV_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a session start waits after the time limit of the capture, so that the capture
/// thread can kill a stuck shell and report the failure.
const WAIT_MARGIN: Duration = Duration::from_secs(2);

/// The line that comes before the environment. An rc file can print a banner before it. Only
/// the bytes after this line are the environment.
const MARKER: &str = "__TOWER_LOGIN_ENV_BEGIN__";

/// The shell command. `command` skips an alias or a function named `env`. `env -0` ends each
/// variable with a NUL byte, so a value with a newline survives.
const CAPTURE_SCRIPT: &str = "printf '%s\\n' __TOWER_LOGIN_ENV_BEGIN__; command env -0";

/// Run the login shell and read its environment. `None` when the shell does not start, does not
/// end before `timeout`, prints no marker, or gives no variable.
pub fn capture_login_env(shell: &OsStr, timeout: Duration) -> Option<EnvMap> {
    let bytes = run_login_shell(shell, CAPTURE_SCRIPT, timeout)?;
    parse_login_env(&bytes)
}

/// Read the bytes after the marker line as `NAME=value` items that end with NUL.
///
/// The last part is dropped when no NUL ends it: it can be text that a shell prints at its end,
/// or a cut value. An item with no `=` or an empty name is skipped.
pub fn parse_login_env(bytes: &[u8]) -> Option<EnvMap> {
    let marker = format!("{MARKER}\n");
    let start = bytes
        .windows(marker.len())
        .position(|window| window == marker.as_bytes())?
        + marker.len();
    let mut parts: Vec<&[u8]> = bytes[start..].split(|&byte| byte == 0).collect();
    parts.pop(); // the part after the last NUL
    let mut env = EnvMap::new();
    for part in parts {
        let Some(equals) = part.iter().position(|&byte| byte == b'=') else {
            continue;
        };
        if equals == 0 {
            continue;
        }
        env.insert(
            OsString::from_vec(part[..equals].to_vec()),
            OsString::from_vec(part[equals + 1..].to_vec()),
        );
    }
    (!env.is_empty()).then_some(env)
}

/// The login shell environment laid over the app environment: a name in `login` wins. With no
/// login environment, the result is the app environment.
pub fn overlay(app: &EnvMap, login: Option<&EnvMap>) -> EnvMap {
    let mut merged = app.clone();
    if let Some(login) = login {
        for (name, value) in login {
            merged.insert(name.clone(), value.clone());
        }
    }
    merged
}

/// Where the login shell environment comes from.
#[derive(Debug, Clone)]
pub enum LoginEnvSource {
    /// Run `shell -lic ...` in a background thread, with this time limit.
    Shell { shell: OsString, timeout: Duration },
    /// No shell runs. `Some(map)` is the login environment; `None` is a failed capture. Tests
    /// use this.
    Fixed(Option<EnvMap>),
}

impl LoginEnvSource {
    /// The real source: `$SHELL` (or `/bin/sh` when it is not set, as in `Resolver::from_env`)
    /// and `LOGIN_ENV_TIMEOUT`.
    pub fn from_env() -> LoginEnvSource {
        let shell = std::env::var_os("SHELL")
            .filter(|shell| !shell.is_empty())
            .unwrap_or_else(|| "/bin/sh".into());
        LoginEnvSource::Shell {
            shell,
            timeout: LOGIN_ENV_TIMEOUT,
        }
    }
}

/// The parent environment of the sessions. It starts the capture at once, in a thread, and `get`
/// waits for the result (at most the time limit of the capture, and a small margin).
pub(crate) struct SessionEnv {
    app: EnvMap,
    wait: Duration,
    cell: Arc<(Mutex<Option<EnvMap>>, Condvar)>,
}

impl SessionEnv {
    pub(crate) fn start(app: EnvMap, source: LoginEnvSource) -> SessionEnv {
        let cell = Arc::new((Mutex::new(None), Condvar::new()));
        let wait = match &source {
            LoginEnvSource::Shell { timeout, .. } => *timeout + WAIT_MARGIN,
            LoginEnvSource::Fixed(_) => Duration::ZERO,
        };
        let publish = {
            let cell = Arc::clone(&cell);
            move |merged: EnvMap| {
                let (slot, ready) = &*cell;
                *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(merged);
                ready.notify_all();
            }
        };
        match source {
            LoginEnvSource::Fixed(login) => publish(overlay(&app, login.as_ref())),
            LoginEnvSource::Shell { shell, timeout } => {
                let app = app.clone();
                thread::spawn(move || {
                    let login = capture_login_env(&shell, timeout);
                    publish(overlay(&app, login.as_ref()));
                });
            }
        }
        SessionEnv { app, wait, cell }
    }

    /// The parent environment for a session. It waits for the capture if it is not done. When
    /// the wait ends first, the result is the app environment.
    pub(crate) fn get(&self) -> EnvMap {
        let (slot, ready) = &*self.cell;
        let deadline = Instant::now() + self.wait;
        let mut guard = slot.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(merged) = guard.as_ref() {
                return merged.clone();
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return self.app.clone();
            }
            guard = ready
                .wait_timeout(guard, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn write_exe(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn map(pairs: &[(&str, &str)]) -> EnvMap {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    fn get<'a>(env: &'a EnvMap, name: &str) -> Option<&'a str> {
        env.get(OsStr::new(name)).and_then(|value| value.to_str())
    }

    /// A shell script that ignores its arguments and runs `body`.
    fn fake_shell(dir: &Path, body: &str) -> OsString {
        let shell = dir.join("fake-shell");
        write_exe(&shell, &format!("#!/bin/sh\n{body}\n"));
        shell.into_os_string()
    }

    #[test]
    fn the_capture_has_the_variables_and_not_the_banner() {
        let dir = tempfile::tempdir().unwrap();
        // A banner, the marker line, then NUL-separated variables. One value has a newline,
        // one has an `=`, one is empty.
        let shell = fake_shell(
            dir.path(),
            "printf 'Welcome to the shell\\nNow using node v24\\n'\n\
             printf '__TOWER_LOGIN_ENV_BEGIN__\\n'\n\
             printf 'CARGO_HOME=/home/a/.cargo\\0MULTI=line one\\nline two\\0EQ=a=b\\0EMPTY=\\0'",
        );
        let env = capture_login_env(&shell, Duration::from_secs(5)).expect("a capture");
        assert_eq!(get(&env, "CARGO_HOME"), Some("/home/a/.cargo"));
        assert_eq!(get(&env, "MULTI"), Some("line one\nline two"));
        assert_eq!(get(&env, "EQ"), Some("a=b"));
        assert_eq!(get(&env, "EMPTY"), Some(""));
        assert_eq!(env.len(), 4, "{:?}", env.keys().collect::<Vec<_>>());
    }

    #[test]
    fn the_script_that_the_app_sends_prints_the_marker_and_the_environment() {
        // The fake shell runs the script from its last argument, like `sh -c`, with a known
        // environment. It shows that `CAPTURE_SCRIPT` and the parser agree.
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(
            dir.path(),
            "echo 'rc file banner'\nexec env -i TOWER_TEST_VAR='a\nb' /bin/sh -c \"$2\"",
        );
        let env = capture_login_env(&shell, Duration::from_secs(5)).expect("a capture");
        assert_eq!(get(&env, "TOWER_TEST_VAR"), Some("a\nb"));
        assert!(
            !env.keys()
                .any(|name| name.to_string_lossy().contains("banner"))
        );
    }

    #[test]
    fn text_after_the_last_nul_is_dropped() {
        // The last part has no NUL: it is a cut value, or text that a shell prints at its end.
        let env = parse_login_env(b"junk\n__TOWER_LOGIN_ENV_BEGIN__\nA=1\0B=2\0CUT=half").unwrap();
        assert_eq!(env, map(&[("A", "1"), ("B", "2")]));
    }

    #[test]
    fn no_marker_gives_none() {
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(dir.path(), "printf 'A=1\\0B=2\\0'");
        assert_eq!(capture_login_env(&shell, Duration::from_secs(5)), None);
        assert_eq!(parse_login_env(b""), None);
    }

    #[test]
    fn zero_variables_gives_none() {
        assert_eq!(
            parse_login_env(b"banner\n__TOWER_LOGIN_ENV_BEGIN__\n"),
            None
        );
        // Only items with no `=` or an empty name.
        assert_eq!(
            parse_login_env(b"__TOWER_LOGIN_ENV_BEGIN__\nnoequals\0=novalue\0"),
            None
        );
    }

    #[test]
    fn a_missing_shell_gives_none() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-such-shell");
        assert_eq!(
            capture_login_env(missing.as_os_str(), Duration::from_secs(1)),
            None
        );
    }

    fn alive(pid: i32) -> bool {
        // A zombie is gone for our purpose.
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => {
                stat.rsplit(')')
                    .next()
                    .and_then(|rest| rest.split_whitespace().next())
                    != Some("Z")
            }
            Err(_) => false,
        }
    }

    #[test]
    fn a_shell_that_is_too_slow_gives_none_at_the_timeout_and_its_processes_are_gone() {
        let dir = tempfile::tempdir().unwrap();
        let pids = dir.path().join("pids");
        let body = format!(
            "echo $$ > '{0}'\nsleep 60 &\necho $! >> '{0}'\nwait",
            pids.display()
        );
        let shell = fake_shell(dir.path(), &body);
        let start = Instant::now();
        let result = capture_login_env(&shell, Duration::from_millis(600));
        let waited = start.elapsed();
        assert_eq!(result, None);
        assert!(waited >= Duration::from_millis(600), "{waited:?}");
        assert!(waited < Duration::from_secs(3), "{waited:?}");

        let pids: Vec<i32> = std::fs::read_to_string(&pids)
            .unwrap()
            .lines()
            .map(|line| line.trim().parse().unwrap())
            .collect();
        assert_eq!(pids.len(), 2, "the shell and its child");
        // An orphan is reaped by init a moment after the kill.
        let deadline = Instant::now() + Duration::from_secs(3);
        while pids.iter().any(|pid| alive(*pid)) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        for pid in pids {
            assert!(!alive(pid), "process {pid} is still alive");
        }
    }

    #[test]
    fn the_real_login_shell_gives_home_and_path() {
        let Some(shell) = std::env::var_os("SHELL").filter(|shell| !shell.is_empty()) else {
            eprintln!("SKIPPED the_real_login_shell_gives_home_and_path: SHELL is not set");
            return;
        };
        let env = capture_login_env(&shell, LOGIN_ENV_TIMEOUT).expect("a capture");
        // Names only: the values are not printed.
        assert!(env.contains_key(OsStr::new("HOME")), "no HOME");
        assert!(env.contains_key(OsStr::new("PATH")), "no PATH");
        assert!(!env.get(OsStr::new("PATH")).unwrap().is_empty());
    }

    #[test]
    fn the_login_environment_goes_over_the_app_environment() {
        let app = map(&[("A", "app"), ("B", "app")]);
        let login = map(&[("B", "login"), ("C", "login")]);
        assert_eq!(
            overlay(&app, Some(&login)),
            map(&[("A", "app"), ("B", "login"), ("C", "login")])
        );
        assert_eq!(overlay(&app, None), app);
    }

    #[test]
    fn session_env_waits_for_the_capture_and_runs_it_one_time() {
        let dir = tempfile::tempdir().unwrap();
        let counter = dir.path().join("runs");
        let shell = fake_shell(
            dir.path(),
            &format!(
                "echo run >> '{}'\nsleep 0.3\nprintf '__TOWER_LOGIN_ENV_BEGIN__\\nB=login\\0'",
                counter.display()
            ),
        );
        let session_env = SessionEnv::start(
            map(&[("A", "app"), ("B", "app")]),
            LoginEnvSource::Shell {
                shell,
                timeout: Duration::from_secs(5),
            },
        );
        let first = session_env.get();
        assert_eq!(first, map(&[("A", "app"), ("B", "login")]));
        assert_eq!(session_env.get(), first);
        assert_eq!(
            std::fs::read_to_string(&counter).unwrap().lines().count(),
            1
        );
    }

    #[test]
    fn session_env_with_a_failed_capture_is_the_app_environment() {
        let app = map(&[("A", "app")]);
        let fixed = SessionEnv::start(app.clone(), LoginEnvSource::Fixed(None));
        assert_eq!(fixed.get(), app);
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(dir.path(), "echo no marker");
        let from_shell = SessionEnv::start(
            app.clone(),
            LoginEnvSource::Shell {
                shell,
                timeout: Duration::from_secs(5),
            },
        );
        assert_eq!(from_shell.get(), app);
    }
}
