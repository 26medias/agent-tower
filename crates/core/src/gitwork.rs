//! The git actions of the app for task worktrees (spec 4.8, changed in M3 on 2026-11-21).
//!
//! # Safety rules
//!
//! - Each command runs with an explicit working folder, with a fixed list of arguments (never a
//!   shell string), with a time limit, and with a clean environment for the variables that can
//!   redirect git (`GIT_DIR` and others).
//! - A command that can run a hook of the repository (`worktree add`, `commit`, `merge`) runs
//!   with `-c core.hooksPath=/dev/null`, so no hook of the operator's repository runs during an
//!   action of the app.
//! - An argument that comes from a task (the title) is never an argument that can start with `-`.
//!   A branch name is built from `[a-z0-9-]` only (`branch_slug`). The title goes into a commit
//!   message after the flag `-m`, and the message starts with the task ID.
//! - The app never uses `--force`, `reset`, `checkout` in the main checkout, or `push`. The one
//!   exception is `discard_worktree`: it removes the worktree of one task with `worktree remove
//!   --force` and the branch of that task with `branch -D`. It checks the path and the branch
//!   name first.
//! - Diffs run with `--no-ext-diff` and `--no-textconv`, so no program that the repository
//!   configures runs to make a diff.
//!
//! The functions here do no locking. The caller (`app/merge.rs`) holds the git lock of the floor.

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

/// The environment variables of git that can send a command to another repository or file.
const GIT_VARS_REMOVED: [&str; 12] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_EXTERNAL_DIFF",
];

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
    let mut command = Command::new("git");
    command.current_dir(cwd);
    if options.no_hooks {
        command.args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
        ]);
    }
    command.args(args);
    for name in GIT_VARS_REMOVED {
        command.env_remove(name);
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .env("GIT_EDITOR", "true")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
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
                    args.first().copied().unwrap_or(""),
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
}

/// Make the branch and the worktree of a task, from the commit `base_commit`.
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
    let path = floor.join(worktree_relative(task_id));
    if path.exists() {
        return Err(format!(
            "the folder {} exists already. The app does not use it.",
            path.display()
        ));
    }
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
    Ok(Created { branch, path })
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
}

/// Does the repository in `cwd` have a user name and an e-mail?
fn has_identity(cwd: &Path) -> bool {
    ["user.name", "user.email"].iter().all(|key| {
        run(cwd, &["config", "--get", key], Options::READ)
            .is_ok_and(|out| out.success && !out.stdout_text().is_empty())
    })
}

/// The `-c user.name=... -c user.email=...` arguments, when the repository has no user.
fn identity_args(cwd: &Path, author: &Author<'_>) -> Vec<String> {
    if has_identity(cwd) {
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
pub fn snapshot(
    worktree: &Path,
    task_id: i64,
    title: &str,
    author: &Author<'_>,
) -> Result<Snapshot, String> {
    run_ok(worktree, &["add", "-A"], Options::WRITE, "git add")?;
    let diff = run(worktree, &["diff", "--cached", "--quiet"], Options::READ)?;
    if diff.success {
        return Ok(Snapshot::NoChanges);
    }
    // Exit code 1 means "there are changes". Another failure is a real error.
    if !diff.stderr.trim().is_empty() {
        return Err(format!("git diff failed: {}", diff.message()));
    }
    let subject = format!("T-{task_id}: {title}");
    let body = format!("By: {} ({})", author.name, author.role);
    let identity = identity_args(worktree, author);
    let mut args: Vec<&str> = identity.iter().map(String::as_str).collect();
    args.extend(["commit", "--no-verify", "-q", "-m", &subject, "-m", &body]);
    run_ok(worktree, &args, Options::WRITE, "git commit")?;
    Ok(Snapshot::Committed {
        commit: head_commit(worktree)?,
    })
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

/// `git merge --no-ff` of the task branch into the branch of the main checkout. On a conflict, the
/// merge is stopped with `git merge --abort`.
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
    let before = head_commit(floor)?;
    let reference = format!("refs/heads/{branch}");
    let message = format!("Merge T-{task_id}: {title} ({branch})");
    let identity = identity_args(floor, author);
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
    if let Some(relative) = relative {
        let path = checked_worktree_path(floor, task_id, relative)?;
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
        if !is_task_branch(branch, task_id) {
            return Err(format!("{branch:?} is not the branch of task T-{task_id}"));
        }
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
    if relative != worktree_relative(task_id) {
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

    #[test]
    fn cut_keeps_a_character_boundary() {
        assert_eq!(cut_at("abc", 5), ("abc", false));
        assert_eq!(cut_at("abcdef", 3), ("abc", true));
        assert_eq!(cut_at("aéb", 2), ("a", true));
    }
}
