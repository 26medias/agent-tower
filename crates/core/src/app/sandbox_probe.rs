//! The probe of the Codex sandbox (Settings, "Codex sandbox"; decision O7, option A).
//!
//! The Codex sandbox on Linux uses `bwrap` (bubblewrap). On Ubuntu 24.04 the kernel setting
//! `kernel.apparmor_restrict_unprivileged_userns = 1` stops `bwrap` from using user namespaces,
//! unless an AppArmor profile allows it. The probe runs the smallest command that needs a user
//! namespace:
//!
//! ```text
//! bwrap --unshare-user --ro-bind / / true
//! ```
//!
//! It finds `bwrap` in the PATH of a session, starts it with no shell and with the environment
//! of a session, and waits at most `PROBE_TIMEOUT`. It starts no Codex session and no model.

use crate::app::program_test::{clean_line, collect, first_line};
use crate::env::EnvMap;
use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// The time limit of the probe. A `bwrap` that runs longer is killed with its process group.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// The program that the probe runs.
pub const BWRAP: &str = "bwrap";

/// The arguments of the probe.
pub const PROBE_ARGS: [&str; 4] = ["--unshare-user", "--ro-bind", "/", "/"];

/// What the probe found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxState {
    /// `bwrap` ran the command and ended with exit code 0.
    Works,
    /// `bwrap` is there, but it failed, or it did not end in time.
    Fails,
    /// No `bwrap` in the PATH of a session.
    Missing,
}

impl SandboxState {
    /// The word in the interface contract.
    pub fn as_str(self) -> &'static str {
        match self {
            SandboxState::Works => "works",
            SandboxState::Fails => "fails",
            SandboxState::Missing => "missing",
        }
    }
}

/// The result of one probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxProbe {
    pub state: SandboxState,
    /// One line, with no control character: the path of `bwrap` for `works`, the first line of
    /// the error for `fails`, and a short sentence for `missing`.
    pub detail: String,
}

/// The first `bwrap` in the PATH of `env`. A folder that is not an absolute path is skipped.
fn find_bwrap(env: &EnvMap) -> Option<PathBuf> {
    let path = env.get(OsStr::new("PATH"))?;
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(BWRAP))
        .find(|candidate| {
            candidate
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

/// Run the probe. `env` is the full environment of the process (nothing else is inherited).
pub(crate) fn run(env: &EnvMap, cwd: &Path, timeout: Duration) -> SandboxProbe {
    let Some(program) = find_bwrap(env) else {
        return SandboxProbe {
            state: SandboxState::Missing,
            detail: "bwrap was not found in the PATH of a session".to_owned(),
        };
    };
    let fails = |text: &str| SandboxProbe {
        state: SandboxState::Fails,
        detail: clean_line(text),
    };
    let mut command = Command::new(&program);
    command
        .args(PROBE_ARGS)
        .arg("true")
        .env_clear()
        .envs(env)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Its own process group: a timeout kills bwrap and what it started.
        .process_group(0);
    let mut child = match crate::programs::spawn_with_retry(&mut command) {
        Ok(child) => child,
        Err(error) => return fails(&format!("cannot run {}: {error}", program.display())),
    };
    let stdout = collect(child.stdout.take());
    let stderr = collect(child.stderr.take());

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) | Err(_) => break None,
        }
    };
    let Some(status) = status else {
        // SAFETY: a negative pid signals the process group that we made for this child. The
        // child is not reaped yet, so the ID is still ours.
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        let _ = child.kill();
        let _ = child.wait();
        return fails(&format!(
            "bwrap did not end in {} s",
            timeout.as_secs_f64().ceil() as u64
        ));
    };
    let out = stdout.take_text();
    let err = stderr.take_text();
    if status.success() {
        return SandboxProbe {
            state: SandboxState::Works,
            detail: clean_line(&format!("bwrap works: {}", program.display())),
        };
    }
    match first_line(&err).or_else(|| first_line(&out)) {
        Some(line) => fails(&line),
        None => match (status.code(), status.signal()) {
            (Some(code), _) => fails(&format!("bwrap ended with exit code {code}")),
            (None, Some(signal)) => fails(&format!("bwrap ended by signal {signal}")),
            (None, None) => fails("bwrap ended with an error"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join(BWRAP);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn env_with(path: &str) -> EnvMap {
        [("PATH", path), ("MARK", "from-the-env")]
            .iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect()
    }

    fn probe(dir: &Path, limit: Duration) -> SandboxProbe {
        let path = format!("{}:/usr/bin:/bin", dir.display());
        run(&env_with(&path), Path::new("/"), limit)
    }

    #[test]
    fn a_bwrap_that_ends_with_zero_works_and_gets_the_probe_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("args.txt");
        script(
            dir.path(),
            &format!("echo \"$@:$MARK\" > {}", log.display()),
        );
        let result = probe(dir.path(), Duration::from_secs(5));
        assert_eq!(result.state, SandboxState::Works, "{result:?}");
        assert!(result.detail.contains("bwrap works"), "{result:?}");
        assert_eq!(
            fs::read_to_string(&log).unwrap().trim(),
            "--unshare-user --ro-bind / / true:from-the-env"
        );
    }

    #[test]
    fn a_bwrap_that_fails_gives_the_first_line_of_the_error() {
        let dir = tempfile::tempdir().unwrap();
        script(
            dir.path(),
            "echo 'bwrap: setting up uid map: Permission denied' >&2; echo 'second' >&2; exit 1",
        );
        let result = probe(dir.path(), Duration::from_secs(5));
        assert_eq!(result.state, SandboxState::Fails);
        assert_eq!(
            result.detail,
            "bwrap: setting up uid map: Permission denied"
        );
    }

    #[test]
    fn a_bwrap_that_fails_with_no_text_gives_the_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        script(dir.path(), "exit 7");
        let result = probe(dir.path(), Duration::from_secs(5));
        assert_eq!(result.state, SandboxState::Fails);
        assert_eq!(result.detail, "bwrap ended with exit code 7");
    }

    #[test]
    fn no_bwrap_in_the_path_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let result = run(
            &env_with(&dir.path().display().to_string()),
            Path::new("/"),
            Duration::from_secs(5),
        );
        assert_eq!(result.state, SandboxState::Missing);
        // A file that is not executable, and a relative folder, are not a bwrap.
        let path = dir.path().join(BWRAP);
        fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        let result = run(
            &env_with(&dir.path().display().to_string()),
            Path::new("/"),
            Duration::from_secs(5),
        );
        assert_eq!(result.state, SandboxState::Missing);
        // A relative PATH entry is skipped, also when it holds a good `bwrap` (a folder of the
        // working folder of the test process).
        let local = tempfile::tempdir_in(".").unwrap();
        script(local.path(), "exit 0");
        let relative = local
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            run(&env_with(&relative), Path::new("/"), Duration::from_secs(5)).state,
            SandboxState::Missing
        );
        assert_eq!(
            run(&EnvMap::new(), Path::new("/"), Duration::from_secs(5)).state,
            SandboxState::Missing
        );
    }

    #[test]
    fn a_bwrap_that_hangs_is_killed_at_the_time_limit() {
        let dir = tempfile::tempdir().unwrap();
        script(dir.path(), "sleep 30");
        let start = Instant::now();
        let result = probe(dir.path(), Duration::from_millis(300));
        assert_eq!(result.state, SandboxState::Fails);
        assert_eq!(result.detail, "bwrap did not end in 1 s");
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}
