//! Helpers for the integration tests that start real children.
#![allow(dead_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use std::time::{Duration, Instant};
use tower_core::env::{EnvMap, build_env};

/// Wait limit for anything that must happen. A test that passes does not wait this long.
pub const LIMIT: Duration = Duration::from_secs(15);

/// Path of a program of this workspace. The first call builds `fake-cli` and `tower-hook`, so
/// `cargo test -p tower-core` is correct also when they were not built or are old.
pub fn bin(name: &str) -> PathBuf {
    static BUILD: Once = Once::new();
    BUILD.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let status = Command::new(cargo)
            .args(["build", "--quiet", "-p", "fakecli", "-p", "tower-hook"])
            .current_dir(root)
            .status()
            .expect("run cargo build");
        assert!(
            status.success(),
            "cargo build of fake-cli and tower-hook failed"
        );
    });
    let exe = std::env::current_exe().expect("test executable path");
    let path = exe
        .parent()
        .and_then(Path::parent)
        .expect("target folder")
        .join(name);
    assert!(path.is_file(), "{} is missing", path.display());
    path
}

pub fn os<S: AsRef<std::ffi::OsStr>>(text: S) -> OsString {
    text.as_ref().to_owned()
}

/// An environment with PATH `/usr/bin:/bin`, built by `build_env` from a fixed parent map.
pub fn env_with(extra: &[(&str, &str)]) -> EnvMap {
    let parent: EnvMap = [("HOME", "/tmp"), ("LANG", "C.UTF-8")]
        .iter()
        .map(|(k, v)| (os(k), os(v)))
        .collect();
    let extra: Vec<(OsString, OsString)> = extra.iter().map(|(k, v)| (os(k), os(v))).collect();
    build_env(&parent, &extra, &[]).expect("build_env")
}

pub fn wait_for(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + LIMIT;
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timeout while waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
