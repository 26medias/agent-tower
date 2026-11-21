//! The programs that the app starts: `claude`, `codex`, `node` and `tower-hook`. Also the socket
//! path of the hook receiver.
//!
//! - `claude`, `codex` and `node` are resolved by `programs::Resolver` at first use. The result
//!   is cached. A failure is cached for [`FAILURE_TTL`] only, so that an operator who installs a
//!   CLI does not need to restart the app. The operator overrides are in `programs.toml` in the
//!   config folder.
//! - `tower-hook` is found next to the app binary, and then in the `target` folder of the
//!   workspace (a dev build).

use super::TowerError;
use crate::programs::{ProgramError, Resolver};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long a failed lookup stays in the cache.
pub const FAILURE_TTL: Duration = Duration::from_secs(30);

/// The names that `programs_set` accepts.
pub const PROGRAM_NAMES: [&str; 3] = ["claude", "codex", "node"];

const HOOK_NAME: &str = "tower-hook";

#[derive(Serialize, Deserialize, Default)]
struct OverridesFile {
    #[serde(default)]
    programs: BTreeMap<String, PathBuf>,
}

/// The overrides of the operator and the cache of the resolved programs.
pub(crate) struct ProgramBook {
    file: PathBuf,
    resolver: Resolver,
    overrides: BTreeMap<String, PathBuf>,
    loaded: bool,
    cache: BTreeMap<String, (Instant, Result<PathBuf, String>)>,
}

impl ProgramBook {
    pub fn new(config_dir: &Path, resolver: Resolver) -> ProgramBook {
        ProgramBook {
            file: config_dir.join("programs.toml"),
            resolver,
            overrides: BTreeMap::new(),
            loaded: false,
            cache: BTreeMap::new(),
        }
    }

    fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        // A missing or bad file means no overrides. A bad file is not an error for the app.
        if let Ok(text) = fs::read_to_string(&self.file)
            && let Ok(file) = toml::from_str::<OverridesFile>(&text)
        {
            self.overrides = file.programs;
        }
    }

    /// The resolved path of a program, or the text of the error.
    pub fn resolve(&mut self, name: &str) -> Result<PathBuf, String> {
        self.load();
        if let Some((at, result)) = self.cache.get(name)
            && (result.is_ok() || at.elapsed() < FAILURE_TTL)
        {
            return result.clone();
        }
        let result = self
            .resolver
            .resolve(name, self.overrides.get(name).map(PathBuf::as_path))
            .map_err(|error: ProgramError| error.to_string());
        self.cache
            .insert(name.to_owned(), (Instant::now(), result.clone()));
        result
    }

    /// The override of the operator for a program, if there is one.
    pub fn override_of(&mut self, name: &str) -> Option<PathBuf> {
        self.load();
        self.overrides.get(name).cloned()
    }

    /// Store an operator override. An empty path removes it. The path is checked first.
    pub fn set(&mut self, name: &str, path: &str) -> Result<(), TowerError> {
        if !PROGRAM_NAMES.contains(&name) {
            return Err(TowerError::Other(format!(
                "unknown program \"{name}\" (use claude, codex or node)"
            )));
        }
        self.load();
        let mut overrides = self.overrides.clone();
        if path.trim().is_empty() {
            overrides.remove(name);
        } else {
            let path = PathBuf::from(path.trim());
            // Check with the real rules (absolute, executable file) before anything is saved.
            self.resolver
                .resolve(name, Some(&path))
                .map_err(|error| TowerError::Other(error.to_string()))?;
            overrides.insert(name.to_owned(), path);
        }
        // Only the `[programs]` table changes. The other tables of the file (`[openai]`) stay.
        let value = toml::Value::try_from(&overrides)
            .map_err(|e| TowerError::Other(format!("cannot write programs.toml: {e}")))?;
        super::settings::update(&self.file, |table| {
            table.insert("programs".to_owned(), value);
        })
        .map_err(|e| io_error(&self.file, e))?;
        self.overrides = overrides;
        self.cache.remove(name);
        Ok(())
    }
}

fn io_error(path: &Path, error: io::Error) -> TowerError {
    TowerError::Other(format!("I/O error on {}: {error}", path.display()))
}

/// The candidates for `tower-hook`, in the order of the search.
///
/// 1. next to the app binary (an installed app). A test binary lives in `<target>/<profile>/deps`,
///    so a binary folder named `deps` also gives its parent folder;
/// 2. the build folder of Cargo: `$CARGO_TARGET_DIR` if it is set, and then `target/` of the
///    workspace (a dev build). In each, a debug build of the app looks in `debug` first; a
///    release build looks in `release` first.
pub fn hook_candidates(exe: &Path, workspace_root: Option<&Path>) -> Vec<PathBuf> {
    candidates_with(
        exe,
        workspace_root,
        std::env::var_os("CARGO_TARGET_DIR").as_deref(),
    )
}

pub(crate) fn candidates_with(
    exe: &Path,
    workspace_root: Option<&Path>,
    cargo_target_dir: Option<&OsStr>,
) -> Vec<PathBuf> {
    let mut list = Vec::new();
    if let Some(dir) = exe.parent() {
        list.push(dir.join(HOOK_NAME));
        if dir.file_name().is_some_and(|name| name == "deps")
            && let Some(profile_dir) = dir.parent()
        {
            list.push(profile_dir.join(HOOK_NAME));
        }
    }
    let order = if cfg!(debug_assertions) {
        ["debug", "release"]
    } else {
        ["release", "debug"]
    };
    let mut target_dirs: Vec<PathBuf> = Vec::new();
    if let Some(custom) = cargo_target_dir.filter(|dir| !dir.is_empty()) {
        let custom = PathBuf::from(custom);
        // A relative `CARGO_TARGET_DIR` is relative to the folder where Cargo ran.
        match (custom.is_absolute(), workspace_root) {
            (true, _) => target_dirs.push(custom),
            (false, Some(root)) => target_dirs.push(root.join(custom)),
            (false, None) => {}
        }
    }
    if let Some(root) = workspace_root {
        target_dirs.push(root.join("target"));
    }
    for dir in target_dirs {
        for profile in order {
            list.push(dir.join(profile).join(HOOK_NAME));
        }
    }
    list
}

/// Find `tower-hook`. The error text says where the search looked.
pub fn locate_hook_program(
    exe: &Path,
    workspace_root: Option<&Path>,
) -> Result<PathBuf, TowerError> {
    locate_hook_with(
        exe,
        workspace_root,
        std::env::var_os("CARGO_TARGET_DIR").as_deref(),
    )
}

fn locate_hook_with(
    exe: &Path,
    workspace_root: Option<&Path>,
    cargo_target_dir: Option<&OsStr>,
) -> Result<PathBuf, TowerError> {
    let candidates = candidates_with(exe, workspace_root, cargo_target_dir);
    for candidate in &candidates {
        if crate::programs::is_executable_file(candidate) {
            return Ok(fs::canonicalize(candidate).unwrap_or_else(|_| candidate.clone()));
        }
    }
    let looked: Vec<String> = candidates.iter().map(|p| p.display().to_string()).collect();
    Err(TowerError::Other(format!(
        "cannot find {HOOK_NAME} (looked in: {})",
        looked.join("; ")
    )))
}

/// The workspace root of this build: two folders above `crates/core`.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The socket path of the hook receiver. `default_socket_path()` first (it needs
/// `XDG_RUNTIME_DIR`). If that is not set: `<temp dir>/agent-tower-<uid>/<pid>.sock`, with the
/// folder at mode 0700. The fallback folder must be a real folder (not a link) that this user
/// owns, because the temp folder is shared.
pub fn socket_path() -> io::Result<PathBuf> {
    match crate::hookrx::default_socket_path() {
        Ok(path) => Ok(path),
        Err(_) => fallback_socket_path(&std::env::temp_dir(), std::process::id()),
    }
}

pub(crate) fn fallback_socket_path(temp_dir: &Path, pid: u32) -> io::Result<PathBuf> {
    // SAFETY: `getuid` has no arguments and cannot fail.
    let uid = unsafe { libc::getuid() };
    let dir = temp_dir.join(format!("agent-tower-{uid}"));
    match fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let meta = fs::symlink_metadata(&dir)?;
            if !meta.is_dir() || meta.uid() != uid {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "{} exists and is not a folder that this user owns",
                        dir.display()
                    ),
                ));
            }
        }
        Err(error) => return Err(error),
    }
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    Ok(dir.join(format!("{pid}.sock")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_is_found_next_to_the_exe_first() {
        let dir = tempfile::tempdir().unwrap();
        let hook = dir.path().join(HOOK_NAME);
        fs::write(&hook, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        let found = locate_hook_program(&dir.path().join("app"), None).unwrap();
        assert_eq!(found, fs::canonicalize(&hook).unwrap());
    }

    #[test]
    fn missing_hook_error_lists_the_places() {
        let dir = tempfile::tempdir().unwrap();
        // No `CARGO_TARGET_DIR`: the test must not depend on the environment of the build.
        let error = locate_hook_with(&dir.path().join("app"), Some(dir.path()), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains(&format!("{}/tower-hook", dir.path().display())));
        assert!(error.contains("target/debug/tower-hook"));
        assert!(error.contains("target/release/tower-hook"));
    }

    #[test]
    fn candidates_have_the_exe_folder_first_then_the_cargo_target_dir() {
        let exe = Path::new("/t/debug/deps/app_floor-abc");
        let root = Path::new("/w");
        let list = candidates_with(exe, Some(root), Some(OsStr::new("/custom")));
        assert_eq!(list[0], Path::new("/t/debug/deps/tower-hook"));
        assert_eq!(list[1], Path::new("/t/debug/tower-hook"));
        let custom: Vec<_> = list.iter().filter(|p| p.starts_with("/custom")).collect();
        assert_eq!(custom.len(), 2);
        // The workspace `target/` stays as the last place.
        assert!(list[list.len() - 1].starts_with("/w/target"));
        // A relative target dir is relative to the workspace root. An unset one adds nothing.
        let list = candidates_with(Path::new("/x/app"), Some(root), Some(OsStr::new("out")));
        assert!(
            list.iter()
                .any(|p| p == Path::new("/w/out/debug/tower-hook"))
        );
        let list = candidates_with(Path::new("/x/app"), Some(root), None);
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn fallback_socket_folder_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = fallback_socket_path(dir.path(), 4242).unwrap();
        let parent = path.parent().unwrap();
        let mode = fs::metadata(parent).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        assert!(path.ends_with("4242.sock"));
    }
}
