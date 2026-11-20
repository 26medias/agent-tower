//! Terminal mode of `fake-cli`. A real TUI puts its terminal in raw mode with no echo. In
//! cooked mode the line discipline holds the input until Enter and changes bytes (`\r` becomes
//! `\n`), so a test would not see what a real CLI sees.

use std::io::{self, Write};
use std::mem::MaybeUninit;

const STDIN: libc::c_int = 0;

/// Raw mode while this value lives. `restore` puts the old mode back.
pub struct RawMode {
    saved: Option<libc::termios>,
}

impl RawMode {
    /// Put stdin in raw mode. If stdin is not a terminal, do nothing (for runs in a pipe).
    pub fn enter() -> io::Result<RawMode> {
        // SAFETY: `isatty`, `tcgetattr`, `cfmakeraw` and `tcsetattr` take a valid fd and
        // pointers to a `termios` that we own. `tcgetattr` fills it before we read it.
        unsafe {
            if libc::isatty(STDIN) != 1 {
                return Ok(RawMode { saved: None });
            }
            let mut term = MaybeUninit::<libc::termios>::zeroed();
            if libc::tcgetattr(STDIN, term.as_mut_ptr()) != 0 {
                return Err(io::Error::last_os_error());
            }
            let saved = term.assume_init();
            let mut raw = saved;
            libc::cfmakeraw(&mut raw);
            // TCSANOW: do not drop the input that is already queued.
            if libc::tcsetattr(STDIN, libc::TCSANOW, &raw) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(RawMode { saved: Some(saved) })
        }
    }

    pub fn restore(&mut self) {
        if let Some(saved) = self.saved.take() {
            // SAFETY: `saved` came from `tcgetattr` on the same fd.
            unsafe {
                libc::tcsetattr(STDIN, libc::TCSANOW, &saved);
            }
        }
    }
}

/// Write bytes to the terminal and flush. Raw mode has no output processing, so use `\r\n`.
pub fn out(bytes: &[u8]) {
    let mut stdout = io::stdout().lock();
    let _ = stdout.write_all(bytes);
    let _ = stdout.flush();
}
