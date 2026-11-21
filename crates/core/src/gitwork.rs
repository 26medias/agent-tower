//! The git actions of the app for task worktrees (spec 4.8, changed in M3 on 2026-11-21).
//!
//! # Safety rules
//!
//! - Each command runs with an explicit working folder, with a fixed list of arguments (never a
//!   shell string), with a time limit, and with a clean environment: no variable that starts with
//!   `GIT_` and no `PAGER` comes from the parent process (`clean_environment`).
//! - Every command runs with `-c core.fsmonitor=false`, so a `core.fsmonitor` program of a
//!   repository config never runs.
//! - A command that can run a hook of the repository (`worktree add`, `commit`, `merge`) runs
//!   with `-c core.hooksPath=/dev/null`, so no hook of the operator's repository runs during an
//!   action of the app.
//! - The agent can write the `.git` file of its worktree. So before the app runs a command in a
//!   worktree, it checks that git says the git dir of that worktree is the one that git made for
//!   it (`verified_git_dir`). Then the commands name that git dir (`--git-dir`), and they do not
//!   read the `.git` file again.
//! - The snapshot does not commit secrets, build output, big files or nested repositories
//!   (`staged_problems`). The one `git reset` of the app is there: it empties the index of the
//!   worktree after a refusal. It is never `--hard`.
//! - An argument that comes from a task (the title) is never an argument that can start with `-`.
//!   A branch name is built from `[a-z0-9-]` only (`branch_slug`). The title goes into a commit
//!   message after the flag `-m`, and the message starts with the task ID.
//! - The app never uses `--force`, `reset --hard`, `checkout` in the main checkout, or `push`. The
//!   one exception is `discard_worktree`: it removes the worktree of one task with `worktree remove
//!   --force` and the branch of that task with `branch -D`. It checks the path and the branch
//!   name first.
//! - Diffs run with `--no-ext-diff` and `--no-textconv`, so no program that the repository
//!   configures runs to make a diff.
//!
//! The functions here do no locking. The caller (`app/merge.rs`) holds the git lock of the floor.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// The time limit of a short git command.
pub const SHORT_TIMEOUT: Duration = Duration::from_secs(30);

/// The time limit of a git command that can take long (a merge, a big diff).
pub const LONG_TIMEOUT: Duration = Duration::from_secs(120);

/// The most bytes that the app keeps of the output of one command.
const OUTPUT_LIMIT: usize = 4 * 1024 * 1024;

/// The longest part of the title in a branch name, in characters.
const SLUG_MAX: usize = 40;

/// The domain of the e-mail of an employee, used only when the repository has no user.
pub const EMPLOYEE_EMAIL_DOMAIN: &str = "agent-tower.local";

/// Is this variable of the parent process one that the app does not pass to git? Every name that
/// starts with `GIT_` can send git to another repository, a config file, a program or an editor.
/// `PAGER` can start a program. The app sets the few that it needs (`GIT_TERMINAL_PROMPT` and
/// others) itself.
fn is_blocked_variable(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    name.starts_with("GIT_") || name == "PAGER"
}

/// Put the environment of a git command in `command`: the variables of `parent`, except those that
/// `is_blocked_variable` names, then the fixed variables of the app.
fn clean_environment(
    command: &mut Command,
    parent: impl IntoIterator<Item = (OsString, OsString)>,
) {
    command.env_clear();
    for (name, value) in parent {
        if !is_blocked_variable(&name) {
            command.env(name, value);
        }
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .env("GIT_EDITOR", "true")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C");
}

/// The result of one git command.
#[derive(Debug, Clone)]
pub struct GitOutput {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl GitOutput {
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).trim().to_owned()
    }

    /// The text for a person: stderr, or stdout when stderr is empty.
    pub fn message(&self) -> String {
        let err = self.stderr.trim();
        if err.is_empty() {
            self.stdout_text()
        } else {
            err.to_owned()
        }
    }
}

/// How `run` starts a command.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Add `-c core.hooksPath=/dev/null` (and `-c commit.gpgsign=false`).
    pub no_hooks: bool,
    pub timeout: Duration,
}

impl Options {
    pub const READ: Options = Options {
        no_hooks: false,
        timeout: SHORT_TIMEOUT,
    };
    pub const WRITE: Options = Options {
        no_hooks: true,
        timeout: LONG_TIMEOUT,
    };
}

/// Run `git <args>` in `cwd`. `Err` is a failure to start the command or a timeout. A command
/// that runs and fails is `Ok` with `success` false.
pub fn run(cwd: &Path, args: &[&str], options: Options) -> Result<GitOutput, String> {
    run_with_environment(cwd, args, options, std::env::vars_os())
}

/// `run`, with the environment of the parent process given as an argument (a test gives its own).
fn run_with_environment(
    cwd: &Path,
    args: &[&str],
    options: Options,
    parent: impl IntoIterator<Item = (OsString, OsString)>,
) -> Result<GitOutput, String> {
    let mut command = Command::new("git");
    command.current_dir(cwd);
    // Never run the `core.fsmonitor` program of a repository config.
    command.args(["-c", "core.fsmonitor=false"]);
    if options.no_hooks {
        command.args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
        ]);
    }
    command.args(args);
    clean_environment(&mut command, parent);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "the program git is not installed, or it is not in the PATH of the app".to_owned()
        } else {
            format!("cannot start git: {e}")
        }
    })?;
    let stdout_reader = drain(child.stdout.take());
    let stderr_reader = drain(child.stderr.take());
    let deadline = Instant::now() + options.timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!(
                    "git {} did not end in {} seconds. The app stopped it.",
                    args.iter()
                        .find(|a| !a.starts_with('-'))
                        .copied()
                        .unwrap_or(""),
                    options.timeout.as_secs()
                ));
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(e) => break Err(format!("cannot wait for git: {e}")),
        }
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    let status = status?;
    Ok(GitOutput {
        success: status.success(),
        stdout,
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// Read a pipe on its own thread. It keeps `OUTPUT_LIMIT` bytes and drops the rest.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut kept = Vec::new();
        if let Some(mut pipe) = pipe {
            let mut limited = (&mut pipe).take(OUTPUT_LIMIT as u64);
            let _ = limited.read_to_end(&mut kept);
            // Read the rest, so the command does not block on a full pipe.
            let _ = std::io::copy(&mut pipe, &mut std::io::sink());
        }
        kept
    })
}

/// Run a command that must succeed. `what` names the action in the error text.
fn run_ok(cwd: &Path, args: &[&str], options: Options, what: &str) -> Result<GitOutput, String> {
    let out = run(cwd, args, options)?;
    if out.success {
        Ok(out)
    } else {
        Err(format!("{what} failed: {}", out.message()))
    }
}

// ---------------------------------------------------------------------------------------------
// The state of the floor folder
// ---------------------------------------------------------------------------------------------

/// What the floor folder is, for the worktree rules of spec 4.8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoState {
    /// The floor folder is the top of a repository that has a commit, on a branch.
    Ready { branch: String, head: String },
    /// No repository (or `git` is not available).
    NotRepository,
    /// The floor folder is inside a repository, but it is not the top of it.
    NotTop,
    /// The repository has no commit.
    NoCommit,
    /// HEAD is not on a branch.
    Detached,
}

impl RepoState {
    /// The sentence for the task message, when no worktree is made.
    pub fn why_no_worktree(&self) -> &'static str {
        match self {
            RepoState::Ready { .. } => "",
            RepoState::NotRepository => "the floor folder is not a git repository",
            RepoState::NotTop => {
                "the floor folder is a subfolder of a git repository, not the top of it"
            }
            RepoState::NoCommit => "the git repository has no commit yet",
            RepoState::Detached => "the main checkout is not on a branch (detached HEAD)",
        }
    }
}

/// Look at the floor folder. It never fails: a problem gives a state that has no worktree.
pub fn repo_state(floor: &Path) -> RepoState {
    let Ok(top) = run(floor, &["rev-parse", "--show-toplevel"], Options::READ) else {
        return RepoState::NotRepository;
    };
    if !top.success {
        return RepoState::NotRepository;
    }
    let same = match (
        std::fs::canonicalize(top.stdout_text()),
        std::fs::canonicalize(floor),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if !same {
        return RepoState::NotTop;
    }
    let head = match run(
        floor,
        &["rev-parse", "--verify", "--quiet", "HEAD"],
        Options::READ,
    ) {
        Ok(out) if out.success => out.stdout_text(),
        _ => return RepoState::NoCommit,
    };
    match current_branch(floor) {
        Some(branch) => RepoState::Ready { branch, head },
        None => RepoState::Detached,
    }
}

/// The branch that the checkout in `cwd` has, or `None` for a detached HEAD.
pub fn current_branch(cwd: &Path) -> Option<String> {
    let out = run(cwd, &["symbolic-ref", "-q", "HEAD"], Options::READ).ok()?;
    if !out.success {
        return None;
    }
    out.stdout_text()
        .strip_prefix("refs/heads/")
        .map(str::to_owned)
}

/// The commit of HEAD in `cwd`.
pub fn head_commit(cwd: &Path) -> Result<String, String> {
    Ok(run_ok(
        cwd,
        &["rev-parse", "--verify", "HEAD"],
        Options::READ,
        "git rev-parse",
    )?
    .stdout_text())
}

// ---------------------------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------------------------

/// The slug of a title for a branch name: `[a-z0-9-]` only, no `-` at the start or the end, no
/// two `-` in a row, `SLUG_MAX` characters at most. A title with no usable character gives
/// `task`. A character that is not ASCII (also a letter) is a separator.
pub fn branch_slug(title: &str) -> String {
    let mut slug = String::new();
    for c in title.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let mut slug: String = slug.trim_matches('-').chars().take(SLUG_MAX).collect();
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "task".to_owned()
    } else {
        slug
    }
}

/// The branch of a task: `task/T-12-add-login`.
pub fn branch_name(task_id: i64, title: &str) -> String {
    format!("task/T-{task_id}-{}", branch_slug(title))
}

/// The worktree folder of a task, relative to the floor folder.
pub fn worktree_relative(task_id: i64) -> String {
    format!(".swarm/worktrees/T-{task_id}")
}

/// The folder name of try number `attempt` (1 is `T-12`, 2 is `T-12-2`).
fn worktree_relative_try(task_id: i64, attempt: u32) -> String {
    if attempt <= 1 {
        worktree_relative(task_id)
    } else {
        format!("{}-{attempt}", worktree_relative(task_id))
    }
}

/// Is `relative` a folder that `create_worktree` makes for task `task_id`?
fn is_worktree_relative(task_id: i64, relative: &str) -> bool {
    let base = worktree_relative(task_id);
    match relative.strip_prefix(&base) {
        Some("") => true,
        Some(rest) => {
            rest.strip_prefix('-')
                .and_then(|digits| {
                    digits.parse::<u32>().ok().filter(|n| {
                        (2..=WORKTREE_NAME_TRIES).contains(n) && n.to_string() == digits
                    })
                })
                .is_some()
        }
        None => false,
    }
}

/// Is `branch` a branch name that this app makes for task `task_id`?
fn is_task_branch(branch: &str, task_id: i64) -> bool {
    let prefix = format!("task/T-{task_id}-");
    branch.strip_prefix(&prefix).is_some_and(|slug| {
        !slug.is_empty()
            && slug
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    })
}

/// A commit ID from `rev-parse`: 40 or 64 hex digits. Anything else could be an option.
fn is_commit_id(text: &str) -> bool {
    (text.len() == 40 || text.len() == 64) && text.chars().all(|c| c.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------------------------
// Worktrees
// ---------------------------------------------------------------------------------------------

/// What `create_worktree` made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub branch: String,
    pub path: PathBuf,
    /// The folder, relative to the floor folder. The task keeps this text.
    pub relative: String,
}

/// The most names that `create_worktree` tries (`T-12`, `T-12-2`, ...).
const WORKTREE_NAME_TRIES: u32 = 50;

/// Is `path` a registered worktree of the repository of `floor`? (`git worktree list`)
fn is_registered_worktree(floor: &Path, path: &Path) -> Result<bool, String> {
    let out = run_ok(
        floor,
        &["worktree", "list", "--porcelain"],
        Options::READ,
        "git worktree list",
    )?;
    let wanted = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    Ok(out.stdout_text().lines().any(|line| {
        line.strip_prefix("worktree ").is_some_and(|listed| {
            let listed = Path::new(listed);
            std::fs::canonicalize(listed).unwrap_or_else(|_| listed.to_path_buf()) == wanted
        })
    }))
}

/// Make the branch and the worktree of a task, from the commit `base_commit`. The folder is
/// `.swarm/worktrees/T-12`. When that folder exists and git does not know it as a worktree of this
/// repository (a leftover), the app takes the first free name: `T-12-2`, `T-12-3`, and so on.
pub fn create_worktree(
    floor: &Path,
    task_id: i64,
    title: &str,
    base_commit: &str,
) -> Result<Created, String> {
    if !is_commit_id(base_commit) {
        return Err(format!("{base_commit:?} is not a commit ID"));
    }
    let branch = branch_name(task_id, title);
    let mut chosen = None;
    for attempt in 1..=WORKTREE_NAME_TRIES {
        let relative = worktree_relative_try(task_id, attempt);
        let path = floor.join(&relative);
        // `symlink_metadata`: a dangling link is a folder that exists, too.
        if std::fs::symlink_metadata(&path).is_err() {
            chosen = Some((relative, path));
            break;
        }
        if is_registered_worktree(floor, &path)? {
            return Err(format!(
                "the folder {} is a worktree of this repository already. The app does not use it.",
                path.display()
            ));
        }
    }
    let Some((relative, path)) = chosen else {
        return Err(format!(
            "the folders .swarm/worktrees/T-{task_id} to T-{task_id}-{WORKTREE_NAME_TRIES} all exist. Remove the old ones."
        ));
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot make the folder {}: {e}", parent.display()))?;
    }
    let path_text = path
        .to_str()
        .ok_or_else(|| "the worktree path is not valid UTF-8".to_owned())?;
    run_ok(
        floor,
        &["worktree", "add", "-b", &branch, path_text, base_commit],
        Options::WRITE,
        "git worktree add",
    )?;
    Ok(Created {
        branch,
        path,
        relative,
    })
}

/// Where a command runs: a folder, and for a worktree the git dir that `verified_git_dir`
/// checked. With a git dir, the command names it (`--git-dir`, `--work-tree`), so git does not
/// read the `.git` file of the worktree, which the agent can write.
#[derive(Clone, Copy)]
struct Scope<'a> {
    cwd: &'a Path,
    git_dir: Option<&'a Path>,
}

impl Scope<'_> {
    fn run(&self, args: &[&str], options: Options) -> Result<GitOutput, String> {
        let Some(git_dir) = self.git_dir else {
            return run(self.cwd, args, options);
        };
        let git_dir = git_dir
            .to_str()
            .ok_or_else(|| "the git dir is not valid UTF-8".to_owned())?;
        let work_tree = self
            .cwd
            .to_str()
            .ok_or_else(|| "the worktree path is not valid UTF-8".to_owned())?;
        let mut full = vec!["--git-dir", git_dir, "--work-tree", work_tree];
        full.extend(args);
        run(self.cwd, &full, options)
    }

    fn run_ok(&self, args: &[&str], options: Options, what: &str) -> Result<GitOutput, String> {
        let out = self.run(args, options)?;
        if out.success {
            Ok(out)
        } else {
            Err(format!("{what} failed: {}", out.message()))
        }
    }
}

/// The git dir that git made for the worktree `worktree`: `<common git dir>/worktrees/<name>`.
/// The app finds it in the `gitdir` files of that folder (the files that `git worktree add`
/// wrote, in the main `.git` folder that the agent cannot write).
fn registered_git_dir(common: &Path, worktree: &Path) -> Result<PathBuf, String> {
    let wanted = std::fs::canonicalize(worktree)
        .map_err(|e| format!("cannot read the worktree {}: {e}", worktree.display()))?;
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(common.join("worktrees")) {
        for entry in entries.flatten() {
            let admin = entry.path();
            let Ok(text) = std::fs::read_to_string(admin.join("gitdir")) else {
                continue;
            };
            let link = Path::new(text.trim());
            let same = link.file_name().is_some_and(|name| name == ".git")
                && link
                    .parent()
                    .and_then(|dir| std::fs::canonicalize(dir).ok())
                    .is_some_and(|dir| dir == wanted);
            if same {
                found.push(admin);
            }
        }
    }
    match found.as_slice() {
        [one] => std::fs::canonicalize(one)
            .map_err(|e| format!("cannot read the git dir {}: {e}", one.display())),
        [] => Err(format!(
            "the folder {} is not a registered worktree of this repository",
            worktree.display()
        )),
        _ => Err(format!(
            "git has more than one worktree record for the folder {}",
            worktree.display()
        )),
    }
}

/// Check the git link of a task worktree before the app runs a command there. The agent can
/// write the `.git` file of its worktree (it is inside `.swarm/worktrees/`). A changed file could
/// aim the commands of the app at the main repository, or at a git dir that the agent wrote.
///
/// `git rev-parse --absolute-git-dir` in the worktree must give
/// `<common git dir of the floor>/worktrees/<name>` of the registered worktree. If it does not,
/// this is an `Err` with the reason, and the app runs nothing else there. On success it returns
/// that git dir.
pub fn verified_git_dir(floor: &Path, worktree: &Path) -> Result<PathBuf, String> {
    let common = run_ok(
        floor,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        Options::READ,
        "git rev-parse --git-common-dir",
    )?
    .stdout_text();
    let common = std::fs::canonicalize(&common)
        .map_err(|e| format!("cannot read the git dir {common}: {e}"))?;
    let expected = registered_git_dir(&common, worktree)?;
    let reported = run(
        worktree,
        &["rev-parse", "--absolute-git-dir"],
        Options::READ,
    )?;
    let refused = |what: String| {
        format!(
            "The git link of the worktree {} is not the one that git made for it ({what}). The app does not run git there. The operator must look at the file .git in that folder.",
            worktree.display()
        )
    };
    if !reported.success {
        return Err(refused(format!("git says: {}", reported.message())));
    }
    let text = reported.stdout_text();
    let actual = std::fs::canonicalize(&text).unwrap_or_else(|_| PathBuf::from(&text));
    if actual != expected {
        return Err(refused(format!(
            "git says the git dir is {}, and it must be {}",
            actual.display(),
            expected.display()
        )));
    }
    Ok(expected)
}

/// Who wrote a snapshot, for the commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Author<'a> {
    pub slug: &'a str,
    pub name: &'a str,
    pub role: &'a str,
}

/// What `snapshot` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Snapshot {
    /// The worktree had no change.
    NoChanges,
    /// A commit was made.
    Committed { commit: String },
    /// The worktree has a file that must not go in a commit (see `staged_problems`). Nothing was
    /// committed, and the index is empty again. `paths` are the files, `reason` is the text for
    /// the agent, with the order to add an ignore rule or to remove the files.
    Blocked { paths: Vec<String>, reason: String },
}

/// The biggest file that a snapshot commits, in bytes.
pub const SNAPSHOT_FILE_MAX_BYTES: u64 = 5 * 1024 * 1024;

/// The most files that one snapshot commits.
pub const SNAPSHOT_FILES_MAX: usize = 2000;

/// A new file in one of these folders is build output or a package cache. The app compares the
/// folder names with no regard to letter case.
const SNAPSHOT_BLOCKED_FOLDERS: [&str; 14] = [
    "target",
    "node_modules",
    "dist",
    "build",
    ".venv",
    "__pycache__",
    "out",
    ".next",
    ".nuxt",
    "coverage",
    ".gradle",
    ".tox",
    ".pytest_cache",
    ".mypy_cache",
];

/// A new file with a folder of this name in its path holds keys or credentials.
const SECRET_FOLDERS: [&str; 3] = [".aws", ".ssh", ".gnupg"];

/// The most paths that the text of a refusal lists.
const PROBLEMS_SHOWN: usize = 20;

/// Does a file name look like a secret? The list is not complete. It has:
/// - `.env` and `.env.*` (not `.env.example`), and `*.env` (not `example.env`, `sample.env`);
/// - `*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.keystore`, `*.jks`, `*.kdbx`;
/// - `id_rsa*`, `id_ed25519*`, `id_ecdsa*`, `id_dsa*`;
/// - `credentials`, `credentials.json`, `.npmrc`, `.netrc`, `.pypirc`.
fn is_secret_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let dot_env = name == ".env" || (name.starts_with(".env.") && name != ".env.example");
    let star_env = name.ends_with(".env") && name != "example.env" && name != "sample.env";
    dot_env
        || star_env
        || [".pem", ".key", ".p12", ".pfx", ".keystore", ".jks", ".kdbx"]
            .iter()
            .any(|end| name.ends_with(end))
        || ["id_rsa", "id_ed25519", "id_ecdsa", "id_dsa"]
            .iter()
            .any(|start| name.starts_with(start))
        || [
            "credentials",
            "credentials.json",
            ".npmrc",
            ".netrc",
            ".pypirc",
        ]
        .contains(&name.as_str())
}

/// Is this folder name one of `names`? The check ignores letter case.
fn folder_in(component: &str, names: &[&str]) -> bool {
    names.iter().any(|n| n.eq_ignore_ascii_case(component))
}

/// Why a NEW file at this path (relative, with `/`) must not go in a commit, by its folders and
/// its name. `None` when the path is fine.
fn new_path_problem(path: &str) -> Option<&'static str> {
    let mut parts: Vec<&str> = path.split('/').collect();
    let name = parts.pop().unwrap_or_default();
    if parts
        .iter()
        .any(|c| folder_in(c, &SNAPSHOT_BLOCKED_FOLDERS))
    {
        Some("it is in a folder of build output or packages")
    } else if is_secret_name(name)
        || parts.iter().any(|c| folder_in(c, &SECRET_FOLDERS))
        || folder_in(name, &SECRET_FOLDERS)
    {
        Some("its name looks like a secret")
    } else {
        None
    }
}

/// One staged file that is not a deletion.
struct Staged {
    path: String,
    new_mode: String,
    added: bool,
}

/// The staged files, from `git diff --cached --raw -z`. A deletion is not in the list.
fn staged_files(scope: &Scope<'_>) -> Result<Vec<Staged>, String> {
    let out = scope.run_ok(
        &[
            "diff",
            "--cached",
            "--raw",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--diff-filter=d",
        ],
        Options::READ,
        "git diff --cached",
    )?;
    let mut parts = out.stdout.split(|b| *b == 0).filter(|p| !p.is_empty());
    let mut files = Vec::new();
    while let Some(head) = parts.next() {
        let Some(path) = parts.next() else { break };
        let head = String::from_utf8_lossy(head);
        let fields: Vec<&str> = head.trim_start_matches(':').split_whitespace().collect();
        if let [_old_mode, new_mode, _old, _new, status, ..] = fields.as_slice() {
            files.push(Staged {
                path: String::from_utf8_lossy(path).into_owned(),
                new_mode: (*new_mode).to_owned(),
                added: status.starts_with('A'),
            });
        }
    }
    Ok(files)
}

/// The staged files that a snapshot must not commit, as `(path, reason)`:
/// - a file over `SNAPSHOT_FILE_MAX_BYTES`;
/// - a nested repository (a git link, mode 160000);
/// - a NEW file in a folder of `SNAPSHOT_BLOCKED_FOLDERS`;
/// - a NEW file whose name looks like a secret (`is_secret_name`), or that has a folder of
///   `SECRET_FOLDERS` in its path;
/// - and, as one more entry, a count over `SNAPSHOT_FILES_MAX`.
///
/// A file that the repository tracks already is not blocked for its folder or its name: the
/// operator chose that. The size and the git link rules hold for every staged file.
fn staged_problems(scope: &Scope<'_>, worktree: &Path) -> Result<Vec<(String, String)>, String> {
    let files = staged_files(scope)?;
    let mut problems = Vec::new();
    if files.len() > SNAPSHOT_FILES_MAX {
        problems.push((
            "(all)".to_owned(),
            format!(
                "{} files are staged, and the limit is {SNAPSHOT_FILES_MAX}",
                files.len()
            ),
        ));
    }
    for file in &files {
        let reason = if file.new_mode == "160000" {
            Some("it is a nested git repository".to_owned())
        } else if let Some(size) = std::fs::symlink_metadata(worktree.join(&file.path))
            .ok()
            .map(|m| m.len())
            .filter(|size| *size > SNAPSHOT_FILE_MAX_BYTES)
        {
            Some(format!(
                "it is {} MB, and the limit is {} MB",
                size.div_ceil(1024 * 1024),
                SNAPSHOT_FILE_MAX_BYTES / (1024 * 1024)
            ))
        } else if file.added {
            new_path_problem(&file.path).map(str::to_owned)
        } else {
            None
        };
        if let Some(reason) = reason {
            problems.push((file.path.clone(), reason));
        }
    }
    Ok(problems)
}

/// The text for the agent when a snapshot is refused.
fn blocked_text(problems: &[(String, String)]) -> String {
    let mut text = String::from(
        "The app did not commit the worktree, because these files must not go in a commit: ",
    );
    let shown: Vec<String> = problems
        .iter()
        .take(PROBLEMS_SHOWN)
        .map(|(path, reason)| format!("{path} ({reason})"))
        .collect();
    text.push_str(&shown.join("; "));
    if problems.len() > PROBLEMS_SHOWN {
        text.push_str(&format!("; and {} more", problems.len() - PROBLEMS_SHOWN));
    }
    text.push_str(
        ". Add an ignore rule for them in the file .gitignore, or remove the files. Then set the status to review again. The app unstaged everything and changed no file.",
    );
    text
}

/// Does the repository in `scope` have a user name and an e-mail?
fn has_identity(scope: &Scope<'_>) -> bool {
    ["user.name", "user.email"].iter().all(|key| {
        scope
            .run(&["config", "--get", key], Options::READ)
            .is_ok_and(|out| out.success && !out.stdout_text().is_empty())
    })
}

/// The `-c user.name=... -c user.email=...` arguments, when the repository has no user.
fn identity_args(scope: &Scope<'_>, author: &Author<'_>) -> Vec<String> {
    if has_identity(scope) {
        return Vec::new();
    }
    vec![
        "-c".to_owned(),
        format!("user.name={}", author.name),
        "-c".to_owned(),
        format!("user.email={}@{EMPLOYEE_EMAIL_DOMAIN}", author.slug),
    ]
}

/// Commit everything that changed in the worktree (`git add -A`, then `git commit`). The message
/// is `T-12: <title>`, then a line `By: <Name> (<Role>)`. When the repository has no user, the
/// commit gets the name and the e-mail of the employee. Otherwise it keeps the identity of the
/// operator.
///
/// `floor` is the main checkout. Before the first command, the app checks the git link of the
/// worktree (`verified_git_dir`): `Err` and nothing else runs when it is not the right one. After
/// `git add -A` the app reads the staged files. If one of them must not be committed
/// (`staged_problems`), it empties the index with `git reset -q` (in this worktree only), makes no
/// commit, and returns `Snapshot::Blocked`.
pub fn snapshot(
    floor: &Path,
    worktree: &Path,
    task_id: i64,
    title: &str,
    author: &Author<'_>,
) -> Result<Snapshot, String> {
    let git_dir = verified_git_dir(floor, worktree)?;
    let scope = Scope {
        cwd: worktree,
        git_dir: Some(&git_dir),
    };
    scope.run_ok(&["add", "-A"], Options::WRITE, "git add")?;
    let problems = staged_problems(&scope, worktree)?;
    if !problems.is_empty() {
        // The one `git reset` of the app. It changes the index of this worktree only, and it is
        // never `--hard`: the files stay as they are.
        scope.run_ok(&["reset", "-q"], Options::WRITE, "git reset")?;
        return Ok(Snapshot::Blocked {
            reason: blocked_text(&problems),
            paths: problems.into_iter().map(|(path, _)| path).collect(),
        });
    }
    let diff = scope.run(&["diff", "--cached", "--quiet"], Options::READ)?;
    if diff.success {
        return Ok(Snapshot::NoChanges);
    }
    // Exit code 1 means "there are changes". Another failure is a real error.
    if !diff.stderr.trim().is_empty() {
        return Err(format!("git diff failed: {}", diff.message()));
    }
    let subject = format!("T-{task_id}: {title}");
    let body = format!("By: {} ({})", author.name, author.role);
    let identity = identity_args(&scope, author);
    let mut args: Vec<&str> = identity.iter().map(String::as_str).collect();
    args.extend(["commit", "--no-verify", "-q", "-m", &subject, "-m", &body]);
    scope.run_ok(&args, Options::WRITE, "git commit")?;
    let commit = scope
        .run_ok(
            &["rev-parse", "--verify", "HEAD"],
            Options::READ,
            "git rev-parse",
        )?
        .stdout_text();
    Ok(Snapshot::Committed { commit })
}

/// The paths that the main checkout has changed in tracked files, outside `.swarm/`. Untracked
/// files do not count (spec 4.8). A rename counts with both paths.
pub fn changes_outside_swarm(floor: &Path) -> Result<Vec<String>, String> {
    let out = run_ok(
        floor,
        &["status", "--porcelain=v1", "-z", "--untracked-files=no"],
        Options::READ,
        "git status",
    )?;
    let mut paths = Vec::new();
    let mut parts = out.stdout.split(|b| *b == 0).filter(|p| !p.is_empty());
    while let Some(entry) = parts.next() {
        if entry.len() < 4 {
            continue;
        }
        let (x, y) = (entry[0], entry[1]);
        paths.push(String::from_utf8_lossy(&entry[3..]).into_owned());
        if matches!(x, b'R' | b'C') || matches!(y, b'R' | b'C') {
            // The next part is the old path.
            if let Some(old) = parts.next() {
                paths.push(String::from_utf8_lossy(old).into_owned());
            }
        }
    }
    paths.retain(|p| !(p == ".swarm" || p.starts_with(".swarm/")));
    Ok(paths)
}

/// The result of a merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeResult {
    /// The branch is merged. `new_commit` is false when the branch had nothing new.
    Merged { commit: String, new_commit: bool },
    /// The merge had a conflict. The app stopped it. The checkout is as before.
    Conflict { files: Vec<String> },
    /// The merge did not run, or it failed with no conflict. The text is from git.
    Failed { reason: String },
}

/// The git operation that the operator started in the main checkout and has not ended, if there is
/// one: a merge (`MERGE_HEAD`), a cherry-pick (`CHERRY_PICK_HEAD`), a revert (`REVERT_HEAD`) or a
/// rebase (`rebase-merge`, `rebase-apply`). The app does not merge over it, and it never aborts it.
pub fn operation_in_progress(floor: &Path) -> Result<Option<&'static str>, String> {
    let git_dir = run_ok(
        floor,
        &["rev-parse", "--absolute-git-dir"],
        Options::READ,
        "git rev-parse --absolute-git-dir",
    )?
    .stdout_text();
    let git_dir = Path::new(&git_dir);
    for (name, what) in [
        ("MERGE_HEAD", "a merge"),
        ("CHERRY_PICK_HEAD", "a cherry-pick"),
        ("REVERT_HEAD", "a revert"),
        ("rebase-merge", "a rebase"),
        ("rebase-apply", "a rebase"),
    ] {
        if std::fs::symlink_metadata(git_dir.join(name)).is_ok() {
            return Ok(Some(what));
        }
    }
    Ok(None)
}

/// The text for the operator when the main checkout is in the middle of `what`.
pub fn operation_in_progress_text(what: &str) -> String {
    format!(
        "The main checkout is in the middle of {what} that the operator started. The app does not merge now, and it does not stop that operation. End it (commit it or abort it), then merge again."
    )
}

/// `git merge --no-ff` of the task branch into the branch of the main checkout. On a conflict, the
/// merge is stopped with `git merge --abort`. That abort is only for a merge that this call
/// started: when the main checkout is in the middle of an operation of the operator (see
/// `operation_in_progress`), nothing runs, and the result is `Failed`.
pub fn merge_branch(
    floor: &Path,
    task_id: i64,
    title: &str,
    branch: &str,
    author: &Author<'_>,
) -> Result<MergeResult, String> {
    if !is_task_branch(branch, task_id) {
        return Err(format!("{branch:?} is not the branch of task T-{task_id}"));
    }
    if let Some(what) = operation_in_progress(floor)? {
        return Ok(MergeResult::Failed {
            reason: operation_in_progress_text(what),
        });
    }
    let before = head_commit(floor)?;
    let reference = format!("refs/heads/{branch}");
    let message = format!("Merge T-{task_id}: {title} ({branch})");
    let scope = Scope {
        cwd: floor,
        git_dir: None,
    };
    let identity = identity_args(&scope, author);
    let mut args: Vec<&str> = identity.iter().map(String::as_str).collect();
    args.extend(["merge", "--no-ff", "--no-edit", "-m", &message, &reference]);
    let out = run(floor, &args, Options::WRITE)?;
    if out.success {
        let commit = head_commit(floor)?;
        return Ok(MergeResult::Merged {
            new_commit: commit != before,
            commit,
        });
    }
    // No operation was in progress before the merge, so a `MERGE_HEAD` now is from this merge.
    let merging = run(
        floor,
        &["rev-parse", "-q", "--verify", "MERGE_HEAD"],
        Options::READ,
    )
    .is_ok_and(|o| o.success);
    if !merging {
        return Ok(MergeResult::Failed {
            reason: out.message(),
        });
    }
    let files = run(
        floor,
        &["diff", "--name-only", "--diff-filter=U", "-z"],
        Options::READ,
    )
    .map(|o| {
        o.stdout
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    let aborted = run(floor, &["merge", "--abort"], Options::WRITE)?;
    if !aborted.success {
        return Ok(MergeResult::Failed {
            reason: format!(
                "the merge has a conflict, and git merge --abort failed: {}. The main checkout needs the operator.",
                aborted.message()
            ),
        });
    }
    Ok(MergeResult::Conflict { files })
}

/// Remove the worktree of a task after a good merge. No `--force`: git refuses when the worktree
/// has a change that is not committed.
pub fn remove_worktree(floor: &Path, task_id: i64, relative: &str) -> Result<(), String> {
    let path = checked_worktree_path(floor, task_id, relative)?;
    let path_text = path
        .to_str()
        .ok_or_else(|| "the worktree path is not valid UTF-8".to_owned())?;
    run_ok(
        floor,
        &["worktree", "remove", path_text],
        Options::WRITE,
        "git worktree remove",
    )?;
    Ok(())
}

/// Delete the merged branch of a task (`git branch -d`: git refuses when it is not merged).
pub fn delete_merged_branch(floor: &Path, task_id: i64, branch: &str) -> Result<(), String> {
    if !is_task_branch(branch, task_id) {
        return Err(format!("{branch:?} is not the branch of task T-{task_id}"));
    }
    run_ok(
        floor,
        &["branch", "-d", branch],
        Options::WRITE,
        "git branch -d",
    )?;
    Ok(())
}

/// Does the branch of a task exist?
pub fn branch_exists(floor: &Path, task_id: i64, branch: &str) -> Result<bool, String> {
    if !is_task_branch(branch, task_id) {
        return Err(format!("{branch:?} is not the branch of task T-{task_id}"));
    }
    let reference = format!("refs/heads/{branch}");
    Ok(run(
        floor,
        &["rev-parse", "--verify", "--quiet", &reference],
        Options::READ,
    )?
    .success)
}

/// The operator discards a task: remove its worktree (`--force`) and its branch (`-D`). The
/// path and the branch name are checked first. A worktree that is gone already is not an error.
pub fn discard_worktree(
    floor: &Path,
    task_id: i64,
    relative: Option<&str>,
    branch: Option<&str>,
) -> Result<(), String> {
    // Check both names before anything is removed, so a bad branch name does not leave a half
    // discard (the worktree gone, the branch kept).
    let path = match relative {
        Some(relative) => Some(checked_worktree_path(floor, task_id, relative)?),
        None => None,
    };
    if let Some(branch) = branch
        && !is_task_branch(branch, task_id)
    {
        return Err(format!("{branch:?} is not the branch of task T-{task_id}"));
    }
    if let Some(path) = path {
        if path.exists() {
            let path_text = path
                .to_str()
                .ok_or_else(|| "the worktree path is not valid UTF-8".to_owned())?;
            run_ok(
                floor,
                &["worktree", "remove", "--force", path_text],
                Options::WRITE,
                "git worktree remove",
            )?;
        } else {
            run_ok(
                floor,
                &["worktree", "prune"],
                Options::WRITE,
                "git worktree prune",
            )?;
        }
    }
    if let Some(branch) = branch {
        let reference = format!("refs/heads/{branch}");
        let exists = run(
            floor,
            &["rev-parse", "--verify", "--quiet", &reference],
            Options::READ,
        )?
        .success;
        if exists {
            run_ok(
                floor,
                &["branch", "-D", branch],
                Options::WRITE,
                "git branch -D",
            )?;
        }
    }
    Ok(())
}

/// The absolute worktree path of a task, after a check that the stored text is the one that the
/// app makes. This keeps a changed database row from pointing a `--force` at another folder.
fn checked_worktree_path(floor: &Path, task_id: i64, relative: &str) -> Result<PathBuf, String> {
    if !is_worktree_relative(task_id, relative) {
        return Err(format!(
            "the stored worktree {relative:?} is not {:?}",
            worktree_relative(task_id)
        ));
    }
    Ok(floor.join(relative))
}

/// The absolute worktree path of a task. See `checked_worktree_path`.
pub fn worktree_path(floor: &Path, task_id: i64, relative: &str) -> Result<PathBuf, String> {
    checked_worktree_path(floor, task_id, relative)
}

// ---------------------------------------------------------------------------------------------
// Diff
// ---------------------------------------------------------------------------------------------

/// The diff of a task branch against the commit where it started: the `--stat` text and the patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    pub stat: String,
    pub patch: String,
}

/// `git diff --stat <base>...<branch>` and `git diff <base>...<branch>`. Only commits count.
pub fn task_diff(
    floor: &Path,
    task_id: i64,
    base_commit: &str,
    branch: &str,
) -> Result<Diff, String> {
    if !is_commit_id(base_commit) {
        return Err(format!("{base_commit:?} is not a commit ID"));
    }
    if !is_task_branch(branch, task_id) {
        return Err(format!("{branch:?} is not the branch of task T-{task_id}"));
    }
    let range = format!("{base_commit}...refs/heads/{branch}");
    let common = ["--no-color", "--no-ext-diff", "--no-textconv"];
    let mut stat_args = vec!["diff", "--stat"];
    stat_args.extend(common);
    stat_args.push(&range);
    let stat = run_ok(floor, &stat_args, Options::WRITE_FREE, "git diff --stat")?;
    let mut patch_args = vec!["diff"];
    patch_args.extend(common);
    patch_args.push(&range);
    let patch = run_ok(floor, &patch_args, Options::WRITE_FREE, "git diff")?;
    Ok(Diff {
        stat: String::from_utf8_lossy(&stat.stdout).into_owned(),
        patch: String::from_utf8_lossy(&patch.stdout).into_owned(),
    })
}

impl Options {
    /// A command that does not write, with the long time limit.
    const WRITE_FREE: Options = Options {
        no_hooks: false,
        timeout: LONG_TIMEOUT,
    };
}

/// Cut a text at `max` bytes on a character boundary. Returns the text and whether it was cut.
pub fn cut_at(text: &str, max: usize) -> (&str, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_uses_only_safe_characters() {
        assert_eq!(branch_slug("Add login"), "add-login");
        assert_eq!(
            branch_slug("--upload-pack=x; rm -rf"),
            "upload-pack-x-rm-rf"
        );
        assert_eq!(branch_slug("日本語 Ünï"), "n");
        assert_eq!(branch_slug("日本語"), "task");
        assert_eq!(branch_slug("  ---  "), "task");
        assert_eq!(branch_slug("a/b..c~d^e:f?g*h[i\\j"), "a-b-c-d-e-f-g-h-i-j");
        let long = branch_slug(&"word ".repeat(30));
        assert!(long.len() <= SLUG_MAX && !long.ends_with('-'), "{long}");
        for title in ["x", "-rf", "$(touch x)", "a\nb", "résumé", ".lock"] {
            let slug = branch_slug(title);
            assert!(!slug.starts_with('-') && !slug.ends_with('-'), "{slug}");
            assert!(
                slug.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{slug}"
            );
        }
    }

    #[test]
    fn branch_names_and_ids_are_checked() {
        assert_eq!(branch_name(12, "Add login"), "task/T-12-add-login");
        assert!(is_task_branch("task/T-12-add-login", 12));
        assert!(!is_task_branch("task/T-12-add-login", 1));
        assert!(!is_task_branch("main", 12));
        assert!(!is_task_branch("task/T-12-", 12));
        assert!(!is_task_branch("task/T-12-a b", 12));
        assert!(is_commit_id(&"a".repeat(40)));
        assert!(!is_commit_id("--help"));
        assert!(!is_commit_id(&"g".repeat(40)));
    }

    // ---- helpers for the tests that run a real git, in temporary folders only ----

    use std::os::unix::fs::PermissionsExt;

    /// Run `git` with a clean environment in a temporary folder, and return stdout.
    fn plain_git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// A repository on `main` with one commit and a user.
    fn temp_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        plain_git(dir.path(), &["init", "-q", "-b", "main"]);
        plain_git(dir.path(), &["config", "user.name", "Operator"]);
        plain_git(
            dir.path(),
            &["config", "user.email", "operator@example.test"],
        );
        plain_git(dir.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.path().join("notes.txt"), "one\n").unwrap();
        plain_git(dir.path(), &["add", "notes.txt"]);
        plain_git(dir.path(), &["commit", "-q", "-m", "first"]);
        dir
    }

    /// A script that writes `marker` when it runs and then copies its first argument to stdout.
    fn marker_script(dir: &Path, name: &str, marker: &Path) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\ntouch '{}'\ncat \"$1\" 2>/dev/null\nexit 0\n",
                marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn os(pairs: &[(&str, String)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    /// The parent environment that a test gives: the real `PATH` (git must start) and the
    /// variables that `extra` adds.
    fn parent_with(extra: &[(&str, String)]) -> Vec<(OsString, OsString)> {
        let mut parent: Vec<(OsString, OsString)> = std::env::vars_os()
            .filter(|(name, _)| !is_blocked_variable(name))
            .collect();
        parent.extend(os(extra));
        parent
    }

    /// The name of the test below, and the variable that marks the child process.
    const CHILD_TEST: &str = "gitwork::tests::the_public_entry_cleans_the_real_process_environment";
    const CHILD_MARK: &str = "TOWER_GITWORK_CHILD_REPO";

    /// `run` is the entry that every git call of the app uses. It reads the real environment of
    /// the process. The test above gives the environment as an argument, so it stays green when
    /// `run_with_environment` forgets `clean_environment`. This test starts the test binary again
    /// as a child process, with `GIT_DIR` set to a wrong folder in the environment of the child.
    /// The child calls `run`. The test process itself never changes its environment, so the
    /// parallel tests are safe.
    #[test]
    fn the_public_entry_cleans_the_real_process_environment() {
        if let Some(repo) = std::env::var_os(CHILD_MARK) {
            // This is the child. `GIT_DIR` points at a folder that is not a repository.
            assert!(
                std::env::var_os("GIT_DIR").is_some(),
                "the parent did not set GIT_DIR"
            );
            let out = run(
                Path::new(&repo),
                &["rev-parse", "--show-toplevel"],
                Options::READ,
            )
            .unwrap();
            assert!(out.success, "git failed in the child: {}", out.message());
            assert_eq!(
                std::fs::canonicalize(out.stdout_text()).unwrap(),
                std::fs::canonicalize(&repo).unwrap()
            );
            return;
        }
        let repo = temp_repo();
        let wrong = tempfile::tempdir().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([CHILD_TEST, "--exact", "--test-threads=1", "--nocapture"])
            .env(CHILD_MARK, repo.path())
            .env("GIT_DIR", wrong.path())
            .env("GIT_WORK_TREE", wrong.path())
            .output()
            .expect("start the child test process");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "the child failed: {text}");
        // The child must really have run (a filter that matches nothing also exits with 0).
        assert!(text.contains("1 passed"), "the child did not run: {text}");
    }

    /// Finding M8: no test showed that the cleaning of the environment matters. The parent
    /// environment is an argument here, so the test does not change the environment of the test
    /// process (the other tests run in parallel).
    #[test]
    fn variables_of_the_parent_that_start_with_git_do_not_reach_git() {
        let a = temp_repo();
        let b = temp_repo();
        let a_top = std::fs::canonicalize(a.path()).unwrap();
        let hostile = parent_with(&[
            ("GIT_DIR", b.path().join(".git").display().to_string()),
            ("GIT_WORK_TREE", b.path().display().to_string()),
            ("GIT_CONFIG_COUNT", "1".to_owned()),
            ("GIT_CONFIG_KEY_0", "alias.zz".to_owned()),
            ("GIT_CONFIG_VALUE_0", "status".to_owned()),
            ("GIT_AUTHOR_NAME", "Evil".to_owned()),
            ("GIT_COMMITTER_NAME", "Evil".to_owned()),
            ("PAGER", "touch /nonexistent-folder/x".to_owned()),
        ]);
        // 1. GIT_DIR and GIT_WORK_TREE would send the command to the other repository.
        let top = run_with_environment(
            a.path(),
            &["rev-parse", "--show-toplevel"],
            Options::READ,
            hostile.clone(),
        )
        .unwrap();
        assert!(top.success, "{}", top.message());
        assert_eq!(std::fs::canonicalize(top.stdout_text()).unwrap(), a_top);
        // 2. GIT_CONFIG_COUNT and the keys would add a config entry.
        let alias = run_with_environment(
            a.path(),
            &["config", "--get", "alias.zz"],
            Options::READ,
            hostile.clone(),
        )
        .unwrap();
        assert!(
            !alias.success && alias.stdout_text().is_empty(),
            "{alias:?}"
        );
        // 3. GIT_AUTHOR_NAME and GIT_COMMITTER_NAME would change the commit.
        let commit = run_with_environment(
            a.path(),
            &["commit", "--allow-empty", "-q", "-m", "x"],
            Options::WRITE,
            hostile,
        )
        .unwrap();
        assert!(commit.success, "{}", commit.message());
        assert_eq!(
            plain_git(a.path(), &["log", "-1", "--format=%an/%cn"]),
            "Operator/Operator"
        );
        // The other variables pass: `PATH` is in the parent, and git started.
        let clean = parent_with(&[("GIT_TERMINAL_PROMPT", "1".to_owned())]);
        let mut command = Command::new("git");
        clean_environment(&mut command, clean);
        let prompt = command
            .get_envs()
            .find(|(name, _)| *name == OsStr::new("GIT_TERMINAL_PROMPT"))
            .and_then(|(_, value)| value);
        assert_eq!(prompt, Some(OsStr::new("0")), "the app sets its own value");
    }

    #[test]
    fn blocked_variable_names() {
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_SYSTEM",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_KEY_0",
            "GIT_CONFIG_VALUE_7",
            "GIT_AUTHOR_NAME",
            "GIT_COMMITTER_EMAIL",
            "GIT_REPLACE_REF_BASE",
            "GIT_ATTR_SOURCE",
            "GIT_CEILING_DIRECTORIES",
            "GIT_EXTERNAL_DIFF",
            "GIT_SSH",
            "GIT_SSH_COMMAND",
            "GIT_ASKPASS",
            "GIT_EDITOR",
            "GIT_SEQUENCE_EDITOR",
            "GIT_PAGER",
            "PAGER",
        ] {
            assert!(is_blocked_variable(OsStr::new(name)), "{name}");
        }
        for name in [
            "PATH",
            "HOME",
            "LANG",
            "GITHUB_TOKEN",
            "MY_GIT_DIR",
            "XDG_CONFIG_HOME",
        ] {
            assert!(!is_blocked_variable(OsStr::new(name)), "{name}");
        }
    }

    /// Finding M8: `core.fsmonitor` of a repository config runs a program for `git status`. Every
    /// command of the app has `-c core.fsmonitor=false`. The first part proves that the probe
    /// works (a plain git runs the program), so the test cannot pass for a wrong reason.
    #[test]
    fn the_fsmonitor_program_of_a_repository_does_not_run() {
        let repo = temp_repo();
        let scripts = tempfile::tempdir().unwrap();
        let marker = scripts.path().join("fsmonitor-ran");
        let script = marker_script(scripts.path(), "fsmonitor.sh", &marker);
        plain_git(
            repo.path(),
            &["config", "core.fsmonitor", script.to_str().unwrap()],
        );
        std::fs::write(repo.path().join("notes.txt"), "one\ntwo\n").unwrap();
        plain_git(repo.path(), &["status", "--porcelain"]);
        assert!(
            marker.exists(),
            "the probe does not run the program: the test is useless"
        );
        std::fs::remove_file(&marker).unwrap();
        let changed = changes_outside_swarm(repo.path()).unwrap();
        assert_eq!(changed, ["notes.txt"]);
        assert!(!marker.exists(), "the core.fsmonitor program ran");
    }

    /// Finding M8: `task_diff` runs with `--no-ext-diff --no-textconv`. A repository config with
    /// `diff.external` or a textconv would run a program to make the diff.
    #[test]
    fn a_diff_does_not_run_the_external_diff_or_the_textconv_of_the_repository() {
        let repo = temp_repo();
        let scripts = tempfile::tempdir().unwrap();
        let ext_marker = scripts.path().join("external-ran");
        let conv_marker = scripts.path().join("textconv-ran");
        let external = marker_script(scripts.path(), "external.sh", &ext_marker);
        let textconv = marker_script(scripts.path(), "textconv.sh", &conv_marker);
        plain_git(
            repo.path(),
            &["config", "diff.external", external.to_str().unwrap()],
        );
        plain_git(
            repo.path(),
            &["config", "diff.probe.textconv", textconv.to_str().unwrap()],
        );
        std::fs::write(repo.path().join(".gitattributes"), "*.txt diff=probe\n").unwrap();
        plain_git(repo.path(), &["add", ".gitattributes"]);
        plain_git(repo.path(), &["commit", "-q", "-m", "attributes"]);
        let base = plain_git(repo.path(), &["rev-parse", "HEAD"]);
        plain_git(repo.path(), &["checkout", "-q", "-b", "task/T-1-probe"]);
        std::fs::write(repo.path().join("notes.txt"), "one\ntwo\n").unwrap();
        plain_git(repo.path(), &["commit", "-qam", "change"]);
        // The probes work: a plain `git diff` runs the external diff, and, without it, the
        // textconv.
        let range = format!("{base}...task/T-1-probe");
        plain_git(repo.path(), &["diff", &range]);
        assert!(ext_marker.exists(), "the external diff probe does not run");
        std::fs::remove_file(&ext_marker).unwrap();
        plain_git(repo.path(), &["diff", "--no-ext-diff", &range]);
        assert!(conv_marker.exists(), "the textconv probe does not run");
        std::fs::remove_file(&conv_marker).unwrap();
        let diff = task_diff(repo.path(), 1, &base, "task/T-1-probe").unwrap();
        assert!(diff.patch.contains("+two"), "{}", diff.patch);
        assert!(!ext_marker.exists(), "diff.external ran");
        assert!(!conv_marker.exists(), "the textconv program ran");
    }

    #[test]
    fn secret_names_and_worktree_folder_names() {
        for name in [
            ".env",
            ".env.local",
            ".ENV.production",
            "server.pem",
            "tls.key",
            "id_rsa",
            "id_rsa.pub",
            "id_ed25519",
            "cert.p12",
            "cert.pfx",
            "prod.env",
            "Staging.ENV",
            "credentials.json",
            "credentials",
            ".npmrc",
            ".netrc",
            ".pypirc",
            "id_ecdsa",
            "id_ecdsa.pub",
            "id_dsa",
            "release.keystore",
            "release.jks",
            "vault.kdbx",
        ] {
            assert!(is_secret_name(name), "{name}");
        }
        for name in [
            ".env.example",
            "example.env",
            "sample.env",
            "credentials.rs",
            "my-credentials.json",
            "environment.rs",
            "keyboard.rs",
            "monkey.txt",
            "main.rs",
        ] {
            assert!(!is_secret_name(name), "{name}");
        }
        assert!(is_worktree_relative(12, ".swarm/worktrees/T-12"));
        assert!(is_worktree_relative(12, ".swarm/worktrees/T-12-2"));
        assert!(is_worktree_relative(12, ".swarm/worktrees/T-12-50"));
        for bad in [
            ".swarm/worktrees/T-12-1",
            ".swarm/worktrees/T-12-02",
            ".swarm/worktrees/T-12-51",
            ".swarm/worktrees/T-12-x",
            ".swarm/worktrees/T-123",
            ".swarm/worktrees/T-1",
            ".swarm/worktrees/T-12/",
            "precious",
        ] {
            assert!(!is_worktree_relative(12, bad), "{bad}");
        }
    }

    #[test]
    fn a_new_path_is_refused_by_its_folders_and_its_name() {
        for path in [
            "target/debug/app",
            "a/node_modules/x.js",
            "out/index.js",
            ".next/cache/x",
            ".nuxt/x",
            "coverage/lcov.info",
            ".gradle/x",
            ".tox/x",
            ".pytest_cache/x",
            ".mypy_cache/x",
            "Build/x.o",
            "DIST/x.js",
            "Target/x",
            "config/prod.env",
            ".aws/config",
            "home/.ssh/known_hosts",
            "x/.GnuPG/pubring.kbx",
            "x/.ssh",
        ] {
            assert!(new_path_problem(path).is_some(), "{path}");
        }
        for path in [
            "src/lib.rs",
            "src/output/x.rs",
            "docs/example.env",
            "docs/sample.env",
            ".env.example",
            "src/awsome/x",
            "ssh.md",
        ] {
            assert_eq!(new_path_problem(path), None, "{path}");
        }
    }

    #[test]
    fn cut_keeps_a_character_boundary() {
        assert_eq!(cut_at("abc", 5), ("abc", false));
        assert_eq!(cut_at("abcdef", 3), ("abc", true));
        assert_eq!(cut_at("aéb", 2), ("a", true));
    }
}
