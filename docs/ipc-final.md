# Final contract changes: path errors and gone floors

This file adds to `docs/ipc-m1.md` and `docs/ipc-m3.md`. It changes no existing field or command,
with two exceptions that the section "Rules for commands with no question" lists (a new field in
the result of `floor_permissions_set`, and a removed notice kind).

## `FloorView.gone`

```ts
interface FloorView { /* ... all the old fields ... */ gone: boolean }
```

`gone` is `false` in normal use. It is `true` when the folder of the floor is gone: the folder, its
`.swarm/` folder or `.swarm/floor.toml` does not exist at the path of the floor. This is the one
rule. `FloorEntry.missing` (from `floor_list`) uses the same rule, and the app computes it at each
call.

`floor_open` of a gone floor that the app still holds does not fail. It returns the last known view
with `gone: true`. The sessions in it show their live state, so the UI can show a banner with the
name of the floor. The app holds a floor after `floor_open` or `floor_create` until `floor_forget`
or the end of the app. A gone floor that the app does not hold (for example after a restart) gives
`NOT_FOUND:` (see below).

## Commands on a gone floor

Each command that reads or writes a floor (the operator commands, and the MCP tools of the
employees) answers with this error text, and writes nothing:

`FLOOR_GONE: The folder of this floor is gone: <path>.`

The commands that still work on a gone floor are `floor_list`, `floor_forget`, `floor_open` (it
returns the last view, see above) and `session_stop`. Live sessions of a gone floor keep running.
When the folder is back at the same path, the app opens the database again from the path, and the
commands work again.

"Nothing is written" has one exception (changed 2026-11-21). A live session of a gone floor keeps
its database handle, and the handle follows the moved file. So the status events of that session
(`status_events`: a `stopped` event after a stop, for example) are written into the moved
database file. The app writes no row in any other table. The test
`a_moved_floor_is_gone_for_each_command_and_nothing_is_written` counts each table apart and checks
this: only `status_events` can grow.

## Errors of `floor_open` and `floor_create` (the path check)

The app checks the path before any other answer. A `~` or a `~/...` path is first expanded to the
home folder of the app (`$HOME`). The error texts start with a stable prefix:

| Prefix | Text |
|---|---|
| `NOT_ABSOLUTE:` | `NOT_ABSOLUTE: Type the full path of the folder, for example /home/you/project.` |
| `NOT_FOUND:` | `NOT_FOUND: The folder <path> does not exist.` |
| `NOT_A_FOLDER:` | `NOT_A_FOLDER: <path> is a file, not a folder.` |
| `NO_WRITE:` | `NO_WRITE: You cannot write in the folder <path>.` |

`<path>` is the path after the expansion of `~`. The write check is a real one: the app makes a
temporary file and removes it. It tries in `.swarm/` when the folder has one, and in the folder
itself when it has not.

A good folder with no `.swarm/` gives the same answer as before: the error text starts with
`NO_FLOOR:`. The UI offers "create" for that error only.

A folder whose `.swarm` exists but has no `floor.toml` gives `NO_FLOOR:` too (changed 2026-11-21).
An example is a `.swarm` that is a symbolic link to a folder with no `floor.toml`. Before, the
answer was a raw `I/O error on .../.swarm/floor.toml`. The meaning is the same: there is no floor
file at this path. A link that points nowhere already gave `NO_FLOOR:`. The text of the error now
ends with `The folder has no .swarm/floor.toml file.` Note for the UI: "create" fails for these
folders (`.swarm` exists), and the error of the create says so.

## Events

`floor-changed` (`{ path }`) is sent when a floor becomes gone, and when it is back. The UI calls
`floor_open` again (and `floor_list`, because `missing` changed).

The `notice` event (`{ path, kind, text }`, see `docs/ipc-m3.md`) has two new kinds. A background
thread of the app checks the open floors each 3 seconds. It sends each notice one time for each
change, not for each check.

| Kind | Text |
|---|---|
| `floor_gone` | `The folder of the floor <name> is gone: <path>.` The app adds ` N sessions of it still run.` (or ` 1 session of it still runs.`) when at least one session of the floor is live. |
| `floor_back` | `The folder of the floor <name> is back: <path>.` |

The app sends `floor-changed` first and then the notice.

### The notice `exchange_stopped` (2026-11-21)

The app refused a plain message (`send`, or `employee_start` with a message) because two employees
sent each other 8 messages in 5 minutes, in both directions together (`docs/spec.md` 4.5,
"Answers and exchanges"). The `notice` payload has one new optional field: `slug`, the slug of the
employee that the app refused. Only this kind has it. The payload of every other notice is the same
as before.

```ts
interface NoticePayload { path: string; kind: string; text: string; slug?: string }
```

| Kind | When | Text | UI |
|---|---|---|---|
| `exchange_stopped` | The FIRST refusal of a window for a pair of employees. ONE for each pair (the order of the two does not matter) and window, not one for each refused call. A later refusal in the same window sends none. After the window, a new refusal sends a new notice. | `The app stopped an exchange between <Name of the refused employee> and <Name of the other employee>: 8 messages in 5 minutes.` | Error toast that stays until the operator closes it, with the button "Open" for the tab of `slug`. One row in "Team activity" (kind `exchange`) with the same text. A click on the row goes to the same tab. |

The messages of the operator and the task messages (`task_assign`, `task_status`) do not count and
are never refused. The window is 5 minutes (`EXCHANGE_WINDOW`) and the limit is 8
(`EXCHANGE_LIMIT`), both in `crates/core/src/app/bus.rs`.

## Rules for commands with no question (changed 2026-11-21)

The rules moved from `[permissions]` of `floor.toml` to the app config (see `docs/ipc-m3.md`,
"Where the rules are", and `docs/spec.md` 4.7).

```ts
// New command. The rules of the old `[permissions]` table of floor.toml that pass the check.
floor_permissions_proposed(path: string): Promise<string[]>

// Changed result. It was `string[]`. The old array is `allow`.
floor_permissions_set(path: string, allow: string[]): Promise<PermissionsSaved>
interface PermissionsSaved { allow: string[]; warnings: string[] }
```

- `floor_permissions_get` and `floor_permissions_set` keep their names and their arguments. They
  read and write the app config.
- `floor_permissions_set` removes the `[permissions]` table from `floor.toml` and keeps the rest of
  the file.
- `floor_permissions_proposed` is empty when the app config has an entry for the floor (also an
  empty list). The app never imports the old table by itself.
- `floor_forget` removes the entry of the floor from the app config.
- The `notice` kind `permissions_restored` is removed. The app does not send it any more.

## Other repairs of the same date

- `task_update` with `status: "done"` from an employee on a discarded task is refused with `Task
  T-12 was discarded by the operator: discarded, so you cannot set it to done. The status stays
  blocked.` The operator can still set `done`. This is the rule of `task_merge`.
- The snapshot of a worktree refuses more files. A new file is refused when its name looks like a
  secret (`*.env` but not `example.env` or `sample.env`, `credentials`, `credentials.json`,
  `.npmrc`, `.netrc`, `.pypirc`, `id_ecdsa*`, `id_dsa*`, `*.keystore`, `*.jks`, `*.kdbx`), or when
  its path has a folder `.aws`, `.ssh` or `.gnupg`, or a folder of build output (`out`, `.next`,
  `.nuxt`, `coverage`, `.gradle`, `.tox`, `.pytest_cache`, `.mypy_cache`, and the earlier ones).
  Folder names are compared with no regard to letter case. The list is not complete. The message of
  an assignment says so, and tells the employee to put no secret in the worktree.

# Changes of 2026-11-21 (the Stop hook, the notification, Browse, program test)

## The Stop hook asks the app (spec 4.5, first path)

This is the wire between the hook program and the app. It is not a Tauri command.

- The program `tower-hook` sends one line to the socket, as before:
  `{"session": "<TOWER_SESSION>", "event": <hook JSON>}`.
- For an event with `hook_event_name` = `Stop`, the line has one more field: `"reply": true`.
  The program then waits for one answer line on the same connection. It waits 2 s at the most
  for the whole run (other events: 1 s, and no answer). The app can close the connection with
  no answer: that means "nothing to say".
- The answer line is `{"decision":"block","reason":"<text>"}`. The program prints exactly
  `{"decision":"block","reason":"<text>"}` and a newline on stdout, and the CLI continues the
  turn with the reason. The program prints nothing in each other case: a closed connection, a
  bad answer (any other JSON, or a `reason` that is empty or not text), a time out, a dead
  socket. It always exits with code 0.
- The app answers for the session of an employee, Claude or Codex (spec item N14 is closed: Codex
  acts on the text, PROBED on 2026-11-21). The setup session gets no answer (the connection closes
  at once).
- The `reason` is the line `This is not an error. Messages from your team arrived while you
  worked. They are from employees, not from the operator.`, an empty line, and the waiting messages. The messages have
  the same selection, order, text (header, the `> ` mark on each body line, the sanitize rule)
  and holds as the paste. At most 60000 bytes of messages go in one answer (the oldest message
  always goes in); the others go in a later answer or by the paste. A message is marked delivered
  only after the answer was written to the program. The app also sets no "message waiting" mark
  then.
- The app blocks a stop only when all of these are true: the session is `working` and has had a
  first turn; a message waits that is not held (a held first message and a message that `inbox`
  has claimed do not count); at most 5 stops in a row were blocked (`STOP_BLOCK_LIMIT`); the
  payload has `stop_hook_active` = `false`, or it has `true` and a message newer than the last
  block waits; the request is less than 1.8 s old. Otherwise the connection closes, the `Stop`
  gives `idle` as before, and the paste delivers the messages.
- A blocked stop keeps the status `working`: the app tells the status machine
  (`Input::StopBlockedByApp`) after it wrote the answer and before the machine gets that `Stop`.
  The status never shows `idle` between, and the paste is not tried.

The payload field `stop_hook_active` is in the real Claude payloads: `probes/logs/claude-hooks.jsonl`
has a `Stop` with `false`, and the `Stop` that follows a block of the probe hook has `true`. The real
Codex payloads have it too: `probes/logs/codex-hooks.jsonl` has a `Stop` with `false`, and the `Stop`
that follows a block of the probe hook has `true`. So both CLIs use the same guards.

## `Settings.notifyOnQuestion`

```ts
interface Settings { /* ... all the old fields ... */ notifyOnQuestion: boolean }  // default true

// New optional third argument. A missing argument or null means "no change".
settings_set(openaiKey: string | null, imageModel: string | null, notifyOnQuestion?: boolean | null): Promise<Settings>
```

`notifyOnQuestion` is stored in the app config file `programs.toml`, table `[interface]`, key
`notify_on_question`. The default (`true`) has no key: the app removes the key (and an empty
table) when the value is set to `true`. A value that is not a boolean is read as `true`.

## The Codex sandbox: three states, `codexSandboxMode`, `codexSandboxEffective` (2026-11-21, decision O7)

Decision of the operator on 2026-11-21: the sandbox is ON by default **when it works** on the
machine. The setting "Codex sandbox" has three stored states.

```ts
type CodexSandboxMode = "auto" | "on" | "off"
type CodexSandboxEffective = "on" | "off" | "refused"

interface Settings { /* ... all the old fields ... */
  codexSandboxMode: CodexSandboxMode            // the stored state; default "auto"
  codexSandboxEffective: CodexSandboxEffective  // what a Codex start gets now
  codexSandbox: boolean                         // OLD field; kept; see below
}

// New optional fifth argument `codexSandboxMode`. A missing argument or null means "no change".
settings_set(openaiKey: string | null, imageModel: string | null,
             notifyOnQuestion?: boolean | null, codexSandbox?: boolean | null,
             codexSandboxMode?: CodexSandboxMode | null): Promise<Settings>

codex_sandbox_probe(): Promise<{ state: "works" | "fails" | "missing"; detail: string }>
```

**The three states and how the app stores them** (`programs.toml`, table `[codex]`, key `sandbox`;
other keys of `[codex]` stay):

| `codexSandboxMode` | Word in Settings | Stored as | A Codex start |
|---|---|---|---|
| `auto` (default) | Automatic (recommended) | no `sandbox` key | The app runs the probe. If it says `works`, Codex gets the sandbox profile. If not, Codex starts with the old full-access flags (`-a on-request -s danger-full-access`), and the app sends ONE information notice in each run of the app (below). No refusal. |
| `on` | Always | `sandbox = true` | The app runs the probe. If it says `works`, Codex gets the sandbox profile. If not, the start is refused with `SANDBOX_DOWN:` (below). |
| `off` | Never | `sandbox = false` | Full access. No probe. |

Migration: an existing file with `sandbox = true` stays `on`. A file with no key is now `auto`
(before 2026-11-21 it meant "off"). A value that is not a boolean is read as `auto`.

**`codexSandboxEffective`** is what a Codex start gets now: `on` (the sandbox profile), `off` (full
access: the mode is `off`, or the mode is `auto` and the sandbox does not work) or `refused` (the mode
is `on` and the sandbox does not work, so a Codex start is refused). The app takes it from the stored
mode and from the LAST probe of this run of the app (any probe: "Test again", `settings_set` with `on`,
a Codex start). If no probe ran yet, `settings_get` runs one. A later change of the machine shows after
the next probe. The UI reads `settings_get` again after each "Test again".

**`codexSandbox` (the old boolean field, kept, no rename)** now has the meaning "effective state":
`true` exactly when `codexSandboxEffective` is `on`. It is `false` for `off` and for `refused`. An old
client that only reads this field sees whether a Codex start gets the sandbox now.

**`settings_set`.** `codexSandboxMode` is the new argument: `"auto"` removes the key, `"on"` stores
`true`, `"off"` stores `false`. Any other word is an error (`REFUSED: codexSandboxMode must be "auto",
"on" or "off", not "<word>"`) and nothing changes. The old boolean `codexSandbox` still works: `true`
is `"on"` and `false` is `"off"` (stored as `sandbox = false`, not as "no key"). A call with both
arguments uses `codexSandboxMode`. Choosing `on` ("Always") runs the probe again and refuses the change
unless it says `works`. The error text is `the Codex sandbox does not work on this machine, so it
cannot be set to "Always": <detail>` (`fails`) or `the Codex sandbox cannot be set to "Always":
<detail>` (`missing`). The command checks the sandbox setting first: a refusal changes nothing else of
the same call. `auto` and `off` are always allowed. The setting applies at the next start of a session
(the app reads it in `launch`). It never applies to a Claude employee or to the setup session.

`codex_sandbox_probe` finds `bwrap` in the PATH of a session (the same environment as
`program_test`), runs `bwrap --unshare-user --ro-bind / / true` with no shell and a time limit of
5 s, and starts no Codex session. `works`: exit code 0 (`detail` is `bwrap works: <path>`).
`fails`: `bwrap` ended with an error (`detail` is the first line of its error, for example
`bwrap: setting up uid map: Permission denied`), or it did not end in 5 s. `missing`: no `bwrap` in
the PATH.

**The check at a Codex start.** In the states `auto` and `on` the app runs the probe first. A `works`
result counts for 60 s (`SANDBOX_WINDOW`), so the start of a team runs the probe one time. A failure
is never kept: the next start runs the probe again. The button "Test again" and `settings_set` with
`on` run the probe too, and they update the same result. The app never changes the stored value by
itself. The instructions of the employee (`codex_permission_instructions`) and the flags of the
adapter both follow the EFFECTIVE state of that start.

**The refusal (state `on` only).** If the probe does not say `works`, the start is refused. The
error text is:

`SANDBOX_DOWN: <Name> did not start: the Codex sandbox does not work on this machine now (<detail>). The setting "Codex sandbox" is "Always". Repair the sandbox, or choose "Automatic" or "Never" in Settings > Codex sandbox.`

(Before this wording on 2026-11-21 the text started with "The Codex sandbox does not work ... The start
of <Name> is refused". The fact that matters, which employee did not start, now comes first.)
`<detail>` is the `detail` of the probe, and `<Name>` is the name of the employee. The refusal applies
to `session_start`, `session_resume`, "Resume the last team" (`team_resume`), and a start by an
employee through the tools `employee_start`, `send` and `task_assign`. It does not apply to a Claude
employee or to the setup session.

- A command of the operator (`session_start`, `session_resume`) returns the error text above.
- `team_resume` starts each employee in turn. A Claude employee resumes. A refused Codex employee
  stays in the list `resumable`, and the team gets ONE `notice` (see below), not `resume_failed`.
- A tool answers with one line that starts with `SANDBOX_DOWN: <Name> did not start: the Codex
  sandbox does not work on this machine now (<detail>). The operator set "Codex sandbox" to "Always".
  Your message is saved and waits. Do not send it again.`

**The notices.**

| Kind | When | Text | UI |
|---|---|---|---|
| `sandbox_down` | A start by a tool, or a team resume, is refused (state `on`). At most one in each 60 s. | The `SANDBOX_DOWN:` error text of the first refused start. | Error toast that stays, with the button "Open Settings". |
| `sandbox_auto_off` | State `auto`, and the probe does not say `works` at a Codex start. ONE in each run of the app, not one for each start. A later start in the same run sends none. | `The Codex sandbox does not work on this machine, so Codex employees run with full access. See Settings > Codex sandbox.` | Information toast that stays, with the button "Open Settings". |

The button "Open Settings" opens Settings at the section `codex-sandbox`. The UI also gives that
button to the error text of a refused start by the operator (the text starts with `SANDBOX_DOWN:`).

**The section "Codex sandbox" in Settings.** One control with three choices, in these words:
"Automatic (recommended): use the sandbox when it works on this machine", "Always: refuse to start a
Codex employee when the sandbox does not work", "Never: Codex employees have full access". Above it:
the probe result and the button "Test again". Under it, one line says what is in effect now, from
`codexSandboxEffective`:

| `codexSandboxEffective` | The line |
|---|---|
| `on` | `In effect now: the sandbox is on.` |
| `off`, mode `off` | `In effect now: the sandbox is off, because you chose Never.` |
| `off`, mode `auto` | `In effect now: the sandbox is off, because it does not work on this machine (<probe detail>).` |
| `refused` | `In effect now: Codex employees cannot start, because the sandbox does not work on this machine (<probe detail>).` A red line also says what to do. |
| (a test runs) | `In effect now: testing the sandbox...` |

"Always" can be chosen only when the last test says `works` (or when it is chosen already, so that the
operator can leave it). "Automatic" and "Never" can always be chosen. The text under the probe result
for `fails` is: "On Ubuntu 24.04 and later, AppArmor blocks bwrap by default. Add an AppArmor profile
that lets /usr/bin/bwrap use user namespaces, then press Test again." For `missing` it is "Install
bubblewrap, so that the program bwrap is in the PATH of the sessions." A line is always shown: "With
the sandbox on, the commands of a Codex employee have no network access (for example a package
install fails)."

## The desktop notification for a question

When a session changes to `question`, the core sends `Observer::question_asked` (the data: the
session, the name and the role of the employee, the name of the floor, the old and new status,
and the switch). The shell decides with `tower_core::app::should_notify(old, new, window_focused,
enabled)`: only a change TO `question`, with no focus on the window (or a minimized window), and
with the switch on. It then calls the official plugin `tauri-plugin-notification` with the title
`Agent Tower` and the body `<Name> (<Role>) asks a question in <Floor name>`, for example
`Eli (Engineer) asks a question in Fin Check`. The setup session is `Setup agent (Setup)`.

- One notification for each question. A change of the draft flag in `question` is not a new one.
- A click on the notification is not handled (the desktop plugin gives no click event).
- The permission is `notification:default` in `src-tauri/capabilities/default.json`.
- A debug build writes the line `agent-tower: notification: <title>: <body>` on stderr.

## The "Browse..." button

The UI opens the system folder dialog with `@tauri-apps/plugin-dialog` (`open`, directory mode,
one folder) and puts the path in the field. It does not open the floor. No Rust command. The
permission is `dialog:allow-open`. In a browser (the mock) the button is disabled, and its title
says why.

## `program_test` and the reset of a program path

```ts
program_test(name: "claude" | "codex" | "node"): Promise<ProgramTestResult>
interface ProgramTestResult { ok: boolean; path: string | null; output: string }

interface Programs { /* ... all the old fields ... */
    overrides: { claude: string | null; codex: string | null; node: string | null }
}
```

- `program_test` runs `<resolved path> --version`. It has no shell. It has the environment of a
  session (`SessionEnv` and `build_env`, with the folders of `claude`, `codex` and `node` in
  front of `PATH`), the home folder as its working folder, no stdin, and its own process group. A
  program that does not end in 10 s is killed with its group. It starts no model session.
- `ok` is true when the program ended with exit code 0. `path` is the path that the app tried,
  or null when the app found no program with this name (then `ok` is false and `output` is the
  error text of the search). `output` is the first line of stdout (for a success with no stdout:
  of stderr; with no text at all: `(no output)`), or the error text: `cannot run it: ...`, `exit
  code N: <first line of stderr>`, `ended by signal N`, `it did not end in 10 s`. It has at most
  200 characters and no control character (a terminal escape sequence is removed).
- A name that is not one of the three is an error text (`unknown program "x" (use claude, codex
  or node)`).
- "Reset" is `programs_set(name, "")`. An empty path removes the override of that program from
  `programs.toml` and the app searches again. The result shows the path that the app finds
  (`null` if it finds none). `programs_set` trims the path, so a path of spaces resets too.
  `overrides` in `programs_get` and `programs_set` tells which programs have an override of the
  operator; the UI shows "reset" only for them.

# Changes of the last repairs (2026-11-21: the message event, `rtk`, the home folder)

## The event `message-delivered`

The app tells the UI when it DELIVERED a message to the session of an employee. The UI shows it as
a row in "Team activity" and as a short mark on the tab of the receiver. A message that is only
stored, or that waits, makes no event.

```ts
// Tauri event "message-delivered". The core calls Observer::message_delivered(path, event).
interface MessageDelivered {
    path: string;            // the key of the floor
    fromSlug: string;        // "operator" for a task that the operator gave
    fromName: string;        // "Operator" for the operator. A removed employee: its slug.
    toSlug: string;
    toName: string;
    kind: "message" | "task";
    taskId: number | null;   // the task of a "task" message, else null
    /** A status message about a task: the status that it names ("review"). "" when the text has
        no valid status line. null for an assignment and for each message that is not about a task. */
    taskStatus: string | null;
    preview: string;         // the first 80 characters of the body (see below). Never the full text.
}
```

- The app sends one event for each message, oldest first, after it wrote the message to the
  session. There are four ways of delivery, and each one makes the event: a paste, the Stop hook,
  the tool `inbox`, and the first message of a session that starts.
- The event comes after the write. A message that a failed write left waiting makes no event
  (the paste path can deliver it later, and then it makes the event). A held first message makes
  the event when its session reaches the first `working`.
- `preview`: the body of the message (not the header, not the orders of the app), with each
  control character and each hidden character removed (the sanitize rule of the paste), each
  newline and tab turned into a space, and then cut to the first 80 characters (Unicode scalar
  values). For a task message the body is `Title: ...` (and `Status: ...`).
- The UI row is `Maya → Eli: <preview>` for `kind` = `message`. For `kind` = `task` it is
  `Maya → Eli: task T-3 assigned` when `taskStatus` is `null`, and `Maya → Eli: T-3 is now in
  review` when it has a status word. A click goes to the tab of the receiver.

## `Programs.rtk`

```ts
interface Programs { /* ... all the old fields ... */ rtk: string | null }
```

`rtk` is the path of the optional program `rtk` (Rust Token Killer), or `null` when the app finds
none. The app looks for it as for `claude` (the login shell, then the known folders). The app
needs it for nothing. It has no override: `programs_set` and `program_test` still accept only
`claude`, `codex` and `node`. The UI uses it to show the examples of the rules in Settings that
fit this machine (`Bash(rtk cargo test:*)` when `rtk` is found).

## `Settings.homeDir`

```ts
interface Settings { /* ... all the old fields ... */ homeDir: string | null }
```

`homeDir` is `HOME` of the environment of the app (`null` if it is not set). The UI opens the
"Browse..." dialog in the parent folder of the last floor of the list that is not missing. With
no such floor it opens the dialog in `homeDir`. With no `homeDir` it gives no start folder.

## MCP tools for the look of a floor (added 2026-11-22)

Three tools of the MCP server `tower` (`crates/core/src/app/look.rs`). Any employee of the floor
may call them. They change no IPC type: after a change the app sends `floor_changed`, and the UI
reads `name`, `description` and `logoVersion` as before.

| Tool | Result |
|---|---|
| `floor_look()` | JSON: `name`, `description`, `project_folder`, `has_logo`, `logo_file`, `logo_subject`, `logo_generation_runs`, `logo_generation_error` |
| `floor_logo_set(file)` | The app reads the picture and writes `.swarm/assets/logo.png`. No image API |
| `floor_look_set(name?, description?)` | Writes `name` and `[brand] description` of `floor.toml`. The rest of the file stays |

Rules of `floor_logo_set`:

- `file` is absolute, or relative to the project folder. After the links are resolved, the file
  must be inside the project folder. It must be a regular file of 20 MB at most.
- The picture must be a PNG, a JPEG or a WebP, with no side above 8192 pixels. An SVG is refused
  with the instruction to convert it.
- The app keeps the colours and the alpha values, crops the border that has alpha 0, and scales
  the picture down to fit 512 x 512. A smaller picture keeps its size.
- While a logo generation runs for the floor, the tool refuses. A success clears `logoError`.

Rules of `floor_look_set`: each text is one line with no control character. `name` has 1 to 60
characters. `description` has 300 characters at most, and an empty text removes it.
