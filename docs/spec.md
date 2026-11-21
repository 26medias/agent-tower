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
| Continue | The same | The message goes to the live session. If the session stopped and had one turn or more, the app resumes it by its CLI session ID. A session with no turn cannot be resumed (the CLI has no conversation); the app starts a fresh one (`claude --resume <id>` VERIFIED flag; `codex resume <id>` VERIFIED subcommand). "Had a turn" means a `working` status event, that is the first `UserPromptSubmit`. PROBED on 2026-11-21 with Claude `haiku`: a session that was stopped 0.8 s after that event and a session that was stopped at once after it (the first prompt sent, no answer) both had the prompt in the transcript file, so `claude --resume` kept the CLI session ID and showed the first prompt in its history. Evidence: `/tmp/agent-tower-review/gaps/claude-resume/`. |
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
  The command text holds the path of `tower-hook`. Seen on 2026-11-21: an app that ran from another build folder showed "Hooks need review: 8 hooks are new or changed" and the session waited in `question`; the app never answers that dialog (the choices write the trust into `~/.codex`). The operator trusts the hooks once for each build path.
- **Codex resume.** `codex resume <id>` sends no hook event until its first prompt (found live on 2026-11-21: a resumed session with no prompt stayed in `starting`, and after 20 s the status showed a false `question`). So a Codex resume with no text from the operator gets a short default first message: "The operator resumed your session, <Name>. Say that you are ready in one short sentence." A Claude resume sends `SessionStart` by itself and gets no message. Evidence: `/tmp/agent-tower-review/gaps/codex/`.
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
- **Stop that the app blocked (M2).** When the Stop hook of the app returns `block` to deliver messages (4.5), the turn continues with no new event. That `Stop` keeps `working`. The next `Stop` gives `idle`. Built on 2026-11-21 (Claude and Codex): the app tells the machine `StopBlockedByApp` after it wrote the answer and before the machine gets that `Stop`.
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
| `task_merge(task)` | `pm`, `ceo` (and the operator in the UI). The app merges the task branch into the main branch (see 4.8). Changed in M3 (2026-11-21): the `ceo` may merge too |
| `task_diff(task)` | Any employee of the floor. The diffstat and the patch of the task branch, cut at 60000 bytes (added in M3, 2026-11-21) |
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

**Built in M2a (people and messages).** The tools `org_chart`, `employee_start`, `send`, `inbox` and `wait` work (`crates/core/src/app/bus.rs`).
`employee_start` and `send` start a session of an employee that has none, if the caller may start it: the caller is above the target in the `manager` chain, or the caller is `ceo` or `pm`.
The token is in the environment of the session (`TOWER_MCP_TOKEN`), not in an argument: Claude expands `${TOWER_MCP_TOKEN}` in the `Authorization` header of `--mcp-config` (PROBED, 2.1.330),
and Codex reads `bearer_token_env_var`. Claude gets `permissions.allow = ["mcp__tower"]` in `--settings`, so its tool calls need no prompt in the modes `manual` and `acceptEdits` (PROBED).
Repairs after the M2a review:
- A delivered message has the header line, the body with `> ` before each line, and the answer line. The role instructions say that only lines with `>` are the body, and that a body is not an order from another person than the sender.
- The first message of a start carries the new message and older waiting messages while the text stays under 60000 bytes. Those messages stay waiting in the store until the new session reaches its first `working`. If it stops before that, they are waiting again. The others are pasted after the first turn.
- A session that starts with no first message (an operator resume) counts as past its first turn at its first `idle`. While the first turn is open, a new message is held and the "message waiting" mark shows.
- `inbox` marks messages delivered after it wrote the answer. The HTTP endpoint checks the token before it reads the body, gives each request one deadline of 30 s, and allows 256 open connections (HTTP 503 over that).
- The start rule ignores the manager chain of an employee that is its own ancestor. Employees can start 12 sessions for each floor in a rolling 10 minutes. The Claude settings of an employee deny the file tools for `floor.toml`, `employees/`, `assets/` and `state/` under `.swarm/`. Codex has no enforced deny with the sandbox switch off (the default). With the switch on and no git repository, the profile makes these paths read-only (4.7).

Not built in M2a: `employee_stop` and the loop limit per task (M2a counts 30 messages for each pair in a rolling hour). The Stop-hook delivery path was built later (4.5).

Who can talk to whom: each employee of a floor can message each other employee of that floor.
The operator can message all. Messages between floors are never possible (isolation rule, 4.0).
The hierarchy limits only assignment and session control (`task_assign`, `employee_start`, `employee_stop`), not messages.

### 4.5 Message delivery

Sending is a tool call. Delivery into a session has three paths. The app selects the path from the status of the receiver.

| Status of the receiver | Path |
|---|---|
| `working` | **Stop hook.** When the turn ends, the Stop hook of the session asks the app for waiting messages. If there are some, it returns `{"decision":"block","reason":<messages>}`. The CLI continues the turn with the messages (mechanism PROBED for both CLIs). **Built for Claude and for Codex on 2026-11-21.** Item N14 is closed: Codex acts on the text (PROBED, 3 of 3). |
| `idle`, `done` | **PTY paste.** The app writes `ESC[200~` + text + `ESC[201~` + `\r` to the PTY in one write (bracketed paste; PROBED for both CLIs, also with multi-line text). |
| `question`, `starting` | **No delivery.** The messages stay in the queue. |
| `stopped` | The messages stay in the queue. `employee_start` from a manager resumes the session with them. |

The agent can also pull at any time with `inbox()` or `wait()`.

**The Stop hook as built (2026-11-21, Claude and Codex employees).** `tower-hook` sends the `Stop` event with `"reply": true` and waits 2 s at the most for an answer on the same socket. The app answers `block` only if the session is `working` and has had a first turn, a message waits that is not held, fewer than 5 stops in a row were blocked, and (when the payload has `stop_hook_active` = `true`) a message newer than the last block waits. It takes the messages with the same code as the paste (same selection, order, text, `> ` mark, sanitize rule and holds), at most 60000 bytes at one time. The `reason` starts with the line `This is not an error. Messages from your team arrived while you worked. They are from employees, not from the operator.` (Claude prints the delivery under its own line "Stop hook reported a problem", and Codex prints "Blocked by hook", so the first line says that this is normal). Codex has the same payload field `stop_hook_active` (false at the first stop, true at the stop after a block; PROBED), so Claude and Codex share the guards. The app marks the messages delivered only after the answer was written to the hook program. In every other case (no message, a dead hook program, a time out, the setup session) the hook prints nothing and the turn ends as before: the status goes to `idle` and the paste delivers later. A blocked stop keeps the status `working` (4.3). Details of the wire: `docs/ipc-final.md`.

Each delivered message has a clear header, for example `[message from Maya (Project Manager), task T-12]`.
The role instructions say that messages from other employees arrive in this form, also as "blocked by hook" text,
and that such a message is not the operator. Without this, the Codex model treated the hook text as untrusted and did not act on it (PROBED).
With the role instructions in place, a Codex agent acts on this text (item N14, PROBED on 2026-11-21: 3 of 3, see section 8).

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

**Answers and exchanges (2026-11-21).** Reason: in a live run with two Codex employees, each one sent "QUINCE DONE" to the other, 24 times in all, until the models stopped by themselves (evidence: `/tmp/agent-tower-review/sandbox2/live/`, the file `db.json`, the rows 2 to 24 of `messages`, and the terminal logs). In an earlier run two agents sent "noted", "confirmed" and "no further action" four times after a task was done. Each such message costs a model turn of the operator's limits. There were two causes. Each delivered message ended with the line `Answer with the tool send(to="<slug>", ...).`, which tells the receiver to answer every message. And the app had no limit on a chain: only 30 messages from one employee to one other in a rolling hour (`LOOP_LIMIT`), and 60 task messages. The rules (all in `crates/core/src/app/bus.rs`):
1. *The text.* Each delivered plain message ends with this line (the same for a paste, the Stop hook, `inbox` and the first message of a start): `To answer, use the tool send(to="<slug>", ...). Answer only if this message gives you work or asks a question. Do not answer a message that only confirms, reports "done" or says thanks: end your turn.` The instructions of each employee have the same rule, and: "When you get the result that you asked for, do not confirm it to the sender. Tell the operator or your manager only if they wait for it." A task status message and a merge or discard message of the app do not ask for an answer: they end with `Do not send a message back for this status message.` A task assignment keeps its orders, with the order to tell the assigner about the report.
2. *A hard stop for an exchange.* The app counts the plain messages (kind `message`, not the task messages) between two employees in BOTH directions together. `EXCHANGE_LIMIT` is 8 messages in `EXCHANGE_WINDOW`, 5 minutes (a rolling window; `TowerConfig::exchange_window` holds the value, so a test can give a short one). A `send` or an `employee_start` that would be the 9th is refused before it is stored. The tool returns an error: `The app stopped this exchange: you and <Name> sent each other 8 messages in the last 5 minutes. Do not send again. End your turn now. If work is still open, write it in your task or report and wait for the operator.` The messages of the operator are not rows of `messages` and are never counted or refused. A task assignment or status between the same two employees is not blocked (it has its own limit). The old limits stay as they are.
3. *The operator sees it.* At the first refusal of a window for a pair, the app sends ONE `notice` of the kind `exchange_stopped` (see `docs/ipc-final.md`) and not one for each refused call. The UI shows an error toast that stays, with the button "Open" for the tab of the employee that was refused, and one row in the list "Team activity".
4. *A softer signal.* The 5th message of a window for a pair succeeds, and the answer of the tool ends with one more line: `Note: you and <Name> exchanged 5 messages in a few minutes. Send again only for new work or a real question.`

### 4.6 Instructions for agents

At start, the app gives each session a text with: the role and its goals, the name of the manager and
the direct reports, the task and message rules, and the worktree path.
Claude: `--append-system-prompt` (VERIFIED flag). Codex: `-c developer_instructions=...` (PROBED). How this mixes with an `AGENTS.md` in the folder is NOT TRACED.

Role texts are files in the app data folder. The operator can edit them.

### 4.7 Permission policy (D3)

Each role has a policy. The adapter maps the policy to the settings of the CLI at start. The app always gives the mode explicitly.

| Policy | Meaning | Default for |
|---|---|---|
| `read_only` | Can read and use the MCP tools. No file edits, except the report files in `.swarm/reports/`. | CEO, Project Manager |
| `edit_in_worktree` | Edits inside its task worktree and the report files without a prompt. An edit of a file of the main checkout prompts. Other actions prompt, unless a rule that the operator set for the floor allows them (changed 2026-11-21, see below). | Engineer, other workers |
| `ask_always` | Each action prompts. | Operator choice. Claude only |

A prompt sets the status to `question`. The operator answers it in the terminal tab. Version 1 has no approval by a manager agent.

**Claude mapping (item N12, closed in M3, 2026-11-21).** The modes of `--permission-mode` are `acceptEdits`, `auto`, `bypassPermissions`, `manual`, `dontAsk`, `plan` (PROBED list).
Changed in M3 (2026-11-21): the app uses `manual` for each policy, and `acceptEdits` only for `edit_in_worktree` in a floor that is not a git repository (no worktree exists there, so the old rule stays). The allow rules go in the `--settings` JSON, with the rule `mcp__tower`:

| Policy | `--permission-mode` | Allow rules besides `mcp__tower` and the rules of the floor (app config) |
|---|---|---|
| `read_only` | `manual` | `Edit(//<floor>/.swarm/reports/**)` and `Edit(./.swarm/reports/**)` |
| `edit_in_worktree`, floor is a git repository | `manual` | the same two rules for `.swarm/worktrees/**` and for `.swarm/reports/**` |
| `edit_in_worktree`, no git repository | `acceptEdits` | none |
| `ask_always` | `manual` | none (also not the rules of the floor) |

The deny rules for `.swarm/floor.toml`, `.gitignore`, `employees/**`, `assets/**` and `state/**` stay for all policies. The setup session has no app server and no rule, and it keeps `acceptEdits`.

PROBED with `claude -p --model haiku` (2.1.330, six calls; evidence in `/tmp/agent-tower-review/m3/probe/`):
1. `manual` with `Edit(//<floor>/.swarm/worktrees/**)` and `Edit(//<floor>/.swarm/reports/**)`: the tools Write and Edit change a file under those folders with no question (also the new files). A Write in the main checkout, and an Edit of `notes.txt` in the main checkout, are not allowed (they would ask). A Write to `.swarm/employees/c.md` of the main floor is refused by the deny rule.
2. The same with the relative rules `Edit(./.swarm/worktrees/**)` and `Edit(./.swarm/reports/**)`: the same result. The app writes both forms.
3. Control, no allow rule: each edit and each write is refused. So the allow rule is what opens the folders.
4. `read_only` shape (only the `reports` rules): a report file is written, the worktree files and the main checkout files are not.
5. A rule for `.swarm/worktrees/**` does not open `.swarm/employees/` of the main floor (a different path). It opens `.swarm/worktrees/<task>/.swarm/employees/d.md`: that is a file of the task branch (a copy of the tracked file), and its merge is a decision of the `pm` and of the operator.
6. `Bash(touch:*)` in `allow`: `touch p5a.txt` runs, `mkdir p5b` is refused. So a rule of the floor of the form `Bash(command:*)` works. (A first try with `git status` was muddied by the RTK hook of the operator, which rewrites the command to `rtk git status`: a rule must match the command after the hooks.)

**Commands with no question (changed in M3, 2026-11-21; moved out of the project folder, 2026-11-21).** The operator lists Claude permission rules for a floor, for example `Bash(rtk cargo test:*)`. The app adds them to the allow list of each Claude employee of the floor, except an `ask_always` employee. **The list is in the app config, not in the project folder.** The first version kept it in `[permissions]` of `floor.toml`. A review showed that this is not safe: `floor.toml` is in the project folder, so a Codex employee (no enforced limits on this machine) or any shell command that is allowed can write it, and then each Claude employee of the floor gets those rules at its next start. So the list is now the table `[floor_permissions]` of the app config file (`programs.toml` in the config folder, mode 0600). Each key is the real absolute path of a floor, and the value is the array of its rules. An agent of the floor cannot change the file through the project. The app does not read `[permissions]` of `floor.toml` for a session any more. When a floor has no entry in the app config and its `floor.toml` has the old table, the app does not import it (an agent can have written it). It offers the rules that pass the check as a proposal (`floor_permissions_proposed`), and the operator saves them with one click. `floor_permissions_set` writes the app config and removes the old table from `floor.toml` (the rest of the file stays). `floor_forget` removes the entry of the floor. The setup session has no guard for this list any more, because the list is not in the project (the notice `permissions_restored` is removed).

**The check of a rule is not a security boundary. It is informed consent:** the operator must know what each rule gives. It cannot be complete: many programs write files or run other commands through their options (`sort -o`, `rg --pre`, `fd -x`, `gh`, `man -P`), and a rule with a `*` lets the agent add any option. See `docs/ipc-m3.md`, "The check of a rule". In short: 1 to 200 printable ASCII characters, one entry is exactly one rule. The check unwraps `rtk` and applies to the words after it (`rtk` alone, `rtk proxy` and the other `rtk` commands that run a command are refused). A list of programs that run other commands, change or destroy data, or send data out is refused, as a best effort (also a versioned name like `python3.12`). `git` rules must be exact: a `git` rule with a `*` is refused, because the agent could add `--output=<file>`. An exact `git` rule must name a subcommand of an allow list, and it must not have `--output`, `--exec`, `--ext-diff`, `--textconv` or `-c`. **Each `Bash` rule with a `*` gets a warning from `floor_permissions_set`** (the program runs with any arguments and no question, and many programs can write files or run other commands through their options). This warning is the real control. An exact `Bash` rule gets none. The build and package tools (`cargo`, `npm`, `make`, `cmake`, `poetry` and others) are allowed, because the operator wants `cargo test` with no question. They run the code of the project, so they get the stronger warning, also for an exact rule. `Read`, `Glob` and `Grep` need a relative path with no `..` and not `.swarm/state`. `WebFetch` takes only `domain:<host>`, and not an IP address, `localhost`, `.local` or `.internal`. A rule must match the command after the hooks of the operator (with the RTK hook, `cargo test` is `rtk cargo test`). The operator sets the list in the settings panel (section "Commands with no question"); the commands are `floor_permissions_get`, `floor_permissions_set` and `floor_permissions_proposed`. A rule applies at the next start of a session. The limits that stay: a Codex employee has no enforced limits on this machine, so it can also write the app config file if it tries. The move takes the list out of the project folder, and it does not sandbox Codex. The rules of a floor are stored by the path of the floor: if another floor takes that path, it gets those rules.

**Codex mapping: decision O7 (see section 9).** Two cases (sandbox on, sandbox off). **Decision of the operator, 2026-11-21: the sandbox is ON by default when it works on the machine.** The setting "Codex sandbox" in Settings (`[codex] sandbox` of the app config) has three stored states: **Automatic** (no key; the default: at a Codex start the app runs the probe; if it says `works`, Codex gets the sandbox, and if not, Codex starts with full access and the app sends ONE information notice in each run of the app), **Always** (`sandbox = true`: as before; a failing probe refuses the start with `SANDBOX_DOWN:`) and **Never** (`sandbox = false`: full access, no probe). An existing config with `sandbox = true` stays "Always"; a config with no key is now "Automatic" (before this decision it meant "off"). The app reads the setting at the start of each session, never for the setup session, and never for a Claude employee. The flags and the instructions of a start follow the EFFECTIVE state of that start. The contract is in `docs/ipc-final.md`.

- **Sandbox off: "Never", or "Automatic" with a sandbox that does not work (option B, as in M1 to M3).** Each Codex session starts with `-a on-request -s danger-full-access`. The CLI does not enforce `read-only` or `edit-in-worktree`; only the role instructions limit the agent. A "read-only" Codex CEO or Project Manager can write in the main checkout. This is the reason: the Codex sandbox does not work on this machine (PROBED): `bwrap` fails with `setting up uid map: Permission denied`, because Ubuntu 24.04 blocks unprivileged user namespaces (`kernel.apparmor_restrict_unprivileged_userns` = 1). With the sandbox on, each shell command fails and then asks for approval.
- **Sandbox on: "Always", or "Automatic" with a sandbox that works (option A, built and tested live on 2026-11-21; PROBED, evidence in `/tmp/agent-tower-review/sandbox/`).** The operator first added an AppArmor profile that lets `/usr/bin/bwrap` use user namespaces (one `sudo` step; **done on 2026-11-21**, the app cannot do it). The app has the command `codex_sandbox_probe` (it runs `bwrap --unshare-user --ro-bind / / true` in the PATH and environment of a session, 5 s at most), and the server refuses to choose "Always" unless the probe says `works`. Then the CLI enforces the policy, and an action outside it asks the operator (`PermissionRequest`, status `question`).

**A stored "Always" must work now (found by a review in the real app, 2026-11-21).** The setting is stored, so it survives a restart, and a system update can break `bwrap` (the AppArmor profile is gone, or the program is removed). The first build then showed the switch on and enabled, and each Codex command failed. Now the app runs the probe at each start of a Codex session in the states "Automatic" and "Always" (a `works` result counts for 60 s, a failure is probed again at the next start). In "Always", if the probe does not say `works`, the start is refused with `SANDBOX_DOWN: <Name> did not start: the Codex sandbox does not work on this machine now (<detail>). The setting "Codex sandbox" is "Always". ...` (the fact that matters comes first; wording of 2026-11-21) (the operator, a resume, "Resume the last team", and a start by an employee through `employee_start`, `send` or `task_assign`). In "Always" the app does not fall back to full access, because a silent fall back would give an employee more rights than the operator chose. In "Automatic" the operator chose the fall back (decision of 2026-11-21), and it is not silent: the app sends one information notice (`sandbox_auto_off`) in each run of the app, with the button "Open Settings", and the line "In effect now" in Settings says it. A Claude employee and the setup session are not affected. The app never changes the stored value: the operator turns the switch off in Settings, which is always allowed. The section "Codex sandbox" has one control with three choices (Automatic, Always, Never) and a line "In effect now"; a refusal in "Always" gives a toast with the button "Open Settings". The tools and the team resume send one `sandbox_down` notice in each 60 s. See `docs/ipc-final.md`.

Exact flags with the switch on (`crates/core/src/adapter/codex.rs`). `<floor>` is the working folder of the session, which is the floor folder (the session runs in the main checkout, 4.8). Each `-c` value is TOML.

| Policy | Flags after `-a on-request` |
|---|---|
| `read_only` | `-c default_permissions="agent_tower"`, `-c permissions.agent_tower.extends=":read-only"`, `-c permissions.agent_tower.filesystem={"<floor>/.swarm/reports"="write"}` |
| `edit_in_worktree`, git floor with worktrees | the same three, with `filesystem={"<floor>"="read","<floor>/.swarm/reports"="write","<floor>/.swarm/worktrees"="write",":slash_tmp"="write",":tmpdir"="write"}`. The entry `"<floor>"="read"` keeps the main checkout read-only also when the floor is inside `/tmp` or `$TMPDIR` (see below) |
| `edit_in_worktree`, no git repository | `-c default_permissions="agent_tower"`, `-c permissions.agent_tower.extends=":workspace"`, `-c permissions.agent_tower.filesystem={"<floor>/.swarm/floor.toml"="read","<floor>/.swarm/employees"="read","<floor>/.swarm/assets"="read","<floor>/.swarm/state"="read"}` (the floor folder is the workspace, as Claude gets `acceptEdits` there; the app files are read-only, as the Claude deny rules make them) |
| `ask_always` | refused, as before (see below) |

**Live test of the default "Automatic" with real Codex (PROBED 2026-11-21; evidence in `/tmp/agent-tower-review/sandbox2/live/`: `result.json`, `db.json`, `floor.db.sql`, the terminal logs `pm-*.txt` and `eng-*.txt`, the screenshots `00` to `08`, and `timeline.txt`).** The real app was built from the new code, with a fresh config that had no `[codex]` key, a git floor, and two Codex employees (`gpt-6-luna`, low effort): a read-only Project Manager and an `edit-in-worktree` Engineer. The `claude` program was a stand-in. Two Codex sessions started in all.
1. *Automatic is in effect.* `settings_get` gave `codexSandboxMode: "auto"`, `codexSandboxEffective: "on"`, `codexSandbox: true`. The section in Settings showed "In effect now: the sandbox is on." with "Automatic" chosen. `cmdline-pm-start.txt` has `-a on-request` and `-c default_permissions="agent_tower"` (the profile flags), and `cmdline-eng-start.txt` has them too. The config file kept no `[codex]` key.
2. *A message between two Codex employees through the Stop hook, with the sandbox on.* The operator pasted one line to the Project Manager while the Engineer ran a job of six `sleep 8` steps. The `messages` row 1 (pm to eng) was created at 306701 ms and had `delivered_at` 363652 ms, inside the working time of the Engineer (`working` at 296733, `idle` at 369335). The terminal of the Engineer shows the hook text "Messages from your team arrived while you worked ...". The Engineer said QUINCE and sent the answer "QUINCE DONE" (row 2, delivered to the Project Manager 4 ms after it was created). The job file `/tmp/sbx2-eng/log.txt` has the six steps, so a write in `/tmp` works in the sandbox.
3. *The answer "yes" to a Codex permission question.* The Project Manager (read-only) was asked to create `approve.txt` in the project folder. The sandbox blocked it, and Codex asked "Would you like to run the following command?" The status became `question`, the strip and the tab were amber (`05-question-*.png`). `pty_write` of `1` ("Yes, proceed") was enough (no Enter needed). The status went `question`, `working`, `idle` (status events 419882, 422609, 423756 ms). The file `approve.txt` has `YES`, and the strip and the amber tab cleared (`06-after-answer-*.png`). The terminal shows "You approved codex to run printf 'YES\n' > approve.txt this time".
4. *Network access is blocked.* The Engineer ran `curl -sS -m 5 https://example.com -o /dev/null; echo "exit=$?"` and reported `curl: (6) Could not resolve host: example.com` and `exit=6`. So the text in Settings ("no network access") is true; the failure is a DNS failure, not a time-out.
Finding, REPAIRED on 2026-11-21 (see 4.5, "Answers and exchanges"): the two employees sent "QUINCE DONE" to each other 24 times (the rows 2 to 24 of `messages`), because each delivered message ended with the line `Answer with the tool send(to="...", ...).` and the Project Manager sent the answer back. The models stopped it by themselves ("This repeats the prior message"). The app had no limit on a chain of messages between two employees. The line now says to answer only a message that gives work or asks a question, and the app refuses the 9th plain message between two employees in 5 minutes.

The hook program, the trust flags (`projects={...}`), the MCP flags, the instructions, the model and the effort do not change. With the sandbox off (also "Automatic" with a sandbox that does not work) the argument vector is byte for byte the old one (`crates/core/tests/adapter_snapshot.rs` and `adapter_codex_sandbox.rs`).

Why a permission profile and not `-s workspace-write` with `writable_roots` for a git floor: `workspace-write` always makes the working folder writable. PROBED with `codex debug prompt-input -a on-request -s workspace-write` (Codex 0.171.0): the rendered file system has `<entry access="write"><path>/tmp/sbx-cwd</path>` for the working folder, and `/tmp` and `$TMPDIR` too, with `read` entries for `.git`, `.agents`, `.codex` and `.aws` inside it. The session runs in the main checkout, so that mode would leave the main checkout writable. The profile has no entry for the working folder, so the main checkout is read-only. The profile must extend `:read-only`: a profile with no `extends` made `codex debug prompt-input` fail with `failed to load AGENTS.md instructions ... fs sandbox helper failed ... bwr...` (the file system had no read root).

**Sources of each flag** (a `NOT TRACED` label means that nothing was run or read for it):
- `-a, --ask-for-approval`: `codex --help` (0.171.0): "`on-request`: The model decides when to ask the user for approval", "`never`: Never ask for user approval Execution failures are immediately returned to the model". Only these two values are in the help.
- `-s, --sandbox`: `codex --help`: "Select the sandbox policy to use when executing model-generated shell commands [possible values: read-only, workspace-write, danger-full-access]".
- `-c default_permissions`, `permissions.<name>.extends`, `permissions.<name>.filesystem.<path>`: the Codex config reference (`learn.chatgpt.com/docs/config-file/config-reference`, read on 2026-11-21 with a web fetch; the CLI has no config documentation installed, and `~/.codex` has none): `default_permissions`: "Name of the default permissions profile to apply to sandboxed tool calls." "Built-ins are `:read-only`, `:workspace`, and `:danger-full-access`;" "Don't combine with `sandbox_mode` or `[sandbox_workspace_write]`." `extends`: "Optional parent profile applied before this named profile." `filesystem.<path-or-glob>`: values `"read" | "write" | "deny"`. The Permissions page says: "Beta. Permission profiles are under active development and may change." "More specific entries override broader entries." The tokens `:slash_tmp` ("The `/tmp` folder, if it exists") and `:tmpdir` ("The `$TMPDIR` location, when one is available") are on the same page. The quotes come from a fetch tool that summarizes pages, so they are not checked word by word. **The flags are checked by the CLI itself**: `codex debug prompt-input` (it starts no model) renders the sandbox that Codex would use. For each of the three cases above it renders the entries that the table says (`crates/core/tests/adapter_codex_sandbox.rs`, last test, runs the real `codex` with a Codex home of its own). A wrong key gives an error: `sandbox_mode="bogus"` gives "unknown variant ... expected one of `read-only`, `workspace-write`, `danger-full-access`".
- A floor that is not a git repository: `:workspace` is the parent of the profile. `codex debug prompt-input` renders the working folder, `:slash_tmp` and `:tmpdir` as `write` and `:root` as `read` (the first build used `-s workspace-write`, which renders the same). The four `read` entries come from the live tests with `codex sandbox` (`instr/instr-C1-no-git-profile.txt`).

**What each policy gets** (rendered by `codex debug prompt-input`; PROBED live on 2026-11-21 with real Codex sessions, `gpt-6-luna`, low effort; evidence `s1`, `s1b`, `s2-first-run`, `s2b`, `s3` and `instr` in `/tmp/agent-tower-review/sandbox/`):
- `read_only`: `:root` is `read`; the only `write` entry is `<floor>/.swarm/reports`. PROBED live (`s1/result.json`: `probe.txt` was not made, `.swarm/reports/probe.md` has `YES`, no question). The employee still writes its report. Codex has no "read-only plus a writable folder" mode, so the profile (a read-only profile with one writable path) is used, and `-s read-only` is not.
- `edit_in_worktree` in a git floor: `write` for `.swarm/reports`, `.swarm/worktrees`, `/tmp` and `$TMPDIR`. The main checkout is not writable. PROBED live (`s2b/result.json`: `plum.txt` written in the worktree, `main.txt` in the main checkout gave `Read-only file system`, the report was written, no question). The app makes the two folders before it starts the session (a missing folder in an entry is no problem for Codex: PROBED with `codex sandbox`, `instr/`). The protected files under `.swarm/` (`floor.toml`, `employees/**`, and the others) are read-only there, because only two sub-folders of `.swarm/` are writable.
- `edit_in_worktree` with no git repository: the floor folder is writable (PROBED live, `s3/result.json`: `inside.txt` written with no question; a file outside, `/var/tmp/...`, blocked). `-s workspace-write` left `.swarm/floor.toml`, `.swarm/employees`, `.swarm/assets` and `.swarm/state` writable (PROBED with `codex sandbox`, `instr/instr-C0-no-git-as-built.txt`), unlike Claude (deny rules). So the case is now a profile that extends `:workspace` and makes those four paths `read` (PROBED live in `s3`: the append to `floor.toml` and the file in `employees/` were blocked, and `floor.toml` was unchanged).
- A floor folder inside `/tmp` or `$TMPDIR` (found on 2026-11-21 in the first run of `s2`): `:slash_tmp` and `:tmpdir` are `write`, so the whole main checkout was writable and Codex wrote `main.txt` there (`s2-first-run/result.json`, `instr/instr-edit-in-worktree-floor-in-tmp.txt`). The entry `"<floor>"="read"` repairs this: a more specific entry wins (PROBED in `s2b`, `instr/instr-B-floor-in-tmp-with-floor-read.txt`; test `the_real_codex_blocks_a_write_in_the_floor_that_is_inside_tmp`, red before the repair). A floor outside `/tmp` was already read-only (`instr/instr-A-floor-in-var-tmp.txt`).
- Network for commands: off by default in each case (the rendered text says "Network access is restricted."). To turn it on later (NOT built): add `-c permissions.agent_tower.network.enabled=true` for the profile cases (PROBED: the rendered text then says "Network access is enabled"), and `-c sandbox_workspace_write.network_access=true` for `workspace-write` (config reference: "Allow outbound network access inside the workspace-write sandbox"; PROBED: the rendered text then says "Network access is enabled"). The profile key `network.enabled` is documented as "Enable network access for commands in this permission profile."
- `ask_always` stays refused for Codex. `-a` has only `on-request` and `never`. `approval_policy = "untrusted"` ends with `approval_policy = "untrusted" is no longer supported; remove this setting` (PROBED with `codex debug prompt-input -c`). The config reference lists `on-failure` as deprecated (PROBED: `codex debug prompt-input -c 'approval_policy="on-failure"'` gives no error; what it does at run time is NOT TRACED), and `granular` (`approval_policy = { granular = { ... } }`) only chooses which prompt categories may show, not "ask before each action". So Codex still has no mode that asks before each action.

**MCP server of the app.** The MCP client is the Codex process itself. The Permissions page says "Local and remote MCP servers use their own process or transport", and the network section says the domain rules do not restrict "MCP server connections". PROBED live on 2026-11-21 (`s1/pm-6f37bfff.txt`: "Called tower.org_chart"): with the sandbox on, `tower.org_chart` answered, and `tower.task_update` worked in `s2b`. The MCP tools are not limited by the sandbox.

**Hooks.** The hooks page does not say if a hook command runs inside or outside the sandbox. PROBED live on 2026-11-21: with the sandbox on (`read_only` profile, `edit_in_worktree` profile, and the `:workspace` profile), `tower-hook` reached the app socket, and the app got the status events `starting`, `working` and `idle` (`s1/db.json`, `s2b/db.json`, `s3/db.json`), and `question` for a permission question (`s1b/db.json`, `PermissionRequest` hook). The `idle` event comes from the Stop hook. NOT TRACED: the delivery of a team message through the feedback of the Stop hook while the sandbox is on (no test had a message that arrived during a turn; the messages of `s2b` were delivered at the start and at idle).

**`bwrap` that Codex uses.** The CLI binary has the text "no system bwrap was found on PATH and no bundled codex-resources/bwrap binary was found next to the Codex executable". So it looks for `bwrap` in the PATH first, and then for its own copy. The own copy of this machine (`.../codex-linux-x64/vendor/x86_64-unknown-linux-musl/codex-resources/bwrap`) fails with the same `setting up uid map: Permission denied` (PROBED with the same command as the probe). It is at another path, so an AppArmor profile for `/usr/bin/bwrap` does not cover it. PROBED on 2026-11-21 with `strace -f -e trace=execve` on `codex sandbox` (`step0/strace-sandbox.txt`): when both exist, the CLI runs `/usr/bin/bwrap` (first `/usr/bin/bwrap --help`, then the real `bwrap --as-pid-1 ... --ro-bind / / ...`), and it never runs the bundled copy. The bundled copy is only for a machine with no `bwrap` in the PATH. The probe tests the `bwrap` of the PATH, so it tests the right one; the live sessions worked with the same PATH.

**Texts.** The employee instructions (`codex_permission_instructions` in `crates/core/src/app/bus.rs`) say that the CLI enforces the policy and asks the operator (sandbox in effect, with the real paths), or that only the instructions limit the employee (sandbox not in effect). They follow the EFFECTIVE state of that start (`StartContext::codex_sandbox`), so "Automatic" with a failing probe gives the second text. `floor_init.md` says both, with "if the Codex sandbox is in effect". Codex adds its own text to the prompt: for a profile it says "`sandbox_mode` is `workspace-write`: ... editing files in `cwd` and `writable_roots`" (PROBED). That text is wrong for the profile, so the model can try to write in the main checkout, get a sandbox error, and ask for approval. The app instructions say the main checkout is read-only. PROBED live: with the plain prompt (`s1`) the model tried the write, saw that the project folder is read-only, and reported it with no question. When the prompt said "if the sandbox blocks it, ask me for approval to run it outside the sandbox" (`s1b`), the model asked for approval (see the next paragraph). Whether a model asks for approval on its own, with no such order in the prompt: NOT TRACED (it did not in `s1`, `s2b` and `s3`).

**The permission question (PROBED live on 2026-11-21, `s1b`).** Codex asked "Would you like to run the following command? ... python -c ... Path('probe.txt').write_text('NO')" with the options "1. Yes, proceed (y)", "2. Yes, and don't ask again for commands that start with ...  (p)", "3. No, and tell Codex what to do differently (esc)" (`s1b/question-log.txt`, screenshot `s1b/2-question-screen.png`). The app status went to `question`, the top strip said "Maya (Project Manager) asks a question" with the button "Open" (amber), the tab was amber with "ASKS", and the character showed `?` (`s1b/result.json`, `question_dom`). The operator answered `esc` (option 3) through `pty_write`: the status went to `idle` (Codex stopped the turn: "Conversation interrupted"), the strip and the amber tab went away, and `probe.txt` was not made. Answers 1 and 2 were not tried (NOT TRACED).

**Git in a worktree.** The `.git` file of a task worktree points to the main repository, and the main `.git` is read-only in the profile. PROBED live (`s2b`) and with `codex sandbox` (`instr/instr-A-floor-in-var-tmp.txt`): in the worktree `git status` and `git diff` work (exit 0); `git add` and `git commit` fail with `Unable to create '<floor>/.git/worktrees/T-1/index.lock': Read-only file system` (exit 128). The app commits for the employee at review (commit `T-1: Plum file` made by the app in `s2b`), and the button "Merge into main" merged the branch, removed the worktree and the branch, and set the task to `done`. This is expected.

| Option | Result |
|---|---|
| A. The operator adds an AppArmor profile that lets `bwrap` use user namespaces (one `sudo` step, **done on 2026-11-21**) | Built; since the decision of 2026-11-21 it is the default when the probe says `works` ("Automatic"). PROBED live on 2026-11-21: with the profile and the switch on, the Codex sandbox enforces `read_only`, `edit_in_worktree` (git floor and no git) as the table above says, and the hooks, the MCP tools, the permission question, resume, the app commit and the merge work |
| B. Codex runs with `-s danger-full-access -a on-request` | Used for "Never", and for "Automatic" when the sandbox does not work. The CLI does not enforce the policy; only the role instructions limit the agent. A "read-only" Codex CEO or Project Manager can write in the main checkout |

### 4.8 Worktrees (D4)

Changed in M3 (2026-11-21). The first draft let the session of an employee run in its worktree. A session cannot change its folder, and one employee has one session. So the session stays in the main checkout, and the worktree is inside the floor folder, where the session can reach it.

- When a task is assigned to an employee with the policy `edit_in_worktree`, and the floor folder is the top of a git repository that has a commit, and the main checkout is on a branch, the app creates a worktree and a branch `task/<id>-<slug>` at `task_assign`, before it sends the task message. The folder is `{CWD}/.swarm/worktrees/<task-id>/`. The base is the branch that the main checkout has at that moment. The app stores the branch, the base branch, the base commit and the worktree folder in the task. The slug uses `[a-z0-9-]` only (at most 40 characters). A task that is given again keeps its worktree.
- The task message tells the employee: the absolute worktree path, the branch, "make all your changes inside this folder, never in the main checkout", "do not commit, the app commits for you", and the absolute path of the report file in the reports folder of the MAIN checkout.
- A floor that is not a git repository, that has no commit, that is a subfolder of a repository, or that has a detached HEAD gets no worktree. The task works as in M2: the message says that the employee edits the project folder directly, and `task_merge` says that there is nothing to merge and sets `done`. An employee with `read_only` or `ask_always` gets no worktree.
- When the task goes to `review`, the app commits the worktree (`git add -A`, `git commit`, message `<task-id>: <title>` and a line `By: <Name> (<Role>)`). If the repository has no user, the commit gets the name and an e-mail of the employee; else it keeps the identity of the operator. No change: no commit, and the note of the status event says so. The same snapshot runs before a merge. The Project Manager can read the diff with `task_diff` and the report file.
- The `pm`, the `ceo` and the operator decide the merge. `task_merge(task)` needs the status `review` or `blocked`. The app runs git, so the Project Manager keeps its `read_only` policy. Steps: (a) commit the worktree; (b) the main checkout must be on the base branch of the task, else the task is `blocked` with the reason; (c) `git status --porcelain --untracked-files=no` must show no change outside `.swarm/`, else `blocked` with the reason; (d) `git merge --no-ff --no-edit`, and on a conflict `git merge --abort`, status `blocked`, and the answer lists the files; (e) on success, status `done`, `git worktree remove`, `git branch -d`. The app never uses `--force`, `reset`, `checkout` in the main checkout, or `push`.
- The result goes to the caller (the tool answer), to the assignee and the assigner (a task message that waits when the receiver has no live session), and to the UI (`tasks-changed`, and a toast for `blocked`). The Project Manager assigns a conflict to an engineer.
- `task_diff(task)`: any employee of the floor. It returns `git diff --stat` and the patch of `<base commit>...<branch>`, cut at 60000 bytes with a note. For a task with no worktree it says so.
- The operator command `task_discard` removes the worktree (`git worktree remove --force`, on the task worktree only) and the branch (`git branch -D`, on the task branch only) of a task that is not `done`, and sets the status `blocked` with the note "discarded by the operator". The UI asks for a confirmation.
- Each git command of the app runs in an explicit folder, with `-c core.hooksPath=/dev/null` where a hook of the repository could run (a hook of the operator does not run for an app action), with a time limit, and with a fixed list of arguments. No task title becomes an argument that starts with `-`.
- The worktree has its own copy of each tracked file, `.swarm/employees/` too when the project tracks it. A change of that copy is a change of the task branch. Its merge is the decision of the `pm` and of the operator (see the probe in 4.7).
- All sessions run in the main checkout, read-only employees too.

### 4.9 Persistence

Each floor keeps its data in its own folder. The app keeps only the list of floor paths, and the settings that an agent of a floor must not change, in its own config folder. Changed 2026-11-21: the settings are the program paths, the logo key, the model names, and the rules of each floor for commands with no question (table `[floor_permissions]` of `programs.toml`, see 4.7).

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
Changed in M3 (2026-11-21): before it kills the sessions at the close, the app stores the employees that had a live session (table `resume_marks` of `floor.db`). A manual stop of a session takes its employee out of the list. The office header shows the button "Resume the last team (N)" when the list is not empty. A click resumes each employee in order, managers first, with a pause of 2 s between two starts, in the background; the button shows the progress, and a start that fails shows as a toast and does not stop the others. Nothing resumes at the start of the app, because each start costs tokens. The contract is in `docs/ipc-m3.md`.

Transcript: the app records the PTY output of each session. It also stores the path of the CLI
transcript file (`~/.claude/projects/...jsonl` VERIFIED location, internal format; `~/.codex/sessions/...` VERIFIED location).

### 4.10 Reports

A manager does not read the transcript of a report. It gets messages, task notes and a report file.

- Each floor has a reports folder: `{CWD}/.swarm/reports/`. Git ignores it by default, so reports are not merged into the code.
- When an employee completes a task, it writes its report to `{CWD}/.swarm/reports/<task-id>.md`: what it did, what it verified, what it did not do, and open questions.
- The employee gives the path in `task_update(..., report)` and in its message to the manager. The message is short and refers to the file. It does not copy the content.
- The role instructions contain this rule and the folder path.
- Each session gets access to the reports folder (Claude: `--add-dir`, flag seen in `claude --help`; Codex with the sandbox switch on: the profile gives `write` for the reports folder, PROBED on 2026-11-21, item N10).

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
| N6 | Codex approval and sandbox settings | DOES NOT WORK with the sandbox off ("Never", or "Automatic" with a sandbox that does not work). Decision O7 (4.7). Update 2026-11-21 (PROBED live, `/tmp/agent-tower-review/sandbox/`): after the operator added the AppArmor profile, the sandbox works with the switch on: the flags of option A are accepted by Codex 0.171.0, `bwrap` of the PATH is used, the policies are enforced, and a real permission question reached the app. NOT TRACED: a model that asks on its own; the answers 1 and 2 of the question |
| N7 | CLI session ID of a live session | WORKS (4.2). Resume keeps the context in both CLIs |
| N8 | `rmcp` crate as the MCP server | CLOSED in M2a: plan B. A hand-written HTTP endpoint (`crates/core/src/app/mcp.rs`, std `TcpListener` and threads, JSON answers) works with both CLIs in the live test. No `rmcp`, no async runtime |
| N9 | Long blocking `wait` | WORKS up to about 60 s in Claude. `wait` is limited to 50 s (4.4) |
| N10 | Codex access to the reports folder outside a worktree | PROBED on 2026-11-21 with the switch on: the profile gives `write` for `.swarm/reports`, and a `read_only` and an `edit_in_worktree` Codex employee wrote their report there with no question (`s1/result.json`, `s2b/reports/T-1.md`). With the switch off, Codex has full access (as before) |
| N11 | How `question` ends when the operator denies a prompt, both CLIs. Claude Ctrl-C and the 2 s quiet rule | PROBED at step 0 (4.3). Evidence in `probes/step0/` |
| N12 | Claude permission mode and tool rules for `read_only` and `edit_in_worktree` | CLOSED in M3 (2026-11-21), PROBED with six `claude -p --model haiku` calls (4.7) |
| N13 | First message as the `[PROMPT]` argument when a startup dialog shows. Codex `check_for_update_on_startup=false` | WORKS, PROBED at step 0 (4.2). Not traced: Codex without `--no-daemon`, and the Codex option "Continue without trusting" hooks |
| N14 | A Codex agent acts on a message that the Stop hook delivers, when the role instructions explain it | CLOSED on 2026-11-21, PROBED live in the real app: Codex `gpt-6-luna`, low effort, the real role instructions of the app (`team_instructions`). A message row waited while Codex wrote a story. At its `Stop` the app returned the `block` answer with the text "This is not an error. Messages from your team arrived while you worked..." and a `> Please say the word <W> in your next reply.` body. Codex showed "Blocked by hook", kept the same turn, called `tower.send` with the word and then said it, 3 of 3 times (KIWI, LIME, FIG; KIWI in one session, LIME and FIG in a second session). The status stayed `working` through each block. Codex has `stop_hook_active`, so the guards are the same as for Claude. The first gate 1 probe saw Codex call the same text untrusted, with no role instructions. Decision: the Stop hook delivers to Codex too. Evidence: `/tmp/agent-tower-review/gaps/stop-probe/` (`run1/rollout.jsonl`, `run2/rollout.jsonl`, `analyse.py`) |
| N15 | Nested worktrees: parent `CLAUDE.md` loading, test runners that collect `.swarm/worktrees/` | Changed in M3 (2026-11-21): the session stays in the main checkout, so Claude loads the `CLAUDE.md` of the main checkout as before. A worktree copy of `CLAUDE.md` loading when an employee reads files in the worktree, and test runners that collect `.swarm/worktrees/`: NOT TRACED |

## 9. Points closed by the operator (2026-11-19)

| # | Question | Answer |
|---|---|---|
| O1 | One CEO for each floor, or one for the building? | One for each floor |
| O2 | Who can message whom? | Each employee can message each other employee of the floor (4.4) |
| O3 | Who merges a task branch? | The Project Manager, with `task_merge` (4.8) |
| O4 | Where is the task board on the screen? | A column on the right side (5) |
| O5 | Platforms | Ubuntu first. Windows and macOS are possible later, not a requirement |
| O6 | Does a manager see the full transcript of a report? | No. The employee writes a report file and refers to it (4.10) |

| O7 | Codex sandbox on this machine (4.7) | Option B for M1 and M2. Option A (AppArmor profile for `bwrap`) was to be tested before M3; if it worked, it would become the rule. Decision for M3 (2026-11-21): option B stays. Update of 2026-11-21: the operator added the AppArmor profile (option A) with `sudo`; that step is DONE. The app side was first behind a switch that was OFF by default (Settings, "Codex sandbox"; `[codex] sandbox`; the command `codex_sandbox_probe`); the default changed later on 2026-11-21 (see the decision below). Option A is PROBED live on 2026-11-21 (evidence in `/tmp/agent-tower-review/sandbox/`, 6 real Codex sessions with `gpt-6-luna`): see 4.7 and N6. **DECISION OF THE OPERATOR, 2026-11-21 (after the recommendation below): the default is "on when the app's test says that the sandbox works".** Built as three stored states: Automatic (no key; the default), Always (`sandbox = true`) and Never (`sandbox = false`), see 4.7 and `docs/ipc-final.md`. The recommendation was: make it "on when the probe says `works`, off when it does not", and keep the switch so that the operator can turn it off. Reason: the sandbox gives real enforcement of `read_only` and `edit_in_worktree` that the instructions alone do not give, and the live test showed no loss (hooks, MCP, resume, review commit, merge, and the question all work). Two conditions: (1) the profile feature of Codex is beta, so rerun `the_real_codex_*` tests when the Codex version changes; (2) answers 1 and 2 of the permission question and a model that asks on its own are NOT TRACED. The operator accepted this on 2026-11-21, so the default is now "Automatic". The exact flags and what is `NOT TRACED` are in 4.7 |
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
