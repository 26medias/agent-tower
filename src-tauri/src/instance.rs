//! One copy of the app for each config folder.
//!
//! Two copies that use the same config folder open the same floors. The second copy would treat
//! the live sessions of the first one as sessions of a crashed run and mark them as ended. So a
//! second copy takes no floor: it tells the operator and ends.
//!
//! The lock is an advisory `flock` on the file `app.lock` in the config folder. The kernel
//! releases it when the process ends, also after a crash, so there is no stale lock.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;

/// The lock of the running copy. Keep it for the life of the process.
pub struct InstanceLock(#[allow(dead_code)] File);

/// Take the lock. `Ok(None)`: another copy of the app holds it.
pub fn take(config_dir: &Path) -> io::Result<Option<InstanceLock>> {
    fs::create_dir_all(config_dir)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(config_dir.join("app.lock"))?;
    // SAFETY: `flock` takes a valid open file descriptor and two flags. It keeps no pointer.
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        return Ok(Some(InstanceLock(file)));
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::WouldBlock {
        Ok(None)
    } else {
        Err(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_take_is_refused_until_the_first_lock_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let first = take(dir.path()).unwrap();
        assert!(first.is_some());
        assert!(take(dir.path()).unwrap().is_none());
        drop(first);
        assert!(take(dir.path()).unwrap().is_some());
        // Another config folder has its own lock.
        let other = tempfile::tempdir().unwrap();
        let held = take(dir.path()).unwrap();
        assert!(take(other.path()).unwrap().is_some());
        drop(held);
    }
}
