//! The desktop notification for a question (2026-11-21).
//!
//! The core tells the shell when a session changes to `question` (`Observer::question_asked`).
//! The shell knows if its window has the focus, so the shell decides (`tower_core::app::
//! should_notify`) and calls the official notification plugin of Tauri. A click on the
//! notification is not handled: the plugin API for desktop gives no click event.
//!
//! `notify_question` has the decision and takes the plugin call as a closure, so a test can check
//! the decision with no plugin and no display. A real notification is not visible in a test: the
//! debug line `agent-tower: notification: ...` shows that the call was made.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_notification::NotificationExt;
use tower_core::app::{
    NOTIFICATION_TITLE, OperatorAsked, QuestionAsked, question_body, request_body, should_notify,
};

/// The last notifications that the app asked the plugin to show: (title, body), oldest first. A
/// test reads it, because a notification on the screen is not visible to a test.
static RECENT: Mutex<VecDeque<(String, String)>> = Mutex::new(VecDeque::new());
const RECENT_MAX: usize = 50;

/// The last notifications that the app asked the plugin to show (at most 50).
#[doc(hidden)]
pub fn recent() -> Vec<(String, String)> {
    RECENT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .cloned()
        .collect()
}

/// Does a window of the app have the focus? A minimized window does not count as focused. An
/// app with no window has no focus.
pub fn window_has_focus<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.webview_windows().values().any(|window| {
        window.is_focused().unwrap_or(false) && !window.is_minimized().unwrap_or(false)
    })
}

/// Decide, and call `show(title, body)` when a notification is due. Returns `true` when it
/// called `show`. One call for each change to `question`; none while the window has the focus;
/// none when the switch is off.
pub fn notify_question(
    event: &QuestionAsked,
    window_focused: bool,
    show: impl FnOnce(&str, &str),
) -> bool {
    if !should_notify(event.old, event.new, window_focused, event.notifications_on) {
        return false;
    }
    show(
        NOTIFICATION_TITLE,
        &question_body(&event.name, &event.role, &event.floor_name),
    );
    true
}

/// The entry for the observer: read the focus, decide, and show with the plugin.
pub fn on_question<R: Runtime>(app: &AppHandle<R>, event: &QuestionAsked) {
    let focused = window_has_focus(app);
    notify_question(event, focused, |title, body| show(app, title, body));
}

/// A request of an employee for the operator: one notification when the window does not have
/// the focus and the switch is on.
pub fn on_request<R: Runtime>(app: &AppHandle<R>, event: &OperatorAsked) {
    if !event.notifications_on || window_has_focus(app) {
        return;
    }
    let request = &event.request;
    let body = request_body(
        &request.name,
        &request.role,
        &event.floor_name,
        &request.text,
    );
    show(app, NOTIFICATION_TITLE, &body);
}

/// Call the plugin. A failure (for example no notification service on this machine) is a line on
/// stderr, never a failure of the app.
fn show<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    {
        let mut recent = RECENT.lock().unwrap_or_else(PoisonError::into_inner);
        recent.push_back((title.to_owned(), body.to_owned()));
        while recent.len() > RECENT_MAX {
            recent.pop_front();
        }
    }
    if cfg!(debug_assertions) {
        eprintln!("agent-tower: notification: {title}: {body}");
    }
    // `NotificationExt::notification` panics when the plugin is not loaded (the tests of the
    // shell do not load it).
    if app
        .try_state::<tauri_plugin_notification::Notification<R>>()
        .is_none()
    {
        if cfg!(debug_assertions) {
            eprintln!("agent-tower: notification plugin is not loaded");
        }
        return;
    }
    let result = app.notification().builder().title(title).body(body).show();
    if let Err(error) = result {
        eprintln!("agent-tower: cannot show the notification: {error}");
    } else if cfg!(debug_assertions) {
        eprintln!("agent-tower: notification call returned ok");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use tower_core::status::Status;

    fn event(old: Status, new: Status, on: bool) -> QuestionAsked {
        QuestionAsked {
            session_id: "s1".into(),
            slug: "eng".into(),
            name: "Eli".into(),
            role: "Engineer".into(),
            floor_name: "Fin Check".into(),
            old,
            new,
            notifications_on: on,
        }
    }

    fn calls(event: &QuestionAsked, focused: bool) -> Vec<(String, String)> {
        let log = RefCell::new(Vec::new());
        let called = notify_question(event, focused, |title, body| {
            log.borrow_mut().push((title.to_owned(), body.to_owned()));
        });
        assert_eq!(called, !log.borrow().is_empty());
        log.into_inner()
    }

    #[test]
    fn one_notification_with_the_title_and_the_body_of_the_contract() {
        let log = calls(&event(Status::Working, Status::Question, true), false);
        assert_eq!(
            log,
            [(
                "Agent Tower".to_owned(),
                "Eli (Engineer) asks a question in Fin Check".to_owned()
            )]
        );
    }

    #[test]
    fn none_while_the_window_has_the_focus() {
        assert!(calls(&event(Status::Working, Status::Question, true), true).is_empty());
    }

    #[test]
    fn none_when_the_switch_is_off() {
        assert!(calls(&event(Status::Working, Status::Question, false), false).is_empty());
    }

    #[test]
    fn none_when_the_status_did_not_change_to_question() {
        assert!(calls(&event(Status::Question, Status::Question, true), false).is_empty());
        assert!(calls(&event(Status::Question, Status::Idle, true), false).is_empty());
        assert!(calls(&event(Status::Idle, Status::Working, true), false).is_empty());
    }
}
