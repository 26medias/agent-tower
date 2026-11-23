//! Requests for the operator (`docs/ipc-final.md`, 2026-11-22). An employee that is blocked and
//! waits for the operator calls the tool `ask_operator`. The app keeps the request in memory and
//! shows it until the operator clears it.
//!
//! Lock. `FloorHandle::requests` is held for a read or a write of the list. Nothing else is taken
//! while it is held.

use super::bus::{Identity, sanitize_message};
use super::live::{locked, now_ms};
use super::{Result, TowerCore, settings};
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// The most characters of the text of a request.
pub const REQUEST_MAX_CHARS: usize = 500;

/// The characters of the text that the desktop notification shows.
const NOTICE_CHARS: usize = 120;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// One request of an employee for the operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorRequest {
    /// Unique in this app run.
    pub id: u64,
    pub slug: String,
    pub name: String,
    pub role: String,
    pub text: String,
    /// Unix milliseconds.
    pub at: i64,
}

/// The event for the observer: a request was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorAsked {
    pub request: OperatorRequest,
    pub floor_name: String,
    /// The switch of the operator (`[interface] notify_on_question`) at the time of the request.
    pub notifications_on: bool,
}

/// The text of the notification, for example `Maya (CEO) needs you in Fin Check: Run sudo ...`.
pub fn request_body(name: &str, role: &str, floor_name: &str, text: &str) -> String {
    let line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut short: String = line.chars().take(NOTICE_CHARS).collect();
    if line.chars().count() > NOTICE_CHARS {
        short.push_str("...");
    }
    format!("{name} ({role}) needs you in {floor_name}: {short}")
}

impl TowerCore {
    /// `ask_operator`: keep the request and tell the observer.
    pub(super) fn tool_ask_operator(
        &self,
        who: &Identity,
        args: &Value,
    ) -> std::result::Result<String, String> {
        let (floor, team, _actor) = self.task_actor(who)?;
        let raw = args
            .get("text")
            .and_then(Value::as_str)
            .ok_or("The argument \"text\" is missing. It must be a text.")?;
        let count = raw.chars().count();
        if count > REQUEST_MAX_CHARS {
            return Err(format!(
                "The text is too long: {count} characters. The limit is {REQUEST_MAX_CHARS}. Say what you need in a few sentences, and put the details in your terminal answer or in a file."
            ));
        }
        let text: String = sanitize_message(raw)
            .chars()
            .filter(|c| *c == '\n' || !c.is_control())
            .collect();
        let text = text.trim().to_owned();
        if text.is_empty() {
            return Err("The text is empty. Say what the operator must do.".to_owned());
        }
        let me = team
            .iter()
            .find(|e| e.slug == who.slug)
            .ok_or("You are not an employee of this floor now.")?;
        let request = OperatorRequest {
            id: NEXT_ID.fetch_add(1, Ordering::SeqCst),
            slug: me.slug.clone(),
            name: me.name.clone(),
            role: me.role.clone(),
            text,
            at: now_ms(),
        };
        {
            let mut list = locked(&floor.requests);
            list.retain(|r| r.slug != request.slug);
            list.push(request.clone());
        }
        let floor_name = locked(&floor.store).floor().name.clone();
        let event = OperatorAsked {
            request,
            floor_name,
            notifications_on: settings::notify_on_question(&self.logo_env.config_file),
        };
        self.notifier.floor_changed(&floor.path);
        self.notifier.operator_asked(&floor.path, &event);
        Ok("The app shows your request to the operator now: in the top strip of the window, and as a desktop notification when the window is not in front. The request stays until the operator clears it. Now end your turn and wait: the operator answers in your terminal. Do not call this tool again for the same request, and do not wait in a loop.".to_owned())
    }

    /// The operator clears a request. An unknown `id` is not an error.
    pub fn operator_request_clear(&self, path: &Path, id: u64) -> Result<()> {
        let floor = self.floor_handle(path)?;
        let before = {
            let mut list = locked(&floor.requests);
            let before = list.len();
            list.retain(|r| r.id != id);
            before != list.len()
        };
        if before {
            self.notifier.floor_changed(&floor.path);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_is_one_line_and_a_long_text_is_cut() {
        assert_eq!(
            request_body("Maya", "CEO", "Fin Check", "Run this:\nsudo x"),
            "Maya (CEO) needs you in Fin Check: Run this: sudo x"
        );
        let body = request_body("Maya", "CEO", "F", &"a".repeat(121));
        assert!(body.ends_with(&format!("{}...", "a".repeat(120))), "{body}");
    }
}
