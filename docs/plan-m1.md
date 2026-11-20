# Plan for milestone M1 — Terminals with status (draft 2)

Spec: `docs/spec.md` (draft 3). This plan covers M1 only.
Date: 2026-11-19. Status: repaired after gate 2 (PASS WITH CHANGES).

## 1. Result of M1

The operator opens the app, opens one floor (a folder), adds employees by hand, and starts Claude and
Codex sessions. Each session is a tab with a real terminal. Each tab shows a correct status:
`starting`, `working`, `idle`, `question`, `stopped`. The operator can stop and resume a session.
The app can paste a test message into an idle session, and it refuses to paste when the rules of spec 4.5 forbid it.

Not in M1: MCP server, tasks, messages between agents, setup agent, worktrees, policies other than one
default for each CLI, generated images, building and office views.

## 2. Toolchain (checked on this machine, 2026-11-19)

| Tool | Version found |
|---|---|
| rustc / cargo | 1.99.1 |
| node / npm / pnpm | 24.10.0 / 11.6.1 / 10.33.2 |
| webkit2gtk-4.1, gtk+-3.0, libsoup-3.0 | 2.52.6, 3.24.41, 3.4.4 |
| OS, display | Ubuntu 24.04.5, X11 |
| `@tauri-apps/cli` (npm) | 2.12.6 available. `cargo tauri` is not installed; use the npm CLI |
| `portable-pty` (crates.io) | 0.9.0 |
| `@xterm/xterm` (npm) | 6.0.0 |

Gate 2 built the `svelte-ts` template with tauri 2.12.7 on this machine in 34 s, with no missing system package.
Tauri: use the stable 2.x line. crates.io shows `3.0.0-alpha.6` as the newest; do not use the alpha.

## 3. Repository layout

```
agent-tower/
    Cargo.toml                workspace
    crates/
        core/                 library, no Tauri dependency
            src/
                model.rs      Floor, Employee, SessionId, Status, CliKind
                status.rs     state machine: (state, event, now) -> state. Pure, no I/O
                hookrx.rs     Unix socket server that receives hook events
                pty.rs        PTY spawn, read loop, write lock, resize, kill
                session.rs    one session: PTY + state machine + draft flag + paste gate
                adapter/
                    mod.rs    trait CliAdapter
                    claude.rs
                    codex.rs
                store.rs      .swarm/ files (floor.toml, employees/*.md) and state/floor.db
                env.rs        clean environment builder
        hook/                 binary `tower-hook`: reads the hook JSON on stdin, sends it to the socket
        fakecli/              binary `fake-cli`: a scripted fake agent for tests (no tokens)
    src-tauri/                Tauri 2 shell: commands and events, no logic
    ui/                       SvelteKit + TypeScript (the `svelte-ts` template of create-tauri-app) + xterm.js
    probes/                   gate 1 probe scripts and logs (regression material)
    docs/
```

Rules:
- All logic is in `crates/core`, so `cargo test -p core` tests it with no window.
- `tower-hook` gets no session data in its arguments. The command text is the same for all sessions (Codex hook trust, spec 4.2).
  The session identity is in the environment: `TOWER_SESSION`, `TOWER_SOCKET`.
- Indentation: 4 spaces in all code.

## 4. Steps

Each step has a test that can fail. "Red on" says which wrong build makes the test fail.

### Step 0 — Probes that the code depends on (N11, N13)
These run first, because steps 2 and 5 depend on the results. Each is one short session. Scripts go in `probes/`.
- N11: how each CLI reports a denied permission prompt (which hook events come), and what Claude does on Ctrl-C.
- N13: does the `[PROMPT]` argument survive a startup dialog (Claude in a folder that is not trusted; Codex with hooks that need review)? Does `check_for_update_on_startup=false` work?
- Copy the PTY output of the probes to `probes/pty/`, so the rules for keys have evidence.
- Result: the rules in spec 4.3 and the argv in step 5 are corrected before the code is written.

### Step 1 — Skeleton
- `git init`. Cargo workspace. Tauri 2 app with an empty Svelte page. `.gitignore`.
- Test: `cargo test --workspace` runs. `pnpm tauri dev` opens a window (manual, one time).

### Step 2 — Status state machine (`status.rs`)
- Types: `Status`, `HookEvent { name, notification_type, .. }`, `Input::{Hook, OperatorKey, PtyQuiet, ProcessExit, Tick}`.
- One function for each adapter: `next(state, input, now) -> state`. No clock inside; the time is an argument.
- Rules from spec 4.3: last event wins; Claude queued-turn mark; `Notification` backup only if `PreToolUse` is last;
  `SubagentStop` and `idle_prompt` ignored; Codex stays `starting` until the first hook; Codex `Interrupt`;
  Claude Esc/Ctrl-C plus 2 s quiet; `starting` for more than 20 s gives `question`.
- "Last event" means the last event that the table of spec 4.3 maps. Ignored events (`SubagentStop`, `idle_prompt`) do not change it.
- **Tests (replay):** a test reads `probes/logs/claude-hooks.jsonl` and `codex-hooks.jsonl`, splits them by `session_id` and then at each `SessionStart`
  (one session ID can cover two processes after a resume), feeds the events, and compares the status sequence with an expected sequence written by hand in the test.
  Gate 2 ran the spec rules over all 70 events with no mismatch. These wrong builds go red on the real logs: no queued-turn mark (Claude `Stop` at t=524.59);
  each `Notification` as `question` (`idle_prompt` at 724.83); submits counted against stops (Codex steering, `Stop` at 1135.72);
  queued-turn mark applied to Codex (same event); Codex `idle` at `SessionStart` (1076.6).
- **Tests (table), with a fake time.** The logs cannot make these fail, so each needs its own case:
  `Notification` backup only if `PreToolUse` is the last event (answer comes before the late notification); `SubagentStop` during `working` is ignored;
  Esc / Ctrl-C plus 2 s quiet; `starting` for more than 20 s; the deny path (from step 0); process exit; `done`.
- Red on: see section 6.

### Step 3 — Hook path (`hook/`, `hookrx.rs`)
- `tower-hook`: read stdin, wrap as `{session, event_json}`, write to `$TOWER_SOCKET`, exit 0 always, timeout 1 s. It prints nothing (a Stop hook that prints JSON changes the turn; that is M2).
- `hookrx`: Unix socket in `$XDG_RUNTIME_DIR/agent-tower/<pid>.sock`, one line of JSON for each event, sends `(SessionId, HookEvent)` on a channel.
- Test: start `hookrx` in the test, run the real `tower-hook` binary with a payload on stdin, assert the event and the session ID.
- Test: `tower-hook` with no socket exits 0 in less than 1.5 s (a hook must never block a CLI).
- Red on: a hook that hangs when the app is closed.

### Step 4 — PTY and session (`pty.rs`, `session.rs`, `env.rs`, `fakecli/`)
- `portable-pty`: spawn with a clean environment. `env.rs` is a pure function `build_env(parent: &Map, extra) -> Map` (allow-list HOME, TERM, LANG, USER, SHELL, XDG_*, DISPLAY, plus `TOWER_*`; never `CLAUDE_CODE_*` or `CLAUDECODE`).
  The tests give it a map; they do not change the environment of the test process.
- **Working folder.** Always set `CommandBuilder::cwd`. Without it, portable-pty 0.9 starts the child in `$HOME` (gate 2), which is the wrong folder and gives a Claude trust dialog.
- **PATH and absolute paths.** A desktop PATH does not contain `claude` (`~/.local/bin`), `codex` or `node` (nvm folder) (gate 2). At first use, the app finds the absolute paths of
  `claude`, `codex`, `node` and `tower-hook` (login shell `command -v`, then stored in the app config, editable). The session PATH is `/usr/bin:/bin` plus the folders of these programs.
- Read loop on a thread, output to a channel and to a log file with a size limit. Resize. Kill. Exit status.
- `Session`: one mutex holds the PTY writer, the state machine and the draft flag together. A hook event changes the status inside the same mutex that a write holds.
- `PtyWriter` is a trait. Tests can use a recording writer.
- Key classifier for the draft flag: a pure function `classify(bytes, status) -> sets_draft`. An operator paste from xterm.js arrives as `ESC[200~ ... ESC[201~` and sets the flag; a single `ESC`, arrow sequences and `\r` do not.
- `Session::operator_input(bytes)`: writes; sets the draft flag by the rule of spec 4.5 rule 4; feeds `OperatorKey` for Esc/Ctrl-C.
- `Session::paste(text) -> Result<(), PasteRefused>`: inside the lock, checks status (`idle` or `done`), draft flag clear, Codex has had a prompt; then writes `ESC[200~ text ESC[201~ \r` in one write.
- `fake-cli`: a small program that acts as an agent. It sets raw mode with no echo on its terminal, as a real TUI does (in cooked mode the terminal holds and changes the bytes, gate 2).
  It calls `tower-hook` before it draws a dialog. It prints a prompt, reads input, calls `tower-hook` with `UserPromptSubmit` / `PreToolUse` / `PermissionRequest` / `Stop` by a script
  (`--script perm-dialog`, `--script queued-turn`, `--script startup-dialog`), and records each byte that it receives.
- Tests with `fake-cli` in a real PTY:
    - paste in `idle` arrives as one bracketed paste with the exact text, also multi-line;
    - paste is refused in `question`, in `starting`, with the draft flag set, and in a Codex-kind session with no prompt yet; the recording writer has zero writes, and `fake-cli` records zero bytes;
    - the child prints its working folder, and it is the floor CWD;
    - `build_env` with `CLAUDE_CODE_CHILD_SESSION` in the parent map gives a map without it;
    - key classifier table: xterm.js paste, `\r` during `question`, arrows, a single `ESC`, printable text in `idle` and in `question`;
    - process exit gives `stopped`.
- **Race test (deterministic).** `Session::paste` has a test-only pause point immediately after the status read. Thread A pastes and stops at the pause point.
  Thread B applies `PermissionRequest`. Then A continues. Correct build: B waits for the mutex until the write ends, and the write happened in `idle`.
  Wrong build (status read before the lock): B goes first and A writes during `question`; the test fails on each run.

### Step 5 — Adapters (`adapter/claude.rs`, `adapter/codex.rs`)
- `start_command` / `resume_command` build argv and environment. From the gate 1 probes:
    - Claude: `--session-id <uuid>`, `--settings <json with hooks that call tower-hook>`, `--permission-mode <mode>`, `--model`, `--effort`, `--append-system-prompt`, `[PROMPT]`. Resume: `--resume <uuid>`.
    - Codex: `--no-daemon`, `-c hooks.<Event>=[...]` for each event, `-c 'projects={"<cwd>"={trust_level="trusted"}}'`, `-c check_for_update_on_startup=false`, `-c developer_instructions=...`, `-m`, `-a on-request -s danger-full-access` (decision O7, option B), `[PROMPT]`. Resume: `resume <id>`.
- `map_event`: raw hook JSON to `HookEvent`. Codex session ID comes from the first hook event; the store saves it at once.
- All programs are started by absolute path. The hook command text is the absolute path of `tower-hook` and nothing more.
- Tests: argv snapshot tests (exact vector). Payload parsing tests with lines from `probes/logs/`. Snapshot tests do not prove a flag, so these tests with the real CLIs and no model call are in the default test run:
    - each flag in the Claude argv is in the output of `claude --help`, and each Codex flag is in `codex --help`;
    - Claude: start the real `claude` with the generated `--settings`, a PATH of `/usr/bin:/bin` plus the stored folders, and no prompt. `SessionStart` must arrive at `hookrx` with the correct `TOWER_SESSION`. Then kill it;
    - Codex: `codex -c 'developer_instructions="<marker>"' debug prompt-input "hi"` prints the marker (gate 2: a wrong key name prints it 0 times);
    - both CLIs print `--version` with the session PATH.
  These tests skip with a clear message if a CLI is not installed.
- Live smoke tests, marked `#[ignore]`, run by hand: Claude with `--model haiku`, one prompt "Reply with only: OK"; assert the status sequence `starting, idle, working, idle` and a resume that keeps the context. Codex: the same, one time only (weekly limit).
- Red on: a wrong flag name (live test); a change in a hook payload after a CLI update (live test). The snapshot tests alone do not prove the flags; the live tests do.

### Step 6 — Store (`store.rs`)
- `.swarm/floor.toml`, `.swarm/employees/<slug>.md` (front matter + role text), `.swarm/.gitignore`, `.swarm/state/floor.db` with tables `sessions` and `status_events` (the task and message tables come in M2). Schema version 1.
- The app config file holds the list of floor paths.
- Tests: write then read gives the same data; a `floor.toml` with a newer version is refused; a session row has the CLI session ID before the first `Stop`.

### Step 7 — App shell and UI (`src-tauri/`, `ui/`)
- Commands: `floor_open(path)`, `employee_add/update/remove`, `session_start(employee, first_message?)`, `session_stop`, `session_resume`, `pty_write`, `pty_resize`, `debug_paste(session, text)`.
- Events: PTY output on a Tauri channel for each session (bytes, base64 or binary); `status_changed`.
- UI: floor picker; a plain list of employees with status dot and "start" (placeholder for the office view); tabs with status dot and "message waiting" mark; xterm.js terminal with the fit addon; a small form to add an employee (name, role, CLI, model, effort).
- Test (manual checklist, section 5). Test (automatic): `cargo test -p core` covers the logic; the shell has no logic to test.

### Step 8 — Live smoke tests
Run the `#[ignore]` live tests of step 5 one time for each CLI, and add the new hook logs to `probes/logs/` as more replay material.

### Step 9 — Feature review
One reviewer for the M1 feature. It runs the test suite, the mutation checks of section 6 and the manual checklist items that a script can drive through a PTY.

## 5. Acceptance checklist (operator, in the real app)

1. Open a floor on a test repository. Add a Claude employee and a Codex employee.
2. Start both. Each tab shows the real CLI. Typing works, also arrows, Esc, Ctrl-C, paste, and a window resize.
3. Give each a prompt that runs a shell command. The dot shows `working`, then `question` at the permission prompt, then `working` after "yes", then `idle`.
4. Press Esc during a long answer. The dot goes to `idle` in less than 5 s, in both CLIs.
5. In Claude, submit a second prompt while the first runs. The dot stays `working` until the second answer ends.
6. `debug_paste` in an idle session: the text arrives as a prompt. With a permission prompt on screen: refused. With a typed draft: refused, and the tab shows the mark.
7. Stop a session, resume it, and ask "what was my last message?". The agent knows.
8. Close the app with live sessions. Open it again: the sessions show `stopped`, and resume works.

## 6. Checks that the tests can fail (for the reviewer)

| Change to the code | Test that must fail |
|---|---|
| Remove the queued-turn mark | Replay test of the Claude queued-turn log |
| Map each `Notification` to `question` | Replay test (the `idle_prompt` events) |
| Count submits against stops | Replay test (Codex steering run) |
| Codex goes `idle` on `SessionStart` or on screen output | Replay test (Codex start); table test "Codex stays `starting`" |
| `Notification` backup with no "last event" condition | Table test (the logs cannot catch this) |
| `SubagentStop` treated as `Stop` | Table test (the logs cannot catch this) |
| Read the status before the lock in `paste` | The deterministic race test with the pause point |
| No `cwd` on the command | The working-folder test |
| Pass the parent environment | The `build_env` test |
| A misspelled flag or config key | The help-output test, the Claude `SessionStart` test, the Codex marker test |
| Classify each ESC sequence as a control key | Key classifier table (xterm.js paste) |
| Put the session ID in the hook command text | Snapshot test "hook command text is the same for two sessions" |

## 7. How the work is done

- Planned code is written by subagents on Sonnet 5.5, one step at a time, in this order: 0, 1, 2, 3, 4, 5, 6, 7, 8. Steps 2 and 3 can run in parallel; step 6 can run in parallel with 4 and 5.
- The orchestrator runs the tests of each step itself. No review for each step. One review at step 9.
- A commit for each step, with explicit paths.
- The first usable build is after step 7 with Claude only if Codex causes a delay; the Codex adapter then follows.
