//! Hook calls of `fake-cli`. It runs the real `tower-hook` program with the hook JSON on stdin,
//! as a CLI does. `tower-hook` reads `TOWER_SESSION` and `TOWER_SOCKET` from the environment,
//! and `fake-cli` passes its own environment on.
//!
//! The field names are the ones in the gate 1 logs (`probes/logs/claude-hooks.jsonl` and
//! `codex-hooks.jsonl`): `hook_event_name`, `session_id`, `prompt`, `tool_name`,
//! `notification_type`.

use serde_json::{Value, json};
use std::cell::Cell;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Claude,
    Codex,
}

pub struct Hooks {
    /// The `tower-hook` program. `None`: no hook is sent.
    pub program: Option<PathBuf>,
    pub kind: Kind,
    /// The ID that the CLI gives to its own session (not the `TOWER_SESSION`).
    pub cli_session_id: String,
    pub cwd: String,
    /// Counts the turns, for `prompt_id` and `turn_id`.
    turn: u32,
    /// Counts the events that this program sends. It is the field `fake_seq` of each payload.
    /// A real CLI has no such field. It lets a test see the order in which `fake-cli` sent the
    /// events, because the socket server does not keep the order of events from different
    /// connections.
    seq: Cell<u64>,
    /// Pause after each hook call. A real CLI has gaps between its hook events (Claude: 40 ms
    /// from `PreToolUse` to `PermissionRequest`). The default is set in `main.rs`.
    pub gap: std::time::Duration,
}

impl Hooks {
    pub fn new(program: Option<PathBuf>, kind: Kind, cli_session_id: String) -> Hooks {
        let cwd = std::env::current_dir()
            .map(|dir| dir.display().to_string())
            .unwrap_or_default();
        Hooks {
            program,
            kind,
            cli_session_id,
            cwd,
            turn: 0,
            seq: Cell::new(0),
            gap: std::time::Duration::ZERO,
        }
    }

    /// A new turn: new `prompt_id` / `turn_id` for the next events.
    pub fn next_turn(&mut self) {
        self.turn += 1;
    }

    /// Build the payload of an event, as the CLI of this kind builds it.
    pub fn payload(&self, event: &str, extra: Value) -> Value {
        let Value::Object(mut object) = json!({
            "session_id": self.cli_session_id,
            "transcript_path": format!("/tmp/fake-cli/{}.jsonl", self.cli_session_id),
            "cwd": self.cwd,
            "hook_event_name": event,
        }) else {
            return Value::Null;
        };
        let turn_id = format!("turn-{}", self.turn);
        match self.kind {
            Kind::Claude => {
                if !matches!(event, "SessionStart" | "SessionEnd") {
                    object.insert("prompt_id".into(), json!(turn_id));
                }
                if !matches!(event, "SessionStart" | "SessionEnd" | "Notification") {
                    object.insert("permission_mode".into(), json!("default"));
                }
                if event == "SessionStart" {
                    object.insert("source".into(), json!("startup"));
                    object.insert("model".into(), json!("fake-model"));
                }
                if event == "Stop" {
                    object.insert("stop_hook_active".into(), json!(false));
                }
                if event == "SessionEnd" {
                    object.insert("reason".into(), json!("other"));
                }
            }
            Kind::Codex => {
                if event != "SessionStart" {
                    object.insert("turn_id".into(), json!(turn_id));
                }
                object.insert("model".into(), json!("fake-model"));
                object.insert("permission_mode".into(), json!("default"));
                if event == "SessionStart" {
                    object.insert("source".into(), json!("startup"));
                }
                if event == "Stop" {
                    object.insert("stop_hook_active".into(), json!(false));
                    object.insert("last_assistant_message".into(), json!("done"));
                }
            }
        }
        self.seq.set(self.seq.get() + 1);
        object.insert("fake_seq".into(), json!(self.seq.get()));
        if let Some(extra) = extra.as_object() {
            for (key, value) in extra {
                object.insert(key.clone(), value.clone());
            }
        }
        Value::Object(object)
    }

    /// Send one event. It waits for `tower-hook` to end, as a CLI does for a hook.
    /// A problem is not an error: a hook must never change a turn.
    pub fn send(&self, event: &str, extra: Value) {
        let Some(program) = &self.program else {
            return;
        };
        let payload = self.payload(event, extra);
        let Ok(mut child) = Command::new(program)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(payload.to_string().as_bytes());
        }
        let _ = child.wait();
        std::thread::sleep(self.gap);
    }

    pub fn send_tool(&self, event: &str, command: &str) {
        self.send(
            event,
            json!({
                "tool_name": "Bash",
                "tool_input": {"command": command, "description": "fake tool call"},
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_payload_has_the_field_names_of_the_logs() {
        let mut hooks = Hooks::new(None, Kind::Claude, "sid".into());
        hooks.next_turn();
        let payload = hooks.payload("UserPromptSubmit", json!({"prompt": "hi"}));
        assert_eq!(payload["hook_event_name"], "UserPromptSubmit");
        assert_eq!(payload["session_id"], "sid");
        assert_eq!(payload["prompt"], "hi");
        assert_eq!(payload["prompt_id"], "turn-1");
        assert!(payload.get("turn_id").is_none());
        let start = hooks.payload("SessionStart", json!({}));
        assert_eq!(start["source"], "startup");
        assert!(start.get("prompt_id").is_none());
    }

    #[test]
    fn codex_payload_has_turn_id() {
        let mut hooks = Hooks::new(None, Kind::Codex, "sid".into());
        hooks.next_turn();
        let payload = hooks.payload("Stop", json!({}));
        assert_eq!(payload["turn_id"], "turn-1");
        assert_eq!(payload["last_assistant_message"], "done");
        assert!(payload.get("prompt_id").is_none());
    }
}
