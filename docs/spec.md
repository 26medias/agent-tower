# Agent Tower — product spec (draft 1)

Status: draft 3, after gate 1 and one check of the repairs (probes in `probes/`). Working name: "Agent Tower".
Date: 2026-11-19.

Labels in this document:
- **VERIFIED** = checked on this machine (flags: Claude Code 2.1.326, codex-cli 0.167.2).
- **PROBED** = a gate 1 probe ran it on this machine (Claude Code 2.1.329, codex-cli 0.170.0). Scripts and logs are in `probes/`.
- **NOT TRACED** = not checked. Do not build on it before milestone M0 proves it.

## 1. Purpose

One desktop app starts and manages many agent CLI sessions (Claude Code, Codex CLI, others later).
It automates the relay work that the operator does by hand today:

- A specs agent writes a milestone.
- A milestone agent does the work and gives parts of it to helper agents.
- The operator copies prompts to the helpers and copies results back.
- The operator tells the specs agent that the milestone is done.

The app replaces the copy steps with a hierarchy of agents, a message bus and a task board.
The app also shows the work as a building with offices, because it must be fun to use.

Not a goal: to replace the agent CLIs. The app is a manager on top of them.

## 2. Decisions made by the operator

| # | Decision | Choice |
|---|---|---|
| D1 | App form | New standalone desktop app. Rust. |
| D2 | Session mode | The real CLI runs in a pseudo-terminal (PTY). The operator can type in it. |
| D3 | Autonomy | A permission policy for each role. Risky actions wait for the operator. |
| D4 | Work folders | One git worktree and one branch for each task. |

## 3. Terms

| Term | Meaning |
|---|---|
| Building | The full workspace of the operator. It holds the floors. It gives them nothing in common. |
| Floor | One project, shown as one company. A floor has a name, a working folder (CWD), a logo, an office image, its employees, its task board and its messages. |
| Employee | One agent on a floor. It has a name, a role, a manager, a CLI, a model, a reasoning effort and a permission policy. |
| Role | A set of goals and functions. Fixed roles: CEO, Project Manager. Other roles are free (Engineer, Reviewer, Specs Writer, ...). |
| Session | One running CLI process of one employee, in a PTY. An employee has zero or one live session. |
| Manager | The employee that another employee answers to. The CEO answers to the operator. |
| Task | One unit of work on the board of a floor. |
| Bus | The message system between employees. |

Out of scope as "employees": the internal subagents of a CLI (for example `.claude/agents/*`).
They stay internal to their CLI. The app does not show or manage them.

## 4. Behaviour

### 4.0 Floors

- The operator can create a floor at any time. The operator gives a working folder (CWD).
- All data of a floor is in `{CWD}/.swarm/` (layout in 4.9). One CWD has one floor.
- **If `{CWD}/.swarm/` exists:** the app imports its employees and settings. No setup step. If the
  `version` in `floor.toml` is newer than the app knows, the app refuses the import and says why.
- **If `{CWD}/.swarm/` does not exist:** the app starts the floor setup (4.0.1).
- The fixed roles (CEO, Project Manager) are always in a floor. The operator can add, change and remove the other employees at any time.
- All sessions of a floor start in the CWD of the floor by default. The one exception is a task worktree (4.8).
- If the CWD is not a git repository, the floor has no worktrees and no `task_merge`. All employees work in the CWD.
- **Isolation rule (hard):** each floor is a separate company. Floors do not share employees, tasks,
  messages, reports or sessions. No MCP tool can read or write the data of a different floor. The
  identity token of a session fixes its floor, and the server checks the floor on each call.
  Messages between floors are never possible, in this version and in later versions.

#### 4.0.1 Floor setup by a setup agent

A CLI agent does the setup, not a form. The operator selects Claude or Codex for it.

1. The app starts one session in the CWD: the **setup agent**. It is not an employee. It shows in a terminal tab.
2. The setup agent gets the **floor-init skill**: a text that the app owns. It says what to do, in which order, and which tools to use.
   The app gives it through the instructions mechanism of the CLI (4.6). A package as a native Claude skill is an option, NOT TRACED.
3. The setup agent reads `CLAUDE.md` and `AGENTS.md` in the CWD (and the README and the folder layout if they are not there) to understand the project.
4. It tells the operator what it understood. It suggests a floor name and a list of employees: role, name, manager, CLI, model, reasoning effort, permission policy, and a role text for each.
5. It asks the operator questions and changes the list until the operator agrees.
6. It writes the result with setup tools of the MCP server: `floor_configure(name, ...)`, `employee_define(...)`, `floor_setup_complete()`.
   The app validates each call and writes the files in `.swarm/`. The setup agent does not write in `.swarm/` directly.
7. On `floor_setup_complete()`, the app makes the images (logo, office, icons), stops the setup session and opens the floor with the CEO.

The setup tools exist only for the setup session. Employee sessions do not get them.
The operator can cancel the setup. Then the app writes no `.swarm/` folder.

### 4.1 Hierarchy and flow

1. The operator talks to the CEO of a floor by default. The operator can also open any session tab and type in it.
2. The CEO sends each request to the correct employee. For a feature request, that is the Project Manager.
3. The Project Manager creates tasks, assigns them, and follows them to completion.
4. An assigned employee does the task and reports the result to its manager through the bus and the task status.
5. The manager reports up. The CEO tells the operator.

Scope of the CEO: one CEO for each floor in version 1. A building-level CEO is a later option.

### 4.2 Session lifecycle

An employee exists in the database when it has no session. A session is started on demand.

| Action | Who can do it | Result |
|---|---|---|
| Start | The manager of the employee (through an MCP tool), or the operator | A new CLI process starts in a PTY with the role instructions and the MCP configuration. |
| Continue | The same | The message goes to the live session. If the session stopped and had one turn or more, the app resumes it by its CLI session ID. A session with no turn cannot be resumed (the CLI has no conversation); the app starts a fresh one (`claude --resume <id>` VERIFIED flag; `codex resume <id>` VERIFIED subcommand). |
| Stop | The manager, or the operator | The process stops. The transcript stays. |
| Fresh start | The manager, or the operator | Stop, then start with a new CLI session and an empty context. |

Rule: only the direct manager chain and the operator can start or stop an employee.

Start rules (from the gate 1 probes; the parts that are not probed are marked):
- **Clean environment.** The app builds the environment of each CLI process. A session gets the environment of the login
  shell of the operator, so that `cargo`, `pnpm`, `NVM_DIR`, `JAVA_HOME` and a proxy work as in a terminal. The operator asked for this
  rule on 2026-11-20. The app runs `$SHELL -lic` one time when it starts (15 s limit), lays the result over its own environment
  (a name from the shell wins), and copies all variables to the session. If the capture fails, the session gets the environment of the app.
  These names stay out: `CLAUDECODE` and `CLAUDE_CODE_*`; `PWD`, `OLDPWD`, `SHLVL`, `_`; `TOWER_*`, `BASH_FUNC_*` and `TAURI_*`;
  and `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_BASE_URL`, `OPENAI_API_KEY`, `OPENAI_BASE_URL` (a key in the environment changes
  how the CLI logs in and who pays). `PATH` is built: the folders of the programs that the app found, then the `PATH` of the parent, then `/usr/bin`
  and `/bin`. `TERM` is `xterm-256color` when the parent has none. An inherited `CLAUDE_CODE_CHILD_SESSION` turns transcript saving off, and then resume has nothing to resume.
- **Working folder and programs.** The app always sets the working folder of the process (the PTY library starts in `$HOME` by default, PROBED at gate 2).
  The app starts `claude`, `codex`, `node` and its hook program by absolute path, because a desktop PATH does not contain them (PROBED at gate 2).
- **Session ID.** Claude: the app makes a UUID and gives it with `--session-id`. Codex: no flag sets the ID. The app reads it
  from the `SessionStart` hook, which fires only at the first prompt. A Codex session with no prompt cannot be resumed; the app starts it fresh.
  The app stores the ID when it gets it, not at the end of the turn.
- **Explicit permission mode.** The app always gives the permission mode. With no flag, Claude takes the mode from the user settings of the operator.
- **First message.** Each session start has a first message. The app gives it as the `[PROMPT]` argument of the CLI, not by typing
  (PROBED at step 0 for both CLIs: the prompt runs after the operator answers a startup dialog). If the operator starts a session with no text, the app uses a short default message of the role.
- **Startup dialogs.** A dialog at start (folder trust, Codex "Hooks need review", Codex "Update available") blocks the session, and no hook fires before it.
  Claude shows no trust dialog in a git worktree of a trusted repository. Codex gets `-c 'projects={"<path>"={trust_level="trusted"}}'`
  (PROBED) and `-c check_for_update_on_startup=false` (PROBED at step 0: with the flag, no update dialog shows). The app never types into a session that is in `starting`.
  A session that stays in `starting` for more than 20 s gets the status `question`, so the operator looks at its tab.
- **Codex daemon.** The app starts Codex with `--no-daemon` (PROBED: it works). A shared daemon of a different version (0.171.0) runs on this machine. If the TUI uses it without the flag, and if `-c` settings apply there, is NOT TRACED.
- **Codex hook trust.** Codex asks the operator to trust each hook command one time. The trust is lost if the command text changes.
  Thus the hook commands are the same text for all sessions, and the identity of the employee goes in the environment only.
- **App crash.** When the app stops, the PTYs close and all CLI processes stop. After a restart, the sessions show as `stopped` and can be resumed.

### 4.3 Status

Each session has one status. The office view and the tabs show it.

| Status | Meaning |
|---|---|
| `starting` | Process started, CLI not ready |
| `working` | A turn is in progress |
| `idle` | Turn ended, nothing waits |
| `question` | The agent waits for a person: a permission prompt, a startup dialog, or the loop limit |
| `done` | The agent set its task to `review` or `done`, and its turn ended |
| `stopped` | No live process |

The source is the hooks of the CLI (PROBED for both CLIs). Claude gets them with `--settings <json>`.
Codex gets them with `-c 'hooks.<Event>=[...]'`. Each hook sends its JSON payload to the app on a local socket.

One state machine for each session. The last event wins. "Last event" means the last event that the table below maps; ignored events do not count. The app does not count submits against stops
(Codex steering gives 2 submits and 1 stop; a Claude queued message starts a turn with no new submit).

| Event | Claude | Codex | New status |
|---|---|---|---|
| `SessionStart` | fires when the CLI is ready | fires only at the first prompt | Claude: `idle`. Codex: stays `starting` until this first hook event, then `working` |
| `UserPromptSubmit`, `PreToolUse`, `PostToolUse` | yes | yes | `working` |
| `PermissionRequest` | yes, 40 ms after `PreToolUse` | yes | `question` |
| `Stop` | yes | yes | `idle` (or `done`) |
| `Interrupt` | does not exist | yes, on Esc | `idle` |
| `SessionEnd` or process exit | yes | yes (PROBED at step 0) | `stopped` |

Special cases:
- **Codex start.** A Codex session leaves `starting` only on its first hook event. Screen output is not a signal, because a startup dialog
  also draws output and then waits. A paste into such a dialog selects its default option (for example "Update now"). So a Codex session
  with no prompt yet never gets a paste; its first message is the `[PROMPT]` argument.
- **Claude interrupt.** Esc fires no hook (PROBED). The app sees the keys of the operator. Rule (PROBED at step 0): Esc or Ctrl-C in a `working` or `question` session, then no hook event and 2 s with no PTY output, gives `idle`. The output stopped 0.03 to 0.14 s after the key in each probe; a second Ctrl-C adds one redraw at 0.8 s. Two Ctrl-C at the idle prompt in 0.8 s end the process, and `SessionEnd` gives `stopped`.
- **Claude queued turn (PROBED).** A message that is submitted during a turn starts a second turn after the `Stop`, with no event at its start.
  Claude adapter rule: a `UserPromptSubmit` that comes while the status is `working` sets a "queued turn" mark. The next `Stop` keeps `working` and clears the mark. The `Stop` after it gives `idle`.
- **`Notification` (Claude).** Used only when `notification_type` is `permission_prompt`, as a backup for `PermissionRequest` (it comes 6 s later, PROBED),
  and only if `PreToolUse` is still the last event of the session. Thus a late notification cannot set `question` again after the operator answered. The type `idle_prompt` is ignored.
- **`SubagentStop` (Claude)** is ignored. It fires for internal agents on most turns.
- **Permission denied (PROBED at step 0).**
    - Codex: a deny (the "No" option or Esc) fires `Interrupt` in less than 0.1 s. `Interrupt` gives `idle`. No other rule is necessary.
    - Claude: a deny fires no hook. No `Stop` comes, and the late `Notification` is cancelled. The turn ends and the CLI waits for the operator.
      Rule: in `question`, an operator key starts a watch. If a hook event comes (`PostToolUse` or `PreToolUse` after an approval), that event sets the status.
      If no hook event comes and the PTY has no output for 2 s, the status becomes `idle`.
      If no hook event comes and the PTY output continues 2 s after an Enter or a digit key, the status becomes `working`: the operator approved, and the tool runs
      (PROBED: an approved long command fires no hook until it ends). A wrong `working` is safe, because the app pastes only in `idle` and `done`.
      This works because the permission prompt draws output each 0.6 s, a working turn each 0.1 s, and the idle prompt draws nothing.
- **Startup dialogs have unsafe defaults (PROBED at step 0).** Enter on the Claude trust dialog selects "No, exit". Enter on the Codex update dialog selects "Update now".
  This is one more reason for the rule that the app never types into `starting`.
- **Stop that the app blocked (M2).** When the Stop hook of the app returns `block` to deliver messages (4.5), the turn continues with no new event. That `Stop` keeps `working`. The next `Stop` gives `idle`.
- The facts about `SubagentStop`, `Notification` timing and the Codex `Interrupt` event are PROBED.

Fallback for a CLI with no hooks: PTY output activity gives `working` or `idle` only. Codex can also give
turn end with `notify` and approval prompts with OSC 9 escape codes, with no hook trust (PROBED).

### 4.4 The MCP server of the app

The app is the MCP server. Each session gets the server in its start configuration
(Claude: `--mcp-config` with the URL and a `headers` entry; Codex: `-c mcp_servers.tower.url=...`, `bearer_token_env_var` and `default_tools_approval_mode="approve"`; all PROBED). Without the `approve` setting, Codex asks the operator before each tool call of the app..

Identity: the app puts a token for the employee in the server URL or environment at start.
An agent cannot act as another employee.

Transport: one local HTTP MCP endpoint in the app, with a bearer token for each session (PROBED for both CLIs with a test server).
The Rust crate for it is NOT TRACED (item N8). Plan B is a hand-written HTTP endpoint with JSON responses, which the test server proves is sufficient.

Tool group "tasks" (a light Jira):

| Tool | Purpose |
|---|---|
| `task_create(title, description, parent?)` | Create a task on the floor board |
| `task_assign(task, employee)` | Assign. Only to employees below the caller in the hierarchy |
| `task_update(task, status, note?, report?)` | Status: `todo`, `in_progress`, `blocked`, `review`, `done`. `report` is the path of the report file (see 4.10) |
| `task_merge(task)` | Project Manager only. The app merges the task branch into the main branch (see 4.8) |
| `task_get(task)` / `task_list(filter)` | Read the board |
| `task_comment(task, text)` | Add a note or a result summary |

Tool group "people":

| Tool | Purpose |
|---|---|
| `org_chart()` | The employees of the floor, with role, manager and status |
| `employee_start(employee, message)` | Start or continue a session of a direct report, with a first message |
| `employee_stop(employee, fresh?)` | Stop, or stop and prepare a fresh start |

Tool group "messages":

| Tool | Purpose |
|---|---|
| `send(to, text, task?)` | Message to one employee |
| `inbox()` | Read the messages that wait |
| `wait(timeout)` | Block until a message comes or a task of a report changes. Maximum 50 s; the agent calls it again. It returns only the number of waiting messages. Only `inbox()` marks messages as read, so a call that times out loses nothing (Claude drops an HTTP tool call after about 60 s, PROBED) |

Who can talk to whom: each employee of a floor can message each other employee of that floor.
The operator can message all. Messages between floors are never possible (isolation rule, 4.0).
The hierarchy limits only assignment and session control (`task_assign`, `employee_start`, `employee_stop`), not messages.

### 4.5 Message delivery

Sending is a tool call. Delivery into a session has three paths. The app selects the path from the status of the receiver.

| Status of the receiver | Path |
|---|---|
| `working` | **Stop hook.** When the turn ends, the Stop hook of the session asks the app for waiting messages. If there are some, it returns `{"decision":"block","reason":<messages>}`. The CLI continues the turn with the messages (mechanism PROBED for both CLIs). |
| `idle`, `done` | **PTY paste.** The app writes `ESC[200~` + text + `ESC[201~` + `\r` to the PTY in one write (bracketed paste; PROBED for both CLIs, also with multi-line text). |
| `question`, `starting` | **No delivery.** The messages stay in the queue. |
| `stopped` | The messages stay in the queue. `employee_start` from a manager resumes the session with them. |

The agent can also pull at any time with `inbox()` or `wait()`.

Each delivered message has a clear header, for example `[message from Maya (Project Manager), task T-12]`.
The role instructions say that messages from other employees arrive in this form, also as "blocked by hook" text,
and that such a message is not the operator. Without this, the Codex model treated the hook text as untrusted and did not act on it (PROBED).
That the role instructions change this is NOT TRACED (item N14). If they do not, Codex sessions get messages only by paste at `idle` and by `inbox()`.

**Rules for PTY paste.** A paste with Enter into a permission dialog approves the action (PROBED in both CLIs: the command ran). So:

1. The app never writes to a PTY in `question` or `starting`.
2. One write lock for each PTY. The app reads the status again inside the lock, immediately before the write.
3. `idle` permits a paste only if no `PreToolUse` or `PermissionRequest` came after the last turn end. A turn end is a `Stop`, a Codex `Interrupt`, or an `idle` from the 2 s quiet rule (deny, Esc, Ctrl-C).
4. **Draft flag.** A key from the operator that can put text into the input box of the CLI, typed while the status is `idle`, `done` or `working`,
   sets a `draft` flag for that session. Such keys are printable text, a paste, and control keys that insert text: Ctrl+V (the CLI reads the clipboard itself),
   Up arrow and Ctrl+P (history), Ctrl+R, Ctrl+Y, Tab and some others (the full table is in `crates/core/src/session.rs`).
   Keys typed during `question` or `starting`, and Enter, Esc, Ctrl-C, Backspace and cursor keys, do not set it. When in doubt, a key sets the flag.
   The flag ends in two ways only: a `UserPromptSubmit` event, or the operator's click on the "draft" or "message waiting" mark of the tab.
   There is no automatic clear. Reason (PROBED in the M1 review): a slash command such as `/help` fires no hook and leaves a dialog open;
   a paste into that dialog loses the message and can select an option.
   The app does not paste while the flag is set, because the draft and the message become one prompt (PROBED).
5. The operator can type in a tab at any time. Operator input always has priority.

The Claude queued-turn rule of 4.3 is a part of this protection. Without it, the probe showed `idle` for 1.3 s while a queued turn ran.

Other safety rules:
- A message can never answer a permission prompt. Only the operator answers prompts, in the terminal tab.
- Loop limit: a maximum number of messages between the same two employees for each task (default 20). Messages with no task count together as one group for that pair.
  The limit counts all delivery paths. At the limit, the app holds the messages, sets `question` and asks the operator.

### 4.6 Instructions for agents

At start, the app gives each session a text with: the role and its goals, the name of the manager and
the direct reports, the task and message rules, and the worktree path.
Claude: `--append-system-prompt` (VERIFIED flag). Codex: `-c developer_instructions=...` (PROBED). How this mixes with an `AGENTS.md` in the folder is NOT TRACED.

Role texts are files in the app data folder. The operator can edit them.

### 4.7 Permission policy (D3)

Each role has a policy. The adapter maps the policy to the settings of the CLI at start. The app always gives the mode explicitly.

| Policy | Meaning | Default for |
|---|---|---|
| `read_only` | Can read and use the MCP tools. No file edits. | CEO, Project Manager |
| `edit_in_worktree` | Edits and safe commands inside its worktree without a prompt. Other actions prompt. | Engineer, other workers |
| `ask_always` | Each action prompts. | Operator choice. Claude only |

A prompt sets the status to `question`. The operator answers it in the terminal tab. Version 1 has no approval by a manager agent.

**Claude mapping.** The modes of `--permission-mode` are `acceptEdits`, `auto`, `bypassPermissions`, `manual`, `dontAsk`, `plan` (PROBED list).
The exact mode and tool rules for each policy are NOT TRACED (item N12).

**Codex mapping: decision O7 (see section 9).** The Codex sandbox does not work on this machine (PROBED): `bwrap` fails because Ubuntu
blocks unprivileged user namespaces (`apparmor_restrict_unprivileged_userns` = 1). With the sandbox on, each shell command fails and then asks for approval.
Codex has only two approval modes, `on-request` and `never`, so `ask_always` does not exist for Codex.

| Option | Result |
|---|---|
| A. The operator adds an AppArmor profile that lets `bwrap` use user namespaces (one `sudo` step, NOT TRACED) | The Codex sandbox enforces `read_only` and `edit_in_worktree` |
| B. Codex runs with `-s danger-full-access -a on-request` | No system change (NOT TRACED: this mode was not run). The CLI does not enforce the policy; only the role instructions limit the agent. A "read-only" Codex CEO or Project Manager can write in the main checkout |

### 4.8 Worktrees (D4)

- When a task is assigned to an employee with an edit policy, the app creates a worktree and a branch
  `task/<id>-<slug>` from the main branch of the floor. The session runs in that worktree.
- When the task goes to `review`, the Project Manager can read the diff and the report file.
- The Project Manager decides the merge. It calls `task_merge(task)`. The app runs the git merge, so
  the Project Manager keeps its `read_only` policy.
- If the main checkout has changes to tracked files outside `.swarm/` that are not committed (`git status --untracked-files=no`), the app does not merge. It sets the task to `blocked` and tells the Project Manager and the operator.
- If the merge has a conflict, the app stops the merge, sets the task to `blocked` and tells the
  Project Manager. The Project Manager assigns the conflict to an engineer.
- After a good merge, the app sets the task to `done` and removes the worktree.
- Read-only employees run in the main checkout.

### 4.9 Persistence

Each floor keeps its data in its own folder. The app keeps only the list of floor paths in its own config folder.

```
{CWD}/.swarm/
    floor.toml            version, floor name, defaults (shareable)
    employees/
        ceo.md            one file for each employee: front matter (role, name, manager, CLI,
        pm.md             model, effort, policy) and the role text (shareable)
        ...
    assets/               logo, office image, employee icons (shareable)
    reports/              report files of tasks (4.10)
    state/                local only, never shared
        floor.db          SQLite: sessions (with CLI session IDs), tasks, task events,
                          messages, status events
        logs/             PTY output of each session, with a size limit and rotation
    worktrees/            one git worktree for each task in progress (4.8)
    .gitignore            ignores state/, worktrees/ (and reports/ by default)
```

- **Tasks are in SQLite** (`state/floor.db`), one file for each floor. Tables: `tasks`, `task_events`
  (each status change, assignment and comment, with time and author), `messages`, `sessions`, `status_events`.
  The file moves with the project folder. A person can query it with the `sqlite3` tool, and agents query it with `task_list(filter)`.
  The schema has a version number, and the app migrates an older file on import.
- The shareable part can go into git. Thus a second machine, or a second person, imports the same company.
- `state/` holds no secrets, but it is local. API keys are never in `.swarm/`. They are in the app config folder.
- Task worktrees are in `{CWD}/.swarm/worktrees/<task>/`, and git ignores that folder. Claude shows no trust dialog in a worktree at this location (PROBED at the repair check). Not traced (item N15): Claude can also load the `CLAUDE.md` of the main checkout from a parent folder, and a test runner that does not read `.gitignore` can collect the worktree copies.
  Sessions in a worktree get the absolute path of the main `.swarm/reports/`.

After an app restart, all employees and tasks are there. Sessions show as `stopped` and can be continued.

Transcript: the app records the PTY output of each session. It also stores the path of the CLI
transcript file (`~/.claude/projects/...jsonl` VERIFIED location, internal format; `~/.codex/sessions/...` VERIFIED location).

### 4.10 Reports

A manager does not read the transcript of a report. It gets messages, task notes and a report file.

- Each floor has a reports folder: `{CWD}/.swarm/reports/`. Git ignores it by default, so reports are not merged into the code.
- When an employee completes a task, it writes its report to `{CWD}/.swarm/reports/<task-id>.md`: what it did, what it verified, what it did not do, and open questions.
- The employee gives the path in `task_update(..., report)` and in its message to the manager. The message is short and refers to the file. It does not copy the content.
- The role instructions contain this rule and the folder path.
- Each session gets access to the reports folder (Claude: `--add-dir`, flag seen in `claude --help`; Codex: NOT TRACED, item N10).

## 5. Screen

```
+-----------+--------------------------------------+-----------+
|           |  OFFICE of the selected floor        |           |
| BUILDING  |  (generated layout, employee icons,  |  TASK     |
|           |   name, status, animation)           |  BOARD    |
| floor 3   +--------------------------------------+  of the   |
| floor 2 * |  [CEO] [PM Maya] [Eng Sam]   <- tabs |  floor    |
| floor 1   +--------------------------------------+           |
|           |  TERMINAL of the active tab          |           |
+-----------+--------------------------------------+-----------+
```

- **Building (left):** one floor for each project. The facade of a floor shows the project logo. A click selects the floor. A small badge shows floors with a `question`.
- **Office (right, top):** the office image of the floor with one icon for each employee. Each icon shows the name, the status and an animation for the status. A click on an icon opens its tab. If the employee has no session, the click offers "start".
- **Tabs (right, middle):** one thin tab for each live session of the floor, with a status dot.
- **Terminal (right, bottom):** the PTY of the active tab. Full keyboard input.
- **Board (right column):** the tasks of the selected floor, in groups by status, with the assigned employee. A click on a task shows its notes and its report file. The operator can close the column to give the terminal more width.
- **Style:** pixel-art office, inspired by Herdr and the Pixel HQ mod of the operator.

Images: the app calls the OpenAI image API (`gpt-image-2.6-flare` is the model that the key of the operator has)
to make the floor logo, the office layout and the employee icons. Images are made one time and stored.
Animations are done by the app (sprite states or CSS effects on a still icon), not by the image model.
The app works with placeholder images when there is no API key.

## 6. Architecture

| Part | Choice | Reason |
|---|---|---|
| Shell | Tauri 2: Rust core, web UI | Rust for the core; the web side gives a mature terminal widget and easy animation |
| Terminal widget | xterm.js | Standard, complete terminal emulation |
| PTY | `portable-pty` crate | Cross-platform PTY in Rust |
| Async | `tokio` | Processes, sockets, timers |
| Database | SQLite (`rusqlite` or `sqlx`) | One file, no server |
| MCP server | `rmcp` crate (official Rust MCP SDK), NOT TRACED fit | Inside the core process |
| Hook receiver | Local Unix socket or localhost HTTP in the core | Gets status events from CLI hooks |
| Git | `git` command-line calls | Worktrees and merges |

Core modules (Rust):

```
core/
    registry     floors, employees, roles, hierarchy rules
    sessions     PTY spawn, resume, stop, input injection, output record
    adapters/    one for each CLI: start arguments, hooks setup, status mapping, resume
        claude
        codex
    status       state machine for each session, from hook events and PTY activity
    bus          message queues, delivery rules, loop limit
    tasks        board, task events
    mcp          the MCP server, tools, identity check
    worktrees    create, merge, clean
    images       image generation and cache
    store        SQLite
ui/              web front end: building, office, tabs, terminal, board
```

Adapter interface (one implementation for each CLI):

```rust
trait CliAdapter {
    fn start_command(&self, emp: &Employee, ctx: &StartContext) -> CommandSpec;
    fn resume_command(&self, emp: &Employee, cli_session_id: &str, ctx: &StartContext) -> CommandSpec;
    fn map_event(&self, raw: &HookEvent) -> Option<StatusChange>;
    fn inject_message(&self, pty: &mut PtyWriter, text: &str) -> Result<()>;
    fn cli_session_id(&self, ctx: &SessionContext) -> Option<String>;
}
```

A new CLI (GLM, DeepSeek, ...) is a new adapter. With no hooks, it uses the PTY activity fallback.

## 7. Milestones

Each milestone ends with an app that a person can use for more than the one before.

| # | Name | Result | Proof |
|---|---|---|---|
| M0 | Probes | Done for most items at gate 1 (section 8). The scripts in `probes/` stay as the check to run after each CLI update. The items that stay open are closed at the start of the milestone that needs them. | Each probe prints output that a person can compare |
| M1 | Terminals | App window. One floor. Add employees by hand. Start Claude and Codex sessions in tabs with a real terminal. The start rules of 4.2 (clean environment, session ID, dialogs, hook trust). The full status state machine of 4.3. The write lock and the draft flag of 4.5. | The operator runs two sessions and sees correct `working` / `idle` / `question`, also after an Esc interrupt and with a permission prompt |
| M2 | Bus and board | `.swarm/` folder: import, and setup by a setup agent with the floor-init skill. MCP server with messages, tasks and people tools. Role instructions. Delivery by Stop hook and by paste. CEO and Project Manager roles. Report files. | The operator gives the CEO a small feature. The PM makes tasks, an engineer does one, the result comes back with no copy by hand |
| M3 | Worktrees and policies | Decision O7 applied. Worktree for each task, `task_merge`, permission policy for each role, loop limit | Two engineers work at the same time on one repository with no collision |
| M4 | Building and office | Building view, office view, generated logo, layout and icons, status animations, board column | The screen of section 5 is complete |
| M5 | More | Other CLIs, cost view, manager approvals | Later |

Placeholder art is used in M1 to M3. The visual work (M4) can move earlier if the operator wants it.

## 8. Probe results (gate 1) and open items

| # | Item | Result |
|---|---|---|
| N1 | Type a message into the Claude TUI and the Codex TUI through a PTY | WORKS with bracketed paste in one write. Dangerous in a permission dialog (4.5) |
| N2 | Claude hook payloads with `--settings` | WORKS. `PermissionRequest` is the permission signal (4.3) |
| N3 | Codex signals for turn start, turn end, approval | WORKS with hooks, after a one-time trust step (4.2). `notify` and OSC 9 as fallback |
| N4 | Codex MCP server for one session with a token | WORKS with HTTP (4.4). Codex stdio MCP: NOT TRACED, not necessary |
| N5 | Codex instructions for one session | WORKS: `developer_instructions` (4.6) |
| N6 | Codex approval and sandbox settings | DOES NOT WORK on this machine. Decision O7 (4.7) |
| N7 | CLI session ID of a live session | WORKS (4.2). Resume keeps the context in both CLIs |
| N8 | `rmcp` crate as the MCP server | NOT TRACED. Version 3.11.0 has an HTTP server feature. Plan B in 4.4. Close at the start of M2 |
| N9 | Long blocking `wait` | WORKS up to about 60 s in Claude. `wait` is limited to 50 s (4.4) |
| N10 | Codex access to the reports folder outside a worktree | NOT TRACED. Depends on O7. Close at the start of M2 |
| N11 | How `question` ends when the operator denies a prompt, both CLIs. Claude Ctrl-C and the 2 s quiet rule | PROBED at step 0 (4.3). Evidence in `probes/step0/` |
| N12 | Claude permission mode and tool rules for `read_only` and `edit_in_worktree` | NOT TRACED. Close at the start of M3 |
| N13 | First message as the `[PROMPT]` argument when a startup dialog shows. Codex `check_for_update_on_startup=false` | WORKS, PROBED at step 0 (4.2). Not traced: Codex without `--no-daemon`, and the Codex option "Continue without trusting" hooks |
| N14 | A Codex agent acts on a message that the Stop hook delivers, when the role instructions explain it | NOT TRACED. Close at the start of M2 |
| N15 | Nested worktrees: parent `CLAUDE.md` loading, test runners that collect `.swarm/worktrees/` | NOT TRACED. Close at the start of M3 |

## 9. Points closed by the operator (2026-11-19)

| # | Question | Answer |
|---|---|---|
| O1 | One CEO for each floor, or one for the building? | One for each floor |
| O2 | Who can message whom? | Each employee can message each other employee of the floor (4.4) |
| O3 | Who merges a task branch? | The Project Manager, with `task_merge` (4.8) |
| O4 | Where is the task board on the screen? | A column on the right side (5) |
| O5 | Platforms | Ubuntu first. Windows and macOS are possible later, not a requirement |
| O6 | Does a manager see the full transcript of a report? | No. The employee writes a report file and refers to it (4.10) |

| O7 | Codex sandbox on this machine (4.7) | Option B for M1 and M2. Option A (AppArmor profile for `bwrap`) is tested before M3; if it works, it becomes the rule |
| O8 | Tasks in git | Not now. Tasks stay in `state/floor.db` only. The `task_events` table keeps the full history, so a text log (`.swarm/tasks/events.jsonl`, merged by git, SQLite built from it) can be added later. To keep that possible, task IDs are time-based and random (ULID), not a counter |

## 10. Risks

1. **Token cost.** Each employee is a full CLI session. A hierarchy of five agents costs much more than one agent. The loop limit and the "stop when done" rule are the first controls. A cost view comes in M5.
2. **CLI changes.** Hook payloads, flags and TUI behaviour can change with a CLI update. The adapters isolate this. The M0 probes stay in the repository as a check to run after each CLI update.
3. **Manager quality.** The CEO and the PM add a relay step that can lose detail. The instructions must tell managers to pass the request of the operator word for word, and to link tasks instead of rewriting them.
4. **PTY injection timing.** A message typed into a TUI at the wrong moment can mix with operator typing. The app injects only at `idle` and not while the operator types in that tab.
5. **Vendor terms.** The app drives the CLIs with the subscriptions of the operator, for personal use. A public release needs a check of the terms of each vendor. NOT TRACED.
6. **Storage.** One Claude transcript was about 600 KB after a few short turns (PROBED). The PTY logs come on top. The logs have a size limit.
7. **Usage limits.** Each employee uses the subscription limits of its CLI. The gate 1 probes alone used a visible part of the weekly Codex limit.
8. **Paste into a dialog.** A paste with Enter can approve a permission prompt (PROBED). The rules of 4.5 are the control. A change in those rules needs a new probe.
