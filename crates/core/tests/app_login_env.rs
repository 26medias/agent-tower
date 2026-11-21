//! The parent environment of a session: the login shell environment laid over the environment of
//! the app (`login_env`). No test here starts the real login shell. The processes are the real
//! `fake-cli`.

mod app_support;

use app_support::Rig;
use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};
use tower_core::env::EnvMap;
use tower_core::login_env::LoginEnvSource;

fn map(pairs: &[(&str, &str)]) -> EnvMap {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

fn get<'a>(env: &'a EnvMap, name: &str) -> Option<&'a str> {
    env.get(OsStr::new(name)).and_then(|value| value.to_str())
}

fn write_exe(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_session_gets_the_login_shell_map_over_the_app_environment() {
    let rig = Rig::with(|config| {
        config.parent_env = map(&[
            ("HOME", "/tmp"),
            ("SHARED", "from the app"),
            ("APP_ONLY", "1"),
            ("PATH", "/app/bin"),
        ]);
        config.login_env = LoginEnvSource::Fixed(Some(map(&[
            ("SHARED", "from the login shell"),
            ("CARGO_HOME", "/home/a/.cargo"),
            ("PATH", "/home/a/.cargo/bin:/usr/bin"),
        ])));
    });
    rig.create_floor();
    rig.start("ceo");
    let call = rig.calls().remove(0);
    // The parent that the adapter got: the login shell wins, the app-only name stays.
    assert_eq!(
        get(&call.parent_env, "SHARED"),
        Some("from the login shell")
    );
    assert_eq!(get(&call.parent_env, "APP_ONLY"), Some("1"));
    assert_eq!(get(&call.parent_env, "CARGO_HOME"), Some("/home/a/.cargo"));
    // The process environment: copied by `build_env`. The PATH is the login shell PATH.
    assert_eq!(get(&call.env, "SHARED"), Some("from the login shell"));
    assert_eq!(get(&call.env, "CARGO_HOME"), Some("/home/a/.cargo"));
    assert_eq!(
        get(&call.env, "PATH"),
        Some("/home/a/.cargo/bin:/usr/bin:/bin")
    );
}

#[test]
fn with_no_login_shell_map_a_session_gets_the_app_environment() {
    let rig = Rig::with(|config| {
        config.parent_env = map(&[("HOME", "/tmp"), ("APP_ONLY", "1"), ("PATH", "/app/bin")]);
        config.login_env = LoginEnvSource::Fixed(None);
    });
    rig.create_floor();
    rig.start("ceo");
    let call = rig.calls().remove(0);
    assert_eq!(
        call.parent_env,
        map(&[("HOME", "/tmp"), ("APP_ONLY", "1"), ("PATH", "/app/bin")])
    );
    assert_eq!(get(&call.env, "APP_ONLY"), Some("1"));
    assert_eq!(get(&call.env, "PATH"), Some("/app/bin:/usr/bin:/bin"));
}

#[test]
fn a_session_uses_the_map_of_a_fake_login_shell() {
    // The same path as production: a shell script that prints the marker and the environment.
    // It is slow on purpose: the session start must wait for it.
    let dir = tempfile::tempdir().unwrap();
    let shell = dir.path().join("fake-shell");
    write_exe(
        &shell,
        "#!/bin/sh\nsleep 0.5\nprintf 'banner\\n__TOWER_LOGIN_ENV_BEGIN__\\n'\nprintf 'NVM_DIR=/home/a/.nvm\\0HOME=/tmp\\0'\n",
    );
    let rig = Rig::with(|config| {
        config.parent_env = map(&[("HOME", "/tmp"), ("APP_ONLY", "1")]);
        config.login_env = LoginEnvSource::Shell {
            shell: shell.into_os_string(),
            timeout: Duration::from_secs(10),
        };
    });
    rig.create_floor();
    rig.start("ceo");
    let call = rig.calls().remove(0);
    assert_eq!(get(&call.env, "NVM_DIR"), Some("/home/a/.nvm"));
    assert_eq!(get(&call.env, "APP_ONLY"), Some("1"));
}

#[test]
fn a_hung_login_shell_does_not_block_the_session_start_longer_than_its_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let shell = dir.path().join("hung-shell");
    write_exe(&shell, "#!/bin/sh\nsleep 60\n");
    let timeout = Duration::from_secs(1);
    let rig = Rig::with(|config| {
        config.parent_env = map(&[("HOME", "/tmp"), ("APP_ONLY", "1")]);
        config.login_env = LoginEnvSource::Shell {
            shell: shell.into_os_string(),
            timeout,
        };
    });
    rig.create_floor();
    let start = Instant::now();
    rig.start("ceo");
    let waited = start.elapsed();
    assert!(
        waited < timeout + Duration::from_secs(2),
        "the start waited {waited:?}"
    );
    // The start used the app environment.
    let call = rig.calls().remove(0);
    assert_eq!(get(&call.env, "APP_ONLY"), Some("1"));
    assert_eq!(get(&call.env, "NVM_DIR"), None);
    // A second start does not wait again.
    let again = Instant::now();
    rig.start("pm");
    assert!(
        again.elapsed() < Duration::from_secs(1),
        "{:?}",
        again.elapsed()
    );
}
