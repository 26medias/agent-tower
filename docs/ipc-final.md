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
