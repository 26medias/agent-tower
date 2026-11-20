//! The logo of a floor, on the side of the app: the state of the generation, the start of the
//! background thread, and the automatic start after a turn of the setup session.
//!
//! The pure work (the HTTP call and the picture clean-up) is in `crate::logo`. This file joins
//! it to the floor: the prompt in `floor.toml`, the key in the app config, the file
//! `.swarm/assets/logo.png` and the notifications.
//!
//! Locks. `FloorHandle::logo` is held for a read or a write of the three fields. Nothing else is
//! taken while it is held. The thread of a generation holds no lock while it waits for the network.

use super::live::{Notifier, locked};
use super::settings;
use super::{FloorHandle, Result, TowerError, other, reload};
use crate::env::EnvMap;
use crate::logo::{self, LogoRequest};
use crate::store::write_atomic;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

/// What a generation needs from the app. One value for each Tower, shared with the threads.
pub(super) struct LogoEnv {
    /// The app config file (`programs.toml`): the key and the image model.
    pub config_file: PathBuf,
    /// The environment for `OPENAI_API_KEY`.
    pub env: EnvMap,
    pub api_base: String,
    pub timeout: Duration,
}

/// The state of the generations of one floor. It lives in memory only.
#[derive(Default)]
pub(super) struct LogoState {
    /// A generation runs now.
    pub busy: bool,
    /// The error text of the last generation, if it failed.
    pub error: Option<String>,
    /// The subject of the last generation that started in this app run. The automatic start does
    /// not try the same subject again, so a failure does not make a loop.
    pub attempted: Option<String>,
    /// The value of `logo_prompt` (trimmed, `None` when not set) when the last setup session
    /// started. The automatic start runs only for a prompt that differs from it: an old prompt
    /// that was in the file before the setup is not a subject that the agent wrote.
    pub setup_prompt: Option<String>,
}

pub(super) fn logo_path(cwd: &Path) -> PathBuf {
    cwd.join(".swarm").join("assets").join("logo.png")
}

/// The modification time of `logo.png` in Unix milliseconds. `None` when there is no file.
pub(super) fn logo_version(cwd: &Path) -> Option<i64> {
    let modified = fs::metadata(logo_path(cwd)).ok()?.modified().ok()?;
    let since = modified.duration_since(UNIX_EPOCH).ok()?;
    Some(since.as_millis() as i64)
}

/// Start a generation in the background. The error is the text for the operator: `BUSY: ...`
/// when a generation runs for this floor already, and `NO_KEY: ...` when no key is known.
///
/// `new_prompt` `Some(text)` is written to `[brand] logo_prompt` first (an empty text removes it).
/// The subject of the picture is `logo_prompt`, or the name of the floor when there is none.
pub(super) fn start(
    env: &Arc<LogoEnv>,
    notifier: &Arc<Notifier>,
    floor: &Arc<FloorHandle>,
    new_prompt: Option<&str>,
) -> Result<()> {
    {
        let mut state = locked(&floor.logo);
        if state.busy {
            return Err(other(
                "BUSY: a logo generation runs for this floor now. Wait for it to end.",
            ));
        }
        state.busy = true;
    }
    match prepare(env, notifier, floor, new_prompt) {
        Ok((subject, key, model)) => {
            {
                let mut state = locked(&floor.logo);
                state.error = None;
                state.attempted = Some(subject.clone());
            }
            notifier.floor_changed(&floor.path);
            let run = Run {
                env: Arc::clone(env),
                notifier: Arc::clone(notifier),
                floor: Arc::clone(floor),
            };
            let spawned = thread::Builder::new()
                .name("app-logo".into())
                .spawn(move || run.finish(&subject, &key, &model));
            if let Err(error) = spawned {
                locked(&floor.logo).busy = false;
                return Err(other(format!("cannot start the logo thread: {error}")));
            }
            Ok(())
        }
        Err(error) => {
            locked(&floor.logo).busy = false;
            Err(error)
        }
    }
}

/// Everything before the thread: the prompt, the subject, the key and the model.
fn prepare(
    env: &LogoEnv,
    notifier: &Notifier,
    floor: &FloorHandle,
    new_prompt: Option<&str>,
) -> Result<(String, String, String)> {
    // The brand on disk can have changed (the setup agent writes it). Read it again.
    reload(floor)?;
    if let Some(text) = new_prompt {
        let text = text.trim();
        let mut store = locked(&floor.store);
        let mut brand = store.floor().brand.clone();
        brand.logo_prompt = (!text.is_empty()).then(|| text.to_owned());
        store.set_brand(brand)?;
        drop(store);
        notifier.floor_changed(&floor.path);
    }
    let subject = {
        let store = locked(&floor.store);
        match store.floor().brand.logo_prompt.as_deref() {
            Some(prompt) if !prompt.trim().is_empty() => prompt.trim().to_owned(),
            _ => store.floor().name.clone(),
        }
    };
    let (key, _) = settings::openai_key(&env.config_file, &env.env).ok_or_else(|| {
        other("NO_KEY: no OpenAI key is known. Save a key in the settings, or set OPENAI_API_KEY.")
    })?;
    let model = settings::image_model(&env.config_file);
    Ok((subject, key, model))
}

struct Run {
    env: Arc<LogoEnv>,
    notifier: Arc<Notifier>,
    floor: Arc<FloorHandle>,
}

impl Run {
    /// The body of the thread: generate, write the file, set the state, tell the observer.
    fn finish(&self, subject: &str, key: &str, model: &str) {
        let result = logo::generate(&LogoRequest {
            api_base: &self.env.api_base,
            key,
            model,
            subject,
            timeout: self.env.timeout,
        })
        .and_then(|png| write_logo(&self.floor.path, &png));
        {
            let mut state = locked(&self.floor.logo);
            state.busy = false;
            state.error = result.err();
        }
        self.notifier.floor_changed(&self.floor.path);
    }
}

fn write_logo(cwd: &Path, png: &[u8]) -> std::result::Result<(), String> {
    let path = logo_path(cwd);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    }
    write_atomic(&path, png).map_err(|e| e.to_string())
}

/// The trimmed `logo_prompt` of the floor in memory, or `None` when it is not set or is empty.
fn current_prompt(floor: &FloorHandle) -> Option<String> {
    locked(&floor.store)
        .floor()
        .brand
        .logo_prompt
        .as_deref()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .map(str::to_owned)
}

/// Remember the `logo_prompt` that the floor has now. `setup_start` calls it, so that the
/// automatic start can tell the subject that the agent writes from an old one.
pub(super) fn remember_setup_prompt(floor: &FloorHandle) {
    let prompt = current_prompt(floor);
    locked(&floor.logo).setup_prompt = prompt;
}

/// After a turn of the setup session (the floor was read again): start a generation when
/// `logo_prompt` is set and differs from the value that it had when the setup started (the agent
/// wrote a new subject), a key is known, and no generation runs or was tried for this prompt in
/// this app run. A `logo.png` that exists does not stop it: the new subject replaces the logo.
/// An error here is not shown: the manual button shows it.
pub(super) fn auto_start(env: &Arc<LogoEnv>, notifier: &Arc<Notifier>, floor: &Arc<FloorHandle>) {
    let Some(prompt) = current_prompt(floor) else {
        return;
    };
    if settings::openai_key(&env.config_file, &env.env).is_none() {
        return;
    }
    {
        let state = locked(&floor.logo);
        if state.setup_prompt.as_deref() == Some(prompt.as_str())
            || state.busy
            || state.attempted.as_deref() == Some(prompt.as_str())
        {
            return;
        }
    }
    let _: std::result::Result<(), TowerError> = start(env, notifier, floor, None);
}
