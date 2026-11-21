//! The desktop notification for a question (item "notification", 2026-11-21).
//!
//! When a session changes to `question`, the operator may not look at the window. The core sends
//! `Observer::question_asked` with the data of the notice. The shell (Tauri) knows if its window
//! has the focus, and it shows the notification. The core has no Tauri type. The rule that
//! decides is `should_notify`, a pure function.

use crate::status::Status;

/// The title of each notification.
pub const NOTIFICATION_TITLE: &str = "Agent Tower";

/// A session changed to `question` (`old` is its status before).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionAsked {
    pub session_id: String,
    pub slug: String,
    /// The name and the role of the employee, for example `Eli` and `Engineer`.
    pub name: String,
    pub role: String,
    pub floor_name: String,
    pub old: Status,
    pub new: Status,
    /// The switch of the operator (`[interface] notify_on_question`) at the time of the change.
    pub notifications_on: bool,
}

/// Does a status change need a desktop notification? Yes, when the session CHANGES to `question`
/// (one notice for each question, not one for each event of the same question), the window does
/// not have the focus (or it is minimized), and the switch of the operator is on.
pub fn should_notify(old: Status, new: Status, window_focused: bool, enabled: bool) -> bool {
    enabled && new == Status::Question && old != Status::Question && !window_focused
}

/// The text of the notification, for example `Eli (Engineer) asks a question in Fin Check`.
pub fn question_body(name: &str, role: &str, floor_name: &str) -> String {
    format!("{name} ({role}) asks a question in {floor_name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Status; 6] = [
        Status::Starting,
        Status::Working,
        Status::Idle,
        Status::Question,
        Status::Done,
        Status::Stopped,
    ];

    #[test]
    fn only_a_change_to_question_with_no_focus_and_the_switch_on_notifies() {
        for old in ALL {
            for new in ALL {
                for focused in [false, true] {
                    for enabled in [false, true] {
                        let expected = new == Status::Question
                            && old != Status::Question
                            && !focused
                            && enabled;
                        assert_eq!(
                            should_notify(old, new, focused, enabled),
                            expected,
                            "{old:?} -> {new:?}, focused {focused}, enabled {enabled}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_body_names_the_employee_the_role_and_the_floor() {
        assert_eq!(
            question_body("Eli", "Engineer", "Fin Check"),
            "Eli (Engineer) asks a question in Fin Check"
        );
    }
}
