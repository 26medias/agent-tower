//! `fake-cli`: a scripted fake agent for tests. It starts no model and costs no tokens.
//!
//! It acts like the terminal side of Claude or Codex:
//! - raw mode with no echo (as a real TUI), and bracketed paste turned on (`ESC[?2004h`);
//! - every byte that it reads from stdin goes to the `--record` file, with a flush after each read;
//! - it reports events by running the real `tower-hook` program with the hook JSON on stdin,
//!   and it does this BEFORE it draws a dialog.
//!
//! Usage: `fake-cli [--script echo|perm-dialog|startup-dialog|queued-turn] [--kind claude|codex]
//! [--record <file>] [--hook <program>] [--marker <file>] [--work-ms <n>] [--hook-gap-ms <n>] [--cli-session-id <id>] [PROMPT]`
//!
//! `PROMPT` (one positional argument, not starting with `--`) is the first prompt, as the `[PROMPT]` of a real CLI.
//!
//! `--hook-gap-ms` (default 25) is the pause after each hook call, as the gaps between the hook
//! events of a real CLI.
//! `--hook` defaults to the environment variable `TOWER_HOOK`. `tower-hook` reads `TOWER_SESSION`
//! and `TOWER_SOCKET` from the environment. Without a hook program, no event is sent.
//!
//! Scripts (a "prompt" is a bracketed paste followed by `\r`, or typed text followed by `\r`):
//! - `echo`: `SessionStart` at start (Codex kind: at the first prompt). On a prompt:
//!   `UserPromptSubmit`, prints the text, works for `--work-ms`, `Stop`.
//! - `perm-dialog`: on a prompt: `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, then the
//!   dialog "1. Yes  2. No". `\r` or `1`: writes the `--marker` file, `PostToolUse`, `Stop`.
//!   `2` or Esc: no event (Claude kind), or `Interrupt` (Codex kind), and back to the prompt.
//! - `startup-dialog`: at start NO hook, a dialog with a dangerous default. `\r` writes the
//!   `--marker` file. `1` goes on with no marker. `2` or Esc ends the process with code 1.
//!   Then it acts as `echo`.
//! - `queued-turn`: on a prompt: `UserPromptSubmit`, then it WAITS for a second prompt (10 s
//!   at most), `UserPromptSubmit` for it, works, `Stop`, then a second turn with no event at
//!   its start, works, `Stop`. This is the Claude queued-turn order.
//!
//! The text `/exit` or `/exit <code>` as a prompt ends the process (Claude kind: `SessionEnd`).
//! End of input ends the process with code 0.

mod hooks;
mod input;
mod term;

use hooks::{Hooks, Kind};
use input::{Input, Key, Next};
use serde_json::json;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use term::out;

const PERM_COMMAND: &str = "touch probe_perm.txt";

/// How long `queued-turn` waits for the second prompt.
const QUEUE_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Script {
    Echo,
    PermDialog,
    StartupDialog,
    QueuedTurn,
}

struct Opts {
    script: Script,
    kind: Kind,
    record: Option<PathBuf>,
    hook: Option<PathBuf>,
    marker: Option<PathBuf>,
    work: Duration,
    /// Pause after each hook call (`--hook-gap-ms`).
    hook_gap: Duration,
    cli_session_id: String,
    /// A positional argument, as the `[PROMPT]` of a real CLI. It is the first prompt.
    first_prompt: Option<String>,
}

fn parse_args() -> Result<Opts, String> {
    let mut opts = Opts {
        script: Script::Echo,
        kind: Kind::Claude,
        record: None,
        hook: std::env::var_os("TOWER_HOOK")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
        marker: None,
        work: Duration::from_millis(100),
        hook_gap: Duration::from_millis(25),
        cli_session_id: "00000000-0000-4000-8000-000000000001".into(),
        first_prompt: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--script" => {
                opts.script = match value()?.as_str() {
                    "echo" => Script::Echo,
                    "perm-dialog" => Script::PermDialog,
                    "startup-dialog" => Script::StartupDialog,
                    "queued-turn" => Script::QueuedTurn,
                    other => return Err(format!("unknown script {other:?}")),
                }
            }
            "--kind" => {
                opts.kind = match value()?.as_str() {
                    "claude" => Kind::Claude,
                    "codex" => Kind::Codex,
                    other => return Err(format!("unknown kind {other:?}")),
                }
            }
            "--record" => opts.record = Some(value()?.into()),
            "--hook" => opts.hook = Some(value()?.into()),
            "--marker" => opts.marker = Some(value()?.into()),
            "--work-ms" => {
                let ms: u64 = value()?.parse().map_err(|_| "--work-ms needs a number")?;
                opts.work = Duration::from_millis(ms);
            }
            "--hook-gap-ms" => {
                let ms: u64 = value()?
                    .parse()
                    .map_err(|_| "--hook-gap-ms needs a number")?;
                opts.hook_gap = Duration::from_millis(ms);
            }
            "--cli-session-id" => opts.cli_session_id = value()?,
            other if !other.starts_with("--") => opts.first_prompt = Some(other.to_owned()),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(opts)
}

fn main() -> ExitCode {
    let opts = match parse_args() {
        Ok(opts) => opts,
        Err(message) => {
            eprintln!("fake-cli: {message}");
            return ExitCode::from(2);
        }
    };
    let code = run(opts);
    ExitCode::from(code)
}

fn run(opts: Opts) -> u8 {
    let record = match &opts.record {
        Some(path) => match File::create(path) {
            Ok(file) => Some(file),
            Err(error) => {
                eprintln!("fake-cli: cannot open {}: {error}", path.display());
                return 2;
            }
        },
        None => None,
    };
    let mut raw = match term::RawMode::enter() {
        Ok(raw) => raw,
        Err(error) => {
            eprintln!("fake-cli: cannot set raw mode: {error}");
            return 2;
        }
    };

    // A thread reads stdin and records each read, so the record shows what arrived even when
    // the main thread is busy in a turn.
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || read_stdin(record, sender));

    out(b"\x1b[?2004h");
    let mut hooks = Hooks::new(opts.hook.clone(), opts.kind, opts.cli_session_id.clone());
    hooks.gap = opts.hook_gap;
    let mut app = App {
        opts,
        hooks,
        input: Input::new(receiver),
        started: false,
        first_prompt: None,
    };
    app.first_prompt = app.opts.first_prompt.take();
    let code = app.run();
    out(b"\x1b[?2004l");
    raw.restore();
    code
}

/// Read stdin until the end. Write each chunk to the record file and flush, then send it on.
fn read_stdin(mut record: Option<File>, sender: mpsc::Sender<Vec<u8>>) {
    let mut stdin = std::io::stdin().lock();
    let mut buffer = [0u8; 4096];
    loop {
        let count = match stdin.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        if let Some(file) = record.as_mut() {
            let _ = file.write_all(&buffer[..count]);
            let _ = file.flush();
        }
        if sender.send(buffer[..count].to_vec()).is_err() {
            break;
        }
    }
}

enum Prompt {
    Text(String),
    Eof,
    Timeout,
}

struct App {
    opts: Opts,
    hooks: Hooks,
    input: Input,
    /// `SessionStart` was sent.
    started: bool,
    /// The positional prompt, used for the first turn.
    first_prompt: Option<String>,
}

impl App {
    fn run(&mut self) -> u8 {
        if self.opts.script == Script::StartupDialog {
            // No hook before the dialog is answered (spec 4.2, startup dialogs).
            if let Some(code) = self.startup_dialog() {
                return code;
            }
        }
        if self.opts.kind == Kind::Claude {
            self.session_start();
        }
        loop {
            let text = match self.first_prompt.take() {
                Some(text) => text,
                None => {
                    out(b"> ");
                    match self.read_prompt(None) {
                        Prompt::Text(text) => text,
                        Prompt::Eof | Prompt::Timeout => return 0,
                    }
                }
            };
            if let Some(code) = parse_exit(&text) {
                if self.opts.kind == Kind::Claude {
                    self.hooks.send("SessionEnd", json!({}));
                }
                return code;
            }
            // Codex sends `SessionStart` only at the first prompt.
            self.session_start();
            self.hooks.next_turn();
            let finished = match self.opts.script {
                Script::Echo | Script::StartupDialog => self.echo_turn(&text),
                Script::PermDialog => self.perm_turn(&text),
                Script::QueuedTurn => self.queued_turn(&text),
            };
            if !finished {
                return 0;
            }
        }
    }

    fn session_start(&mut self) {
        if !self.started {
            self.started = true;
            self.hooks.send("SessionStart", json!({}));
        }
    }

    fn work(&self) {
        std::thread::sleep(self.opts.work);
    }

    /// Read a prompt: typed text or a paste, ended by Enter.
    fn read_prompt(&mut self, deadline: Option<Instant>) -> Prompt {
        let mut draft = String::new();
        loop {
            match self.input.next(deadline) {
                Next::Eof => return Prompt::Eof,
                Next::Timeout => return Prompt::Timeout,
                Next::Key(Key::Char(c)) => draft.push(c),
                Next::Key(Key::Paste(text)) => draft.push_str(&text),
                Next::Key(Key::Backspace) => {
                    draft.pop();
                }
                Next::Key(Key::CtrlC) => draft.clear(),
                Next::Key(Key::Enter) if !draft.is_empty() => return Prompt::Text(draft),
                Next::Key(Key::Enter | Key::Esc | Key::Seq(_)) => {}
            }
        }
    }

    fn echo_turn(&mut self, text: &str) -> bool {
        self.hooks
            .send("UserPromptSubmit", json!({ "prompt": text }));
        out(format!("\r\nyou said: {}\r\n", text.replace('\n', "\r\n")).as_bytes());
        self.work();
        self.hooks.send("Stop", json!({}));
        true
    }

    fn perm_turn(&mut self, text: &str) -> bool {
        self.hooks
            .send("UserPromptSubmit", json!({ "prompt": text }));
        self.hooks.send_tool("PreToolUse", PERM_COMMAND);
        self.hooks.send_tool("PermissionRequest", PERM_COMMAND);
        // The hooks are sent. Now draw the dialog.
        out(format!("\r\nRun: {PERM_COMMAND}\r\n  1. Yes  2. No\r\n").as_bytes());
        loop {
            match self.input.next(None) {
                Next::Eof | Next::Timeout => return false,
                Next::Key(Key::Enter | Key::Char('1')) => {
                    if let Some(marker) = &self.opts.marker {
                        let _ = std::fs::write(marker, "approved\n");
                    }
                    out(b"approved\r\n");
                    self.hooks.send_tool("PostToolUse", PERM_COMMAND);
                    self.work();
                    self.hooks.send("Stop", json!({}));
                    return true;
                }
                Next::Key(Key::Char('2') | Key::Esc) => {
                    // Claude sends nothing on a deny. Codex sends `Interrupt`.
                    if self.opts.kind == Kind::Codex {
                        self.hooks.send("Interrupt", json!({}));
                    }
                    out(b"denied\r\n");
                    return true;
                }
                // Other keys, and a paste, do nothing in a dialog.
                Next::Key(_) => {}
            }
        }
    }

    fn queued_turn(&mut self, text: &str) -> bool {
        self.hooks
            .send("UserPromptSubmit", json!({ "prompt": text }));
        out(format!("\r\nturn 1: {text}\r\n").as_bytes());
        let mut queued = None;
        match self.read_prompt(Some(Instant::now() + QUEUE_WAIT)) {
            Prompt::Text(second) => {
                self.hooks.next_turn();
                self.hooks
                    .send("UserPromptSubmit", json!({ "prompt": second }));
                queued = Some(second);
            }
            Prompt::Eof => return false,
            Prompt::Timeout => {}
        }
        self.work();
        self.hooks.send("Stop", json!({}));
        if let Some(second) = queued {
            // The queued turn starts with no event.
            out(format!("turn 2: {second}\r\n").as_bytes());
            self.work();
            self.hooks.send("Stop", json!({}));
        }
        true
    }

    /// Draw the dialog and wait. `None` means go on. `Some(code)` means end with that code.
    fn startup_dialog(&mut self) -> Option<u8> {
        out(
            b"Do you trust the files in this folder?\r\n  1. Yes, proceed\r\n  2. No, exit\r\n\
              (Enter selects: No, exit)\r\n",
        );
        loop {
            match self.input.next(None) {
                Next::Eof | Next::Timeout => return Some(0),
                Next::Key(Key::Enter) => {
                    if let Some(marker) = &self.opts.marker {
                        let _ = std::fs::write(marker, "enter at startup dialog\n");
                    }
                    out(b"dialog answered\r\n");
                    return None;
                }
                Next::Key(Key::Char('1')) => {
                    out(b"dialog answered\r\n");
                    return None;
                }
                Next::Key(Key::Char('2') | Key::Esc) => {
                    out(b"exit\r\n");
                    return Some(1);
                }
                Next::Key(_) => {}
            }
        }
    }
}

/// `/exit` or `/exit <code>`.
fn parse_exit(text: &str) -> Option<u8> {
    let rest = text.trim().strip_prefix("/exit")?;
    let rest = rest.trim();
    if rest.is_empty() {
        Some(0)
    } else {
        rest.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_command() {
        assert_eq!(parse_exit("/exit"), Some(0));
        assert_eq!(parse_exit(" /exit 7 "), Some(7));
        assert_eq!(parse_exit("/exit x"), None);
        assert_eq!(parse_exit("hello"), None);
        assert_eq!(parse_exit("/exiting"), None);
    }
}
