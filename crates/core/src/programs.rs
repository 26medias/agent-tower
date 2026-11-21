//! Find the absolute paths of programs that a desktop PATH does not contain (spec 4.2,
//! "Working folder and programs").
//!
//! A desktop session has no `~/.local/bin` (Claude), no nvm folder (Codex, `node`) in its PATH
//! (PROBED at gate 2). So the app finds each program once, in this order:
//!
//! 1. the override path that the caller gives (the app config), which must be good;
//! 2. the login shell: `$SHELL -lic 'command -v <name>'`, with a time limit;
//! 3. the folders of the `PATH` of the app itself (a start from a terminal has the full PATH);
//! 4. known folders: `~/.local/bin`, `~/.cargo/bin`, the `bin` folder of each Node version of
//!    nvm (newest first), the folders of bun, volta, npm and pnpm, `/usr/local/bin`,
//!    `/opt/homebrew/bin`.
//!
//! A script that starts with `#!/usr/bin/env node` needs `node` on the session PATH.
//! `Resolver::interpreter_dirs` reads the shebang and resolves the interpreter the same way.

use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// Time limit for the login shell. A shell that loads nvm and other tools needs some seconds.
pub const DEFAULT_SHELL_TIMEOUT: Duration = Duration::from_secs(8);

/// How much of a script we read to find its shebang.
const SHEBANG_READ_LIMIT: u64 = 512;

#[derive(Debug, thiserror::Error)]
pub enum ProgramError {
    /// The name could be a shell command. Only `a-z A-Z 0-9 . _ + -` are allowed.
    #[error("{0:?} is not a valid program name")]
    InvalidName(String),
    /// The override path is not an absolute path to an executable file.
    #[error("the path for {name} is not an executable file: {}", path.display())]
    BadOverride { name: String, path: PathBuf },
    /// Nothing found. `tried` says what was tried, for the message to the operator.
    #[error("cannot find the program \"{name}\" (tried: {})", tried.join("; "))]
    NotFound { name: String, tried: Vec<String> },
    /// The program is a script that needs an interpreter that cannot be found.
    #[error("{} needs \"{interpreter}\", which cannot be found: {source}", script.display())]
    Interpreter {
        script: PathBuf,
        interpreter: String,
        source: Box<ProgramError>,
    },
    #[error("cannot read {}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
}

/// Where to look. `Resolver::from_env()` is the real one; tests build their own.
#[derive(Debug, Clone)]
pub struct Resolver {
    /// The login shell. `None` skips step 2.
    pub shell: Option<OsString>,
    /// Folders for step 3, in order.
    pub known_dirs: Vec<PathBuf>,
    /// Time limit of the login shell.
    pub shell_timeout: Duration,
}

impl Resolver {
    /// `$SHELL` (or `/bin/sh` when it is not set), the folders of `$PATH`, and the known
    /// folders under `$HOME`.
    pub fn from_env() -> Resolver {
        let shell = std::env::var_os("SHELL")
            .filter(|shell| !shell.is_empty())
            .or_else(|| Some("/bin/sh".into()));
        let home = std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(PathBuf::from);
        let path = std::env::var_os("PATH").unwrap_or_default();
        Resolver {
            shell,
            known_dirs: search_dirs(&path, home.as_deref()),
            shell_timeout: DEFAULT_SHELL_TIMEOUT,
        }
    }

    /// Find the absolute path of `name`. See the module text for the order.
    ///
    /// An override that is not good is an error. It does not fall back to the other steps:
    /// the operator typed that path, so a silent use of another program would surprise them.
    pub fn resolve(
        &self,
        name: &str,
        override_path: Option<&Path>,
    ) -> Result<PathBuf, ProgramError> {
        if !is_valid_name(name) {
            return Err(ProgramError::InvalidName(name.to_owned()));
        }
        if let Some(path) = override_path {
            if path.is_absolute() && is_executable_file(path) {
                return Ok(path.to_path_buf());
            }
            return Err(ProgramError::BadOverride {
                name: name.to_owned(),
                path: path.to_path_buf(),
            });
        }

        let mut tried = Vec::new();
        match &self.shell {
            Some(shell) => match login_shell_lookup(shell, name, self.shell_timeout) {
                Some(path) => return Ok(path),
                None => tried.push(format!(
                    "login shell {} (nothing usable)",
                    Path::new(shell).display()
                )),
            },
            None => tried.push("login shell (none set)".to_owned()),
        }
        for dir in &self.known_dirs {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                return Ok(candidate);
            }
            tried.push(dir.display().to_string());
        }
        Err(ProgramError::NotFound {
            name: name.to_owned(),
            tried,
        })
    }

    /// The folders that a script needs on its PATH because of its shebang.
    ///
    /// `#!/usr/bin/env node` gives the folder of `node`. A file with no shebang, a binary,
    /// or a shebang with an interpreter by path gives an empty list.
    pub fn interpreter_dirs(&self, script: &Path) -> Result<Vec<PathBuf>, ProgramError> {
        let io_error = |source| ProgramError::Io {
            path: script.to_path_buf(),
            source,
        };
        let mut head = Vec::new();
        std::fs::File::open(script)
            .and_then(|file| file.take(SHEBANG_READ_LIMIT).read_to_end(&mut head))
            .map_err(io_error)?;
        let Some(interpreter) = shebang_env_program(&head) else {
            return Ok(Vec::new());
        };
        let path =
            self.resolve(&interpreter, None)
                .map_err(|source| ProgramError::Interpreter {
                    script: script.to_path_buf(),
                    interpreter: interpreter.clone(),
                    source: Box::new(source),
                })?;
        Ok(path.parent().map(Path::to_path_buf).into_iter().collect())
    }

    /// The PATH folders for a session that runs `programs`: the folder of each program and the
    /// folders of its interpreters, with no duplicates. Give the result to `build_env`.
    pub fn path_dirs_for(&self, programs: &[PathBuf]) -> Result<Vec<PathBuf>, ProgramError> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        for program in programs {
            let own = program.parent().map(Path::to_path_buf);
            for dir in own.into_iter().chain(self.interpreter_dirs(program)?) {
                if !dirs.contains(&dir) {
                    dirs.push(dir);
                }
            }
        }
        Ok(dirs)
    }
}

/// `Resolver::from_env().resolve(name, override_path)`.
pub fn resolve(name: &str, override_path: Option<&Path>) -> Result<PathBuf, ProgramError> {
    Resolver::from_env().resolve(name, override_path)
}

/// `Resolver::from_env().interpreter_dirs(script)`.
pub fn interpreter_dirs(script: &Path) -> Result<Vec<PathBuf>, ProgramError> {
    Resolver::from_env().interpreter_dirs(script)
}

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
}

/// A file (after symlinks) with an execute bit.
pub fn is_executable_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// The folders for the steps 3 and 4 of the module text, in order, with no folder two times:
/// the absolute folders of `path` (the value of `PATH`), then the known folders under `home`,
/// then the known system folders.
pub fn search_dirs(path: &OsStr, home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .collect();
    if let Some(home) = home {
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".cargo/bin"));
        dirs.extend(nvm_bin_dirs(&home.join(".nvm/versions/node")));
        for known in [
            ".bun/bin",
            ".volta/bin",
            ".npm-global/bin",
            ".local/share/pnpm",
        ] {
            dirs.push(home.join(known));
        }
    }
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|dir| seen.insert(dir.clone()));
    dirs
}

/// The `bin` folder of each Node version that nvm installed, the newest version first.
fn nvm_bin_dirs(versions: &Path) -> Vec<PathBuf> {
    // `v24.10.0` gives [24, 10, 0]. A name that is not a version sorts last.
    fn version(name: &str) -> Vec<u64> {
        name.trim_start_matches('v')
            .split('.')
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    }
    let mut names: Vec<String> = std::fs::read_dir(versions)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort_by_key(|name| std::cmp::Reverse(version(name)));
    names
        .into_iter()
        .map(|name| versions.join(name).join("bin"))
        .collect()
}

/// Run `$SHELL -lic 'command -v <name>'` and parse the output. `None` on any failure.
fn login_shell_lookup(shell: &OsStr, name: &str, timeout: Duration) -> Option<PathBuf> {
    let mut command = Command::new(shell);
    command
        .arg("-lic")
        .arg(format!("command -v {name}"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // A new process group: an interactive shell with no terminal must not touch the
        // job control of ours, and a timeout can kill the whole group.
        .process_group(0);
    let mut child = spawn_with_retry(&mut command).ok()?;
    let mut stdout = child.stdout.take()?;

    // Read in a thread. A tool that an rc file starts in the background can hold the pipe
    // open after the shell ends. So we do not wait for the end of the pipe.
    let output = Arc::new(Mutex::new(Vec::<u8>::new()));
    let reader = {
        let output = Arc::clone(&output);
        thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(count) = stdout.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                output
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .extend_from_slice(&buffer[..count]);
            }
        })
    };

    let deadline = Instant::now() + timeout;
    let finished = loop {
        match child.try_wait() {
            Ok(Some(_)) => break true,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) | Err(_) => break false,
        }
    };
    if !finished {
        // SAFETY: `kill` with a negative pid signals a process group. The group is the one
        // that we made for this child, and the child is not yet reaped, so the id is ours.
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    // The shell ended. Give the reader a short time to take the last bytes.
    let grace = Instant::now() + Duration::from_millis(200);
    while !reader.is_finished() && Instant::now() < grace {
        thread::sleep(Duration::from_millis(5));
    }
    let bytes = output
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    last_absolute_executable(&bytes)
}

/// Start a command. A script that was just written can give "text file busy" for a moment,
/// when another thread forks while the write handle is open. Retry a few times.
fn spawn_with_retry(command: &mut Command) -> io::Result<std::process::Child> {
    let mut attempts = 0;
    loop {
        match command.spawn() {
            Err(error) if error.kind() == io::ErrorKind::ExecutableFileBusy && attempts < 20 => {
                attempts += 1;
                thread::sleep(Duration::from_millis(25));
            }
            result => return result,
        }
    }
}

/// An interactive shell can print other text (a banner, "Now using node v24"). Take the last
/// line that is an absolute path to an existing executable file.
fn last_absolute_executable(output: &[u8]) -> Option<PathBuf> {
    let text = String::from_utf8_lossy(output);
    text.lines()
        .rev()
        .map(|line| strip_escapes(line).trim().to_owned())
        .find(|line| line.starts_with('/') && is_executable_file(Path::new(line)))
        .map(PathBuf::from)
}

/// Remove ANSI escape sequences (`ESC [ ... letter` and `ESC ] ... BEL`) from a line.
fn strip_escapes(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for next in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                for next in chars.by_ref() {
                    if next == '\u{7}' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// For `#!/usr/bin/env [-S] [VAR=x] <program> ...`, the name of `<program>`. `None` for any
/// other first line, or when the program is given with a path.
fn shebang_env_program(head: &[u8]) -> Option<String> {
    let first_line = head.split(|&b| b == b'\n').next()?;
    let line = std::str::from_utf8(first_line).ok()?.strip_prefix("#!")?;
    let mut words = line.split_whitespace();
    let interpreter = words.next()?;
    if Path::new(interpreter).file_name()? != "env" {
        return None;
    }
    let program = words.find(|word| !word.starts_with('-') && !word.contains('='))?;
    (!program.contains('/') && is_valid_name(program)).then(|| program.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::{EnvMap, build_env, current_env};
    use std::fs;

    /// Write an executable file.
    fn write_exe(path: &Path, text: &str) {
        fs::write(path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn the_search_folders_are_the_path_then_the_known_folders_with_nvm_newest_first() {
        let home = tempfile::tempdir().unwrap();
        for version in ["v9.11.2", "v24.10.0", "v22.3.0"] {
            fs::create_dir_all(
                home.path()
                    .join(".nvm/versions/node")
                    .join(version)
                    .join("bin"),
            )
            .unwrap();
        }
        let path =
            std::env::join_paths(["/opt/tools/bin", "relative/bin", "/usr/local/bin"]).unwrap();
        let dirs = search_dirs(&path, Some(home.path()));
        let position = |dir: PathBuf| {
            dirs.iter()
                .position(|d| *d == dir)
                .unwrap_or_else(|| panic!("{} is not in {dirs:?}", dir.display()))
        };
        // The PATH comes first, in its order. A relative folder is not used.
        assert_eq!(position("/opt/tools/bin".into()), 0);
        assert_eq!(position("/usr/local/bin".into()), 1);
        assert!(!dirs.contains(&PathBuf::from("relative/bin")));
        // No folder is there two times.
        assert_eq!(
            dirs.iter()
                .filter(|d| **d == Path::new("/usr/local/bin"))
                .count(),
            1
        );
        let nvm = |version: &str| {
            home.path()
                .join(".nvm/versions/node")
                .join(version)
                .join("bin")
        };
        assert!(position(home.path().join(".local/bin")) < position(nvm("v24.10.0")));
        assert!(position(nvm("v24.10.0")) < position(nvm("v22.3.0")));
        assert!(position(nvm("v22.3.0")) < position(nvm("v9.11.2")));
        assert!(dirs.contains(&PathBuf::from("/opt/homebrew/bin")));
    }

    #[test]
    fn a_program_in_an_nvm_folder_is_found_when_the_login_shell_finds_nothing() {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join(".nvm/versions/node/v24.10.0/bin");
        fs::create_dir_all(&bin).unwrap();
        let program = bin.join("codex");
        fs::write(&program, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let resolver = Resolver {
            shell: Some("/bin/false".into()),
            known_dirs: search_dirs(OsStr::new("/usr/bin:/bin"), Some(home.path())),
            shell_timeout: Duration::from_secs(1),
        };
        assert_eq!(resolver.resolve("codex", None).unwrap(), program);
    }

    /// A resolver with a fake login shell that prints `output` and ignores its arguments.
    fn resolver_with_shell(dir: &Path, output: &str, known: &[&Path]) -> Resolver {
        let shell = dir.join("fake-shell");
        write_exe(
            &shell,
            &format!("#!/bin/sh\ncat <<'EOF_OUT'\n{output}\nEOF_OUT\n"),
        );
        Resolver {
            shell: Some(shell.into()),
            known_dirs: known.iter().map(|p| p.to_path_buf()).collect(),
            shell_timeout: Duration::from_secs(5),
        }
    }

    #[test]
    fn override_wins_over_the_shell() {
        let dir = tempfile::tempdir().unwrap();
        let from_shell = dir.path().join("from-shell");
        let from_override = dir.path().join("from-override");
        write_exe(&from_shell, "#!/bin/sh\n");
        write_exe(&from_override, "#!/bin/sh\n");
        let resolver = resolver_with_shell(dir.path(), &from_shell.display().to_string(), &[]);
        let found = resolver.resolve("tool", Some(&from_override)).unwrap();
        assert_eq!(found, from_override);
        // And with no override, the shell answer is used.
        assert_eq!(resolver.resolve("tool", None).unwrap(), from_shell);
    }

    #[test]
    fn a_bad_override_is_an_error_and_does_not_fall_back() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("good");
        write_exe(&good, "#!/bin/sh\n");
        let resolver = resolver_with_shell(dir.path(), &good.display().to_string(), &[]);
        let missing = dir.path().join("missing");
        let not_exec = dir.path().join("not-exec");
        fs::write(&not_exec, "x").unwrap();
        for bad in [
            missing.as_path(),
            not_exec.as_path(),
            dir.path(),
            Path::new("relative/tool"),
        ] {
            let result = resolver.resolve("tool", Some(bad));
            assert!(
                matches!(result, Err(ProgramError::BadOverride { .. })),
                "{bad:?}: {result:?}"
            );
        }
    }

    #[test]
    fn shell_noise_is_skipped_and_the_last_good_line_wins() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let last = dir.path().join("last");
        write_exe(&first, "#!/bin/sh\n");
        write_exe(&last, "#!/bin/sh\n");
        let missing = dir.path().join("does-not-exist");
        let not_exec = dir.path().join("not-exec");
        fs::write(&not_exec, "x").unwrap();
        let output = format!(
            "Welcome to the shell\nNow using node v24.10.0\n{}\n\x1b[32m{}\x1b[0m\nalias tool='tool --x'\n{}\n{}\nrelative/tool\ntrailing banner",
            first.display(),
            last.display(),
            missing.display(),
            not_exec.display()
        );
        let resolver = resolver_with_shell(dir.path(), &output, &[]);
        assert_eq!(resolver.resolve("tool", None).unwrap(), last);
    }

    #[test]
    fn shell_with_only_noise_falls_back_to_known_folders() {
        let dir = tempfile::tempdir().unwrap();
        let known = dir.path().join("known");
        fs::create_dir(&known).unwrap();
        write_exe(&known.join("tool"), "#!/bin/sh\n");
        let resolver = resolver_with_shell(dir.path(), "tool not found\nalias tool='x'", &[&known]);
        assert_eq!(resolver.resolve("tool", None).unwrap(), known.join("tool"));
    }

    #[test]
    fn known_folders_are_tried_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        write_exe(&b.join("tool"), "#!/bin/sh\n");
        let resolver = Resolver {
            shell: None,
            known_dirs: vec![a.clone(), b.clone()],
            shell_timeout: Duration::from_secs(1),
        };
        assert_eq!(resolver.resolve("tool", None).unwrap(), b.join("tool"));
        write_exe(&a.join("tool"), "#!/bin/sh\n");
        assert_eq!(resolver.resolve("tool", None).unwrap(), a.join("tool"));
    }

    #[test]
    fn a_stuck_shell_times_out_and_we_fall_back() {
        let dir = tempfile::tempdir().unwrap();
        let shell = dir.path().join("slow-shell");
        write_exe(&shell, "#!/bin/sh\nsleep 30\n");
        let known = dir.path().join("known");
        fs::create_dir(&known).unwrap();
        write_exe(&known.join("tool"), "#!/bin/sh\n");
        let resolver = Resolver {
            shell: Some(shell.into()),
            known_dirs: vec![known.clone()],
            shell_timeout: Duration::from_millis(300),
        };
        let start = Instant::now();
        assert_eq!(resolver.resolve("tool", None).unwrap(), known.join("tool"));
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn a_missing_shell_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let known = dir.path().join("known");
        fs::create_dir(&known).unwrap();
        write_exe(&known.join("tool"), "#!/bin/sh\n");
        let resolver = Resolver {
            shell: Some(dir.path().join("no-such-shell").into()),
            known_dirs: vec![known.clone()],
            shell_timeout: Duration::from_secs(1),
        };
        assert_eq!(resolver.resolve("tool", None).unwrap(), known.join("tool"));
    }

    #[test]
    fn not_found_says_what_was_tried() {
        let dir = tempfile::tempdir().unwrap();
        let resolver = Resolver {
            shell: None,
            known_dirs: vec![dir.path().to_path_buf()],
            shell_timeout: Duration::from_secs(1),
        };
        let error = resolver.resolve("tool", None).unwrap_err();
        assert!(matches!(error, ProgramError::NotFound { .. }));
        assert!(
            error
                .to_string()
                .contains(&dir.path().display().to_string())
        );
    }

    #[test]
    fn names_that_are_not_plain_are_refused() {
        let resolver = Resolver {
            shell: None,
            known_dirs: Vec::new(),
            shell_timeout: Duration::from_secs(1),
        };
        for name in ["", "a b", "a;b", "$(x)", "a/b", "-x", "a\nb", "`x`"] {
            assert!(
                matches!(
                    resolver.resolve(name, None),
                    Err(ProgramError::InvalidName(_))
                ),
                "{name:?}"
            );
        }
    }

    #[test]
    fn shebang_parser() {
        let parse = |text: &str| shebang_env_program(text.as_bytes());
        assert_eq!(parse("#!/usr/bin/env node\nrest"), Some("node".into()));
        assert_eq!(parse("#!/usr/bin/env node"), Some("node".into()));
        assert_eq!(
            parse("#!/usr/bin/env -S node --flag\n"),
            Some("node".into())
        );
        assert_eq!(parse("#!/usr/bin/env FOO=1 node\n"), Some("node".into()));
        assert_eq!(parse("#!  /bin/env   python3  \n"), Some("python3".into()));
        assert_eq!(parse("#!/bin/sh\n"), None);
        assert_eq!(parse("#!/home/a/.nvm/bin/node\n"), None);
        assert_eq!(parse("#!/usr/bin/env\n"), None);
        assert_eq!(parse("#!/usr/bin/env ./local\n"), None);
        assert_eq!(parse("echo hi\n#!/usr/bin/env node\n"), None);
        assert_eq!(parse(""), None);
        assert_eq!(shebang_env_program(b"\x7fELF\x02\x01\x01\x00"), None);
    }

    #[test]
    fn interpreter_dirs_resolves_node_like_any_program() {
        let dir = tempfile::tempdir().unwrap();
        let nvm = dir.path().join("nvm-bin");
        fs::create_dir(&nvm).unwrap();
        let node = nvm.join("node");
        write_exe(&node, "#!/bin/sh\n");
        let script = nvm.join("codex");
        write_exe(&script, "#!/usr/bin/env node\nconsole.log(1)\n");
        let resolver = resolver_with_shell(dir.path(), &node.display().to_string(), &[]);
        assert_eq!(
            resolver.interpreter_dirs(&script).unwrap(),
            vec![nvm.clone()]
        );

        // No shebang, a plain shell script and a binary need nothing.
        let plain = dir.path().join("plain");
        write_exe(&plain, "echo hi\n");
        let sh = dir.path().join("sh-script");
        write_exe(&sh, "#!/bin/sh\necho hi\n");
        let binary = dir.path().join("binary");
        fs::write(&binary, b"\x7fELF\x02\x01\x01\x00\xff\xfe").unwrap();
        for file in [&plain, &sh, &binary] {
            assert_eq!(
                resolver.interpreter_dirs(file).unwrap(),
                Vec::<PathBuf>::new()
            );
        }

        // The folder of the program and the folder of node, with no duplicates.
        let dirs = resolver
            .path_dirs_for(&[script.clone(), sh.clone()])
            .unwrap();
        assert_eq!(dirs, vec![nvm.clone(), dir.path().to_path_buf()]);
    }

    #[test]
    fn interpreter_dirs_reports_a_missing_interpreter() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("tool");
        write_exe(&script, "#!/usr/bin/env node\n");
        let resolver = Resolver {
            shell: None,
            known_dirs: Vec::new(),
            shell_timeout: Duration::from_secs(1),
        };
        let error = resolver.interpreter_dirs(&script).unwrap_err();
        assert!(
            matches!(error, ProgramError::Interpreter { .. }),
            "{error:?}"
        );
        assert!(matches!(
            resolver.interpreter_dirs(&dir.path().join("missing")),
            Err(ProgramError::Io { .. })
        ));
    }

    /// Resolve the real `claude` or `codex`, and run `<path> --version` in an environment from
    /// `build_env`. `--version` starts no model session. Skips when the program is not here.
    fn run_real_version(name: &str) {
        let resolver = Resolver::from_env();
        let path = match resolver.resolve(name, None) {
            Ok(path) => path,
            Err(error) => {
                eprintln!("SKIPPED real_{name}_version: {name} is not installed ({error})");
                return;
            }
        };
        assert!(path.is_absolute(), "{path:?}");
        let dirs = resolver.path_dirs_for(std::slice::from_ref(&path)).unwrap();
        let parent: EnvMap = current_env();
        let env = build_env(&parent, &[], &dirs).unwrap();
        let work = tempfile::tempdir().unwrap();
        let output = Command::new(&path)
            .arg("--version")
            .env_clear()
            .envs(&env)
            .current_dir(work.path())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        eprintln!(
            "real_{name}_version: {} --version -> {:?} (PATH={:?})",
            path.display(),
            String::from_utf8_lossy(&output.stdout).trim(),
            env.get(OsStr::new("PATH")).unwrap()
        );
        assert_eq!(
            output.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.stdout.is_empty(), "no version text");
    }

    #[test]
    fn real_claude_version() {
        run_real_version("claude");
    }

    #[test]
    fn real_codex_version() {
        run_real_version("codex");
    }
}
