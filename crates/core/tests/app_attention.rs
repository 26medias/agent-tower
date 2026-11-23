//! The tool `ask_operator` and the command `operator_request_clear` (`docs/ipc-final.md`).

mod app_support;

use app_support::Rig;
use serde_json::json;

fn rig() -> Rig {
    let rig = Rig::new();
    rig.create_floor();
    rig.add_employee("eng", Some("ceo"));
    rig.start("ceo");
    rig.start("eng");
    rig
}

fn requests(rig: &Rig) -> Vec<(String, String)> {
    rig.tower
        .floor_open(&rig.cwd)
        .unwrap()
        .requests
        .into_iter()
        .map(|r| (r.slug, r.text))
        .collect()
}

#[test]
fn a_request_shows_in_the_floor_view_until_the_operator_clears_it() {
    let rig = rig();
    assert!(requests(&rig).is_empty());

    let answer = rig.tool(
        "eng",
        "ask_operator",
        json!({"text": "  Run this:\nsudo apt install x\u{1b}[31m  "}),
    );
    assert!(!answer.is_error, "{}", answer.text);
    assert!(answer.text.contains("end your turn"), "{}", answer.text);
    // The text is trimmed, the newline stays, the escape character is removed.
    assert_eq!(
        requests(&rig),
        [(
            "eng".to_owned(),
            "Run this:\nsudo apt install x[31m".to_owned()
        )]
    );
    let view = rig.tower.floor_open(&rig.cwd).unwrap();
    let first = view.requests[0].clone();
    assert_eq!(
        (first.name.as_str(), first.role.as_str()),
        ("ENG", "Role of eng")
    );
    assert!(first.at > 0);

    // The observer got one event, with the floor name and the switch (on by default).
    let asked = rig.events.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].0, rig.cwd);
    assert_eq!(asked[0].1.request, first);
    assert_eq!(asked[0].1.floor_name, "Test Floor");
    assert!(asked[0].1.notifications_on);

    // A second employee asks. The order is oldest first.
    let answer = rig.tool("ceo", "ask_operator", json!({"text": "Decide: A or B?"}));
    assert!(!answer.is_error, "{}", answer.text);
    // A new request of the same employee replaces its old one, with a new id, at the end.
    let answer = rig.tool("eng", "ask_operator", json!({"text": "Second text"}));
    assert!(!answer.is_error, "{}", answer.text);
    assert_eq!(
        requests(&rig),
        [
            ("ceo".to_owned(), "Decide: A or B?".to_owned()),
            ("eng".to_owned(), "Second text".to_owned())
        ]
    );
    let view = rig.tower.floor_open(&rig.cwd).unwrap();
    assert_ne!(view.requests[1].id, first.id);

    // The old id is unknown now: no error, no change. The request stays when the session ends.
    rig.tower
        .operator_request_clear(&rig.cwd, first.id)
        .unwrap();
    assert_eq!(requests(&rig).len(), 2);
    let eng_session = view.sessions.iter().find(|s| s.slug == "eng").unwrap();
    rig.tower.session_stop(&eng_session.session_id).unwrap();
    assert_eq!(requests(&rig).len(), 2);

    rig.tower
        .operator_request_clear(&rig.cwd, view.requests[1].id)
        .unwrap();
    assert_eq!(
        requests(&rig),
        [("ceo".to_owned(), "Decide: A or B?".to_owned())]
    );
}

#[test]
fn a_bad_text_is_refused_and_makes_no_request() {
    let rig = rig();
    for (args, expected) in [
        (json!({}), "\"text\" is missing"),
        (json!({"text": 5}), "\"text\" is missing"),
        (json!({"text": " \n "}), "The text is empty"),
        (json!({"text": "x".repeat(501)}), "too long"),
    ] {
        let answer = rig.tool("eng", "ask_operator", args.clone());
        assert!(answer.is_error, "{args}: {}", answer.text);
        assert!(answer.text.contains(expected), "{args}: {}", answer.text);
    }
    assert!(requests(&rig).is_empty());
    assert!(rig.events.asked.lock().unwrap().is_empty());
    let answer = rig.tool("eng", "ask_operator", json!({"text": "x".repeat(500)}));
    assert!(!answer.is_error, "{}", answer.text);
}
