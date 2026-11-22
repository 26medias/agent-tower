//! The tools of the MCP server for the look of a floor: `floor_look`, `floor_logo_set` and
//! `floor_look_set`. An employee can show the logo that the project has already, with no call to
//! an image API.
//!
//! The picture file must be inside the project folder. The app reads it, makes a PNG that fits
//! 512 x 512 and writes `.swarm/assets/logo.png`. The employee does not write under `.swarm/`.

use super::bus::Identity;
use super::live::locked;
use super::{TowerCore, logo_run, reload};
use crate::logo;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

/// The most characters of a floor name.
const NAME_MAX_CHARS: usize = 60;
/// The most characters of a floor description.
const DESCRIPTION_MAX_CHARS: usize = 300;

fn optional_text<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(format!("The argument \"{key}\" must be a text.")),
    }
}

/// One line of text with no control character, trimmed, with a length limit.
fn one_line(what: &str, text: &str, max: usize) -> Result<String, String> {
    let text = text.trim();
    if text.chars().any(char::is_control) {
        return Err(format!(
            "The {what} must be one line with no control character."
        ));
    }
    let count = text.chars().count();
    if count > max {
        return Err(format!(
            "The {what} is too long: {count} characters. The limit is {max}."
        ));
    }
    Ok(text.to_owned())
}

/// The real path of the picture file. It must be a file inside the project folder `floor`.
fn picture_file(floor: &Path, file: &str) -> Result<PathBuf, String> {
    let file = file.trim();
    if file.is_empty() {
        return Err("The argument \"file\" is empty.".to_owned());
    }
    let asked = Path::new(file);
    let joined = if asked.is_absolute() {
        asked.to_path_buf()
    } else {
        floor.join(asked)
    };
    let real = fs::canonicalize(&joined)
        .map_err(|error| format!("Cannot find the file {}: {error}", joined.display()))?;
    let root = fs::canonicalize(floor).unwrap_or_else(|_| floor.to_path_buf());
    if !real.starts_with(&root) {
        return Err(format!(
            "The file must be inside the project folder {}. Copy the picture into the project first.",
            root.display()
        ));
    }
    let meta = fs::metadata(&real)
        .map_err(|error| format!("Cannot read the file {}: {error}", real.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file.", real.display()));
    }
    if meta.len() > logo::MAX_IMPORT_BYTES {
        return Err(format!(
            "The file has {} bytes. The limit is {} bytes.",
            meta.len(),
            logo::MAX_IMPORT_BYTES
        ));
    }
    Ok(real)
}

impl TowerCore {
    /// `floor_look`: the name, the description and the logo of the floor of the caller.
    pub(super) fn tool_floor_look(&self, who: &Identity) -> Result<String, String> {
        let (floor, _team, _actor) = self.task_actor(who)?;
        reload(&floor).map_err(|e| e.to_string())?;
        let (name, brand) = {
            let store = locked(&floor.store);
            (store.floor().name.clone(), store.floor().brand.clone())
        };
        let (busy, error) = {
            let state = locked(&floor.logo);
            (state.busy, state.error.clone())
        };
        let logo_file = logo_run::logo_path(&floor.path);
        serde_json::to_string_pretty(&json!({
            "name": name,
            "description": brand.description,
            "project_folder": floor.path,
            "has_logo": logo_file.is_file(),
            "logo_file": logo_file,
            "logo_subject": brand.logo_prompt,
            "logo_generation_runs": busy,
            "logo_generation_error": error,
        }))
        .map_err(|e| e.to_string())
    }

    /// `floor_logo_set`: make the logo of the floor from a picture file of the project.
    pub(super) fn tool_floor_logo_set(
        &self,
        who: &Identity,
        args: &Value,
    ) -> Result<String, String> {
        let (floor, _team, _actor) = self.task_actor(who)?;
        let file = args
            .get("file")
            .and_then(Value::as_str)
            .ok_or("The argument \"file\" is missing. It must be a text.")?;
        let real = picture_file(&floor.path, file)?;
        let bytes = fs::read(&real)
            .map_err(|error| format!("Cannot read the file {}: {error}", real.display()))?;
        let png = logo::import(&bytes).map_err(|error| format!("{}: {error}", real.display()))?;
        {
            // A generation that runs now writes the same file when it ends. Do not race it.
            let mut state = locked(&floor.logo);
            if state.busy {
                return Err(
                    "A logo generation runs for this floor now. Wait for it to end, then call this tool again."
                        .to_owned(),
                );
            }
            logo_run::write_logo(&floor.path, &png)?;
            state.error = None;
        }
        self.notifier.floor_changed(&floor.path);
        Ok(format!(
            "The logo of the floor is now the picture {}. The tower and the office show it. The app keeps its own copy: a later change of that file does not change the logo.",
            real.display()
        ))
    }

    /// `floor_look_set`: change the name and/or the description of the floor.
    pub(super) fn tool_floor_look_set(
        &self,
        who: &Identity,
        args: &Value,
    ) -> Result<String, String> {
        let (floor, _team, _actor) = self.task_actor(who)?;
        let name = optional_text(args, "name")?
            .map(|text| one_line("name", text, NAME_MAX_CHARS))
            .transpose()?;
        let description = optional_text(args, "description")?
            .map(|text| one_line("description", text, DESCRIPTION_MAX_CHARS))
            .transpose()?;
        if name.is_none() && description.is_none() {
            return Err(
                "Give the argument \"name\", the argument \"description\", or the two.".to_owned(),
            );
        }
        if name.as_deref() == Some("") {
            return Err("The name is empty.".to_owned());
        }
        // The file on disk can have changed (the setup agent writes it). Read it again first.
        reload(&floor).map_err(|e| e.to_string())?;
        let mut done = Vec::new();
        {
            let mut store = locked(&floor.store);
            if let Some(name) = &name {
                store.set_name(name).map_err(|e| e.to_string())?;
                done.push(format!("The name of the floor is now \"{name}\"."));
            }
            if let Some(description) = description {
                let mut brand = store.floor().brand.clone();
                brand.description = (!description.is_empty()).then_some(description);
                done.push(match &brand.description {
                    Some(text) => format!("The description of the floor is now \"{text}\"."),
                    None => "The floor has no description now.".to_owned(),
                });
                store.set_brand(brand).map_err(|e| e.to_string())?;
            }
        }
        self.notifier.floor_changed(&floor.path);
        Ok(done.join(" "))
    }
}
