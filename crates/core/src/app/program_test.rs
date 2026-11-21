//! The test of a program path (Settings, "Programs", the button "test").
//!
//! `Tower::program_test(name)` runs `<resolved path> --version` with the environment that a
//! session gets (`SessionEnv` and `env::build_env`). It starts no model session and no shell.
//! The result has a stable shape: `ok`, the path that was tried, and one line of text.

use crate::env::EnvMap;
use std::io::Read;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// The time limit of one test. A program that runs longer is killed with its process group.
pub const PROGRAM_TEST_TIMEOUT: Duration = Duration::from_secs(10);

/// The longest `output`, in characters.
pub const OUTPUT_MAX_CHARS: usize = 200;

/// How much of the output of the program the app keeps. A program that prints more is cut.
const READ_LIMIT: usize = 64 * 1024;

/// The result of `program_test`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramTest {
    /// The program ran, and it ended with exit code 0.
    pub ok: bool,
    /// The path that was tried. `None` when the app found no program with this name.
    pub path: Option<PathBuf>,
    /// The first line of what the program printed, or the text of the error. At most
    /// `OUTPUT_MAX_CHARS` characters, with no control character.
    pub output: String,
}

impl ProgramTest {
    pub(crate) fn not_found(error: &str) -> ProgramTest {
        ProgramTest {
            ok: false,
            path: None,
            output: clean_line(error),
        }
    }
}

/// Run `<path> --version`. `env` is the full environment of the process (nothing else is
/// inherited). `cwd` is the working folder.
pub(crate) fn run_version(path: &Path, env: &EnvMap, cwd: &Path, timeout: Duration) -> ProgramTest {
    let result = |ok: bool, text: &str| ProgramTest {
        ok,
        path: Some(path.to_path_buf()),
        output: clean_line(text),
    };
    let mut command = Command::new(path);
    command
        .arg("--version")
        .env_clear()
        .envs(env)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Its own process group: a timeout kills the program and what it started.
        .process_group(0);
    let mut child = match crate::programs::spawn_with_retry(&mut command) {
        Ok(child) => child,
        Err(error) => return result(false, &format!("cannot run it: {error}")),
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
        return result(
            false,
            &format!(
                "it did not end in {} s",
                timeout.as_secs_f64().ceil() as u64
            ),
        );
    };
    // The program ended. Give the readers a short time to take the last bytes.
    let out = stdout.take_text();
    let err = stderr.take_text();
    if status.success() {
        let line = first_line(&out).or_else(|| first_line(&err));
        return result(true, line.as_deref().unwrap_or("(no output)"));
    }
    let how = match (status.code(), status.signal()) {
        (Some(code), _) => format!("exit code {code}"),
        (None, Some(signal)) => format!("ended by signal {signal}"),
        (None, None) => "ended with an error".to_owned(),
    };
    match first_line(&err).or_else(|| first_line(&out)) {
        Some(line) => result(false, &format!("{how}: {line}")),
        None => result(false, &how),
    }
}

/// Reads a pipe in a thread, up to `READ_LIMIT` bytes (the rest is read and dropped, so the
/// program never blocks on a full pipe).
struct Collected {
    bytes: Arc<Mutex<Vec<u8>>>,
    reader: Option<thread::JoinHandle<()>>,
}

fn collect<R: Read + Send + 'static>(pipe: Option<R>) -> Collected {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let reader = pipe.map(|mut pipe| {
        let bytes = Arc::clone(&bytes);
        thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(count) = pipe.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                let mut kept = bytes.lock().unwrap_or_else(PoisonError::into_inner);
                let room = READ_LIMIT.saturating_sub(kept.len());
                kept.extend_from_slice(&buffer[..count.min(room)]);
            }
        })
    });
    Collected { bytes, reader }
}

impl Collected {
    /// The text read so far. It waits a short time for the reader to end: a program that
    /// started a background process can hold the pipe open after it ended. The program has ended,
    /// so the bytes are in the pipe, and the reader only needs some time to take them (a busy
    /// machine can make it late).
    fn take_text(self) -> String {
        let grace = Instant::now() + Duration::from_millis(1000);
        if let Some(reader) = &self.reader {
            while !reader.is_finished() && Instant::now() < grace {
                thread::sleep(Duration::from_millis(5));
            }
        }
        let bytes = self.bytes.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// The first line that has text, with the escape sequences of a terminal removed.
fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(clean_line)
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_owned())
}

/// The first line of `text`, with no escape sequence and no control character, cut to
/// `OUTPUT_MAX_CHARS` characters.
pub(crate) fn clean_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("");
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // An escape sequence: `ESC [ ... final byte`. Other escapes lose only the ESC.
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }
        if !c.is_control() {
            out.push(c);
        }
        if out.chars().count() >= OUTPUT_MAX_CHARS {
            break;
        }
    }
    out.chars().take(OUTPUT_MAX_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn env() -> EnvMap {
        [("PATH", "/usr/bin:/bin"), ("MARK", "from-the-env")]
            .iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect()
    }

    fn run(path: &Path, timeout: Duration) -> ProgramTest {
        run_version(path, &env(), Path::new("/"), timeout)
    }

    const LIMIT: Duration = Duration::from_secs(5);

    #[test]
    fn a_good_program_gives_its_first_line_of_stdout() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(
            dir.path(),
            "good",
            "echo '2.1.330 (Claude Code)'; echo second",
        );
        let result = run(&program, LIMIT);
        assert!(result.ok, "{result:?}");
        assert_eq!(result.output, "2.1.330 (Claude Code)");
        assert_eq!(result.path.as_deref(), Some(program.as_path()));
    }

    #[test]
    fn the_program_gets_the_given_environment_and_only_that() {
        let dir = tempfile::tempdir().unwrap();
        // HOME is not in the given environment, so it is empty.
        let program = script(dir.path(), "envs", "echo \"$MARK:${HOME:-none}:$1\"");
        let result = run(&program, LIMIT);
        assert!(result.ok);
        assert_eq!(result.output, "from-the-env:none:--version");
    }

    #[test]
    fn a_failing_program_gives_the_exit_code_and_the_error_text() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "bad", "echo 'bad flag' >&2; exit 3");
        let result = run(&program, LIMIT);
        assert!(!result.ok);
        assert_eq!(result.output, "exit code 3: bad flag");
    }

    #[test]
    fn a_program_that_cannot_start_gives_the_error() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("not-here");
        let result = run(&program, LIMIT);
        assert!(!result.ok);
        assert!(
            result.output.starts_with("cannot run it:"),
            "{}",
            result.output
        );
        // A file that is not executable.
        let plain = dir.path().join("plain");
        fs::write(&plain, "x").unwrap();
        let result = run(&plain, LIMIT);
        assert!(!result.ok);
        assert!(
            result.output.starts_with("cannot run it:"),
            "{}",
            result.output
        );
    }

    #[test]
    fn a_program_that_runs_too_long_is_killed_with_its_group() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("child-ran");
        // The child of the script would write the marker after 2 s. The kill must stop it.
        let program = script(
            dir.path(),
            "slow",
            &format!("(sleep 2; touch {}) &\nsleep 30", marker.display()),
        );
        let started = Instant::now();
        let result = run(&program, Duration::from_millis(400));
        assert!(!result.ok);
        assert_eq!(result.output, "it did not end in 1 s");
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        thread::sleep(Duration::from_millis(2200));
        assert!(!marker.exists(), "the child of the program was not killed");
    }

    #[test]
    fn a_noisy_program_does_not_block_and_the_output_is_cut() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(
            dir.path(),
            "noisy",
            "head -c 300000 /dev/zero | tr '\\0' 'a'",
        );
        let result = run(&program, LIMIT);
        assert!(result.ok, "{result:?}");
        assert_eq!(result.output.chars().count(), OUTPUT_MAX_CHARS);
    }

    #[test]
    fn a_program_that_prints_the_version_on_stderr_is_still_good() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "err", "echo 'v9.9' >&2");
        let result = run(&program, LIMIT);
        assert!(result.ok);
        assert_eq!(result.output, "v9.9");
        let quiet = script(dir.path(), "quiet", "true");
        assert_eq!(run(&quiet, LIMIT).output, "(no output)");
    }

    #[test]
    fn the_output_has_one_line_at_most_200_characters_and_no_control_character() {
        assert_eq!(clean_line("one\ntwo"), "one");
        assert_eq!(clean_line("a\u{7}b\tc\u{0}d\u{9b}e"), "abcde");
        assert_eq!(clean_line("\u{1b}[31mred\u{1b}[0m text"), "red text");
        assert_eq!(clean_line(&"é".repeat(500)).chars().count(), 200);
        assert_eq!(clean_line(""), "");
        // Leading blank lines are skipped for a success, by `first_line`.
        assert_eq!(
            first_line("\n  \nversion 1\n"),
            Some("version 1".to_owned())
        );
    }
}
