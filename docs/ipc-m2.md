# M2 slice 2 contract: the task board (Rust shell and UI)

This file is the contract for the task board. It adds to `docs/ipc-m1.md`. The conventions of that
file stay: Tauri commands (`invoke`), snake_case command names, JSON with camelCase field names,
and a failed command returns a string that a person can read.

The tools that the employees use (`task_create`, `task_assign`, `task_update`, `task_get`,
`task_list`, `task_comment`) are MCP tools of the app. They are in `docs/spec.md` (4.4). This file
is about the commands of the operator, and the event.

## Types

```ts
type TaskStatus = "todo" | "in_progress" | "blocked" | "review" | "done";

interface TaskView {
    id: number;                // the number of the task in its floor. The UI shows it as "T-" + id
    title: string;             // one line, 200 characters at most
    description: string;       // 20000 characters at most
    parent: number | null;     // the id of the parent task
    creator: string;           // an employee slug, or "operator"
    assignee: string | null;   // an employee slug
    assigner: string | null;   // who gave the task to the assignee: an employee slug, or "operator"
    status: TaskStatus;
    report: string | null;     // the report file, relative to the floor folder: ".swarm/reports/T-12.md"
    createdAt: number;         // Unix ms
    updatedAt: number;         // Unix ms
}

interface TaskEvent {
    id: number;
    at: number;                // Unix ms
    author: string;            // an employee slug, or "operator"
    kind: "created" | "assigned" | "status" | "comment" | "report";
    value: string | null;      // "status": the new status. "assigned": the assignee slug. "report": the path
    text: string | null;       // "status": the note. "comment": the comment
}

interface TaskDetail { task: TaskView; events: TaskEvent[] }  // events: oldest first
```

An event of the kind `status` is written when the status changes. A note with no status change is
an event of the kind `comment`. An event of the kind `report` is written when the report path
changes.

## Commands

The operator has all rights. The author of each change is `operator`.

| Command | Arguments | Result | Notes |
|---|---|---|---|
| `task_list` | `path` | `TaskView[]` | All tasks of the floor, oldest first |
| `task_get` | `path`, `id` | `TaskDetail` | Error text when the task does not exist |
| `task_create` | `path`, `title`, `description` | `TaskView` | Status `todo`. Creator `operator`. The title is one line (a new line becomes a space). Errors: empty title, title over 200 characters, description over 20000 characters |
| `task_assign` | `path`, `id`, `slug` | `TaskView` | Gives the task to the employee. The app sends the employee the message `[task T-12 from the operator]` and starts its session if it has none. An unknown slug is an error |
| `task_update` | `path`, `id`, `status`, `note` (string or null) | `TaskView` | Sets the status. A note is stored as an event (of the kind `status` if the status changed, else `comment`). If the status changed, the person who assigned the task gets a message (nobody gets one when the operator assigned it) |
| `task_report` | `path`, `id` | `string` or `null` | The text of the report file of the task. `null` when the task has no report path. An error text when the file is missing or outside `.swarm/reports/`. At most 256 KB: a longer file is cut, and the text says so at the end |

## Events

| Event | Payload | When |
|---|---|---|
| `tasks-changed` | `{ path: string }` | After each change of the tasks of a floor: a task is made, assigned, updated or commented, by an employee (MCP tool) or by the operator (command). `path` is the key of the floor, as in `FloorView.path`. The UI reads the list again |

## Messages that the app sends for a task

The employee that gets a task sees the header `[task T-12 from <Name> (<Role>)]`, or
`[task T-12 from the operator]`. The body is the title and the description, with `> ` before each
line. The app writes the three orders for the assignee after the body. A status message has the
same header form. Its body has the title, the new status, the note and the report path. These
messages do not count for the limit of 30 messages for each pair of employees.

## Report files

A report is a file in `{floor}/.swarm/reports/`. The app makes this folder when a floor is created
or opened. Git ignores it (`.swarm/.gitignore`). The UI shows the text of a report with
`task_report`, as plain text.

## Mock

`ui/src/lib/mock.ts` implements these commands in memory, with the same argument names and the
same error texts, and it emits `tasks-changed`.
