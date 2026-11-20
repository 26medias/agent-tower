//! The model lists (`docs/ipc-settings.md`, section 2).
//!
//! - Claude: a built-in list of aliases. Claude Code has no local model file.
//! - Codex: the `models_cache.json` of the Codex home. The app only reads this file. If it is
//!   missing or does not parse, a built-in list is used.
//! - Image: a built-in list.
//! - Custom models: the `[models]` table of the app config file (`programs.toml`), keys `claude`,
//!   `codex` and `image`. A custom model comes after the other models of its list.
//!
//! A model name goes to a CLI as an argument. So this file also holds the check of a name: a name
//! that starts with `-` could become a flag of the CLI.

use super::settings;
use serde::Deserialize;
use std::fs;
use std::io::Read;
use std::path::Path;

/// The efforts of a Claude model, of a custom model, and of a built-in Codex model.
pub const STANDARD_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// The longest model name.
const MODEL_NAME_LIMIT: usize = 100;

/// The longest effort name.
const EFFORT_LIMIT: usize = 20;

/// The most bytes that the app reads from `models_cache.json`. The real file has about 600 KB.
const CACHE_LIMIT: u64 = 16 * 1024 * 1024;

/// The characters of a model name, other than letters and digits.
const NAME_SYMBOLS: [char; 6] = ['.', '_', ':', '/', '[', ']'];

/// A model name is valid if it has 1 to 100 characters, each from `A-Z a-z 0-9 . _ : / [ ] -`, and
/// the first character is not `-`.
pub fn is_valid_model_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MODEL_NAME_LIMIT
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || NAME_SYMBOLS.contains(&c))
}

/// An effort is valid if it has 1 to 20 characters from `a-z`.
pub fn is_valid_effort(effort: &str) -> bool {
    !effort.is_empty()
        && effort.len() <= EFFORT_LIMIT
        && effort.chars().all(|c| c.is_ascii_lowercase())
}

/// The error text for a bad model name. `what` says whose name it is. The name is shown with
/// `{:?}`, so a control character cannot break the text.
pub fn model_name_error(what: &str, name: &str) -> String {
    format!(
        "the {what} {name:?} is not a valid model name. A model name has 1 to 100 characters from \
         A-Z a-z 0-9 . _ : / [ ] - and does not start with \"-\"."
    )
}

/// The error text for a bad effort.
pub fn effort_error(what: &str, effort: &str) -> String {
    format!(
        "the {what} {effort:?} is not a valid effort. An effort has 1 to 20 characters from a-z."
    )
}

/// Check an optional model. `None` and the empty text mean "not set" and are fine.
pub fn check_model(what: &str, model: Option<&str>) -> Result<(), String> {
    match model {
        Some(name) if !name.is_empty() && !is_valid_model_name(name) => {
            Err(model_name_error(what, name))
        }
        _ => Ok(()),
    }
}

/// Check an optional effort. `None` and the empty text mean "not set" and are fine.
pub fn check_effort(what: &str, effort: Option<&str>) -> Result<(), String> {
    match effort {
        Some(text) if !text.is_empty() && !is_valid_effort(text) => Err(effort_error(what, text)),
        _ => Ok(()),
    }
}

/// One model of a list (`ModelOption` of the contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOption {
    /// The value that goes to the CLI.
    pub id: String,
    pub label: String,
    /// Empty for an image model.
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
    /// From the `[models]` table of the app config.
    pub custom: bool,
}

/// Where the Codex list came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexSource {
    Cache,
    Builtin,
}

/// `ModelCatalog` of the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCatalog {
    pub claude: Vec<ModelOption>,
    pub codex: Vec<ModelOption>,
    pub image: Vec<ModelOption>,
    pub codex_source: CodexSource,
}

/// The three lists that take custom models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Claude,
    Codex,
    Image,
}

impl ModelKind {
    /// `claude`, `codex` or `image`.
    pub fn parse(text: &str) -> Result<ModelKind, String> {
        match text {
            "claude" => Ok(ModelKind::Claude),
            "codex" => Ok(ModelKind::Codex),
            "image" => Ok(ModelKind::Image),
            other => Err(format!(
                "unknown model list {other:?} (use claude, codex or image)"
            )),
        }
    }

    fn key(self) -> &'static str {
        match self {
            ModelKind::Claude => "claude",
            ModelKind::Codex => "codex",
            ModelKind::Image => "image",
        }
    }
}

fn efforts(list: &[&str]) -> Vec<String> {
    list.iter().map(|e| (*e).to_owned()).collect()
}

fn option(id: &str, label: &str, efforts: Vec<String>, default: Option<&str>) -> ModelOption {
    ModelOption {
        id: id.to_owned(),
        label: label.to_owned(),
        efforts,
        default_effort: default.map(str::to_owned),
        custom: false,
    }
}

fn builtin_claude() -> Vec<ModelOption> {
    [
        ("fable", "Fable"),
        ("opus", "Opus"),
        ("sonnet", "Sonnet"),
        ("haiku", "Haiku"),
    ]
    .iter()
    .map(|(id, label)| option(id, label, efforts(&STANDARD_EFFORTS), None))
    .collect()
}

/// The Codex list that is used when `models_cache.json` is missing or does not parse.
fn builtin_codex() -> Vec<ModelOption> {
    [
        ("gpt-6.2-sol", "GPT-6.2-Sol"),
        ("gpt-6.2-astra", "GPT-6.2-Astra"),
        ("gpt-6.1-terra", "GPT-6.1 Terra"),
        ("gpt-6.1-sol", "GPT-6.1-Sol"),
        ("gpt-6-astra", "GPT-6-Astra"),
        ("gpt-6-sol", "GPT-6-Sol"),
        ("gpt-6-luna", "GPT-6-Luna"),
        ("gpt-5.6-sol", "GPT-5.6-Sol"),
        ("gpt-5.6-terra", "GPT-5.6-Terra"),
        ("gpt-5.6-luna", "GPT-5.6-Luna"),
    ]
    .iter()
    .map(|(id, label)| option(id, label, efforts(&STANDARD_EFFORTS), Some("medium")))
    .collect()
}

fn builtin_image() -> Vec<ModelOption> {
    ["gpt-image-2.6-sunburst", "gpt-image-2.6-flare"]
        .iter()
        .map(|id| option(id, id, Vec::new(), None))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The Codex cache file
// ---------------------------------------------------------------------------------------------

/// The fields of `models_cache.json` that the app needs. Other fields are ignored.
#[derive(Deserialize)]
struct CacheFile {
    models: Vec<CacheModel>,
}

#[derive(Deserialize)]
struct CacheModel {
    slug: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    visibility: Option<String>,
    #[serde(default)]
    default_reasoning_level: Option<String>,
    #[serde(default)]
    supported_reasoning_levels: Vec<CacheEffort>,
}

#[derive(Deserialize)]
struct CacheEffort {
    effort: String,
}

/// The models with `visibility` = `list`, in the file order. `None` when the file is missing,
/// is too big, does not parse, or has no usable model.
///
/// An entry whose slug is not a valid model name is left out, because the slug goes to the CLI.
/// An effort that is not valid is left out of the efforts of the entry.
fn read_codex_cache(codex_home: &Path) -> Option<Vec<ModelOption>> {
    let file = fs::File::open(codex_home.join("models_cache.json")).ok()?;
    let mut bytes = Vec::new();
    // One byte more than the limit shows that the file is too big.
    file.take(CACHE_LIMIT + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > CACHE_LIMIT {
        return None;
    }
    let parsed: CacheFile = serde_json::from_slice(&bytes).ok()?;
    let mut list: Vec<ModelOption> = Vec::new();
    for model in parsed.models {
        if model.visibility.as_deref() != Some("list")
            || !is_valid_model_name(&model.slug)
            || list.iter().any(|known| known.id == model.slug)
        {
            continue;
        }
        let mut model_efforts: Vec<String> = Vec::new();
        for level in model.supported_reasoning_levels {
            if is_valid_effort(&level.effort) && !model_efforts.contains(&level.effort) {
                model_efforts.push(level.effort);
            }
        }
        let default_effort = model
            .default_reasoning_level
            .filter(|effort| is_valid_effort(effort));
        let label = model
            .display_name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| model.slug.clone());
        list.push(ModelOption {
            id: model.slug,
            label,
            efforts: model_efforts,
            default_effort,
            custom: false,
        });
    }
    (!list.is_empty()).then_some(list)
}

// ---------------------------------------------------------------------------------------------
// Custom models
// ---------------------------------------------------------------------------------------------

/// The custom names of one kind, from the `[models]` table. A value that is not a text, and a
/// name that is not valid, is left out: a hand-written file must not put a flag in a list.
fn read_custom(table: &toml::Table, kind: ModelKind) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(array) = table
        .get("models")
        .and_then(toml::Value::as_table)
        .and_then(|models| models.get(kind.key()))
        .and_then(toml::Value::as_array)
    else {
        return out;
    };
    for value in array {
        if let Some(name) = value.as_str()
            && is_valid_model_name(name)
            && !out.iter().any(|known| known == name)
        {
            out.push(name.to_owned());
        }
    }
    out
}

fn custom_option(kind: ModelKind, id: &str) -> ModelOption {
    let efforts = match kind {
        ModelKind::Image => Vec::new(),
        ModelKind::Claude | ModelKind::Codex => efforts(&STANDARD_EFFORTS),
    };
    ModelOption {
        id: id.to_owned(),
        label: id.to_owned(),
        efforts,
        default_effort: None,
        custom: true,
    }
}

/// The base list (built-in or from the cache) and then the custom models that are not in it.
fn with_custom(base: Vec<ModelOption>, kind: ModelKind, custom: &[String]) -> Vec<ModelOption> {
    let mut list = base;
    for name in custom {
        if !list.iter().any(|known| known.id == *name) {
            list.push(custom_option(kind, name));
        }
    }
    list
}

/// The base list of a kind, with no custom model. The codex list reads the cache file.
fn base_list(kind: ModelKind, codex_home: Option<&Path>) -> (Vec<ModelOption>, CodexSource) {
    match kind {
        ModelKind::Claude => (builtin_claude(), CodexSource::Builtin),
        ModelKind::Image => (builtin_image(), CodexSource::Builtin),
        ModelKind::Codex => match codex_home.and_then(read_codex_cache) {
            Some(list) => (list, CodexSource::Cache),
            None => (builtin_codex(), CodexSource::Builtin),
        },
    }
}

/// The lists, read again from disk. `config_file` is `programs.toml`.
pub(crate) fn catalog(config_file: &Path, codex_home: Option<&Path>) -> ModelCatalog {
    let table = settings::read_table(config_file);
    let build = |kind: ModelKind| {
        let (base, source) = base_list(kind, codex_home);
        (with_custom(base, kind, &read_custom(&table, kind)), source)
    };
    let (claude, _) = build(ModelKind::Claude);
    let (codex, codex_source) = build(ModelKind::Codex);
    let (image, _) = build(ModelKind::Image);
    ModelCatalog {
        claude,
        codex,
        image,
        codex_source,
    }
}

/// Replace the custom list of one kind. The names are checked first and nothing is written if one
/// is bad. A duplicate, and a name that is already in the other models of the list, is dropped.
pub(crate) fn set_custom(
    config_file: &Path,
    codex_home: Option<&Path>,
    kind: ModelKind,
    names: &[String],
) -> Result<(), String> {
    let (base, _) = base_list(kind, codex_home);
    let mut keep: Vec<String> = Vec::new();
    for name in names {
        let name = name.trim();
        if !is_valid_model_name(name) {
            return Err(model_name_error("model name", name));
        }
        if !base.iter().any(|known| known.id == name) && !keep.iter().any(|k| k == name) {
            keep.push(name.to_owned());
        }
    }
    settings::update(config_file, |table| {
        let section = table
            .entry("models")
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        // A `models` value that is not a table is replaced.
        if !section.is_table() {
            *section = toml::Value::Table(toml::Table::new());
        }
        let now_empty = match section.as_table_mut() {
            Some(models) => {
                if keep.is_empty() {
                    models.remove(kind.key());
                } else {
                    let array = keep.iter().cloned().map(toml::Value::String).collect();
                    models.insert(kind.key().to_owned(), toml::Value::Array(array));
                }
                models.is_empty()
            }
            None => true,
        };
        if now_empty {
            table.remove("models");
        }
    })
    .map_err(|e| format!("cannot write the app config: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names_follow_the_contract() {
        for good in [
            "haiku",
            "gpt-6-luna",
            "gpt-5.6-sol",
            "claude-opus-4-1[1m]",
            "us.anthropic/x:1_2",
            "a",
            &"a".repeat(100),
        ] {
            assert!(is_valid_model_name(good), "{good}");
        }
        for bad in [
            "",
            "-x",
            "--foo",
            "a b",
            "a\nb",
            "a=b",
            "a;b",
            "é",
            &"a".repeat(101),
        ] {
            assert!(!is_valid_model_name(bad), "{bad:?}");
        }
        assert!(is_valid_effort("xhigh"));
        assert!(is_valid_effort(&"a".repeat(20)));
        assert!(!is_valid_effort(""));
        assert!(!is_valid_effort("High"));
        assert!(!is_valid_effort("-low"));
        assert!(!is_valid_effort(&"a".repeat(21)));
    }
}
