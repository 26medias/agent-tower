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

Repair of 2026-11-21 (review findings): no field and no command is new or renamed. These values
change:

- `TaskView.worktree` can be `.swarm/worktrees/T-12-2` (`-3`, and so on, up to `-50`) when the
  folder `T-12` exists and git does not know it as a worktree of this repository (a leftover).
- `task_update` (the operator, `status: "done"`) on a task that has a branch: the command works,
  and the app also sends a `notice` of the kind `task_done_unmerged` (see "Events").
- `task_discard` also accepts a `done` task when its worktree still exists.

## Commands

| Command | Arguments | Result | Notes |
|---|---|---|---|
| `task_merge` | `path`, `id` | `TaskMerge` | The operator merges the branch of a task. The task must be in `review` or `blocked`, else the command is an error. A merge that the app could not do (another branch in the main checkout, uncommitted changes, a conflict) is a result with `merged: false`: the task is `blocked` and `message` says why. A task with no branch gets the status `done` with the message "There is nothing to merge" |
| `task_diff` | `path`, `id` | `TaskDiff` | Only committed changes show. The app commits the worktree when the task goes to `review` |
| `task_discard` | `path`, `id` | `TaskView` | For a task that is not `done` (or that is `done`, and its worktree still exists). The app removes the worktree (`git worktree remove --force`) and the branch (`git branch -D`) of that one task, and sets the status `blocked` with the note "discarded by the operator". The UI asks for a confirmation first |
| `floor_permissions_get` | `path` | `string[]` | The rules of `[permissions] allow` of `floor.toml`, as stored |
| `floor_permissions_set` | `path`, `allow` | `string[]` | Checks each rule (below). One bad rule refuses the whole call and writes nothing. The text is trimmed and a duplicate is dropped. At most 100 rules. Returns the stored list. The new rules apply at the next start of a session |
| `team_resume` | `path` | `ResumeProgress` | Resumes the employees of `FloorView.resumable` in a background thread, one by one, with a pause of 2 s between two starts. Returns at once with `{ done: 0, total: N }`. Errors: nothing to resume, or a resume runs already |

Errors that the commands give: `Task T-12 has the status todo. Only a task in review or blocked can
be merged.`, `Task T-12 is done and has no worktree. The app does not discard a done task.`,
`Task T-12 was discarded by the operator: discarded, nothing to merge. The status stays blocked.`, `the rule "Bash(*)" is
refused: ...`, `there is no session of the last team to resume`, `the last team is resuming now`.

### The check of a rule

Changed on 2026-11-21 (finding H1). A rule is 1 to 200 characters. Each character is printable
ASCII (0x20 to 0x7E). The whole entry is one of:

- `WebSearch` alone (`WebSearch(...)` with a pattern is refused);
- `Tool(pattern)` with `Tool` one of `Bash`, `Read`, `Glob`, `Grep`, `WebFetch`. The pattern has no
  `(` and no `)`, so one entry is one rule (`Bash(ls) Bash(rm:*)` and `Bash(ls:*))` are refused).

The pattern must not be empty, and it depends on the tool:

- `Bash`: the pattern does not start with a space. The first word (up to the first space or `:`)
  is not empty and has only `[A-Za-z0-9._/+-]`, so `Bash(*)`, `Bash(* *)`, `Bash(?*)`, `Bash(*.sh)`
  and `Bash(:*)` are refused. The first word is not a program that runs other commands or
  destroys data or sends data out: `sh`, `bash`, `zsh`, `dash`, `fish`, `env`, `xargs`, `eval`,
  `exec`, `sudo`, `su`, `doas`, `nohup`, `time`, `timeout`, `nice`, `python`, `python3` (and
  `python3.12` and so on), `node`, `perl`, `ruby`, `php`, `rm`, `dd`, `mkfs`, `chmod`, `chown`,
  `curl`, `wget`, `ssh`, `scp`, `nc` (a folder in front, like `/bin/sh`, does not help; the case
  does not matter). `git` needs a subcommand that starts with a lower-case letter, and it is not
  `push`, `reset`, `clean`, `checkout` or `config`: `Bash(git status:*)` is allowed, `Bash(git:*)`
  and `Bash(git push:*)` are refused.
- `Read`, `Glob`, `Grep`: a path that is relative to the project (not `/...`, not `~...`) with no
  `..` part. The bare names `Read`, `Glob` and `Grep` are refused now: they open the whole disk.
- `WebFetch`: only `domain:<host>`, with a host of `[A-Za-z0-9.-]` that has a dot and no empty
  part, so `WebFetch(*)` and `WebFetch(domain:*)` are refused.

A rule must match the command as Claude sees it after the hooks of the operator. On a machine with
a hook that rewrites commands (for example RTK), `cargo test` becomes `rtk cargo test`, so the rule
is `Bash(rtk cargo test:*)`. The UI repeats the check (`ui/src/lib/permissions.ts`) and the Rust
side checks again (`crates/core/src/model.rs`, `check_allow_rule`). A rule that a hand edit of
`floor.toml` puts in the file and that fails the check is dropped when a session starts. The
error text of a refusal names the reason.

The setup session can write `floor.toml` (it runs with `acceptEdits`). So the app keeps the value
of `[permissions]` that it read when the setup session started. After each turn of the setup
session (and at its stop), if the table in the file is not that value, the app writes the old
value back and sends the `notice` `permissions_restored`. A change that the operator makes with
`floor_permissions_set` while the setup session runs is the new value.

## Events

| Event | Payload | When |
|---|---|---|
| `notice` | `{ path, kind, text }` | Something that the operator must see. `kind` is `task_blocked` (a merge that the app could not do: `text` is `Task T-12 is blocked: <reason>`), `resume_failed` (`text` is `Could not resume <slug>: <error>`), `permissions_restored` (new, 2026-11-21: `text` is `The setup agent tried to change the commands with no question. The app put the old list back.`) or `task_done_unmerged` (new, 2026-11-21: the operator set `done` on a task that has a branch. `text` is `Task T-12 is set to done, but its branch <branch> is not merged. The app did not merge it. Use the merge on the board to merge it.`). The UI shows a toast |

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
- The snapshot does not commit everything (2026-11-21, finding M5). After `git add -A` the app
  reads the staged files. It does not commit when one of these is staged: a file over 5 MB; more
  than 2000 files; a nested repository (a git link, mode 160000); a NEW file in a folder named
  `target`, `node_modules`, `dist`, `build`, `.venv` or `__pycache__`; a NEW file named `.env`,
  `.env.*` (not `.env.example`), `*.pem`, `*.key`, `id_rsa*`, `id_ed25519*`, `*.p12` or `*.pfx`.
  (A file that the repository tracks already is not blocked for its folder or its name.) Then the
  app empties the index of the worktree (`git reset -q`, never `--hard`, no file changes), makes no
  commit, and sets the task to `blocked`. The text lists the paths and orders the agent to add an
  ignore rule in `.gitignore` or to remove the files. At `review` the caller gets this as an error
  result. At `task_merge` it is the reason of the block.
- Before the app runs a git command in a worktree, it checks that `git rev-parse
  --absolute-git-dir` there gives the git dir that git made for that worktree
  (`<common git dir>/worktrees/<name>`). The agent can write the `.git` file of its worktree; a
  changed file is refused with the text `The git link of the worktree ... is not the one that git
  made for it`. Then the commands name that git dir (`--git-dir`, `--work-tree`).
- Every git command of the app has `-c core.fsmonitor=false`, and no variable that starts with
  `GIT_` (and no `PAGER`) comes from the parent process.
- Only `task_merge` sets `done` for a task that has a branch. `task_update` from an employee with
  `status: "done"` on such a task is an error: `Task T-12 has a branch, so you cannot set it to
  done. Set it to review. ...` (the assignee) or `Task T-12 has a branch: use task_merge. ...`
  (the others). The operator can still set any status.
- `task_merge` (the `pm`, the `ceo`, the operator): block the task (status `blocked`, nothing
  changed) when the main checkout is in the middle of a merge, a cherry-pick, a revert or a rebase
  that the operator started (`MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `rebase-merge`,
  `rebase-apply`): the app never ends such an operation. `git merge --abort` runs only for a merge
  that this call started. A discarded task answers `discarded, nothing to merge` and does not
  become `done`. Then: commit the worktree, check that the main
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

The list is the table `resume_marks` of the floor database. Changed on 2026-11-21 (finding M7):
a session of an employee puts its mark when it starts (so a crash of the app keeps the list; a
session that never had a turn cannot resume, and `FloorView.resumable` drops it). A session that
the operator stops with `session_stop` leaves the list. A session that ends by itself (the CLI
exits) leaves the list, unless the app is closing. The setup session never has a mark.
`shutdown` still replaces the list with the sessions that are live, BEFORE it kills them. A floor
with no live session at the close keeps its old list. Nothing resumes by itself at the start of the
app.

## Limits on task messages

At most 60 task messages (`task_assign` and the status messages) for one ordered pair of employees
(sender, receiver) in a rolling hour (`TASK_MESSAGE_LIMIT` in `crates/core/src/store.rs`). Over the
limit, `task_assign` and `task_update` are errors that say `Task message limit: ... Stop now. ...
Wait for the operator.`, and nothing is changed. The messages of the operator do not count. They do
not count for the limit of 30 plain messages of `send`, either.

## Mock

`ui/src/lib/mock.ts` implements these commands in memory, with the same argument names and the
same error texts, and it emits `tasks-changed`, `floor-changed` and `notice`. `__towerMock` has
`taskWorktree`, `blockMerge`, `setResumable` and `failResume` for the e2e check.
