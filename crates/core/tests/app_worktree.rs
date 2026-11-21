//! Worktrees and merges (milestone M3): the worktree and the branch of a task, the commit when a
//! task goes to review, `task_merge`, `task_diff` and `task_discard`. Each test uses a temporary
//! repository (`init_repo`), never a real one. The processes are the real `fake-cli`.

mod app_support;

use app_support::{Rig, ToolResult, git, git_try, init_repo, wait_for};
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tower_core::store::{TaskRecord, TaskStatus};

/// A rig with a floor that is a git repository (`main`, one commit), the employees `eng` and
/// `eng2` (policy `edit-in-worktree`, manager `pm`), and live idle sessions for all four.
fn git_rig() -> Rig {
    let rig = Rig::new();
    rig.create_floor();
    init_repo(&rig.cwd);
    rig.add_editor("eng", Some("pm"));
    rig.add_editor("eng2", Some("pm"));
    for slug in ["ceo", "pm", "eng", "eng2"] {
        rig.start(slug);
        rig.live_idle(slug);
    }
    rig
}

fn ok(result: &ToolResult) -> &str {
    assert!(!result.is_error, "{result:?}");
    &result.text
}

fn refused(result: &ToolResult) -> &str {
    assert!(result.is_error, "{result:?}");
    &result.text
}

fn task(rig: &Rig, id: i64) -> TaskRecord {
    rig.db().get_task(id).unwrap().expect("the task exists")
}

/// Make a task as `pm` and return its number.
fn make_task(rig: &Rig, title: &str) -> i64 {
    let result = rig.tool(
        "pm",
        "task_create",
        json!({"title": title, "description": "Do it."}),
    );
    let text = ok(&result);
    text.split("T-")
        .nth(1)
        .unwrap()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap()
}

/// Give a task to `slug` as `pm`.
fn assign(rig: &Rig, id: i64, slug: &str) -> String {
    let result = rig.tool(
        "pm",
        "task_assign",
        json!({"task": format!("T-{id}"), "employee": slug}),
    );
    ok(&result).to_owned()
}

/// The employee sets the status `review`: it writes a report file and calls `task_update`.
fn review_as(rig: &Rig, id: i64, slug: &str) -> ToolResult {
    let report = rig.cwd.join(format!(".swarm/reports/T-{id}.md"));
    fs::write(&report, "Done.\n").unwrap();
    rig.tool(
        slug,
        "task_update",
        json!({"task": format!("T-{id}"), "status": "review", "report": report.to_str().unwrap()}),
    )
}

/// Write a file inside the worktree of a task (the work of an employee).
fn work(rig: &Rig, id: i64, file: &str, text: &str) {
    let path = rig.worktree(id).join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Make a task, give it to `slug`, write one file in its worktree, and send it to review.
fn task_in_review(rig: &Rig, title: &str, slug: &str, file: &str, text: &str) -> i64 {
    let id = make_task(rig, title);
    assign(rig, id, slug);
    work(rig, id, file, text);
    ok(&review_as(rig, id, slug));
    id
}

fn merge_as(rig: &Rig, who: &str, id: i64) -> ToolResult {
    rig.tool(who, "task_merge", json!({"task": format!("T-{id}")}))
}

fn branch_of(rig: &Rig, id: i64) -> String {
    task(rig, id).branch.expect("a branch")
}

fn branches(rig: &Rig) -> String {
    rig.git(&["branch", "--list", "task/*", "--format=%(refname:short)"])
}

fn paste_of(rig: &Rig, slug: &str) -> String {
    let id = rig
        .calls()
        .into_iter()
        .rev()
        .find(|c| c.slug == slug)
        .unwrap()
        .app_session_id;
    rig.paste_text(&id).unwrap_or_default()
}

/// A hook that writes a marker file when it runs. `fail` makes the hook end with an error.
fn install_hook(repo: &Path, name: &str, marker: &str, fail: bool) {
    let hooks = repo.join(".git/hooks");
    fs::create_dir_all(&hooks).unwrap();
    let path = hooks.join(name);
    let end = if fail { "exit 1" } else { "exit 0" };
    fs::write(
        &path,
        format!("#!/bin/sh\necho ran >> \"{marker}\"\n{end}\n"),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

// ---------------------------------------------------------------------------------------------
// The worktree and the branch at task_assign
// ---------------------------------------------------------------------------------------------

#[test]
fn an_assignment_makes_a_worktree_and_a_branch_and_tells_the_assignee_the_rules() {
    let rig = git_rig();
    let head = rig.git(&["rev-parse", "HEAD"]);
    let id = make_task(&rig, "Add login");
    let answer = assign(&rig, id, "eng");
    assert!(answer.contains("Task T-1 is given to ENG"), "{answer}");

    let made = task(&rig, id);
    assert_eq!(made.branch.as_deref(), Some("task/T-1-add-login"));
    assert_eq!(made.base_branch.as_deref(), Some("main"));
    assert_eq!(made.base_commit.as_deref(), Some(head.as_str()));
    assert_eq!(made.worktree.as_deref(), Some(".swarm/worktrees/T-1"));
    // The folder is a real worktree of the repository, with the files of the base commit.
    assert!(rig.worktree(1).join("notes.txt").is_file());
    assert!(
        rig.git(&["worktree", "list"])
            .contains(".swarm/worktrees/T-1")
    );
    assert_eq!(branches(&rig), "task/T-1-add-login");
    assert_eq!(rig.git(&["rev-parse", "task/T-1-add-login"]), head);
    // The main checkout did not change.
    assert_eq!(
        rig.git(&["status", "--porcelain", "--untracked-files=no"]),
        ""
    );
    assert_eq!(rig.git(&["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    // Decision 1: the session of the employee stays in the main checkout.
    let call = rig
        .calls()
        .into_iter()
        .rev()
        .find(|c| c.slug == "eng")
        .unwrap();
    assert_eq!(call.cwd, rig.cwd);

    let kinds: Vec<(String, Option<String>)> = rig
        .db()
        .task_events(id)
        .unwrap()
        .into_iter()
        .map(|e| (e.kind, e.value))
        .collect();
    assert_eq!(
        kinds,
        [
            ("created".to_owned(), None),
            ("assigned".to_owned(), Some("eng".to_owned())),
            ("worktree".to_owned(), Some("task/T-1-add-login".to_owned())),
        ]
    );

    // The message carries the rules, with absolute paths.
    wait_for("the paste", || !paste_of(&rig, "eng").is_empty());
    let pasted = paste_of(&rig, "eng");
    let worktree = rig.worktree(1);
    assert!(
        pasted.contains("Orders of the app for this task:"),
        "{pasted}"
    );
    assert!(
        pasted.contains(&format!("Worktree: {}", worktree.display())),
        "{pasted}"
    );
    assert!(pasted.contains("Branch: task/T-1-add-login"), "{pasted}");
    assert!(
        pasted.contains(
            "Make ALL your changes inside this folder. Never change a file in the main checkout."
        ),
        "{pasted}"
    );
    assert!(pasted.contains("Do not commit."), "{pasted}");
    assert!(
        pasted.contains(&format!(
            "write your report to {}/.swarm/reports/T-1.md",
            rig.cwd.display()
        )),
        "{pasted}"
    );
    assert!(pasted.contains("not in the worktree"), "{pasted}");
    assert!(pasted.contains("send(to=\"pm\", ...)"), "{pasted}");

    // A second assignment of the same task (to the other editor) keeps the same worktree.
    assign(&rig, id, "eng2");
    assert_eq!(task(&rig, id).worktree, made.worktree);
    assert_eq!(branches(&rig), "task/T-1-add-login");
}

#[test]
fn the_branch_name_uses_safe_characters_only_also_for_a_hostile_title() {
    let rig = git_rig();
    let hostile = make_task(&rig, "--upload-pack=x; rm -rf");
    let unicode = make_task(&rig, "日本語 Ünï");
    let shell = make_task(&rig, "$(touch pwned) `touch pwned2`");
    let long = make_task(&rig, &"word ".repeat(40));
    for id in [hostile, unicode, shell, long] {
        assign(&rig, id, "eng");
    }
    assert_eq!(branch_of(&rig, hostile), "task/T-1-upload-pack-x-rm-rf");
    assert_eq!(branch_of(&rig, unicode), "task/T-2-n");
    assert_eq!(branch_of(&rig, shell), "task/T-3-touch-pwned-touch-pwned2");
    let long_branch = branch_of(&rig, long);
    assert!(
        long_branch.len() < 60 && !long_branch.ends_with('-'),
        "{long_branch}"
    );
    for id in [hostile, unicode, shell, long] {
        let branch = branch_of(&rig, id);
        let (valid, text) = git_try(&rig.cwd, &["check-ref-format", "--branch", &branch]);
        assert!(valid, "{branch}: {text}");
        assert!(rig.worktree(id).is_dir());
        // The title is kept as it was written.
        assert!(!task(&rig, id).title.is_empty());
    }
    // No title ran as a command or as an option.
    for name in ["pwned", "pwned2", "x", "rf"] {
        assert!(!rig.cwd.join(name).exists(), "{name} exists");
    }
    // The commit message of a hostile title is one value of `-m`.
    work(&rig, hostile, "f.txt", "x\n");
    ok(&review_as(&rig, hostile, "eng"));
    let subject = rig.git(&["log", "-1", "--format=%s", &branch_of(&rig, hostile)]);
    assert_eq!(subject, "T-1: --upload-pack=x; rm -rf");
}

#[test]
fn no_worktree_for_a_folder_that_is_not_a_plain_repository_or_for_an_assignee_that_does_not_edit() {
    enum Setup {
        NotARepository,
        NoCommit,
        DetachedHead,
        Subfolder,
        ReadOnlyAssignee,
    }
    for (name, setup, reason) in [
        (
            "not a repository",
            Setup::NotARepository,
            "not a git repository",
        ),
        ("no commit", Setup::NoCommit, "has no commit yet"),
        ("detached HEAD", Setup::DetachedHead, "detached HEAD"),
        (
            "subfolder",
            Setup::Subfolder,
            "subfolder of a git repository",
        ),
        ("read-only assignee", Setup::ReadOnlyAssignee, ""),
    ] {
        let rig = Rig::new();
        rig.create_floor();
        match setup {
            Setup::NotARepository => {}
            Setup::NoCommit => {
                git(&rig.cwd, &["init", "-q", "-b", "main"]);
            }
            Setup::DetachedHead => {
                init_repo(&rig.cwd);
                git(&rig.cwd, &["checkout", "-q", "--detach"]);
            }
            Setup::Subfolder => {
                // The floor is `<temp>/project`. The repository is `<temp>`.
                init_repo(rig.cwd.parent().unwrap());
            }
            Setup::ReadOnlyAssignee => init_repo(&rig.cwd),
        }
        if matches!(setup, Setup::ReadOnlyAssignee) {
            rig.add_employee("eng", Some("pm"));
        } else {
            rig.add_editor("eng", Some("pm"));
        }
        for slug in ["pm", "eng"] {
            rig.start(slug);
            rig.live_idle(slug);
        }
        let id = make_task(&rig, "Add login");
        assign(&rig, id, "eng");
        let made = task(&rig, id);
        assert_eq!(
            (
                made.branch.clone(),
                made.worktree.clone(),
                made.base_commit.clone()
            ),
            (None, None, None),
            "{name}"
        );
        assert!(
            !rig.cwd.join(".swarm/worktrees").join("T-1").exists(),
            "{name}"
        );
        let git_dir = if matches!(setup, Setup::Subfolder) {
            rig.cwd.parent().unwrap().to_path_buf()
        } else {
            rig.cwd.clone()
        };
        if !matches!(setup, Setup::NotARepository) {
            let (_, list) = git_try(&git_dir, &["branch", "--list", "task/*"]);
            assert_eq!(list, "", "{name}: no branch");
        }
        wait_for("the paste", || !paste_of(&rig, "eng").is_empty());
        let pasted = paste_of(&rig, "eng");
        if reason.is_empty() {
            // A read-only assignee gets the default orders.
            assert!(
                pasted.contains("1. When you start the work"),
                "{name}: {pasted}"
            );
            assert!(!pasted.contains("worktree"), "{name}: {pasted}");
        } else {
            assert!(
                pasted.contains("The app made no git worktree for this task"),
                "{name}: {pasted}"
            );
            assert!(pasted.contains(reason), "{name}: {pasted}");
            assert!(
                pasted.contains("Edit the project folder directly."),
                "{name}: {pasted}"
            );
        }

        // Nothing to commit at review, and nothing to merge: `task_merge` sets `done`.
        let report = rig.cwd.join(".swarm/reports/T-1.md");
        fs::write(&report, "x\n").unwrap();
        ok(&rig.tool(
            "eng",
            "task_update",
            json!({"task": "T-1", "status": "review", "report": report.to_str().unwrap()}),
        ));
        let merged = merge_as(&rig, "pm", id);
        assert!(
            ok(&merged).contains("There is nothing to merge"),
            "{name}: {merged:?}"
        );
        assert_eq!(task(&rig, id).status, TaskStatus::Done, "{name}");
    }
}

// ---------------------------------------------------------------------------------------------
// The commit when a task goes to review
// ---------------------------------------------------------------------------------------------

#[test]
fn review_commits_the_worktree_with_changes_a_new_file_and_the_identity_of_the_operator() {
    let rig = git_rig();
    let id = make_task(&rig, "Change notes");
    assign(&rig, id, "eng");
    let base = task(&rig, id).base_commit.unwrap();
    work(&rig, id, "notes.txt", "one\nTWO\nthree\n");
    work(&rig, id, "new.txt", "new file\n");
    work(&rig, id, "deep/er/file.txt", "deep\n");
    ok(&review_as(&rig, id, "eng"));

    let branch = branch_of(&rig, id);
    let (author, email, subject, body) = {
        let text = rig.git(&["log", "-1", "--format=%an|%ae|%s|%b", &branch]);
        let mut parts = text.splitn(4, '|');
        (
            parts.next().unwrap().to_owned(),
            parts.next().unwrap().to_owned(),
            parts.next().unwrap().to_owned(),
            parts.next().unwrap().to_owned(),
        )
    };
    // The repository has a user, so the commit keeps the identity of the operator.
    assert_eq!(
        (author.as_str(), email.as_str()),
        ("Operator", "operator@example.test")
    );
    assert_eq!(subject, "T-1: Change notes");
    assert_eq!(body, "By: ENG (Role of eng)");
    let files = rig.git(&["show", "--name-only", "--format=", &branch]);
    let mut files: Vec<&str> = files.lines().collect();
    files.sort_unstable();
    assert_eq!(files, ["deep/er/file.txt", "new.txt", "notes.txt"]);
    assert_eq!(rig.git(&["rev-parse", &format!("{branch}~1")]), base);
    // The worktree is clean now, and the main checkout is untouched.
    assert_eq!(git(&rig.worktree(id), &["status", "--porcelain"]), "");
    assert_eq!(
        fs::read_to_string(rig.cwd.join("notes.txt")).unwrap(),
        "one\ntwo\nthree\n"
    );
    // The status event says what the app did.
    let events = rig.db().task_events(id).unwrap();
    let status = events.iter().find(|e| e.kind == "status").unwrap();
    assert_eq!(status.value.as_deref(), Some("review"));
    assert!(
        status
            .text
            .as_deref()
            .unwrap()
            .contains("[app] The app committed the worktree on task/T-1-change-notes"),
        "{status:?}"
    );
}

#[test]
fn review_with_no_change_makes_no_commit_and_says_so() {
    let rig = git_rig();
    let id = make_task(&rig, "Nothing");
    assign(&rig, id, "eng");
    let base = task(&rig, id).base_commit.unwrap();
    ok(&review_as(&rig, id, "eng"));
    assert_eq!(rig.git(&["rev-parse", &branch_of(&rig, id)]), base);
    let events = rig.db().task_events(id).unwrap();
    let status = events.iter().find(|e| e.kind == "status").unwrap();
    assert!(
        status
            .text
            .as_deref()
            .unwrap()
            .contains("No changes in the worktree: no commit.")
    );
    // The user note stays, and the line of the app comes after it.
    let result = rig.tool(
        "eng",
        "task_update",
        json!({"task": "T-1", "status": "in_progress", "note": "back to work"}),
    );
    ok(&result);
    work(&rig, id, "x.txt", "x\n");
    let report = rig.cwd.join(".swarm/reports/T-1.md");
    ok(&rig.tool(
        "eng",
        "task_update",
        json!({"task": "T-1", "status": "review", "note": "ready", "report": report.to_str().unwrap()}),
    ));
    let events = rig.db().task_events(id).unwrap();
    let last = events.iter().rev().find(|e| e.kind == "status").unwrap();
    let text = last.text.as_deref().unwrap();
    assert!(
        text.starts_with("ready\n[app] The app committed the worktree"),
        "{text}"
    );
}

#[test]
fn a_repository_with_no_user_gets_the_name_and_the_email_of_the_employee() {
    let rig = git_rig();
    rig.git(&["config", "user.name", ""]);
    rig.git(&["config", "user.email", ""]);
    let id = make_task(&rig, "Anonymous");
    assign(&rig, id, "eng");
    work(&rig, id, "a.txt", "A\n");
    ok(&review_as(&rig, id, "eng"));
    let text = rig.git(&["log", "-1", "--format=%an|%ae|%b", &branch_of(&rig, id)]);
    assert_eq!(text, "ENG|eng@agent-tower.local|By: ENG (Role of eng)");
}

#[test]
fn a_caller_that_may_not_change_the_task_makes_no_commit() {
    let rig = git_rig();
    let id = make_task(&rig, "Guarded");
    assign(&rig, id, "eng");
    work(&rig, id, "a.txt", "A\n");
    let base = task(&rig, id).base_commit.unwrap();
    // `eng2` is not the assignee, the creator, the ceo or the pm.
    let result = review_as(&rig, id, "eng2");
    assert!(refused(&result).contains("Only the assignee"), "{result:?}");
    assert_eq!(
        rig.git(&["rev-parse", &branch_of(&rig, id)]),
        base,
        "no commit"
    );
    assert_eq!(task(&rig, id).status, TaskStatus::Todo);
}

#[test]
fn a_hook_of_the_repository_does_not_run_during_the_actions_of_the_app() {
    let rig = git_rig();
    let marker = rig.root.path().join("hook-ran");
    let marker = marker.to_str().unwrap();
    // `pre-commit` fails, and the other hooks only write a marker. None may run.
    install_hook(&rig.cwd, "pre-commit", marker, true);
    install_hook(&rig.cwd, "post-checkout", marker, false);
    install_hook(&rig.cwd, "commit-msg", marker, true);
    install_hook(&rig.cwd, "pre-merge-commit", marker, true);
    install_hook(&rig.cwd, "post-merge", marker, false);
    install_hook(&rig.cwd, "post-commit", marker, false);
    let id = make_task(&rig, "Hooks");
    assign(&rig, id, "eng");
    work(&rig, id, "a.txt", "A\n");
    ok(&review_as(&rig, id, "eng"));
    assert!(
        rig.git(&["log", "-1", "--format=%s", &branch_of(&rig, id)])
            .starts_with("T-1:")
    );
    let merged = merge_as(&rig, "pm", id);
    ok(&merged);
    assert!(rig.cwd.join("a.txt").is_file());
    assert!(!Path::new(marker).exists(), "a hook ran");
    // The check can fail: the hook is a real one. A plain git commit runs it.
    fs::write(rig.cwd.join("b.txt"), "B\n").unwrap();
    git(&rig.cwd, &["add", "b.txt"]);
    let (committed, _) = git_try(&rig.cwd, &["commit", "-q", "-m", "by hand"]);
    assert!(!committed, "the pre-commit hook must stop a commit by hand");
    assert!(
        Path::new(marker).exists(),
        "the hook did not run for a commit by hand"
    );
}

// ---------------------------------------------------------------------------------------------
// task_merge
// ---------------------------------------------------------------------------------------------

#[test]
fn a_merge_puts_the_work_in_the_base_branch_and_removes_the_worktree_and_the_branch() {
    let rig = git_rig();
    let a = task_in_review(&rig, "Add a", "eng", "a.txt", "A\n");
    let b = task_in_review(&rig, "Add b", "eng2", "b.txt", "B\n");
    // Two worktrees and two branches exist at the same time.
    assert_eq!(branches(&rig).lines().count(), 2);
    assert!(rig.worktree(a).is_dir() && rig.worktree(b).is_dir());

    let first = merge_as(&rig, "pm", a);
    let text = ok(&first);
    assert!(text.contains("Merged task/T-1-add-a into main"), "{text}");
    assert!(text.contains("The status is done."), "{text}");
    let second = merge_as(&rig, "ceo", b);
    ok(&second);

    // Both files are in the base branch and in the main checkout.
    assert_eq!(fs::read_to_string(rig.cwd.join("a.txt")).unwrap(), "A\n");
    assert_eq!(fs::read_to_string(rig.cwd.join("b.txt")).unwrap(), "B\n");
    assert_eq!(rig.git(&["show", "main:a.txt"]), "A");
    // `--no-ff`: each merge is a merge commit with two parents.
    let parents = rig.git(&["log", "--merges", "--format=%P"]);
    assert_eq!(parents.lines().count(), 2, "{parents}");
    assert!(parents.lines().all(|l| l.split(' ').count() == 2));
    // The worktrees and the branches are gone.
    assert_eq!(branches(&rig), "");
    assert!(!rig.worktree(a).exists() && !rig.worktree(b).exists());
    assert_eq!(rig.git(&["worktree", "list"]).lines().count(), 1);
    assert_eq!(
        rig.git(&["status", "--porcelain", "--untracked-files=no"]),
        ""
    );
    // The tasks are done. The branch name stays as a record, the worktree does not.
    for id in [a, b] {
        let done = task(&rig, id);
        assert_eq!(done.status, TaskStatus::Done);
        assert!(done.branch.is_some() && done.worktree.is_none());
    }
    // The UI is told. The assignee (live) gets the result as a task message.
    assert!(rig.events.tasks.lock().unwrap().len() >= 4);
    wait_for("the merge message", || {
        paste_of(&rig, "eng").contains("The task is merged and done.")
    });
    let pasted = paste_of(&rig, "eng");
    assert!(
        pasted.contains("[task T-1 from Maya (Project Manager)]"),
        "{pasted}"
    );
    assert!(pasted.contains("> Status: done"), "{pasted}");
    // The caller gets no message about its own action: `pm` merged T-1 and sent no message to
    // itself. `ceo` merged T-2, and `pm` gave T-2 out, so `pm` is told about T-2.
    wait_for("the message for the pm", || {
        paste_of(&rig, "pm").contains("[task T-2 from Ada (CEO)]")
    });
    assert!(!paste_of(&rig, "pm").contains("[task T-1 from Maya (Project Manager)]"));
}

#[test]
fn only_the_pm_the_ceo_and_the_operator_may_merge_and_only_from_review_or_blocked() {
    let rig = git_rig();
    let id = task_in_review(&rig, "Add a", "eng", "a.txt", "A\n");
    let base = rig.git(&["rev-parse", "HEAD"]);
    for who in ["eng", "eng2"] {
        let result = merge_as(&rig, who, id);
        assert!(
            refused(&result).contains("Only the pm, the ceo and the operator"),
            "{result:?}"
        );
    }
    assert_eq!(rig.git(&["rev-parse", "HEAD"]), base);
    assert_eq!(task(&rig, id).status, TaskStatus::Review);

    // A task that is not in review or blocked is refused, and nothing changes.
    let todo = make_task(&rig, "Not ready");
    assign(&rig, todo, "eng2");
    let result = merge_as(&rig, "pm", todo);
    assert!(
        refused(&result).contains("Only a task in review or blocked"),
        "{result:?}"
    );
    assert_eq!(task(&rig, todo).status, TaskStatus::Todo);
    refused(&merge_as(&rig, "pm", 99));
    refused(&rig.tool("pm", "task_merge", json!({"task": "bad"})));

    // The operator may merge, through the command.
    let report = rig.tower.task_merge(&rig.cwd, id).unwrap();
    assert!(report.merged, "{report:?}");
    assert_eq!(report.task.status, TaskStatus::Done);
    // A task that is done cannot be merged again.
    let again = merge_as(&rig, "pm", id);
    assert!(refused(&again).contains("has the status done"), "{again:?}");
}

#[test]
fn a_merge_is_blocked_when_the_main_checkout_is_on_another_branch_and_works_after_the_fix() {
    let rig = git_rig();
    let id = task_in_review(&rig, "Add a", "eng", "a.txt", "A\n");
    rig.git(&["checkout", "-q", "-b", "other"]);
    let head = rig.git(&["rev-parse", "HEAD"]);

    let result = merge_as(&rig, "pm", id);
    let text = refused(&result);
    assert!(
        text.contains("The main checkout is on the branch other, and the task needs main"),
        "{text}"
    );
    assert!(text.contains("blocked"), "{text}");
    let blocked = task(&rig, id);
    assert_eq!(blocked.status, TaskStatus::Blocked);
    // The app did not change the branch, and nothing was merged or removed.
    assert_eq!(rig.git(&["rev-parse", "--abbrev-ref", "HEAD"]), "other");
    assert_eq!(rig.git(&["rev-parse", "HEAD"]), head);
    assert!(rig.worktree(id).is_dir());
    assert_eq!(branches(&rig), "task/T-1-add-a");
    assert!(!rig.cwd.join("a.txt").exists());
    // The UI gets a notice.
    let notices = rig.events.notices.lock().unwrap().clone();
    assert!(
        notices
            .iter()
            .any(|(_, kind, text)| kind == "task_blocked" && text.contains("T-1")),
        "{notices:?}"
    );
    // The reason is in the status event, as the note.
    let events = rig.db().task_events(id).unwrap();
    let last = events.last().unwrap();
    assert_eq!(
        (last.kind.as_str(), last.value.as_deref()),
        ("status", Some("blocked"))
    );
    assert!(last.text.as_deref().unwrap().contains("branch other"));
    // The assignee gets the result as a task message (it is live).
    wait_for("the blocked message", || {
        paste_of(&rig, "eng").contains("The merge of this task is blocked.")
    });

    // The operator fixes the branch. The same call now works from `blocked`.
    rig.git(&["checkout", "-q", "main"]);
    let result = merge_as(&rig, "pm", id);
    ok(&result);
    assert_eq!(task(&rig, id).status, TaskStatus::Done);
    assert!(rig.cwd.join("a.txt").is_file());
}

#[test]
fn a_merge_is_blocked_by_a_change_in_a_tracked_file_but_not_by_an_untracked_file_or_swarm() {
    let rig = git_rig();
    // A tracked file under `.swarm/` (the shareable part of a floor).
    fs::write(rig.cwd.join(".swarm/floor-note.txt"), "v1\n").unwrap();
    rig.git(&["add", "-f", ".swarm/floor-note.txt"]);
    rig.git(&["commit", "-q", "-m", "swarm note"]);
    let id = task_in_review(&rig, "Add a", "eng", "a.txt", "A\n");
    let head = rig.git(&["rev-parse", "HEAD"]);

    // A change in a tracked file outside `.swarm/` blocks the merge.
    fs::write(rig.cwd.join("notes.txt"), "one\nchanged by hand\nthree\n").unwrap();
    let result = merge_as(&rig, "pm", id);
    let text = refused(&result);
    assert!(
        text.contains("uncommitted changes in tracked files outside .swarm/: notes.txt"),
        "{text}"
    );
    assert_eq!(task(&rig, id).status, TaskStatus::Blocked);
    assert_eq!(rig.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(
        fs::read_to_string(rig.cwd.join("notes.txt")).unwrap(),
        "one\nchanged by hand\nthree\n",
        "the change of the operator is kept"
    );
    assert!(rig.worktree(id).is_dir());

    // The operator gives the change back. A new untracked file and a change under `.swarm/` do not
    // block the merge.
    rig.git(&["checkout", "--", "notes.txt"]);
    fs::write(rig.cwd.join("untracked.txt"), "x\n").unwrap();
    fs::write(rig.cwd.join(".swarm/floor-note.txt"), "v2\n").unwrap();
    let result = merge_as(&rig, "pm", id);
    ok(&result);
    assert_eq!(task(&rig, id).status, TaskStatus::Done);
    assert_eq!(
        fs::read_to_string(rig.cwd.join("untracked.txt")).unwrap(),
        "x\n"
    );
    assert_eq!(
        fs::read_to_string(rig.cwd.join(".swarm/floor-note.txt")).unwrap(),
        "v2\n"
    );
}

#[test]
fn a_conflict_stops_the_merge_leaves_the_checkout_clean_and_names_the_file() {
    let rig = git_rig();
    // Two tasks change the same line of the same file.
    let first = task_in_review(&rig, "First", "eng", "notes.txt", "one\nfrom eng\nthree\n");
    let second = task_in_review(
        &rig,
        "Second",
        "eng2",
        "notes.txt",
        "one\nfrom eng2\nthree\n",
    );
    ok(&merge_as(&rig, "pm", first));
    let head = rig.git(&["rev-parse", "HEAD"]);

    let result = merge_as(&rig, "pm", second);
    let text = refused(&result);
    assert!(
        text.contains("The merge has a conflict in: notes.txt"),
        "{text}"
    );
    assert!(text.contains("blocked"), "{text}");
    assert_eq!(task(&rig, second).status, TaskStatus::Blocked);
    // After the abort the main checkout is clean and the same as before.
    assert_eq!(rig.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(
        rig.git(&["status", "--porcelain"])
            .lines()
            .filter(|l| !l.starts_with("??"))
            .count(),
        0
    );
    let (merging, _) = git_try(&rig.cwd, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]);
    assert!(!merging, "no merge in progress");
    assert_eq!(
        fs::read_to_string(rig.cwd.join("notes.txt")).unwrap(),
        "one\nfrom eng\nthree\n"
    );
    // The branch and the worktree of the second task stay, so an engineer can solve it.
    assert!(rig.worktree(second).is_dir());
    assert!(branches(&rig).contains("task/T-2-second"));
    // The first task is done.
    assert_eq!(task(&rig, first).status, TaskStatus::Done);
}

// ---------------------------------------------------------------------------------------------
// task_diff
// ---------------------------------------------------------------------------------------------

#[test]
fn task_diff_shows_the_stat_and_the_patch_and_cuts_a_big_patch() {
    let rig = git_rig();
    let id = task_in_review(&rig, "Add a", "eng", "a.txt", "A\n");
    // Any employee of the floor may read the diff, also one that is not on the task.
    for who in ["eng2", "ceo"] {
        let result = rig.tool(who, "task_diff", json!({"task": "T-1"}));
        let text = ok(&result);
        assert!(text.contains("branch task/T-1-add-a, base main"), "{text}");
        assert!(text.contains("Diffstat:"), "{text}");
        assert!(text.contains("a.txt | 1 +"), "{text}");
        assert!(text.contains("+++ b/a.txt"), "{text}");
        assert!(text.contains("+A"), "{text}");
    }
    // The operator command returns the parts.
    let view = rig.tower.task_diff(&rig.cwd, id).unwrap();
    assert_eq!(view.branch.as_deref(), Some("task/T-1-add-a"));
    assert!(view.stat.contains("a.txt") && view.patch.contains("+A") && !view.truncated);

    // A big change is cut at 60000 bytes, with a note.
    let big = task_in_review(
        &rig,
        "Big",
        "eng2",
        "big.txt",
        &"0123456789abcdef\n".repeat(8000),
    );
    let result = rig.tool("pm", "task_diff", json!({"task": format!("T-{big}")}));
    let text = ok(&result);
    assert!(
        text.contains("The patch is cut at 60000 bytes"),
        "{}",
        &text[text.len() - 300..]
    );
    assert!(text.len() < 62_000 + 1000, "{}", text.len());
    assert!(rig.tower.task_diff(&rig.cwd, big).unwrap().truncated);

    // Only committed changes show.
    let late = make_task(&rig, "Late");
    assign(&rig, late, "eng");
    work(&rig, late, "late.txt", "not committed\n");
    let result = rig.tool("pm", "task_diff", json!({"task": format!("T-{late}")}));
    assert!(ok(&result).contains("There is no change on the branch yet."));

    // A task with no worktree says so. The branch of a merged task is gone.
    let plain = make_task(&rig, "Read only");
    let result = rig.tool("pm", "task_diff", json!({"task": format!("T-{plain}")}));
    assert!(ok(&result).contains("This task has no git branch"));
    ok(&merge_as(&rig, "pm", id));
    let result = rig.tool("pm", "task_diff", json!({"task": "T-1"}));
    assert!(
        ok(&result).contains("does not exist any more"),
        "{result:?}"
    );
    refused(&rig.tool("pm", "task_diff", json!({"task": "T-99"})));
}

// ---------------------------------------------------------------------------------------------
// task_discard
// ---------------------------------------------------------------------------------------------

#[test]
fn the_operator_discards_a_task_that_is_not_done() {
    let rig = git_rig();
    let id = make_task(&rig, "Throw away");
    assign(&rig, id, "eng");
    work(&rig, id, "half.txt", "not committed, not clean\n");
    assert!(rig.worktree(id).is_dir());

    let task = rig.tower.task_discard(&rig.cwd, id).unwrap();
    assert_eq!(task.status, TaskStatus::Blocked);
    assert_eq!(
        (
            task.branch,
            task.base_branch,
            task.base_commit,
            task.worktree
        ),
        (None, None, None, None)
    );
    assert!(!rig.worktree(id).exists());
    assert_eq!(branches(&rig), "");
    assert_eq!(rig.git(&["worktree", "list"]).lines().count(), 1);
    let events = rig.db().task_events(id).unwrap();
    let last = events.last().unwrap();
    assert_eq!(last.value.as_deref(), Some("blocked"));
    assert_eq!(last.text.as_deref(), Some("discarded by the operator"));
    // The main checkout and the other files are untouched.
    assert_eq!(rig.git(&["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(
        fs::read_to_string(rig.cwd.join("notes.txt")).unwrap(),
        "one\ntwo\nthree\n"
    );
    // The assignee is told.
    wait_for("the discard message", || {
        paste_of(&rig, "eng").contains("The operator discarded this task")
    });

    // A done task is refused. A task with no worktree is only set to `blocked`.
    let done = task_in_review(&rig, "Add a", "eng", "a.txt", "A\n");
    ok(&merge_as(&rig, "pm", done));
    let error = rig
        .tower
        .task_discard(&rig.cwd, done)
        .unwrap_err()
        .to_string();
    assert!(error.contains("is done"), "{error}");
    let plain = make_task(&rig, "Plain");
    let task = rig.tower.task_discard(&rig.cwd, plain).unwrap();
    assert_eq!(task.status, TaskStatus::Blocked);
    // A discarded task can be given again: it gets a fresh worktree with the same name.
    assign(&rig, id, "eng2");
    assert!(rig.worktree(id).is_dir());
    assert_eq!(branch_of(&rig, id), "task/T-1-throw-away");
}

// ---------------------------------------------------------------------------------------------
// Safety of the git calls
// ---------------------------------------------------------------------------------------------

/// Change the `worktree` and the `branch` of a task row, as someone with the database file can.
fn set_row(rig: &Rig, id: i64, worktree: &str, branch: &str) {
    let conn = rusqlite::Connection::open(rig.cwd.join(".swarm/state/floor.db")).unwrap();
    conn.execute(
        "UPDATE tasks SET worktree = ?2, branch = ?3 WHERE id = ?1",
        rusqlite::params![id, worktree, branch],
    )
    .unwrap();
}

#[test]
fn a_changed_row_cannot_point_a_forced_removal_at_another_folder() {
    let rig = git_rig();
    let id = make_task(&rig, "Safe");
    assign(&rig, id, "eng");
    let precious = rig.cwd.join("precious");
    fs::create_dir_all(&precious).unwrap();
    fs::write(precious.join("keep.txt"), "keep\n").unwrap();
    // Someone changes the database row: the worktree is "precious", the branch is "main".
    set_row(&rig, id, "precious", "main");
    let error = rig
        .tower
        .task_discard(&rig.cwd, id)
        .unwrap_err()
        .to_string();
    assert!(error.contains("could not discard"), "{error}");
    assert!(precious.join("keep.txt").is_file());
    assert_eq!(rig.git(&["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(
        rig.git(&["branch", "--list", "main"])
            .trim_start_matches("* ")
            .trim(),
        "main"
    );
}

/// The two checks of `discard_worktree` with values that git does not refuse by itself (finding
/// M8). `main` and `precious` above fail in git whatever the app checks. A real worktree of
/// another task and a branch that nobody has checked out pass in git, so only the checks of the
/// app stop them.
#[test]
fn the_discard_checks_stop_the_worktree_of_another_task_and_a_branch_of_no_task() {
    let rig = git_rig();
    rig.git(&["branch", "feature"]);
    let one = make_task(&rig, "One");
    assign(&rig, one, "eng");
    let two = make_task(&rig, "Two");
    assign(&rig, two, "eng2");
    work(&rig, two, "precious.txt", "uncommitted work of eng2\n");
    let one_branch = branch_of(&rig, one);

    // 1. The row of T-1 names the worktree of T-2. The branch is the right one.
    set_row(&rig, one, ".swarm/worktrees/T-2", &one_branch);
    let error = rig
        .tower
        .task_discard(&rig.cwd, one)
        .unwrap_err()
        .to_string();
    assert!(error.contains("could not discard"), "{error}");
    assert!(
        rig.worktree(two).join("precious.txt").is_file(),
        "the work of T-2 is lost"
    );

    // 2. The row of T-1 names a branch that is not a task branch. The worktree is the right one.
    set_row(&rig, one, ".swarm/worktrees/T-1", "feature");
    let error = rig
        .tower
        .task_discard(&rig.cwd, one)
        .unwrap_err()
        .to_string();
    assert!(error.contains("could not discard"), "{error}");
    assert!(
        rig.git(&["branch", "--list", "feature"])
            .contains("feature"),
        "the branch feature is deleted"
    );
    assert!(
        rig.worktree(one).is_dir(),
        "nothing is removed when a name is bad"
    );
}

// ---------------------------------------------------------------------------------------------
// M1: the `.git` file of a worktree is not trusted
// ---------------------------------------------------------------------------------------------

/// The agent can write the `.git` file of its worktree. It makes a git dir of its own with a
/// `core.fsmonitor` command and aims the `.git` file at it (finding M1, probe P1). The app must
/// refuse to run git there, and it must change nothing.
#[test]
fn a_git_file_aimed_at_a_fake_git_dir_makes_the_review_refuse() {
    let rig = git_rig();
    let id = make_task(&rig, "Evil");
    assign(&rig, id, "eng");
    let wt = rig.worktree(id);
    let marker = rig.root.path().join("pwned");
    let fake = wt.join("fakegit");
    fs::create_dir_all(fake.join("objects")).unwrap();
    fs::create_dir_all(fake.join("refs/heads")).unwrap();
    fs::write(fake.join("HEAD"), "ref: refs/heads/x\n").unwrap();
    fs::write(
        fake.join("config"),
        format!(
            "[core]\n\trepositoryformatversion = 0\n\tbare = false\n\tfsmonitor = \"touch {}; false\"\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::write(wt.join(".git"), "gitdir: fakegit\n").unwrap();
    work(&rig, id, "a.txt", "A\n");
    let before = task(&rig, id);
    let result = review_as(&rig, id, "eng");
    let text = refused(&result);
    assert!(text.contains("git link"), "{text}");
    assert!(text.contains("not the one that git made"), "{text}");
    assert!(!marker.exists(), "the program of core.fsmonitor ran");
    // Nothing is set: the status is as before and the fake git dir has no commit.
    assert_eq!(task(&rig, id).status, before.status);
    assert_eq!(task(&rig, id).report, before.report);
    assert!(!fake.join("index").exists());
    // The merge of the same task is blocked in the same way, and nothing ran there either.
    rig.tower
        .task_update(&rig.cwd, id, TaskStatus::Blocked, None)
        .unwrap();
    let merged = merge_as(&rig, "pm", id);
    assert!(
        refused(&merged).contains("not the one that git made"),
        "{merged:?}"
    );
    assert!(!marker.exists());
}

/// The `.git` file points at the main git dir: `git add -A` and `git commit` would run in the
/// operator's checkout (probe P1b). The staged file of the operator must stay staged, and the
/// branch `main` must not move.
#[test]
fn a_git_file_aimed_at_the_main_repository_commits_nothing_there() {
    let rig = git_rig();
    fs::write(rig.cwd.join("staged.txt"), "operator staged\n").unwrap();
    rig.git(&["add", "staged.txt"]);
    let main_before = rig.git(&["rev-parse", "main"]);
    let id = make_task(&rig, "Evil2");
    assign(&rig, id, "eng");
    let wt = rig.worktree(id);
    fs::write(
        wt.join(".git"),
        format!("gitdir: {}\n", rig.cwd.join(".git").display()),
    )
    .unwrap();
    work(&rig, id, "agent.txt", "agent\n");
    let result = review_as(&rig, id, "eng");
    assert!(
        refused(&result).contains("not the one that git made"),
        "{result:?}"
    );
    assert_eq!(rig.git(&["rev-parse", "main"]), main_before);
    assert_eq!(
        rig.git(&["diff", "--cached", "--name-only"]),
        "staged.txt",
        "the index of the operator changed"
    );
    // A deleted `.git` file makes git look in the parent folders: the main checkout. Same result.
    fs::remove_file(wt.join(".git")).unwrap();
    let result = review_as(&rig, id, "eng");
    assert!(
        refused(&result).contains("not the one that git made"),
        "{result:?}"
    );
    assert_eq!(rig.git(&["rev-parse", "main"]), main_before);
    assert_eq!(rig.git(&["diff", "--cached", "--name-only"]), "staged.txt");
}

// ---------------------------------------------------------------------------------------------
// M2: a merge that the operator started is not aborted
// ---------------------------------------------------------------------------------------------

fn merge_head_exists(rig: &Rig, name: &str) -> bool {
    git_try(&rig.cwd, &["rev-parse", "-q", "--verify", name]).0
}

/// The operator is in the middle of `git merge --no-commit` (probe P2). `task_merge` must block
/// with a clear text and must not run `git merge --abort`.
#[test]
fn a_merge_of_the_operator_in_progress_blocks_the_task_merge_and_is_not_aborted() {
    let rig = git_rig();
    let id = task_in_review(&rig, "Add a", "eng", "a.txt", "A\n");
    rig.git(&["branch", "side"]);
    rig.git(&["checkout", "-q", "side"]);
    fs::write(rig.cwd.join("side.txt"), "side\n").unwrap();
    rig.git(&["add", "side.txt"]);
    rig.git(&["commit", "-q", "-m", "side"]);
    rig.git(&["checkout", "-q", "main"]);
    rig.git(&["merge", "-q", "-s", "ours", "--no-commit", "side"]);
    assert!(merge_head_exists(&rig, "MERGE_HEAD"));
    let head = rig.git(&["rev-parse", "HEAD"]);
    let result = merge_as(&rig, "pm", id);
    let text = refused(&result);
    assert!(text.contains("in the middle of a merge"), "{text}");
    assert!(text.contains("The status is blocked"), "{text}");
    assert!(
        merge_head_exists(&rig, "MERGE_HEAD"),
        "the merge of the operator was aborted"
    );
    assert_eq!(rig.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(task(&rig, id).status, TaskStatus::Blocked);
    assert!(rig.worktree(id).is_dir(), "the worktree stays");
    assert!(branches(&rig).contains("task/T-1"));
    // The operator ends the merge. The same task now merges.
    rig.git(&["commit", "-q", "-m", "operator merge"]);
    ok(&merge_as(&rig, "pm", id));
    assert_eq!(task(&rig, id).status, TaskStatus::Done);
}

/// A cherry-pick in progress with nothing staged (probe P2b), and a rebase (probe P15).
#[test]
fn a_cherry_pick_or_a_rebase_in_progress_blocks_the_task_merge() {
    let rig = git_rig();
    let id = task_in_review(&rig, "Add a", "eng", "a.txt", "A\n");
    rig.git(&["branch", "side"]);
    rig.git(&["checkout", "-q", "side"]);
    fs::write(rig.cwd.join("notes.txt"), "one\nSIDE\nthree\n").unwrap();
    rig.git(&["commit", "-qam", "side"]);
    rig.git(&["checkout", "-q", "main"]);
    fs::write(rig.cwd.join("notes.txt"), "one\nMAIN\nthree\n").unwrap();
    rig.git(&["commit", "-qam", "main change"]);
    let (_ok, out) = git_try(&rig.cwd, &["cherry-pick", "side"]);
    assert!(
        out.contains("conflict") || out.contains("CONFLICT"),
        "{out}"
    );
    // The operator picks the main version: nothing is staged and CHERRY_PICK_HEAD stays.
    rig.git(&["checkout", "HEAD", "--", "notes.txt"]);
    assert!(merge_head_exists(&rig, "CHERRY_PICK_HEAD"));
    let result = merge_as(&rig, "pm", id);
    assert!(
        refused(&result).contains("in the middle of a cherry-pick"),
        "{result:?}"
    );
    assert!(
        merge_head_exists(&rig, "CHERRY_PICK_HEAD"),
        "the cherry-pick was ended"
    );
    rig.git(&["cherry-pick", "--abort"]);

    // A rebase with a conflict.
    rig.git(&["checkout", "-q", "-b", "side2", "side"]);
    fs::write(rig.cwd.join("notes.txt"), "one\nSIDE2\nthree\n").unwrap();
    rig.git(&["commit", "-qam", "side2"]);
    rig.git(&["checkout", "-q", "main"]);
    let (_ok, out) = git_try(&rig.cwd, &["rebase", "side2"]);
    assert!(out.contains("onflict"), "{out}");
    let state = rig.git(&["rev-parse", "--git-path", "rebase-merge"]);
    assert!(rig.cwd.join(state).exists());
    let result = merge_as(&rig, "pm", id);
    assert!(
        refused(&result).contains("in the middle of a rebase"),
        "{result:?}"
    );
    assert!(
        rig.cwd
            .join(rig.git(&["rev-parse", "--git-path", "rebase-merge"]))
            .exists()
    );
    rig.git(&["rebase", "--abort"]);
    assert_eq!(task(&rig, id).status, TaskStatus::Blocked);
}

// ---------------------------------------------------------------------------------------------
// M3: only a merge makes a task with a branch `done`
// ---------------------------------------------------------------------------------------------

#[test]
fn an_employee_cannot_set_done_on_a_task_with_a_branch() {
    let rig = git_rig();
    let id = make_task(&rig, "Add a");
    assign(&rig, id, "eng");
    work(&rig, id, "a.txt", "A\n");
    let report = rig.cwd.join(format!(".swarm/reports/T-{id}.md"));
    fs::write(&report, "Done.\n").unwrap();
    // The assignee: told to set review.
    let result = rig.tool(
        "eng",
        "task_update",
        json!({"task": "T-1", "status": "done", "report": report.to_str().unwrap()}),
    );
    let text = refused(&result);
    assert!(text.contains("Set it to review"), "{text}");
    assert_eq!(task(&rig, id).status, TaskStatus::Todo);
    // The pm: told to use task_merge. The task is in review first.
    ok(&review_as(&rig, id, "eng"));
    let result = rig.tool(
        "pm",
        "task_update",
        json!({"task": "T-1", "status": "done"}),
    );
    let text = refused(&result);
    assert!(text.contains("use task_merge"), "{text}");
    assert_eq!(task(&rig, id).status, TaskStatus::Review);
    assert!(!rig.cwd.join("a.txt").exists());
    // The merge sets done.
    ok(&merge_as(&rig, "pm", id));
    assert_eq!(task(&rig, id).status, TaskStatus::Done);
    assert!(rig.cwd.join("a.txt").exists());
}

#[test]
fn the_operator_can_set_done_with_a_warning_and_discard_a_done_task_that_has_a_worktree() {
    let rig = git_rig();
    let id = task_in_review(&rig, "Add a", "eng", "a.txt", "A\n");
    let notices_before = app_support::lock(&rig.events.notices).len();
    // The operator may set any status. The notice says that the branch is not merged.
    let after = rig
        .tower
        .task_update(&rig.cwd, id, TaskStatus::Done, None)
        .unwrap();
    assert_eq!(after.status, TaskStatus::Done);
    let notices = app_support::lock(&rig.events.notices).clone();
    let (_, kind, text) = &notices[notices_before];
    assert_eq!(kind, "task_done_unmerged");
    assert!(text.contains("not merged"), "{text}");
    assert!(!rig.cwd.join("a.txt").exists());
    // The worktree is still there, so the operator can discard the task.
    assert!(rig.worktree(id).is_dir());
    let discarded = rig.tower.task_discard(&rig.cwd, id).unwrap();
    assert_eq!(discarded.status, TaskStatus::Blocked);
    assert!(!rig.worktree(id).exists());
    assert_eq!(branches(&rig), "");
    // A merge of the discarded task does not make it `done`.
    let result = merge_as(&rig, "pm", id);
    let text = refused(&result);
    assert!(text.contains("discarded, nothing to merge"), "{text}");
    assert_eq!(task(&rig, id).status, TaskStatus::Blocked);
    let merged = rig.tower.task_merge(&rig.cwd, id);
    assert!(
        merged
            .unwrap_err()
            .to_string()
            .contains("discarded, nothing to merge")
    );
    assert_eq!(task(&rig, id).status, TaskStatus::Blocked);
}

// ---------------------------------------------------------------------------------------------
// M5: the snapshot does not commit everything
// ---------------------------------------------------------------------------------------------

/// A secret file, build output, a package folder, a file over 5 MB and a nested repository
/// (probe P8). The snapshot commits none of them, unstages everything, blocks the task, and names
/// the files. After an ignore rule, the same task goes to review.
#[test]
fn the_snapshot_blocks_secrets_build_output_big_files_and_nested_repositories() {
    let rig = git_rig();
    let id = make_task(&rig, "Build");
    assign(&rig, id, "eng");
    let base = task(&rig, id).base_commit.unwrap();
    work(&rig, id, ".env", "API_KEY=fake-not-real\n");
    work(&rig, id, ".env.example", "API_KEY=\n");
    work(&rig, id, "target/debug/app", &"x".repeat(1000));
    work(&rig, id, "node_modules/a/index.js", "x\n");
    work(&rig, id, "deploy/server.pem", "not a key\n");
    work(&rig, id, "src/lib.rs", "fn main() {}\n");
    let big = rig.worktree(id).join("big.bin");
    fs::write(&big, vec![7u8; 6 * 1024 * 1024]).unwrap();
    let nested = rig.worktree(id).join("vendor/lib");
    fs::create_dir_all(&nested).unwrap();
    init_repo(&nested);

    let result = review_as(&rig, id, "eng");
    let text = refused(&result);
    for path in [
        ".env",
        "target/debug/app",
        "node_modules/a/index.js",
        "deploy/server.pem",
        "big.bin",
        "vendor/lib",
    ] {
        assert!(text.contains(path), "{path} is not in: {text}");
    }
    assert!(!text.contains(".env.example"), "{text}");
    assert!(!text.contains("src/lib.rs"), "{text}");
    assert!(text.contains("Add an ignore rule"), "{text}");
    assert!(text.contains("The status is blocked"), "{text}");
    // Nothing is committed, the index is empty, the files are still on disk.
    let wt = rig.worktree(id);
    assert_eq!(rig.git(&["rev-parse", &branch_of(&rig, id)]), base);
    assert_eq!(git(&wt, &["diff", "--cached", "--name-only"]), "");
    assert!(wt.join(".env").is_file() && big.is_file());
    assert_eq!(task(&rig, id).status, TaskStatus::Blocked);

    // The employee adds an ignore rule and removes the nested repository. Now it commits.
    fs::remove_dir_all(&nested).unwrap();
    work(
        &rig,
        id,
        ".gitignore",
        ".env\ntarget/\nnode_modules/\n*.pem\nbig.bin\n",
    );
    ok(&review_as(&rig, id, "eng"));
    assert_eq!(task(&rig, id).status, TaskStatus::Review);
    let committed = git(&wt, &["show", "--name-only", "--format=", "HEAD"]);
    let mut names: Vec<&str> = committed.lines().collect();
    names.sort_unstable();
    assert_eq!(names, [".env.example", ".gitignore", "src/lib.rs"]);
}

#[test]
fn the_snapshot_blocks_more_than_2000_files_and_a_normal_change_still_commits() {
    let rig = git_rig();
    let id = make_task(&rig, "Many");
    assign(&rig, id, "eng");
    let dir = rig.worktree(id).join("gen");
    fs::create_dir_all(&dir).unwrap();
    for n in 0..2001 {
        fs::write(dir.join(format!("f{n}.txt")), "x\n").unwrap();
    }
    let result = review_as(&rig, id, "eng");
    let text = refused(&result);
    assert!(text.contains("2001 files are staged"), "{text}");
    assert_eq!(
        git(&rig.worktree(id), &["diff", "--cached", "--name-only"]),
        ""
    );
    // A normal change commits, and the merge brings it to the main checkout.
    fs::remove_dir_all(&dir).unwrap();
    work(&rig, id, "docs/guide.md", "# Guide\n");
    work(&rig, id, "notes.txt", "one\ntwo\nthree\nfour\n");
    ok(&review_as(&rig, id, "eng"));
    ok(&merge_as(&rig, "pm", id));
    assert!(rig.cwd.join("docs/guide.md").is_file());
    assert_eq!(task(&rig, id).status, TaskStatus::Done);
}

/// A file that the repository tracks already is the choice of the operator: a change of it is
/// committed, also in a folder named `build` or with the name `.env`.
#[test]
fn a_tracked_file_in_a_build_folder_is_committed_when_it_changes() {
    let rig = Rig::new();
    rig.create_floor();
    init_repo(&rig.cwd);
    fs::create_dir_all(rig.cwd.join("build")).unwrap();
    fs::write(rig.cwd.join("build/tracked.txt"), "v1\n").unwrap();
    rig.git(&["add", "build/tracked.txt"]);
    rig.git(&["commit", "-q", "-m", "track build"]);
    rig.add_editor("eng", Some("pm"));
    for slug in ["ceo", "pm", "eng"] {
        rig.start(slug);
        rig.live_idle(slug);
    }
    let id = make_task(&rig, "Edit build");
    assign(&rig, id, "eng");
    work(&rig, id, "build/tracked.txt", "v2\n");
    ok(&review_as(&rig, id, "eng"));
    assert_eq!(
        git(
            &rig.worktree(id),
            &["show", "--name-only", "--format=", "HEAD"]
        ),
        "build/tracked.txt"
    );
}

// ---------------------------------------------------------------------------------------------
// A leftover folder does not stop an assignment for ever
// ---------------------------------------------------------------------------------------------

#[test]
fn a_leftover_folder_with_the_worktree_name_gives_a_free_name() {
    let rig = git_rig();
    let id = make_task(&rig, "Leftover");
    // A folder that git does not know, with the name of the worktree of the task.
    let leftover = rig.worktree(id);
    fs::create_dir_all(&leftover).unwrap();
    fs::write(leftover.join("old.txt"), "old\n").unwrap();
    assign(&rig, id, "eng");
    let row = task(&rig, id);
    assert_eq!(row.worktree.as_deref(), Some(".swarm/worktrees/T-1-2"));
    assert!(rig.cwd.join(".swarm/worktrees/T-1-2/.git").exists());
    assert!(
        leftover.join("old.txt").is_file(),
        "the leftover is not touched"
    );
    // The task works in the new folder: review, merge and the removal use the stored path.
    let path = rig.cwd.join(".swarm/worktrees/T-1-2/a.txt");
    fs::write(path, "A\n").unwrap();
    ok(&review_as(&rig, id, "eng"));
    ok(&merge_as(&rig, "pm", id));
    assert!(rig.cwd.join("a.txt").is_file());
    assert!(!rig.cwd.join(".swarm/worktrees/T-1-2").exists());
    assert!(leftover.join("old.txt").is_file());
}
