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
use tower_core::login_env::LoginEnvSource;
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

/// The MCP tokens that the app of ONE test gave to its sessions: (slug, token). Each `App` has
/// its own list. A list that all tests share (a `static`) lets a test read the token of a session
/// of another test, which is a token for another Tower.
type Tokens = Arc<Mutex<Vec<(String, String)>>>;

/// Starts `fake-cli` in place of a CLI. The first message is the positional argument. The
/// second field is the token list of the test that owns this adapter.
struct FakeAdapter(CliKind, Tokens);

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
        if let Some(mcp) = &ctx.mcp {
            self.1
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((ctx.employee.slug.clone(), mcp.token.clone()));
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
    mcp_url: String,
    tokens: Tokens,
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
    // No test runs a real login shell.
    config.login_env = LoginEnvSource::Fixed(None);
    let tokens: Tokens = Arc::default();
    let adapter_tokens = Arc::clone(&tokens);
    config.adapters =
        Arc::new(move |kind| Box::new(FakeAdapter(kind, Arc::clone(&adapter_tokens))));
    // The Codex home is a folder of the test, never the real one.
    config.codex_home = Some(base.join("codex-home"));
    // No test reaches the real image API, also not by mistake.
    config.image_api_base = "http://127.0.0.1:1".to_owned();
    let tower = Arc::new(Tower::new(config).unwrap());
    let mcp_url = tower.mcp_url().to_owned();
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
    for name in ["session-changed", "floor-changed", "employee-started"] {
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
        mcp_url,
        tokens,
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
        json!([{ "path": path, "name": "Acme", "missing": false, "logoVersion": null }])
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

#[test]
fn the_logo_settings_and_setup_commands_work_through_the_ipc_with_the_contract_shapes() {
    let app = app();
    let path = app.path();
    let floor = app
        .call("floor_create", json!({ "path": path, "name": "Acme" }))
        .unwrap();
    // The new fields of `FloorView` and `FloorEntry`.
    for (field, value) in [
        ("description", Value::Null),
        ("logoPrompt", Value::Null),
        ("logoVersion", Value::Null),
        ("logoBusy", json!(false)),
        ("logoError", Value::Null),
        ("setupSessionId", Value::Null),
    ] {
        assert_eq!(floor[field], value, "{field}");
    }
    let list = app.call("floor_list", json!({})).unwrap();
    assert_eq!(list[0]["logoVersion"], Value::Null);
    assert_eq!(
        app.call("floor_logo", json!({ "path": path })).unwrap(),
        Value::Null
    );

    // Settings. The arguments are camelCase, and a null is "no change".
    assert_eq!(
        app.call("settings_get", json!({})).unwrap(),
        json!({ "openaiKeySet": false, "openaiKeySource": null, "imageModel": "gpt-image-2.6-sunburst" })
    );
    let set = app
        .call(
            "settings_set",
            json!({ "openaiKey": null, "imageModel": "model-y" }),
        )
        .unwrap();
    assert_eq!(set["imageModel"], "model-y");
    assert_eq!(set["openaiKeySet"], json!(false));
    let set = app
        .call(
            "settings_set",
            json!({ "openaiKey": "sk-test-1", "imageModel": null }),
        )
        .unwrap();
    assert_eq!(
        set,
        json!({ "openaiKeySet": true, "openaiKeySource": "config", "imageModel": "model-y" })
    );
    assert!(!set.to_string().contains("sk-test-1"));
    let set = app
        .call(
            "settings_set",
            json!({ "openaiKey": "", "imageModel": null }),
        )
        .unwrap();
    assert_eq!(set["openaiKeySet"], json!(false));

    // No key: the error starts with NO_KEY:, and the prompt is saved.
    let error = app
        .call("logo_generate", json!({ "path": path, "prompt": "A bee." }))
        .unwrap_err();
    assert!(error.starts_with("NO_KEY:"), "{error}");
    let floor = app.call("floor_open", json!({ "path": path })).unwrap();
    assert_eq!(floor["logoPrompt"], "A bee.");

    // The slug `setup` is reserved.
    let error = app
        .call(
            "employee_save",
            json!({ "path": path, "employee": {
                "slug": "setup", "name": "S", "role": "R", "manager": null,
                "cli": "claude", "model": null, "effort": null,
                "policy": "read_only", "roleText": ""
            }}),
        )
        .unwrap_err();
    assert!(error.contains("reserved"), "{error}");

    // The setup session: slug `setup`, one at a time, and the live ID in the floor view.
    let session = app
        .call(
            "setup_start",
            json!({ "path": path, "cli": "claude", "rows": 24, "cols": 80 }),
        )
        .unwrap();
    let id = session["sessionId"].as_str().unwrap().to_owned();
    assert_eq!(session["slug"], "setup");
    assert_eq!(session["status"], "starting");
    let floor = app.call("floor_open", json!({ "path": path })).unwrap();
    assert_eq!(floor["setupSessionId"], json!(id));
    assert!(
        floor["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["slug"] == "setup" && s["sessionId"] == json!(id))
    );
    let error = app
        .call(
            "setup_start",
            json!({ "path": path, "cli": "codex", "rows": 24, "cols": 80 }),
        )
        .unwrap_err();
    assert!(error.contains("already has a live session"), "{error}");
    app.wait_event("idle after the first turn", |e| {
        e["session"]["sessionId"] == json!(id) && e["session"]["status"] == "idle"
    });
    app.call("session_stop", json!({ "sessionId": id }))
        .unwrap();
    app.wait_event("stopped", |e| {
        e["session"]["sessionId"] == json!(id) && e["session"]["status"] == "stopped"
    });
    let _ = &app.dir;
}

#[test]
fn the_model_and_defaults_commands_work_through_the_ipc_with_the_contract_shapes() {
    let app = app();
    let path = app.path();
    let floor = app
        .call("floor_create", json!({ "path": path, "name": "Acme" }))
        .unwrap();
    // `FloorView.defaults` is there, with four null fields.
    assert_eq!(
        floor["defaults"],
        json!({ "cli": null, "model": null, "effort": null, "policy": null })
    );

    // `models_get`: the camelCase fields. The Codex home is a folder of the test (no file).
    let catalog = app.call("models_get", json!({})).unwrap();
    assert_eq!(catalog["codexSource"], "builtin");
    assert_eq!(
        catalog["claude"][3],
        json!({
            "id": "haiku", "label": "Haiku", "custom": false, "defaultEffort": null,
            "efforts": ["low", "medium", "high", "xhigh", "max"]
        })
    );
    assert_eq!(catalog["image"][0]["id"], "gpt-image-2.6-sunburst");
    assert_eq!(catalog["image"][0]["efforts"], json!([]));

    // The Codex file of the test Codex home is used, and read again on each call.
    let home = app.dir.path().join("codex-home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("models_cache.json"),
        r#"{"models": [{"slug": "gpt-t", "display_name": "GPT T", "visibility": "list",
            "default_reasoning_level": "high", "supported_reasoning_levels": [{"effort": "high"}]}]}"#,
    )
    .unwrap();
    let catalog = app.call("models_get", json!({})).unwrap();
    assert_eq!(catalog["codexSource"], "cache");
    assert_eq!(
        catalog["codex"],
        json!([{ "id": "gpt-t", "label": "GPT T", "efforts": ["high"],
            "defaultEffort": "high", "custom": false }])
    );

    // `models_set_custom`: the argument names are `kind` and `models`.
    let catalog = app
        .call(
            "models_set_custom",
            json!({ "kind": "codex", "models": ["my-model", "gpt-t"] }),
        )
        .unwrap();
    assert_eq!(catalog["codex"][1]["id"], "my-model");
    assert_eq!(catalog["codex"][1]["custom"], json!(true));
    assert_eq!(catalog["codex"].as_array().unwrap().len(), 2);
    let error = app
        .call(
            "models_set_custom",
            json!({ "kind": "claude", "models": ["--foo"] }),
        )
        .unwrap_err();
    assert!(error.contains("not a valid model name"), "{error}");
    let error = app
        .call("models_set_custom", json!({ "kind": "bad", "models": [] }))
        .unwrap_err();
    assert!(error.contains("unknown model list"), "{error}");

    // `floor_defaults_set`: the JSON of the contract, the policy in snake_case, and an event.
    let before = app.events.lock().unwrap().len();
    let view = app
        .call(
            "floor_defaults_set",
            json!({ "path": path, "defaults": {
                "cli": "codex", "model": "gpt-t", "effort": "high", "policy": "ask_always"
            }}),
        )
        .unwrap();
    assert_eq!(
        view["defaults"],
        json!({ "cli": "codex", "model": "gpt-t", "effort": "high", "policy": "ask_always" })
    );
    assert!(app.events.lock().unwrap().len() > before);
    assert!(
        app.events
            .lock()
            .unwrap()
            .iter()
            .any(|(name, payload)| name == "floor-changed" && payload == &json!({ "path": path }))
    );
    let floor = app.call("floor_open", json!({ "path": path })).unwrap();
    assert_eq!(floor["defaults"]["model"], "gpt-t");
    // A null field goes out of the file.
    let view = app
        .call(
            "floor_defaults_set",
            json!({ "path": path, "defaults": {
                "cli": null, "model": null, "effort": null, "policy": null
            }}),
        )
        .unwrap();
    assert_eq!(view["defaults"]["cli"], Value::Null);
    let text = std::fs::read_to_string(app.cwd.join(".swarm/floor.toml")).unwrap();
    assert!(!text.contains("defaults"), "{text}");

    // The model name check at the other doors, through the IPC.
    let error = app
        .call(
            "floor_defaults_set",
            json!({ "path": path, "defaults": {
                "cli": null, "model": "--foo", "effort": null, "policy": null
            }}),
        )
        .unwrap_err();
    assert!(error.contains("not a valid model name"), "{error}");
    let error = app
        .call(
            "employee_save",
            json!({ "path": path, "employee": {
                "slug": "bad", "name": "B", "role": "R", "manager": null,
                "cli": "claude", "model": "--foo", "effort": null,
                "policy": "read_only", "roleText": ""
            }}),
        )
        .unwrap_err();
    assert!(error.contains("not a valid model name"), "{error}");
    let error = app
        .call(
            "settings_set",
            json!({ "openaiKey": null, "imageModel": "--foo" }),
        )
        .unwrap_err();
    assert!(error.contains("not a valid model name"), "{error}");
    let _ = &app.dir;
}

/// One `tools/call` over HTTP, as a CLI would send it.
fn mcp_call(url: &str, token: &str, tool: &str, args: Value) -> Value {
    use std::io::{Read, Write};
    let host = url.trim_start_matches("http://").split('/').next().unwrap();
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": tool, "arguments": args}}).to_string();
    let mut stream = std::net::TcpStream::connect(host).unwrap();
    stream
        .write_all(
            format!(
                "POST /mcp HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    serde_json::from_str(raw.split_once("\r\n\r\n").unwrap().1).unwrap()
}

#[test]
fn an_employee_start_comes_as_a_session_changed_and_an_employee_started_event() {
    let app = app();
    let path = app.path();
    app.call("floor_create", json!({ "path": path, "name": "Tower" }))
        .unwrap();
    let ceo = app
        .call(
            "session_start",
            json!({ "path": path, "slug": "ceo", "firstMessage": null, "rows": 24, "cols": 80 }),
        )
        .unwrap();
    let token = app
        .tokens
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .rev()
        .find(|(slug, _)| slug == "ceo")
        .map(|(_, token)| token.clone())
        .expect("the session has a token");
    assert_ne!(ceo["sessionId"], Value::Null);

    let answer = mcp_call(
        &app.mcp_url,
        &token,
        "employee_start",
        json!({"employee": "pm", "message": "Plan it."}),
    );
    assert_eq!(answer["result"]["isError"], false, "{answer}");

    // The UI hears about the new session as for any session, and about who started it.
    app.wait_event("the session of the pm shows", |e| {
        e["session"]["slug"] == "pm"
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let started = loop {
        let found = app
            .events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(name, _)| name == "employee-started")
            .map(|(_, payload)| payload.clone());
        if let Some(found) = found {
            break found;
        }
        assert!(Instant::now() < deadline, "no employee-started event");
        std::thread::sleep(Duration::from_millis(10));
    };
    let pm_session = app
        .session_events()
        .into_iter()
        .find(|e| e["session"]["slug"] == "pm")
        .unwrap();
    assert_eq!(
        started,
        json!({
            "path": path,
            "sessionId": pm_session["session"]["sessionId"],
            "slug": "pm",
            "name": "Maya",
            "bySlug": "ceo",
            "byName": "Ada",
            "resumed": false,
        })
    );
}
