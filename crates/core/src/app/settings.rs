//! The app config file `programs.toml` in the config folder. It holds the program overrides
//! (`[programs]`, see `tools.rs`) and the settings of the logo (`[openai]`):
//!
//! ```toml
//! [openai]
//! key = "sk-..."          # the OpenAI key (the file is made with mode 0600)
//! image_model = "..."     # the image model; the default is `logo::DEFAULT_IMAGE_MODEL`
//! ```
//!
//! The interface settings are in the table `[interface]` (since 2026-11-21):
//!
//! ```toml
//! [interface]
//! notify_on_question = false   # a desktop notification when an agent asks a question; the default is true
//! ```
//!
//! The Codex settings are in the table `[codex]` (since 2026-11-21):
//!
//! ```toml
//! [codex]
//! sandbox = true   # "Always": refuse a Codex start when the sandbox does not work
//! sandbox = false  # "Never": Codex employees have full access, and no probe runs
//! # no `sandbox` key: "Automatic" (the default): the sandbox is on when it works
//! ```
//!
//! Before 2026-11-21 (the second decision of that day) a missing key meant "off". Now it means
//! "Automatic". A stored `sandbox = true` stays "Always".
//!
//! The rules of the floors for commands with no question (since 2026-11-21) are in the table
//! `[floor_permissions]`. Each key is the real absolute path of a floor, and the value is the
//! list of the rules. The file is in the config folder of the app, not in a project folder, so an
//! agent of a floor cannot change it through the project:
//!
//! ```toml
//! [floor_permissions]
//! "/home/you/my project" = ["Bash(rtk cargo test:*)", "Read(./src/**)"]
//! ```
//!
//! Each writer reads the file again, changes its own table and writes the file back, so one
//! writer keeps the tables of the other. The key never goes in a log, an error text or a view.

use crate::env::EnvMap;
use crate::logo::DEFAULT_IMAGE_MODEL;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::Mutex;

/// The name of the variable that is the second place of the key.
pub const KEY_VARIABLE: &str = "OPENAI_API_KEY";

/// Only one writer at a time, so that two read-change-write steps cannot lose a change.
static WRITE: Mutex<()> = Mutex::new(());

/// Where the key comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    Config,
    Env,
}

/// The stored state of the setting "Codex sandbox" (decision O7, changed on 2026-11-21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexSandboxMode {
    /// No key in the file. The sandbox is on when the probe says that it works.
    Auto,
    /// `sandbox = true`. The sandbox is on. A failing probe refuses a Codex start.
    On,
    /// `sandbox = false`. Codex starts with full access. No probe runs.
    Off,
}

impl CodexSandboxMode {
    /// The word in the interface contract.
    pub fn as_str(self) -> &'static str {
        match self {
            CodexSandboxMode::Auto => "auto",
            CodexSandboxMode::On => "on",
            CodexSandboxMode::Off => "off",
        }
    }

    /// The reverse of `as_str`. `None` for any other text.
    pub fn parse(text: &str) -> Option<CodexSandboxMode> {
        match text {
            "auto" => Some(CodexSandboxMode::Auto),
            "on" => Some(CodexSandboxMode::On),
            "off" => Some(CodexSandboxMode::Off),
            _ => None,
        }
    }
}

/// What a Codex start gets now (see `SettingsView::codex_sandbox_effective`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxEffective {
    /// A Codex start gets the sandbox profile.
    On,
    /// A Codex start gets full access: the mode is `off`, or the mode is `auto` and the probe
    /// does not say `works`.
    Off,
    /// The mode is `on` and the probe does not say `works`: a Codex start is refused.
    Refused,
}

impl SandboxEffective {
    /// The word in the interface contract.
    pub fn as_str(self) -> &'static str {
        match self {
            SandboxEffective::On => "on",
            SandboxEffective::Off => "off",
            SandboxEffective::Refused => "refused",
        }
    }
}

/// `Settings` of the contract. It has no key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsView {
    pub openai_key_set: bool,
    pub openai_key_source: Option<KeySource>,
    pub image_model: String,
    /// `[interface] notify_on_question`: show a desktop notification when an agent asks a
    /// question and the window has no focus. The default is `true`.
    pub notify_on_question: bool,
    /// `[claude] auto_mode`: a Claude employee starts in the permission mode `auto` (not for
    /// the policy `ask-always`). The default is `true`.
    pub claude_auto_mode: bool,
    /// `[codex] sandbox` as it is stored: `auto` (no key), `on` (`true`) or `off` (`false`).
    pub codex_sandbox_mode: CodexSandboxMode,
    /// What a Codex start gets now. It is filled by `Tower::settings_get`, from the stored mode
    /// and the last probe of the sandbox. The file alone cannot say it, so `view` gives `Off`.
    pub codex_sandbox_effective: SandboxEffective,
    /// The old boolean field: `true` when the effective state is `On` (a Codex start gets the
    /// sandbox now). It is `false` for `Off` and for `Refused`.
    pub codex_sandbox: bool,
    /// The home folder of the app (`HOME` of its environment). The UI opens the folder dialog
    /// there when no floor gives a better place. `None` when `HOME` is not set.
    pub home_dir: Option<std::path::PathBuf>,
}

/// The table of a config file. A missing or unreadable file is an empty table.
pub(crate) fn read_table(file: &Path) -> toml::Table {
    fs::read_to_string(file)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

/// Read the file, apply `change` to the table, and write the file back (mode 0600, atomic).
pub(crate) fn update(file: &Path, change: impl FnOnce(&mut toml::Table)) -> io::Result<()> {
    let _guard = WRITE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut table = read_table(file);
    change(&mut table);
    let text = toml::to_string(&table).map_err(io::Error::other)?;
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = file.with_extension("toml.tmp");
    let result = (|| {
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        out.write_all(text.as_bytes())?;
        out.sync_all()?;
        fs::rename(&temp, file)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn text_of(table: &toml::Table, section: &str, name: &str) -> Option<String> {
    let text = table.get(section)?.as_table()?.get(name)?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// The key and where it came from: the config file first, then `OPENAI_API_KEY`.
pub(crate) fn openai_key(file: &Path, env: &EnvMap) -> Option<(String, KeySource)> {
    if let Some(key) = text_of(&read_table(file), "openai", "key") {
        return Some((key, KeySource::Config));
    }
    let value = env
        .get(std::ffi::OsStr::new(KEY_VARIABLE))?
        .to_str()?
        .trim();
    (!value.is_empty()).then(|| (value.to_owned(), KeySource::Env))
}

pub(crate) fn image_model(file: &Path) -> String {
    text_of(&read_table(file), "openai", "image_model")
        .unwrap_or_else(|| DEFAULT_IMAGE_MODEL.to_owned())
}

pub(crate) fn view(file: &Path, env: &EnvMap) -> SettingsView {
    let key = openai_key(file, env);
    SettingsView {
        openai_key_set: key.is_some(),
        openai_key_source: key.map(|(_, source)| source),
        image_model: image_model(file),
        notify_on_question: notify_on_question(file),
        claude_auto_mode: claude_auto_mode(file),
        codex_sandbox_mode: codex_sandbox_mode(file),
        codex_sandbox_effective: SandboxEffective::Off,
        codex_sandbox: false,
        home_dir: env
            .get(std::ffi::OsStr::new("HOME"))
            .filter(|home| !home.is_empty())
            .map(std::path::PathBuf::from),
    }
}

/// `[interface] notify_on_question`. A missing file, a missing key or a value that is not a
/// boolean gives the default: `true`.
pub(crate) fn notify_on_question(file: &Path) -> bool {
    read_table(file)
        .get("interface")
        .and_then(toml::Value::as_table)
        .and_then(|section| section.get("notify_on_question"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(true)
}

/// `[claude] auto_mode`. A missing file, a missing key or a value that is not a boolean gives
/// the default: `true`.
pub(crate) fn claude_auto_mode(file: &Path) -> bool {
    read_table(file)
        .get("claude")
        .and_then(toml::Value::as_table)
        .and_then(|section| section.get("auto_mode"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(true)
}

/// Set `[claude] auto_mode`. The default value (`true`) removes the key, and an empty table
/// goes too.
pub(crate) fn set_claude_auto_mode(table: &mut toml::Table, on: bool) {
    let section = table
        .entry("claude")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let now_empty = match section.as_table_mut() {
        Some(section) => {
            if on {
                section.remove("auto_mode");
            } else {
                section.insert("auto_mode".to_owned(), toml::Value::Boolean(false));
            }
            section.is_empty()
        }
        None => false,
    };
    if now_empty {
        table.remove("claude");
    }
}

/// Set `[interface] notify_on_question`. The default value (`true`) removes the key, and an
/// empty table goes too, so the file stays as small as it was.
pub(crate) fn set_notify_on_question(table: &mut toml::Table, on: bool) {
    let section = table
        .entry("interface")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let now_empty = match section.as_table_mut() {
        Some(section) => {
            if on {
                section.remove("notify_on_question");
            } else {
                section.insert("notify_on_question".to_owned(), toml::Value::Boolean(false));
            }
            section.is_empty()
        }
        None => false,
    };
    if now_empty {
        table.remove("interface");
    }
}

/// `[codex] sandbox`. A missing file, a missing key or a value that is not a boolean is
/// `Auto`. `true` is `On` (so an old file keeps its meaning). `false` is `Off`.
pub(crate) fn codex_sandbox_mode(file: &Path) -> CodexSandboxMode {
    match read_table(file)
        .get("codex")
        .and_then(toml::Value::as_table)
        .and_then(|section| section.get("sandbox"))
        .and_then(toml::Value::as_bool)
    {
        Some(true) => CodexSandboxMode::On,
        Some(false) => CodexSandboxMode::Off,
        None => CodexSandboxMode::Auto,
    }
}

/// Set `[codex] sandbox`. `Auto` (the default) removes the key, and an empty table goes too.
/// `On` stores `true` and `Off` stores `false`. Other keys of `[codex]` stay.
pub(crate) fn set_codex_sandbox_mode(table: &mut toml::Table, mode: CodexSandboxMode) {
    let section = table
        .entry("codex")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let now_empty = match section.as_table_mut() {
        Some(section) => {
            match mode {
                CodexSandboxMode::Auto => {
                    section.remove("sandbox");
                }
                CodexSandboxMode::On => {
                    section.insert("sandbox".to_owned(), toml::Value::Boolean(true));
                }
                CodexSandboxMode::Off => {
                    section.insert("sandbox".to_owned(), toml::Value::Boolean(false));
                }
            }
            section.is_empty()
        }
        None => false,
    };
    if now_empty {
        table.remove("codex");
    }
}

/// Set (or remove, for an empty text) one text of the `[openai]` table.
pub(crate) fn set_openai_text(table: &mut toml::Table, name: &str, text: &str) {
    let text = text.trim();
    let section = table
        .entry("openai")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let now_empty = match section.as_table_mut() {
        Some(section) => {
            if text.is_empty() {
                section.remove(name);
            } else {
                section.insert(name.to_owned(), toml::Value::String(text.to_owned()));
            }
            section.is_empty()
        }
        None => false,
    };
    if now_empty {
        table.remove("openai");
    }
}

/// The name of the table of the rules of the floors.
const FLOOR_PERMISSIONS: &str = "floor_permissions";

/// The key of a floor in the table: its path as text. `None` when the path is not valid UTF-8
/// (TOML has only text keys).
fn floor_key(floor: &Path) -> Option<&str> {
    floor.to_str()
}

/// The rules of a floor, as stored. `None` when the app config has no entry for the floor (the
/// operator did not set the list yet). An entry that is not a list of texts is an empty list.
pub(crate) fn floor_rules(file: &Path, floor: &Path) -> Option<Vec<String>> {
    let table = read_table(file);
    let value = table
        .get(FLOOR_PERMISSIONS)?
        .as_table()?
        .get(floor_key(floor)?)?;
    Some(
        value
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    )
}

/// Store the rules of a floor. An empty list is an entry too: the operator chose no rule.
/// `false` when the path has no text form (nothing is stored then).
pub(crate) fn set_floor_rules(table: &mut toml::Table, floor: &Path, rules: &[String]) -> bool {
    let Some(key) = floor_key(floor) else {
        return false;
    };
    let section = table
        .entry(FLOOR_PERMISSIONS)
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !section.is_table() {
        *section = toml::Value::Table(toml::Table::new());
    }
    if let Some(section) = section.as_table_mut() {
        section.insert(
            key.to_owned(),
            toml::Value::Array(rules.iter().cloned().map(toml::Value::String).collect()),
        );
    }
    true
}

/// Remove the entry of a floor. The table goes too when it is empty.
pub(crate) fn forget_floor_rules(table: &mut toml::Table, floor: &Path) {
    let Some(key) = floor_key(floor) else { return };
    let now_empty = match table
        .get_mut(FLOOR_PERMISSIONS)
        .and_then(toml::Value::as_table_mut)
    {
        Some(section) => {
            section.remove(key);
            section.is_empty()
        }
        None => false,
    };
    if now_empty {
        table.remove(FLOOR_PERMISSIONS);
    }
}

/// Does the app config have an entry for this floor?
pub(crate) fn has_floor_rules(file: &Path, floor: &Path) -> bool {
    floor_rules(file, floor).is_some()
}
