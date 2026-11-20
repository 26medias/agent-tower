//! The commands and events of `docs/ipc-m1.md`, called through the real IPC path of Tauri (the
//! mock runtime: argument names, JSON conversion, error texts, events). The sessions are the
//! real `fake-cli`. The channel of `session_attach` is not covered here (it needs a webview).

use agent_tower_lib::{install_tower, register_commands};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, Once, PoisonError};
use std::time::{Duration, Instant};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{Listener, WebviewWindow, WebviewWindowBuilder};
use tower_core::adapter::{CliAdapter, SpawnPlan, StartContext};
use tower_core::app::{Tower, TowerConfig};
use tower_core::env::build_env;
use tower_core::model::CliKind;
use tower_core::programs::Resolver;
use tower_core::pty::SpawnSpec;

fn bin(name: &str) -> PathBuf {
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let status = Command::new(cargo)
            .args(["build", "--quiet", "-p", "fakecli", "-p", "tower-hook"])
            .current_dir(root)
            .status()
            .expect("run cargo build");
        assert!(status.success(), "cargo build of the test programs failed");
    });
    let exe = std::env::current_exe().expect("test executable path");
    exe.parent()
        .and_then(Path::parent)
        .expect("target folder")
        .join(name)
}

/// Starts `fake-cli` in place of a CLI. The first message is the positional argument.
struct FakeAdapter(CliKind);

impl FakeAdapter {
    fn build(&self, ctx: &StartContext, cli_id: &str) -> tower_core::adapter::Result<SpawnPlan> {
        let mut args: Vec<OsString> = vec![
            "--hook".into(),
            ctx.hook_program.clone().into_os_string(),
            "--cli-session-id".into(),
            cli_id.into(),
        ];
        if self.0 == CliKind::Codex {
            args.extend(["--kind".into(), "codex".into()]);
        }
        if let Some(message) = &ctx.first_message {
            args.push(message.into());
        }
        let extra: Vec<(OsString, OsString)> = vec![
            ("TOWER_SESSION".into(), ctx.app_session_id.clone().into()),
            ("TOWER_SOCKET".into(), ctx.socket.clone().into_os_string()),
        ];
        Ok(SpawnPlan {
            spec: SpawnSpec {
                program: bin("fake-cli"),
                args,
                env: build_env(&ctx.parent_env, &extra, &[])?,
                cwd: ctx.cwd.clone(),
                rows: ctx.rows,
                cols: ctx.cols,
                log: ctx.log.clone(),
            },
            cli_session_id: Some(cli_id.to_owned()),
        })
    }
}

impl CliAdapter for FakeAdapter {
    fn start(&self, ctx: &StartContext) -> tower_core::adapter::Result<SpawnPlan> {
        self.build(ctx, "cli-session-1")
    }

    fn resume(&self, ctx: &StartContext, id: &str) -> tower_core::adapter::Result<SpawnPlan> {
        self.build(ctx, id)
    }
}

struct App {
    webview: WebviewWindow<MockRuntime>,
    events: Arc<Mutex<Vec<(String, Value)>>>,
    dir: tempfile::TempDir,
    cwd: PathBuf,
    _app: tauri::App<MockRuntime>,
}

fn app() -> App {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let cwd = base.join("project");
    std::fs::create_dir(&cwd).unwrap();
    let mut config = TowerConfig::new(base.join("config"));
    config.socket_path = Some(base.join("hook.sock"));
    config.hook_program = Some(bin("tower-hook"));
    config.resolver = Resolver {
        shell: None,
        known_dirs: vec![],
        shell_timeout: Duration::from_secs(1),
    };
    config.parent_env = [("HOME", "/tmp")]
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect();
    config.adapters = Arc::new(|kind| Box::new(FakeAdapter(kind)));
    let tower = Arc::new(Tower::new(config).unwrap());
    for name in ["claude", "codex"] {
        tower
            .programs_set(name, bin("fake-cli").to_str().unwrap())
            .unwrap();
    }
    tower.programs_set("node", "/bin/true").unwrap();

    let app = register_commands(mock_builder())
        .build(mock_context(noop_assets()))
        .unwrap();
    install_tower(&app, tower);
    let events = Arc::new(Mutex::new(Vec::new()));
    for name in ["session-changed", "floor-changed"] {
        let events = Arc::clone(&events);
        app.listen(name, move |event| {
            let payload: Value = serde_json::from_str(event.payload()).unwrap();
            events
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((name.to_owned(), payload));
        });
    }
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    App {
        webview,
        events,
        dir,
        cwd,
        _app: app,
    }
}

impl App {
    fn call(&self, cmd: &str, args: Value) -> Result<Value, String> {
        let response = get_ipc_response(
            &self.webview,
            InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        );
        match response {
            Ok(body) => Ok(body.deserialize::<Value>().unwrap()),
            Err(error) => Err(error
                .as_str()
                .map_or_else(|| error.to_string(), str::to_owned)),
        }
    }

    fn path(&self) -> String {
        self.cwd.to_str().unwrap().to_owned()
    }

    fn session_events(&self) -> Vec<Value> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(name, _)| name == "session-changed")
            .map(|(_, payload)| payload.clone())
            .collect()
    }

    fn wait_event(&self, what: &str, test: impl Fn(&Value) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.session_events().iter().any(&test) {
            assert!(
                Instant::now() < deadline,
                "timeout: {what}. Events: {:?}",
                self.session_events()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn every_command_works_through_the_ipc_with_the_contract_names() {
    let app = app();
    let path = app.path();

    // No floor: the error is a text that starts with NO_FLOOR:.
    let error = app.call("floor_open", json!({ "path": path })).unwrap_err();
    assert!(error.starts_with("NO_FLOOR:"), "{error}");
    assert_eq!(app.call("floor_list", json!({})).unwrap(), json!([]));

    // Create: the two default employees, in the JSON of the contract.
    let floor = app
        .call("floor_create", json!({ "path": path, "name": "Acme" }))
        .unwrap();
    assert_eq!(floor["path"], json!(path));
    assert_eq!(floor["name"], json!("Acme"));
    assert_eq!(floor["employeeErrors"], json!([]));
    assert_eq!(floor["sessions"], json!([]));
    let ceo = &floor["employees"][0];
    assert_eq!(ceo["slug"], "ceo");
    assert_eq!(ceo["role"], "CEO");
    assert_eq!(ceo["cli"], "claude");
    assert_eq!(ceo["policy"], "read_only");
    assert_eq!(ceo["manager"], Value::Null);
    assert!(ceo["roleText"].as_str().unwrap().len() > 10);
    assert_eq!(floor["employees"][1]["manager"], "ceo");
    let list = app.call("floor_list", json!({})).unwrap();
    assert_eq!(
        list,
        json!([{ "path": path, "name": "Acme", "missing": false }])
    );

    // Save an employee as the UI sends it (camelCase).
    let saved = app
        .call(
            "employee_save",
            json!({ "path": path, "employee": {
                "slug": "eng-1", "name": "Eli", "role": "Engineer", "manager": "pm",
                "cli": "codex", "model": null, "effort": "high",
                "policy": "edit_in_worktree", "roleText": "Write code."
            }}),
        )
        .unwrap();
    assert_eq!(saved["employees"].as_array().unwrap().len(), 3);
    assert!(
        app.events
            .lock()
            .unwrap()
            .iter()
            .any(|(name, payload)| name == "floor-changed" && payload == &json!({ "path": path }))
    );
    let removed = app
        .call("employee_remove", json!({ "path": path, "slug": "eng-1" }))
        .unwrap();
    assert_eq!(removed["employees"].as_array().unwrap().len(), 2);

    // Start (arguments as the UI sends them) and the events.
    let session = app
        .call(
            "session_start",
            json!({ "path": path, "slug": "ceo", "firstMessage": null, "rows": 24, "cols": 80 }),
        )
        .unwrap();
    let id = session["sessionId"].as_str().unwrap().to_owned();
    assert_eq!(session["slug"], "ceo");
    assert_eq!(session["status"], "starting");
    assert_eq!(
        (
            session["draft"].clone(),
            session["messageWaiting"].clone(),
            session["canResume"].clone()
        ),
        (json!(false), json!(false), json!(false))
    );
    app.wait_event("idle after the first turn", |e| {
        e["path"] == json!(path)
            && e["session"]["sessionId"] == json!(id)
            && e["session"]["status"] == "idle"
    });
    // The first turn: starting, idle, working, idle (the last `idle` is after `working`).
    app.wait_event("working", |e| e["session"]["status"] == "working");
    let statuses: Vec<String> = app
        .session_events()
        .iter()
        .map(|e| e["session"]["status"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(&statuses[..3], ["starting", "idle", "working"]);

    // A second start is refused with a text.
    let error = app
        .call(
            "session_start",
            json!({ "path": path, "slug": "ceo", "firstMessage": "x", "rows": 24, "cols": 80 }),
        )
        .unwrap_err();
    assert!(error.contains("already has a live session"), "{error}");

    // The floor view has the session.
    let floor = app.call("floor_open", json!({ "path": path })).unwrap();
    assert_eq!(floor["sessions"][0]["sessionId"], json!(id));

    // Wait for the end of the first turn, then write (base64) and resize.
    app.wait_event("idle again", |e| {
        e["session"]["status"] == "idle"
            && app
                .session_events()
                .iter()
                .any(|w| w["session"]["status"] == "working")
    });
    std::thread::sleep(Duration::from_millis(300));
    app.call(
        "pty_write",
        json!({ "sessionId": id, "data": STANDARD.encode("half") }),
    )
    .unwrap();
    app.wait_event("the draft flag", |e| e["session"]["draft"] == json!(true));
    let error = app
        .call("debug_paste", json!({ "sessionId": id, "text": "hi" }))
        .unwrap_err();
    assert_eq!(error, "REFUSED: draft");
    app.wait_event("message waiting", |e| {
        e["session"]["messageWaiting"] == json!(true)
    });
    app.call("clear_draft", json!({ "sessionId": id })).unwrap();
    app.call("debug_paste", json!({ "sessionId": id, "text": "hi" }))
        .unwrap();
    app.call(
        "pty_resize",
        json!({ "sessionId": id, "rows": 30, "cols": 100 }),
    )
    .unwrap();
    let error = app
        .call("pty_write", json!({ "sessionId": id, "data": "!!!" }))
        .unwrap_err();
    assert!(error.contains("base64"), "{error}");

    // Programs.
    let programs = app.call("programs_get", json!({})).unwrap();
    assert_eq!(programs["claude"], json!(bin("fake-cli").to_str().unwrap()));
    assert_eq!(programs["hook"], json!(bin("tower-hook").to_str().unwrap()));
    let error = app
        .call(
            "programs_set",
            json!({ "name": "codex", "path": "/no/such" }),
        )
        .unwrap_err();
    assert!(error.contains("not an executable file"), "{error}");
    let set = app
        .call(
            "programs_set",
            json!({ "name": "node", "path": "/bin/true" }),
        )
        .unwrap();
    assert_eq!(set["node"], "/bin/true");

    // Stop, then the floor shows a stopped session that can resume.
    app.call("session_stop", json!({ "sessionId": id }))
        .unwrap();
    app.wait_event("stopped", |e| {
        e["session"]["sessionId"] == json!(id) && e["session"]["status"] == "stopped"
    });
    let floor = app.call("floor_open", json!({ "path": path })).unwrap();
    assert_eq!(floor["sessions"][0]["status"], "stopped");
    assert_eq!(floor["sessions"][0]["canResume"], json!(true));
    let resumed = app
        .call(
            "session_resume",
            json!({ "path": path, "slug": "ceo", "message": null, "rows": 24, "cols": 80 }),
        )
        .unwrap();
    assert_ne!(resumed["sessionId"], json!(id));
    app.call("session_stop", json!({ "sessionId": resumed["sessionId"] }))
        .unwrap();

    // Forget removes the floor from the list and keeps the files.
    app.wait_event("the resumed session stopped", |e| {
        e["session"]["sessionId"] == resumed["sessionId"] && e["session"]["status"] == "stopped"
    });
    app.call("floor_forget", json!({ "path": path })).unwrap();
    assert_eq!(app.call("floor_list", json!({})).unwrap(), json!([]));
    assert!(app.cwd.join(".swarm/floor.toml").exists());
    let _ = &app.dir;
}

#[test]
fn an_unknown_session_gives_a_text_error() {
    let app = app();
    let error = app
        .call("session_stop", json!({ "sessionId": "nope" }))
        .unwrap_err();
    assert!(error.contains("no session"), "{error}");
}
