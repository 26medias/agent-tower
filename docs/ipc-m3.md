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
| `floor_permissions_get` | `path` | `string[]` | The rules of the floor, as stored in the app config (table `[floor_permissions]`, key = the real path of the floor). Empty when the operator set no list |
| `floor_permissions_set` | `path`, `allow` | `PermissionsSaved` | Checks each rule (below). One bad rule refuses the whole call and writes nothing. The text is trimmed and a duplicate is dropped. At most 100 rules. Writes the app config (not the project folder), and removes the old `[permissions]` table from `floor.toml` (the rest of the file stays). Returns the stored list and the warnings. The new rules apply at the next start of a session |
| `floor_permissions_proposed` | `path` | `string[]` | New (2026-11-21). The rules of the old `[permissions]` table of `floor.toml` that pass the check, with no duplicate. It is empty when the app config has an entry for the floor already (also an empty list), or when the file has no table. The app never imports these rules by itself, because an agent can have written them. The UI shows them in Settings with a button "Use these rules". The click calls `floor_permissions_set` with the list that the operator sees |
| `team_resume` | `path` | `ResumeProgress` | Resumes the employees of `FloorView.resumable` in a background thread, one by one, with a pause of 2 s between two starts. Returns at once with `{ done: 0, total: N }`. Errors: nothing to resume, or a resume runs already |

Errors that the commands give: `Task T-12 has the status todo. Only a task in review or blocked can
be merged.`, `Task T-12 is done and has no worktree. The app does not discard a done task.`,
`Task T-12 was discarded by the operator: discarded, nothing to merge. The status stays blocked.`, `Task T-12 was discarded by the operator: discarded, so you cannot set it to done. The status stays blocked.` (an employee with `task_update`; the operator can set `done`), `the rule "Bash(*)" is
refused: ...`, `there is no session of the last team to resume`, `the last team is resuming now`.

### Where the rules are (changed 2026-11-21)

```ts
interface PermissionsSaved {
  allow: string[];     // the stored rules, trimmed, with no duplicate
  warnings: string[];  // one text for each rule that runs the code of the project, and for each Bash rule with a *
}
```

`floor_permissions_set` returned `string[]` before. It returns `PermissionsSaved` now. The old
array is the field `allow`. A warning is `<rule>: <text>`. The rule is saved. The warning is for
the operator to read. The UI shows each warning next to the saved list. There are two texts, and a
rule gets one at most (the stronger one):

- For a build or package tool, and for a script of the project (also an exact rule):
  `This rule lets each agent of the floor run the project's own code with no question.`
- For each other `Bash` rule that has a `*` in it:
  `This rule lets each agent of the floor run this program with any arguments and no question. Many programs can write files or run other commands through their options.`

An exact `Bash` rule of another program (no `*`) has no warning: it allows only that command.

The rules are in the app config file (`programs.toml` in the config folder, mode 0600), in the
table `[floor_permissions]`. Each key is the real absolute path of a floor (symbolic links
resolved), and the value is the array of its rules:

```toml
[floor_permissions]
"/home/you/my project.v2" = ["Bash(rtk cargo test:*)", "Read(./src/**)"]
```

They are not in the project folder. A Codex employee, or any shell command that is allowed, can
write `floor.toml`, so a list in that file is a list that an agent can change (finding H1 of the
second review). The app does not read `[permissions]` of `floor.toml` for a session any more.
When a floor is opened and the app config has no entry for it, and `floor.toml` has the old table,
the app only offers the rules in `floor_permissions_proposed`. `floor_forget` removes the entry of
the floor. The setup session has no put-back guard any more.

### The check of a rule

Changed on 2026-11-21 (finding H1, twice). A rule is 1 to 200 characters. Each character is
printable ASCII (0x20 to 0x7E). The whole entry is one of:

- `WebSearch` alone (`WebSearch(...)` with a pattern is refused);
- `Tool(pattern)` with `Tool` one of `Bash`, `Read`, `Glob`, `Grep`, `WebFetch`. The pattern has no
  `(` and no `)`, so one entry is one rule (`Bash(ls) Bash(rm:*)` and `Bash(ls:*))` are refused).

**The check is not a security boundary. It is informed consent:** the operator must know what each
rule gives. A list of blocked programs can never be complete. Many programs write files or run
other commands through their options (`sort -o`, `rg --pre`, `fd -x`, `gh`, `psql`, `man -P`,
`less`, `patch`, and more), and a rule with a `*` lets the agent add any option. So the check does
two things:

1. It refuses what it can name: the programs and forms below. The list of blocked programs is a
   best effort, and it is not complete.
2. It gives a warning for each `Bash` rule that has a `*` in it (the texts are above). This
   warning is the real control. The list is the choice of the operator, and an agent cannot write
   it.

The pattern must not be empty, and it depends on the tool:

- `Bash`: the pattern does not start with a space. The first word (up to the first space or `:`)
  is not empty and has only `[A-Za-z0-9._/+-]`, so `Bash(*)`, `Bash(* *)`, `Bash(?*)`, `Bash(*.sh)`
  and `Bash(:*)` are refused.
  - `rtk` is unwrapped first: the check applies to the words after `rtk`. `Bash(rtk:*)`, `rtk`
    with an option after it (`rtk -v ...`), and `rtk` with `proxy`, `run`, `err`, `test`,
    `summary`, `pipe`, `rewrite`, `hook`, `init`, `trust`, `untrust` or `config` are refused: they
    run any command or they change the settings of `rtk`.
  - The program (the word after `rtk` when there is one) is not one of these, which run other
    commands, change or destroy data, or send data out: `sh`, `bash`, `zsh`, `dash`, `fish`,
    `ksh`, `csh`, `tcsh`, `pwsh`, `powershell`, `busybox`, `env`, `xargs`, `eval`, `exec`, `sudo`,
    `su`, `doas`, `pkexec`, `nohup`, `time`, `timeout`, `nice`, `setsid`, `stdbuf`, `flock`,
    `unshare`, `script`, `watch`, `parallel`, `strace`, `ltrace`, `gdb`, `command`, `builtin`,
    `source`, `.`, `python`, `python3`, `pypy`, `pypy3`, `node`, `deno`, `bun`, `lua`, `perl`,
    `ruby`, `php`, `find`, `awk`, `gawk`, `mawk`, `sed`, `tar`, `tee`, `cp`, `mv`, `ln`, `rm`,
    `dd`, `mkfs`, `shred`, `truncate`, `unlink`, `chmod`, `chown`, `rsync`, `curl`, `wget`, `ssh`,
    `scp`, `nc`, `ncat`, `socat`, `telnet`, `ftp`, `docker`, `podman`, `kubectl`, `vim`, `vi`,
    `nano`, `emacs`, `kill`, `pkill`, `systemctl`, `mount`, `umount`, `crontab`, `at`, and a few
    wrappers (`ssh-agent`, `sshpass`, `expect`, `chroot`, `nsenter`, `ionice`, `taskset`,
    `numactl`, `runuser`, `setpriv`, `start-stop-daemon`, `run-parts`, `systemd-run`, `screen`,
    `tmux`), and, as a best effort (added 2026-11-21): `sqlite3`, `psql`, `mysql`, `man`, `less`,
    `more`, `nvim`, `ex`, `ed`, `nodejs`, `tclsh`, `julia`, `Rscript`, `irb`, `ipython`,
    `osascript`, `xdg-open`, `uvx`, `pipx`, `nix-shell`, `nix`, `direnv`, `mise`, `chrt`,
    `prlimit`, `fakeroot`, `firejail`, `bwrap`, `valgrind`, `perf`, `hyperfine`, `install`,
    `patch`. A folder in front (`/bin/sh`) does not help, the case does not matter, and a
    versioned name is the same program (`python3.12`, `lua5.4`). The list is not complete.
  - `git` rules must be exact. A `git` rule (also after `rtk`) with a `*` is refused with this
    text: `A git rule must be the exact command, for example Bash(git status) or Bash(git diff --stat). A rule with * lets an agent add options such as --output, which writes files.`
    The reason: a rule with `*` lets the agent add `--output=<file>`, which writes a file, so no
    `git` subcommand is safe with a wildcard. The subcommand allow list stays for an exact rule:
    the word after `git` is one of `status`, `log`, `diff`, `show`, `blame`, `ls-files`,
    `rev-parse`, `describe`. Every other subcommand, no subcommand, and an option before the
    subcommand are refused. An exact rule that has `--output`, `--exec`, `--ext-diff`,
    `--textconv` or ` -c ` is refused too. `Bash(git status)` and `Bash(git diff --stat)` are
    allowed, with no warning. `Bash(git status:*)`, `Bash(git log:*)`, `Bash(rtk git log:*)`,
    `Bash(git diff *)`, `Bash(git:*)`, `Bash(git push:*)` and `Bash(git log --output=x)` are refused.
  - The build and package tools are allowed, with the warning about the code of the project:
    `cargo`, `npm`, `npx`, `pnpm`, `yarn`, `make`, `just`, `uv`, `pip`, `go`, `gradle`, `mvn`,
    `pytest`, `tox`, `bundle`, `rake`, `composer`, `dotnet`, `cmake`, `ninja`, `bazel`, `poetry`,
    `meson`, `scons`, also after `rtk`, also with a versioned name (`pip3`), and also for an exact
    rule (`Bash(cargo fmt --check)`). A program that a relative path names
    (`./scripts/check.sh`) gets the warning too. They run the code of the project.
  - Each other program is allowed. With a `*` in the rule it gets the wildcard warning
    (`Bash(ls:*)`, `Bash(sort:*)`, `Bash(gh:*)`, `Bash(rg:*)`, `Bash(jq:*)`). With no `*` it gets
    no warning (`Bash(rtk git status)`, `Bash(cat README.md)`).
- `Read`, `Glob`, `Grep`: a path that is relative to the project (not `/...`, not `~...`) with no
  `..` part. The bare names `Read`, `Glob` and `Grep` are refused: they open the whole disk. A
  pattern that starts with `.swarm/state` or `./.swarm/state`, a pattern that has
  `**/.swarm/state`, and `.swarm/**` or `.swarm/*` are refused: that folder has the database and
  the logs of the floor.
- `WebFetch`: only `domain:<host>`, with a host of `[A-Za-z0-9.-]` that has a dot and no empty
  part, so `WebFetch(*)` and `WebFetch(domain:*)` are refused. An IP address (also `127.1` and
  `0x7f.0.0.1`: a host whose last part starts with a digit), `localhost`, and a host that ends
  with `.localhost`, `.local`, `.internal` or `.localdomain` are refused.

A rule must match the command as Claude sees it after the hooks of the operator. On a machine with
a hook that rewrites commands (for example RTK), `cargo test` becomes `rtk cargo test`, so the rule
is `Bash(rtk cargo test:*)`. The UI repeats the check (`ui/src/lib/permissions.ts`) and the Rust
side checks again (`crates/core/src/model.rs`, `check_allow_rule`). A rule in the app config that
fails the check (the check can change in a new version, or someone edited the file by hand) is
dropped when a session starts. The error text of a refusal names the reason.

What stays open (known limits). The build and package tools run the code of the project, and an
agent of the floor can write the code of its own worktree. So a rule for them lets that agent run
code with no question. The warning says this. The app config is outside the project, but a Codex
employee has no enforced limits on this machine, so it can write the file if it tries. The list of
blocked programs is not complete, and it cannot be: the warning of each wildcard rule is the
control. The check does not resolve a host name, so `WebFetch(domain:127.0.0.1.nip.io)` passes.

Known limit: the rules of a floor are stored by the path of the floor. If another floor takes that
path (for example a folder is moved away and another folder is renamed to the same path), that
floor gets those rules.

## Events

| Event | Payload | When |
|---|---|---|
| `notice` | `{ path, kind, text }` | Something that the operator must see. `kind` is `task_blocked` (a merge that the app could not do: `text` is `Task T-12 is blocked: <reason>`), `resume_failed` (`text` is `Could not resume <slug>: <error>`) or `task_done_unmerged` (new, 2026-11-21: the operator set `done` on a task that has a branch. `text` is `Task T-12 is set to done, but its branch <branch> is not merged. The app did not merge it. Use the merge on the board to merge it.`). The UI shows a toast. The kind `permissions_restored` is removed (2026-11-21): the rules are not in the project folder any more, so the setup agent cannot change them |

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
