//! The check of a folder path that the operator types (`floor_open` and `floor_create`).
//!
//! The check gives a plain error with a stable prefix, so that the operator sees the real cause
//! at once, and not a raw I/O error after "create". See `docs/ipc-final.md`.

use super::TowerError;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// `~` and `~/x` mean the home folder. Any other path stays as it is.
pub(super) fn expand_home(path: &Path, home: Option<&Path>) -> PathBuf {
    let Some(home) = home else {
        return path.to_path_buf();
    };
    if path == Path::new("~") {
        return home.to_path_buf();
    }
    match path.strip_prefix("~") {
        Ok(rest) => home.join(rest),
        Err(_) => path.to_path_buf(),
    }
}

/// Check that the path is an absolute path (after the expansion of `~`). It does not touch the disk.
pub(super) fn check_absolute(path: &Path) -> Result<(), TowerError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(TowerError::NotAbsolute)
    }
}

/// Check that the path is a folder that the app can write in. The order of the checks is the
/// order of the errors: `NOT_FOUND`, `NOT_A_FOLDER`, `NO_WRITE`.
///
/// The write check is a real one: the app makes a temporary file and removes it. The mode bits
/// are not trusted (a read-only mount, an ACL, or the root user change the result). The app
/// writes in `.swarm/` when the folder has one, and in the folder itself when it has not (a new
/// floor makes `.swarm/` there).
pub(super) fn check_folder(path: &Path) -> Result<(), TowerError> {
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            return Err(TowerError::NoWrite(path.to_path_buf()));
        }
        Err(_) => return Err(TowerError::NotFound(path.to_path_buf())),
    };
    if !meta.is_dir() {
        return Err(TowerError::NotAFolder(path.to_path_buf()));
    }
    let swarm = path.join(".swarm");
    let target = if swarm.is_dir() { &swarm } else { path };
    if !can_write(target) {
        return Err(TowerError::NoWrite(path.to_path_buf()));
    }
    Ok(())
}

/// Make a temporary file in the folder, and remove it.
fn can_write(folder: &Path) -> bool {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let probe = folder.join(format!(
        ".agent-tower-write-check-{}-{nanos}",
        std::process::id()
    ));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(file) => {
            drop(file);
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_is_expanded_for_a_tilde_path_only() {
        let home = Path::new("/home/test");
        assert_eq!(expand_home(Path::new("~"), Some(home)), home);
        assert_eq!(
            expand_home(Path::new("~/work/x"), Some(home)),
            Path::new("/home/test/work/x")
        );
        // `~user` and a tilde inside the path are not the home folder.
        assert_eq!(
            expand_home(Path::new("~user/x"), Some(home)),
            Path::new("~user/x")
        );
        assert_eq!(
            expand_home(Path::new("/a/~/x"), Some(home)),
            Path::new("/a/~/x")
        );
        // No home: the path stays, and `check_absolute` refuses it.
        assert_eq!(expand_home(Path::new("~/x"), None), Path::new("~/x"));
        assert!(check_absolute(Path::new("~/x")).is_err());
    }
}
