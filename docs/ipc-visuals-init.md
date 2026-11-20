# Contract: building and office view, and floor setup ("project init")

This file adds to `docs/ipc-m1.md`. All rules of that file stay in effect (snake_case command names,
camelCase JSON fields, string errors). The Rust side and the UI side are built at the same time against
this file. A change to this file needs both sides to change.

## 1. Scope

1. The UI shows the building (one floor for each known project) and the office of the open floor, with
   the pixel art from `prototype/` (see `prototype/building.html` for the layout rules and the numbers).
2. Each floor has a logo sticker. The app generates it with the OpenAI image API and stores it in
   `{CWD}/.swarm/assets/logo.png`.
3. Floor setup: a "setup agent" (a normal CLI session with special instructions) reads the project,
   proposes employees, asks the operator, and writes the floor files.

Not in scope: the MCP server, tasks, messages. The setup agent writes files directly. The MCP setup
tools of spec 4.0.1 replace this later (milestone M2).

## 2. Data on disk

`{CWD}/.swarm/floor.toml` gets one new optional table. Old files without it must load.

```toml
[brand]
description = "One sentence that says what the project is."
logo_prompt = "A golden honeycomb hexagon with a small bee."   # the subject of the logo, no style words
```

`{CWD}/.swarm/assets/logo.png`: the logo sticker, PNG with alpha, at most 512 x 512.

The OpenAI key is never stored in `.swarm/`. The app reads it in this order: the app config file
(the same file that stores the program paths), then the environment variable `OPENAI_API_KEY`.

## 3. Types (additions)

```ts
interface FloorEntry {            // existing fields stay
    logoVersion: number | null;   // modification time of logo.png in ms, null when there is no logo
}

interface FloorView {             // existing fields stay
    description: string | null;   // [brand] description
    logoPrompt: string | null;    // [brand] logo_prompt
    logoVersion: number | null;
    logoBusy: boolean;            // a logo generation runs now for this floor
    logoError: string | null;     // the error text of the last failed generation (in memory only)
    setupSessionId: string | null; // the live setup session, if one runs
}

interface Settings { openaiKeySet: boolean; openaiKeySource: "config" | "env" | null; imageModel: string }
```

`SessionView.slug` is `"setup"` for the setup session. The slug `setup` is reserved: `employee_save`
refuses it. The setup session is in `FloorView.sessions` while it is live or is the last session of
that slug, the same as for an employee.

## 4. Commands (additions)

| Command | Arguments | Result | Notes |
|---|---|---|---|
| `floor_logo` | `path` | `string` or null | Base64 of `logo.png`, null when there is no file |
| `logo_generate` | `path`, `prompt` (string or null) | none | Returns immediately. If `prompt` is not null, the app first writes it to `[brand] logo_prompt`. Uses `logo_prompt`; if there is none, uses the floor name. Sets `logoBusy`, sends `floor-changed`, runs in the background, then clears `logoBusy`, sets or clears `logoError`, and sends `floor-changed` again. Error text starts with `NO_KEY:` when no OpenAI key is known, and with `BUSY:` when a generation already runs for this floor |
| `setup_start` | `path`, `cli` (`claude` or `codex`), `rows`, `cols` | `SessionView` | Starts a fresh setup session in the floor CWD. Refused while a setup session is live for this floor |
| `settings_get` | none | `Settings` | Never returns the key |
| `settings_set` | `openaiKey` (string or null), `imageModel` (string or null) | `Settings` | A null argument means "no change". An empty string for `openaiKey` removes the key from the app config |

`floor_create` does not change: it writes `.swarm/` with the employees `ceo` and `pm`. The UI then
offers "Run the setup agent".

The existing session commands (`session_stop`, `session_attach`, `pty_write`, `pty_resize`,
`clear_draft`) work for the setup session with its `sessionId`. `session_resume` is not offered for it.

## 5. Behaviour of the setup session

- It is a normal session: PTY, hooks, status machine, tab, terminal.
- CLI flags: the same as an employee with policy `edit_in_worktree` (Claude: `acceptEdits`). Model and
  effort come from `[defaults]` of `floor.toml`.
- The instructions (the "floor-init skill") go in through the same path as the role text of an
  employee. Their text is in `crates/core/src/app/floor_init.md` (included with `include_str!`). They tell the agent:
    1. Read `CLAUDE.md`, `AGENTS.md`, `README*` and the top-level layout of the project. Use the file read and search tools, not shell commands. Do not change project files outside `.swarm/`.
    2. Tell the operator in a few lines what the project is, and propose a team: always one `ceo` and one `pm`, plus 1 to 6 other employees with a role, a CLI and a policy for each (a model and an effort only if the operator asks for them or the project documents state them; otherwise "floor default" and the fields stay out of the file). Say what will be written, and write nothing before the operator agrees. Ask the operator to confirm or change it. Wait for the answer.
    3. After the operator agrees: write one file `.swarm/employees/<slug>.md` for each employee, in the exact format that the instructions show (the format of the store), and delete employee files that are no longer wanted. Write `[brand] description` and `[brand] logo_prompt` into `.swarm/floor.toml` and keep the other content of that file.
    4. Tell the operator that the setup is complete and that they can stop this session.
- The first message of the session is a fixed short text: `Start the floor setup.`
- After each turn of the setup session ends (status goes to `idle` or `done`) and when it stops, the
  app reads the floor again and sends `floor-changed`. Employee files that do not load show in
  `employeeErrors`, as today.
- When a setup session starts, the app remembers the value of `logo_prompt` at that time (or "not set").
  After a reload at the end of a turn, the app starts a logo generation by itself if: `logo_prompt` is
  set AND differs from the remembered value (the agent wrote a new subject), a key is known, and no
  generation runs or was tried for this prompt in this app run. It does this also when `logo.png`
  exists: the new subject replaces the logo. A prompt that was in `floor.toml` before the setup
  started, and that the agent did not change, starts nothing.

## 6. Logo generation

- Request: `POST https://api.openai.com/v1/images/generations` with `model` = `imageModel`
  (default `gpt-image-2.6-sunburst`), `size` `1024x1024`, `quality` `medium`, `background` `transparent`, `n` 1.
  The answer has `data[0].b64_json`.
- Prompt: `One die-cut sticker logo with a thick white sticker border, flat front view, bold simple emblem style, no text, no letters: {logo_prompt}. Centered, large, transparent background.`
- After the download: set alpha to 0 where alpha is below 128, crop to the bounding box of the
  remaining pixels, scale down to fit 512 x 512, write `logo.png` (write to a temporary file, then rename).
- The base URL of the API can be changed for tests (config field or constructor argument), so that a
  test can use a local fake server. No test calls the real API.
- Timeout 180 s. The key never goes into a log or an error text.

## 7. UI behaviour

Art files: copy `prototype/web/floor-pixel.webp`, `room-pixel.webp`, `desk-back.webp`,
`agent-0.webp` to `agent-5.webp` to `ui/static/art/`. The numbers for stacking, the sticker position,
the desk grid, the agent position and the name tags are in the script of `prototype/building.html`. Use them as they are.

- **Left: the building.** One storey for each floor of `floor_list`, the first floor at the bottom.
  The sticker of a floor is its logo (`floor_logo`, fetched again when `logoVersion` changes). A floor
  without a logo shows a plain placeholder sticker with the first letter of its name. A click opens the
  floor; the open floor is bright and the others are dim. A missing floor (`missing: true`) is grey.
  With 1 to 3 floors the building is at its natural size at the bottom of the column; with more floors
  the column scrolls. Below the building: the floor name list is not needed; keep "open folder",
  "create", "forget" and the programs panel reachable (a small toolbar or a collapsible panel).
- **Right, top: the office.** A header with the logo, the name, the path, the description, and the
  buttons "Run the setup agent" (asks for Claude or Codex) and "New logo" (a text field for the prompt,
  filled with `logoPrompt`). While `logoBusy` is true the logo shows a spinner. `logoError` shows as
  text; when it starts with `NO_KEY:` the UI shows a field to enter the OpenAI key (`settings_set`).
  The room shows one desk and one character for each employee, with the name tag (name + status).
  An employee with no live session uses the status `stopped` (the character is partly transparent).
  Character choice: role `CEO` gets sprite 0, `Project Manager` sprite 1, each other employee gets a
  stable sprite from 2 to 5 (hash of the slug).
  A click on a character selects the employee: a bar below the room shows the name, role, CLI, model,
  and the buttons that the old cards had (start, resume, stop, edit, remove). If the employee has a
  live session, the click also makes its tab active. The "add employee" form stays reachable.
- **Right, middle and bottom:** tabs and terminal as today. The setup session has a tab named "Setup".
- The debug paste field stays, in a collapsed "debug" area.
- All functions of the M1 UI stay reachable. The mock (`ui/src/lib/mock.ts`) supports the new
  commands, so that the page works in a browser without Tauri.
