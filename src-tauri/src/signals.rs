//! Clean exit on SIGTERM, SIGINT and SIGHUP.
//!
//! A signal handler may only call functions that are safe in a signal handler. So the handler
//! writes one byte to a socket (`write(2)` is safe), and a normal thread reads the socket and
//! asks the app to exit. The exit runs `RunEvent::Exit`, which shuts the `Tower` down: the
//! sessions stop and the hook socket is removed.

use std::io::{self, Read};
use std::os::fd::IntoRawFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicI32, Ordering};
use tauri::AppHandle;

/// The write end of the socket pair, for the handler.
static WAKE_FD: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_signal(_signal: libc::c_int) {
    let fd = WAKE_FD.load(Ordering::Relaxed);
    if fd >= 0 {
        let byte = 1u8;
        // SAFETY: `write(2)` is async-signal-safe. `fd` is a socket that lives until exit, and
        // the buffer is one byte on our stack. A full socket or an error is ignored on purpose.
        unsafe {
            libc::write(fd, (&raw const byte).cast::<libc::c_void>(), 1);
        }
    }
}

/// Install the handlers. The thread asks the app to exit when a signal comes.
pub fn install(handle: AppHandle) -> io::Result<()> {
    let (mut reader, writer) = UnixStream::pair()?;
    writer.set_nonblocking(true)?;
    WAKE_FD.store(writer.into_raw_fd(), Ordering::SeqCst);
    for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        // SAFETY: `sigaction` with a zeroed struct, an empty mask and a handler that only calls
        // `write(2)`. The struct is fully set before the call.
        let result = unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as usize;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(signal, &action, std::ptr::null_mut())
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    std::thread::Builder::new()
        .name("signal-wait".into())
        .spawn(move || {
            let mut byte = [0u8; 1];
            if reader.read_exact(&mut byte).is_ok() {
                handle.exit(0);
            }
        })?;
    Ok(())
}
