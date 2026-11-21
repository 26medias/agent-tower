# Final contract changes: path errors and gone floors

This file adds to `docs/ipc-m1.md` and `docs/ipc-m3.md`. It changes no existing field or command.

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
