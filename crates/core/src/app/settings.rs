//! The app config file `programs.toml` in the config folder. It holds the program overrides
//! (`[programs]`, see `tools.rs`) and the settings of the logo (`[openai]`):
//!
//! ```toml
//! [openai]
//! key = "sk-..."          # the OpenAI key (the file is made with mode 0600)
//! image_model = "..."     # the image model; the default is `logo::DEFAULT_IMAGE_MODEL`
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

/// `Settings` of the contract. It has no key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsView {
    pub openai_key_set: bool,
    pub openai_key_source: Option<KeySource>,
    pub image_model: String,
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
