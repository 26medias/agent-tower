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
