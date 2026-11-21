//! `task_merge`, `task_diff` and `task_discard` (M3, spec 4.8, changed 2026-11-21).
//!
//! # Who may do what
//!
//! | Action | Employee | Operator |
//! |---|---|---|
//! | `task_merge` | the `pm` and the `ceo` | yes |
//! | `task_diff` | any employee of the floor | yes |
//! | `task_discard` | nobody | yes (command only) |
//!
//! # `task_merge`
//!
//! The task must be in `review` or `blocked`. A task with no branch has nothing to merge: the
//! call sets it to `done`. For a task with a branch, the app does these steps, in the main
//! checkout, under the git lock of the floor (`FloorHandle::git`):
//!
//! 0. The main checkout must not be in the middle of a merge, a cherry-pick, a revert or a rebase
//!    that the operator started (`gitwork::operation_in_progress`). The app never ends such an
//!    operation. It runs `git merge --abort` only for a merge that this call started.
//! 1. It commits the worktree (the same snapshot as at `review`). A file that must not go in a
//!    commit (a secret, build output, a big file, a nested repository) stops the merge.
//! 2. The main checkout must be on the base branch of the task.
//! 3. `git status --porcelain --untracked-files=no` must show no change outside `.swarm/`.
//! 4. `git merge --no-ff --no-edit`. On a conflict, `git merge --abort`.
//! 5. On success: `git worktree remove` (no `--force`), `git branch -d`, status `done`.
//!
//! Only `task_merge` sets the status `done` for a task that has a branch (an employee cannot, and
//! `task_update` refuses it). A task that the operator discarded answers "discarded, nothing to
//! merge" and does not become `done`.
//!
//! A step that does not pass sets the status `blocked`, with the reason as the note. The result
//! goes to the caller (the tool answer), to the assignee and the assigner (a task message), and
//! to the UI (`tasks_changed`, and a `notice` of the kind `task_blocked`). The app never uses
//! `--force`, `reset`, `checkout` in the main checkout, or `push` here.

use super::bus::{Sent, Slot, db_error, merge_result_orders};
use super::tasks::{Actor, parse_task_ref, status_body, task_error};
use super::{FloorHandle, OPERATOR, Result, TowerCore, locked, now_ms, other};
use crate::gitwork::{self, MergeResult, Snapshot};
use crate::model::Employee;
use crate::store::{GitAfter, KIND_TASK_STATUS, NewTaskMessage, TaskRecord, TaskStatus};
use serde_json::Value;
use std::sync::Arc;

/// The most bytes of the patch that `task_diff` returns.
pub const DIFF_PATCH_MAX_BYTES: usize = 60_000;

/// The most bytes of the diffstat that `task_diff` returns.
pub const DIFF_STAT_MAX_BYTES: usize = 20_000;

/// The note of the status event of a discard. `task_merge` reads it to know a discarded task.
const DISCARD_NOTE: &str = "discarded by the operator";

/// The most file names in the text of a blocked merge.
const FILES_SHOWN: usize = 10;

/// The result of a merge that the app tried. A merge that was refused (a wrong status, a caller
/// that may not merge) is an error and not a report: nothing changed then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeReport {
    /// The branch is merged and the task is `done`.
    pub merged: bool,
    /// The text for the caller.
    pub text: String,
    /// The task after the action.
    pub task: TaskRecord,
}

/// The diff of a task (`TaskDiff` of the contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDiffView {
    /// The task has a branch.
    pub branch: Option<String>,
    pub base_branch: Option<String>,
    pub stat: String,
    pub patch: String,
    /// The patch or the stat was cut.
    pub truncated: bool,
    /// A note for the reader: there is no branch, the branch is gone, the patch was cut.
    pub note: Option<String>,
}

fn short(commit: &str) -> &str {
    &commit[..commit.len().min(12)]
}

/// The file names for a text: at most `FILES_SHOWN`, then the count of the others.
fn file_list(files: &[String]) -> String {
    let shown: Vec<&str> = files.iter().take(FILES_SHOWN).map(String::as_str).collect();
    let mut text = shown.join(", ");
    if files.len() > FILES_SHOWN {
        text.push_str(&format!(" and {} more", files.len() - FILES_SHOWN));
    }
    text
}

impl TowerCore {
    // ---- the tools ----

    pub(super) fn tool_task_merge(
        &self,
        who: &super::bus::Identity,
        args: &Value,
    ) -> std::result::Result<String, String> {
        let (floor, team, actor) = self.task_actor(who)?;
        let id = parse_task_ref(args.get("task"))?;
        let report = self.merge_task_as(&floor, &team, &actor, id)?;
        // A merge that did not happen is an error result for the agent: it must act.
        if report.merged {
            Ok(report.text)
        } else {
            Err(report.text)
        }
    }

    pub(super) fn tool_task_diff(
        &self,
        who: &super::bus::Identity,
        args: &Value,
    ) -> std::result::Result<String, String> {
        let (floor, _team, _actor) = self.task_actor(who)?;
        let id = parse_task_ref(args.get("task"))?;
        let diff = self.diff_task(&floor, id)?;
        Ok(diff_text(id, &diff))
    }

    // ---- the commands of the operator ----

    /// The operator merges a task (see the module text). A merge that is blocked is `Ok` with
    /// `merged` false.
    pub fn task_merge(&self, path: &std::path::Path, id: i64) -> Result<MergeReport> {
        let floor = self.floor_handle(path)?;
        let team = self.team_of(&floor).map_err(other)?;
        self.merge_task_as(&floor, &team, &Actor::operator(), id)
            .map_err(other)
    }

    /// The changes of a task, for the UI.
    pub fn task_diff(&self, path: &std::path::Path, id: i64) -> Result<TaskDiffView> {
        let floor = self.floor_handle(path)?;
        self.diff_task(&floor, id).map_err(other)
    }

    /// The operator discards a task: the app removes its worktree (`--force`, this one worktree
    /// only) and its branch (`-D`, this one branch only), and sets the status `blocked` with the
    /// note "discarded by the operator". The assignee and the assigner get a message. A `done`
    /// task is refused, unless its worktree still exists (a `done` with no merge, or a merge that
    /// could not remove the worktree).
    pub fn task_discard(&self, path: &std::path::Path, id: i64) -> Result<TaskRecord> {
        let floor = self.floor_handle(path)?;
        let team = self.team_of(&floor).map_err(other)?;
        let actor = Actor::operator();
        let after = {
            let _git = locked(&floor.git);
            let (task, _) = self.read_task(&floor, id)?;
            if task.status == TaskStatus::Done && task.worktree.is_none() {
                return Err(other(format!(
                    "Task T-{id} is done and has no worktree. The app does not discard a done task."
                )));
            }
            gitwork::discard_worktree(
                &floor.path,
                id,
                task.worktree.as_deref(),
                task.branch.as_deref(),
            )
            .map_err(|e| other(format!("The app could not discard task T-{id}: {e}")))?;
            locked(&floor.store)
                .db()
                .finish_task(
                    id,
                    &actor.slug,
                    TaskStatus::Blocked,
                    DISCARD_NOTE,
                    GitAfter::AllGone,
                    now_ms(),
                )
                .map_err(|e| other(task_error(e)))?
                .1
        };
        self.notifier.tasks_changed(&floor.path);
        self.tell_about_task(
            &floor,
            &team,
            &actor,
            &after,
            DISCARD_NOTE,
            merge_result_orders(false, true),
        );
        Ok(after)
    }

    // ---- the rules ----

    /// `task_diff` for an employee or the operator.
    fn diff_task(
        &self,
        floor: &Arc<FloorHandle>,
        id: i64,
    ) -> std::result::Result<TaskDiffView, String> {
        let (task, _) = self.read_task(floor, id).map_err(|e| e.to_string())?;
        let (Some(branch), Some(base_commit)) = (&task.branch, &task.base_commit) else {
            return Ok(TaskDiffView {
                branch: None,
                base_branch: None,
                stat: String::new(),
                patch: String::new(),
                truncated: false,
                note: Some("This task has no git branch: no worktree was made for it. The employee edits the project folder directly, so there is no diff to show.".to_owned()),
            });
        };
        let gone = |note: String| TaskDiffView {
            branch: Some(branch.clone()),
            base_branch: task.base_branch.clone(),
            stat: String::new(),
            patch: String::new(),
            truncated: false,
            note: Some(note),
        };
        let exists = gitwork::branch_exists(&floor.path, id, branch)?;
        if !exists {
            return Ok(gone(format!(
                "The branch {branch} does not exist any more (it was merged, or the task was discarded)."
            )));
        }
        let diff = gitwork::task_diff(&floor.path, id, base_commit, branch)?;
        let (stat, stat_cut) = gitwork::cut_at(&diff.stat, DIFF_STAT_MAX_BYTES);
        let (patch, patch_cut) = gitwork::cut_at(&diff.patch, DIFF_PATCH_MAX_BYTES);
        let note = patch_cut.then(|| {
            format!(
                "The patch is cut at {DIFF_PATCH_MAX_BYTES} bytes ({} bytes in all). Read the files in the worktree for the rest.",
                diff.patch.len()
            )
        });
        Ok(TaskDiffView {
            branch: Some(branch.clone()),
            base_branch: task.base_branch.clone(),
            stat: stat.to_owned(),
            patch: patch.to_owned(),
            truncated: stat_cut || patch_cut,
            note,
        })
    }

    /// `task_merge` for an employee or the operator. `Err` is a refusal: nothing changed.
    fn merge_task_as(
        &self,
        floor: &Arc<FloorHandle>,
        team: &[Employee],
        actor: &Actor,
        id: i64,
    ) -> std::result::Result<MergeReport, String> {
        if !(actor.operator || actor.slug == "pm" || actor.slug == "ceo") {
            return Err("Only the pm, the ceo and the operator may merge a task. Tell the pm that the task is ready.".to_owned());
        }
        // One git action at a time on the main checkout.
        let git = locked(&floor.git);
        let (task, events) = self.read_task(floor, id).map_err(|e| e.to_string())?;
        if !matches!(task.status, TaskStatus::Review | TaskStatus::Blocked) {
            return Err(format!(
                "Task T-{id} has the status {}. Only a task in review or blocked can be merged.",
                task.status.as_str()
            ));
        }
        // A discarded task has no branch and no worktree any more. That is not "nothing to
        // merge": the work was thrown away, and the task must not become `done`.
        if task.branch.is_none()
            && events
                .iter()
                .any(|e| e.kind == "status" && e.text.as_deref() == Some(DISCARD_NOTE))
        {
            return Err(format!(
                "Task T-{id} was discarded by the operator: discarded, nothing to merge. The status stays {}.",
                task.status.as_str()
            ));
        }
        let outcome = self.merge_steps(floor, team, actor, &task)?;
        drop(git);
        self.notifier.tasks_changed(&floor.path);
        if !outcome.merged {
            self.notifier.notice(
                &floor.path,
                "task_blocked",
                &format!("Task T-{id} is blocked: {}", outcome.note),
            );
        }
        self.tell_about_task(
            floor,
            team,
            actor,
            &outcome.task,
            &outcome.note,
            merge_result_orders(outcome.merged, false),
        );
        let text = if outcome.merged {
            format!("Task T-{id}: {} The status is done.", outcome.note)
        } else {
            format!(
                "Task T-{id} is blocked and not merged. {} The status is blocked. The branch and the worktree stay as they are.",
                outcome.note
            )
        };
        Ok(MergeReport {
            merged: outcome.merged,
            text,
            task: outcome.task,
        })
    }

    /// The steps of a merge, with the git lock held. `Err` is a failure of the store (nothing was
    /// recorded). A step that does not pass is a `Stop` with the status `blocked`.
    fn merge_steps(
        &self,
        floor: &Arc<FloorHandle>,
        team: &[Employee],
        actor: &Actor,
        task: &TaskRecord,
    ) -> std::result::Result<Outcome, String> {
        let id = task.id;
        let finish = |status: TaskStatus,
                      note: &str,
                      git: GitAfter|
         -> std::result::Result<TaskRecord, String> {
            locked(&floor.store)
                .db()
                .finish_task(id, &actor.slug, status, note, git, now_ms())
                .map(|(_, after)| after)
                .map_err(task_error)
        };
        let block = |reason: String| -> std::result::Result<Outcome, String> {
            let after = finish(TaskStatus::Blocked, &reason, GitAfter::Keep)?;
            Ok(Outcome {
                merged: false,
                note: reason,
                task: after,
            })
        };

        // A task with no branch has nothing to merge.
        let (Some(branch), Some(base_branch)) = (&task.branch, &task.base_branch) else {
            let note = "There is nothing to merge: the task has no worktree and no branch. The work is in the project folder.";
            let after = finish(TaskStatus::Done, note, GitAfter::Keep)?;
            return Ok(Outcome {
                merged: true,
                note: note.to_owned(),
                task: after,
            });
        };

        // 0. The operator may be in the middle of a merge, a cherry-pick, a revert or a rebase.
        match gitwork::operation_in_progress(&floor.path) {
            Ok(None) => {}
            Ok(Some(what)) => return block(gitwork::operation_in_progress_text(what)),
            Err(error) => return block(format!("The app could not read the git state: {error}")),
        }

        // 1. Commit what the worktree has now.
        if let Some(relative) = &task.worktree {
            let path = gitwork::worktree_path(&floor.path, id, relative)?;
            if path.is_dir() {
                let assignee = task
                    .assignee
                    .as_deref()
                    .and_then(|slug| team.iter().find(|e| e.slug == slug));
                let author = gitwork::Author {
                    slug: assignee.map_or("employee", |e| e.slug.as_str()),
                    name: assignee.map_or("Employee", |e| e.name.as_str()),
                    role: assignee.map_or("Employee", |e| e.role.as_str()),
                };
                match gitwork::snapshot(&floor.path, &path, id, &task.title, &author) {
                    Ok(Snapshot::NoChanges | Snapshot::Committed { .. }) => {}
                    Ok(Snapshot::Blocked { reason, .. }) => return block(reason),
                    Err(error) => {
                        return block(format!(
                            "The app could not commit the changes of the worktree: {error}"
                        ));
                    }
                }
            }
        }

        // 2. The main checkout must be on the base branch.
        match gitwork::current_branch(&floor.path) {
            Some(current) if current == *base_branch => {}
            Some(current) => {
                return block(format!(
                    "The main checkout is on the branch {current}, and the task needs {base_branch}. The app never changes the branch of the main checkout. Switch it to {base_branch}, then merge again."
                ));
            }
            None => {
                return block(format!(
                    "The main checkout is not on a branch (detached HEAD), and the task needs {base_branch}. Switch the main checkout to {base_branch}, then merge again."
                ));
            }
        }

        // 3. No uncommitted change in tracked files outside .swarm/.
        match gitwork::changes_outside_swarm(&floor.path) {
            Ok(changed) if changed.is_empty() => {}
            Ok(changed) => {
                return block(format!(
                    "The main checkout has uncommitted changes in tracked files outside .swarm/: {}. The app does not merge over them. Commit or stash them, then merge again.",
                    file_list(&changed)
                ));
            }
            Err(error) => return block(format!("The app could not read the git status: {error}")),
        }

        // 4. The merge.
        let author = gitwork::Author {
            slug: &actor.slug,
            name: &actor.name,
            role: if actor.operator {
                "Operator"
            } else {
                "Manager"
            },
        };
        let merged = match gitwork::merge_branch(&floor.path, id, &task.title, branch, &author) {
            Ok(result) => result,
            Err(error) => return block(format!("The app could not run the merge: {error}")),
        };
        let (commit, new_commit) = match merged {
            MergeResult::Merged { commit, new_commit } => (commit, new_commit),
            MergeResult::Conflict { files } => {
                return block(format!(
                    "The merge has a conflict in: {}. The app stopped the merge, and the main checkout is as it was before.",
                    file_list(&files)
                ));
            }
            MergeResult::Failed { reason } => {
                return block(format!("git could not merge: {reason}"));
            }
        };

        // 5. Clean up. A failure here does not undo the merge.
        let mut note = if new_commit {
            format!(
                "Merged {branch} into {base_branch} (commit {}).",
                short(&commit)
            )
        } else {
            format!("{branch} had nothing new for {base_branch}: no merge commit was needed.")
        };
        let mut git_after = GitAfter::Keep;
        let removed = match &task.worktree {
            Some(relative) => gitwork::remove_worktree(&floor.path, id, relative),
            None => Ok(()),
        };
        match removed {
            Ok(()) => {
                git_after = GitAfter::WorktreeGone;
                note.push_str(" The worktree is removed.");
                match gitwork::delete_merged_branch(&floor.path, id, branch) {
                    Ok(()) => note.push_str(" The branch is removed."),
                    Err(error) => note.push_str(&format!(" The branch stays: {error}.")),
                }
            }
            Err(error) => {
                note.push_str(&format!(
                    " The worktree stays, because git refused to remove it: {error}. The branch stays too."
                ));
            }
        }
        let after = finish(TaskStatus::Done, &note, git_after)?;
        Ok(Outcome {
            merged: true,
            note,
            task: after,
        })
    }

    /// Tell the assignee and the assigner about an action of the app on a task (a merge, a
    /// discard). The caller of the action does not get a message, and neither does the operator.
    /// The message waits when the receiver has no live session: the app never starts a session for
    /// it (a start costs tokens).
    fn tell_about_task(
        &self,
        floor: &Arc<FloorHandle>,
        team: &[Employee],
        actor: &Actor,
        task: &TaskRecord,
        note: &str,
        orders: &str,
    ) {
        let mut receivers: Vec<&str> = Vec::new();
        for slug in [task.assignee.as_deref(), task.assigner.as_deref()]
            .into_iter()
            .flatten()
        {
            if slug != actor.slug
                && slug != OPERATOR
                && team.iter().any(|e| e.slug == slug)
                && !receivers.contains(&slug)
            {
                receivers.push(slug);
            }
        }
        for slug in receivers {
            let stored = locked(&floor.store).db().insert_task_message(
                task.id,
                &NewTaskMessage {
                    from_slug: actor.slug.clone(),
                    to_slug: slug.to_owned(),
                    text: status_body(task, Some(note)),
                    kind: KIND_TASK_STATUS,
                    orders: Some(orders.to_owned()),
                },
                now_ms(),
            );
            match stored {
                Ok(message_id) => {
                    let subject = format!("The status message for task T-{}", task.id);
                    let sent = Sent {
                        by_slug: &actor.slug,
                        by_name: &actor.name,
                        subject: &subject,
                        answer_tail: false,
                        slot: Slot::Free,
                    };
                    let _ = self.send_stored(floor, team, slug, message_id, false, &sent);
                }
                Err(error) => {
                    super::live::warn(&format!("cannot store a task message: {}", db_error(error)));
                }
            }
        }
    }
}

/// What the steps of a merge did.
struct Outcome {
    merged: bool,
    note: String,
    task: TaskRecord,
}

/// The text of `task_diff` for an agent.
fn diff_text(id: i64, diff: &TaskDiffView) -> String {
    let Some(branch) = &diff.branch else {
        return diff.note.clone().unwrap_or_default();
    };
    let mut text = format!(
        "Task T-{id}, branch {branch}, base {}. Only committed changes show: the app commits the worktree when the task goes to review.\n",
        diff.base_branch.as_deref().unwrap_or("unknown")
    );
    if let Some(note) = &diff.note
        && diff.patch.is_empty()
    {
        text.push_str(note);
        return text;
    }
    if diff.stat.trim().is_empty() && diff.patch.trim().is_empty() {
        text.push_str("There is no change on the branch yet.");
        return text;
    }
    text.push_str("\nDiffstat:\n");
    text.push_str(&diff.stat);
    text.push_str("\nPatch:\n");
    text.push_str(&diff.patch);
    if let Some(note) = &diff.note {
        text.push_str(&format!("\n[{note}]"));
    }
    text
}
