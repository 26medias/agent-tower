# M1 contract between the app shell (Rust) and the UI

This file is the contract for plan step 7. The Rust side and the UI side are built at the same time
against it. A change to this file needs both sides to change.

All commands are Tauri commands (`invoke`). Names are snake_case. Arguments and results are JSON,
with camelCase field names. A command that fails returns a string error message that a person can read.

## Types

```ts
type Status = "starting" | "working" | "idle" | "question" | "done" | "stopped";
type CliKind = "claude" | "codex";
type Policy = "read_only" | "edit_in_worktree" | "ask_always";

interface FloorEntry { path: string; name: string | null; missing: boolean }

interface Employee {
    slug: string;            // [a-z0-9][a-z0-9-]*, unique in the floor
    name: string;
    role: string;            // "CEO", "Project Manager", "Engineer", ...
    manager: string | null;  // slug of the manager
    cli: CliKind;
    model: string | null;
    effort: string | null;
    policy: Policy;
    roleText: string;
}

interface SessionView {
    sessionId: string;       // app session ID
    slug: string;            // employee
    status: Status;
    draft: boolean;          // the operator has unsent text in this terminal
    messageWaiting: boolean; // a paste was refused and waits
    canResume: boolean;      // stopped, and a CLI session ID is stored
}

interface FloorView {
    path: string;
    name: string;
    employees: Employee[];
    employeeErrors: string[];        // employee files that did not load
    sessions: SessionView[];         // one for each employee that has a live or a last session
}

interface Programs { claude: string | null; codex: string | null; node: string | null; hook: string }
```

## Commands

| Command | Arguments | Result | Notes |
|---|---|---|---|
| `floor_list` | none | `FloorEntry[]` | The known floors from the app config |
| `floor_open` | `path` | `FloorView` | Imports `{path}/.swarm/`. Error text starts with `NO_FLOOR:` when there is no `.swarm/` |
| `floor_create` | `path`, `name` | `FloorView` | Writes a new `.swarm/` with two employees: `ceo` and `pm` (Claude, policy `read_only`) |
| `floor_forget` | `path` | none | Removes the floor from the list. Does not delete files |
| `employee_save` | `path`, `employee` | `FloorView` | Create or update |
| `employee_remove` | `path`, `slug` | `FloorView` | Refused while the employee has a live session |
| `session_start` | `path`, `slug`, `firstMessage` (string or null), `rows`, `cols` | `SessionView` | Starts a fresh CLI session. If null, the app uses a short default message of the role |
| `session_resume` | `path`, `slug`, `message` (string or null), `rows`, `cols` | `SessionView` | Resumes the last CLI session of the employee |
| `session_stop` | `sessionId` | none | Stops the process |
| `session_attach` | `sessionId`, `channel` (a Tauri `Channel<OutputMessage>`) | `{ scrollback: string }` | `scrollback` is base64 of the recent output (from the log, at most 256 KB). After it, new output comes on the channel. One attach for each session; a new attach replaces the old one |
| `pty_write` | `sessionId`, `data` (base64 of the bytes that xterm.js gave) | none | Operator input. Always goes through |
| `pty_resize` | `sessionId`, `rows`, `cols` | none | |
| `clear_draft` | `sessionId` | none | The operator clears the draft flag |
| `debug_paste` | `sessionId`, `text` | none | The M1 test for message delivery. Error text when the paste is refused, for example `REFUSED: status question` or `REFUSED: draft` |
| `programs_get` | none | `Programs` | Resolved paths of the CLIs |
| `programs_set` | `name` (`claude`, `codex` or `node`), `path` | `Programs` | Operator override, stored in the app config |

`OutputMessage` on the channel: `{ data: string }`, base64 of the output bytes, in order.

## Events (Tauri events, to all windows)

| Event | Payload | When |
|---|---|---|
| `session-changed` | `{ path: string, session: SessionView }` | The status, the draft flag or the "message waiting" flag of a session changed, or a session was started |
| `floor-changed` | `{ path: string }` | Employees changed. The UI calls `floor_open` again |
| `employee-started` | `{ path, sessionId, slug, name, bySlug, byName, resumed }` | An employee started the session of another employee with the tools of the app (`employee_start`, or `send` to an employee with no live session). See below |

### `employee-started` (milestone M2, the people and message tools)

An employee can start another employee through the MCP server of the app. The app starts the session
with the same code as `session_start` (30 rows, 120 columns), so the UI gets the normal events:
`session-changed` with the status `starting`, and the later statuses. The UI did not call
`session_start` for this session, so it must handle a `session-changed` for a session ID that it has
never seen: it makes a tab (a live session always has a tab) and shows the status in the room and in
the list. It must NOT make that tab the active one.

`employee-started` is sent once for each start, after the first `session-changed` of the new session.
`slug` and `name` are the employee that was started. `bySlug` and `byName` are the employee that
started it. `resumed` is `true` when the app resumed the last CLI session. The UI shows one toast:
`"{byName} started {name}"`. The event has no other effect.

## UI behaviour in M1

- Left: the list of floors (plain list; the building view is milestone M4). "Open folder" asks for a path (text field in M1), and offers "create" when the error starts with `NO_FLOOR:`.
- Right, top: the employees of the open floor as a plain row of cards (the office view is M4): name, role, CLI, status dot, buttons "start", "resume", "stop".
- Right, middle: tabs, one for each live session, with a status dot and a "message waiting" mark. A click on the mark calls `clear_draft`.
- Right, bottom: the xterm.js terminal of the active tab. Each tab keeps its own terminal instance, so a tab change loses no output.
- A small form adds or edits an employee.
- A "debug paste" field sends a test message to the active session and shows the refusal text.
- Status colours: starting grey, working blue, idle green, question amber (with a pulse), done teal, stopped dark grey.
