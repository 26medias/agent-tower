# M3 contract: worktrees, merge, commands with no question, restore of the team

This file is the contract for milestone M3 (changed 2026-11-21). It adds to `docs/ipc-m1.md`,
`docs/ipc-settings.md` and `docs/ipc-m2.md`. The conventions of those files stay: Tauri commands
(`invoke`), snake_case command names, JSON with camelCase field names, and a failed command
returns a string that a person can read.

## Changes to the types

```ts
interface TaskView {                 // docs/ipc-m2.md, with four new fields
    // ... the fields of M2 ...
    branch: string | null;           // "task/T-12-add-login". null when the task has no git branch
    baseBranch: string | null;       // the branch of the main checkout when the worktree was made ("main")
    baseCommit: string | null;       // the commit of the main checkout at that moment (40 hex digits)
    worktree: string | null;         // relative to the floor folder: ".swarm/worktrees/T-12". null when there is none now
}

type TaskEventKind = "created" | "assigned" | "status" | "comment" | "report" | "worktree";
// "worktree": the app made the branch. value = the branch, text = "Branch ... from main at <12 digits>."

interface ResumeProgress { done: number; total: number }  // done counts the starts that were tried

interface FloorView {                // docs/ipc-m1.md, with two new fields
    // ... the fields of M1 and of the settings contract ...
    resumable: string[];             // slugs that were live at the last close, have no live session now, and can resume. Managers first
    resumeProgress: ResumeProgress | null;  // the background resume of the last team. null when none runs
}

interface TaskMerge { merged: boolean; message: string; task: TaskView }
interface TaskDiff {
    branch: string | null;           // null: the task has no branch
    baseBranch: string | null;
    stat: string;                    // git diff --stat, at most 20000 bytes
    patch: string;                   // git diff, at most 60000 bytes
    truncated: boolean;              // the stat or the patch was cut
    note: string | null;             // no branch, branch gone, or the patch was cut
}
```

A task has a branch only when the app made a worktree for it (see "Worktrees" below). After a good
merge, `worktree` is `null` and the other three fields stay, as a record. After a discard, all four
are `null`.

## Commands

| Command | Arguments | Result | Notes |
|---|---|---|---|
| `task_merge` | `path`, `id` | `TaskMerge` | The operator merges the branch of a task. The task must be in `review` or `blocked`, else the command is an error. A merge that the app could not do (another branch in the main checkout, uncommitted changes, a conflict) is a result with `merged: false`: the task is `blocked` and `message` says why. A task with no branch gets the status `done` with the message "There is nothing to merge" |
| `task_diff` | `path`, `id` | `TaskDiff` | Only committed changes show. The app commits the worktree when the task goes to `review` |
| `task_discard` | `path`, `id` | `TaskView` | For a task that is not `done`. The app removes the worktree (`git worktree remove --force`) and the branch (`git branch -D`) of that one task, and sets the status `blocked` with the note "discarded by the operator". The UI asks for a confirmation first |
| `floor_permissions_get` | `path` | `string[]` | The rules of `[permissions] allow` of `floor.toml`, as stored |
| `floor_permissions_set` | `path`, `allow` | `string[]` | Checks each rule (below). One bad rule refuses the whole call and writes nothing. The text is trimmed and a duplicate is dropped. At most 100 rules. Returns the stored list. The new rules apply at the next start of a session |
| `team_resume` | `path` | `ResumeProgress` | Resumes the employees of `FloorView.resumable` in a background thread, one by one, with a pause of 2 s between two starts. Returns at once with `{ done: 0, total: N }`. Errors: nothing to resume, or a resume runs already |

Errors that the commands give: `Task T-12 has the status todo. Only a task in review or blocked can
be merged.`, `Task T-12 is done. The app does not discard a done task.`, `the rule "Bash(*)" is
refused: ...`, `there is no session of the last team to resume`, `the last team is resuming now`.

### The check of a rule

A rule is 1 to 200 characters with no control character, and it is one of:

- `Read`, `Glob` or `Grep` alone;
- `Tool(pattern)` with `Tool` one of `Bash`, `Read`, `Glob`, `Grep`, `WebFetch`, `WebSearch`.

The pattern must not be empty. `Bash` alone, `WebFetch` alone and `WebSearch` alone are refused. A
`Bash` pattern that has only `*` and `:` is refused (`Bash(*)`, `Bash(:*)`, `Bash(**)`): it would
allow every shell command. The UI repeats this check (`ui/src/lib/permissions.ts`) and the Rust
side checks again (`crates/core/src/model.rs`, `check_allow_rule`). A rule that a hand edit of
`floor.toml` puts in the file and that fails the check is dropped when a session starts.

## Events

| Event | Payload | When |
|---|---|---|
| `notice` | `{ path, kind, text }` | Something that the operator must see. `kind` is `task_blocked` (a merge that the app could not do: `text` is `Task T-12 is blocked: <reason>`) or `resume_failed` (`text` is `Could not resume <slug>: <error>`). The UI shows a toast |

`tasks-changed` follows each merge, discard and worktree. `floor-changed` follows each start of the
resume, so that `resumeProgress` and `resumable` stay current.

## Worktrees (spec 4.8)

- The session of an employee stays in the main checkout (the floor folder). The worktree of a task
  is `{floor}/.swarm/worktrees/T-<id>/`, inside the floor folder, so the session can reach it.
- `task_assign` makes the worktree and the branch `task/T-<id>-<slug>` when: the assignee has the
  policy `edit_in_worktree`, the floor folder is the top of a git repository, the repository has a
  commit, and the main checkout is on a branch. The slug uses `[a-z0-9-]` only (at most 40
  characters). The base is the branch and the commit that the main checkout has at that moment.
- Otherwise there is no worktree. The task message says that the employee edits the project folder
  directly, and `task_merge` only sets `done`.
- When the status goes to `review`, the app runs `git add -A` and `git commit` in the worktree. The
  message is `T-12: <title>` and a line `By: <Name> (<Role>)`. If the repository has no user, the
  commit gets the name of the employee and the e-mail `<slug>@agent-tower.local`; else it keeps the
  identity of the operator. No change: no commit, and the note of the status event says so.
- `task_merge` (the `pm`, the `ceo`, the operator): commit the worktree, check that the main
  checkout is on the base branch, check that `git status --porcelain --untracked-files=no` shows no
  change outside `.swarm/`, `git merge --no-ff`, then `git worktree remove` and `git branch -d`.
  A conflict stops the merge with `git merge --abort`. The app never uses `--force` (except
  `task_discard`, on one task worktree), `reset`, `checkout` in the main checkout, or `push`.
- Each git command runs with an explicit working folder, with `-c core.hooksPath=/dev/null` where
  a hook could run (`worktree add`, `commit`, `merge`), with a time limit, and with a fixed list of
  arguments. A repository hook does not run during an action of the app.

## MCP tools (spec 4.4)

| Tool | Who | Result |
|---|---|---|
| `task_diff(task)` | any employee of the floor | The diffstat and the patch (cut at 60000 bytes) |
| `task_merge(task)` | the `pm`, the `ceo` | The text of the result. A merge that did not happen is an error result, and the task is `blocked` |

The assignment message of a task with a worktree carries these orders: the absolute worktree path
and the branch, "make all your changes inside this folder", "do not commit", and the absolute path
of the report file in the reports folder of the MAIN checkout. The status message that tells a
manager about a `review` has the merge steps. The merge result goes as a task message to the
assignee and the assigner (not to the caller, and not to the operator). The app never starts a
session for it: the message waits when the receiver has no live session.

## The restore of the last team

`shutdown` stores the employees that have a live session (table `resume_marks` of the floor
database) BEFORE it kills the sessions. A floor with no live session at the close keeps its old
list. A session that the operator stops with `session_stop` leaves the list (also when the app
closes a moment later). Nothing resumes by itself at the start of the app.

## Mock

`ui/src/lib/mock.ts` implements these commands in memory, with the same argument names and the
same error texts, and it emits `tasks-changed`, `floor-changed` and `notice`. `__towerMock` has
`taskWorktree`, `blockMerge`, `setResumable` and `failResume` for the e2e check.
