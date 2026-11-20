//! PTY layer: start a CLI process in a pseudo terminal, read its output, write to it, resize it,
//! kill it, and tell the caller when it exits.
//!
//! Threads: std threads, not tokio. For each process there are two:
//! - the reader thread reads the PTY, appends to the log file (with rotation) and sends
//!   `PtyEvent::Output` on the channel;
//! - the waiter thread waits for the process and then sends `PtyEvent::Exit`.
//!
//! The waiter sends `Exit` after the reader has ended, so all output comes before `Exit`.
//! If a grandchild keeps the PTY open after the process ended, the waiter waits only
//! `EXIT_GRACE` for the reader, then sends `Exit` anyway.
//!
//! What the PTY layer adds to the environment of the child: `SHELL` when the given map has none
//! (portable-pty 0.9 always sets it, from the password database). Nothing else was found
//! (see the test `child_environment_is_exactly_the_given_map`).

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// Size of one read from the PTY.
const READ_CHUNK: usize = 16 * 1024;

/// How long the waiter waits for the reader after the process ended.
const EXIT_GRACE: Duration = Duration::from_millis(500);

/// How long `Drop` waits for the process to end after the first signal, before `SIGKILL`.
const KILL_GRACE: Duration = Duration::from_secs(1);

/// Write to a PTY. A trait, so that tests (and `Session` tests) can record the writes.
pub trait PtyWriter {
    /// Write all bytes. One call is one `write_all`: a paste must be one call.
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()>;
}

/// The real writer, over the master side of the PTY.
///
/// When it is dropped, portable-pty writes `\n` and the EOF character of the terminal to the
/// PTY. For a live child in raw mode this is two stray bytes. Drop it after the child ended.
pub struct PtyMasterWriter {
    inner: Box<dyn Write + Send>,
}

impl PtyWriter for PtyMasterWriter {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.inner.write_all(bytes)?;
        self.inner.flush()
    }
}

/// A writer that records each call. Clones share the record, so a test can give one clone to a
/// `Session` and keep the other to read the writes.
#[derive(Debug, Clone, Default)]
pub struct RecordingWriter {
    writes: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl RecordingWriter {
    pub fn new() -> RecordingWriter {
        RecordingWriter::default()
    }

    /// Each `write_all` call, in order.
    pub fn writes(&self) -> Vec<Vec<u8>> {
        self.writes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The number of `write_all` calls.
    pub fn write_count(&self) -> usize {
        self.writes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// All bytes of all writes, joined.
    pub fn all_bytes(&self) -> Vec<u8> {
        self.writes().concat()
    }
}

impl PtyWriter for RecordingWriter {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(bytes.to_vec());
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    #[error("the working folder does not exist or is not a folder: {}", .0.display())]
    CwdMissing(PathBuf),
    #[error("the program must be an absolute path: {}", .0.display())]
    ProgramNotAbsolute(PathBuf),
    #[error("cannot open the log file {}: {source}", path.display())]
    Log { path: PathBuf, source: io::Error },
    #[error("cannot open a PTY: {0}")]
    Open(String),
    #[error("cannot start {}: {message}", program.display())]
    Spawn { program: PathBuf, message: String },
    #[error("PTY I/O error: {0}")]
    Io(#[from] io::Error),
}

/// A log file with a size limit. When the next write would pass the limit, the file becomes
/// `<path>.1` (the old `<path>.1` is deleted) and a new file starts. So the log keeps between
/// `max_bytes` and `2 * max_bytes` of the latest output.
#[derive(Debug, Clone)]
pub struct LogConfig {
    pub path: PathBuf,
    pub max_bytes: u64,
}

/// What to start.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    /// Absolute path of the program.
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// The environment of the process. It gets EXACTLY this (see the module text for the one
    /// value that the PTY layer adds).
    pub env: BTreeMap<OsString, OsString>,
    /// The working folder. Required: without it portable-pty starts the child in `$HOME`.
    /// A missing folder is an error (portable-pty would also fall back to `$HOME`).
    pub cwd: PathBuf,
    pub rows: u16,
    pub cols: u16,
    pub log: Option<LogConfig>,
}

/// How a process ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyExit {
    /// The exit code. For a process that a signal ended, portable-pty gives 1.
    pub code: u32,
    /// The signal that ended the process, as text (for example "Hangup").
    pub signal: Option<String>,
}

impl PtyExit {
    pub fn success(&self) -> bool {
        self.signal.is_none() && self.code == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtyEvent {
    /// A chunk of output. `at_ms` is the time since the process started (monotonic clock).
    Output { bytes: Vec<u8>, at_ms: u64 },
    /// The process ended. This is the last event.
    Exit(PtyExit),
}

/// A set-once flag that threads can wait for.
#[derive(Default)]
struct Flag {
    set: Mutex<bool>,
    changed: Condvar,
}

impl Flag {
    fn set(&self) {
        *self.set.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.changed.notify_all();
    }

    fn is_set(&self) -> bool {
        *self.set.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Wait until the flag is set or the time ends. Returns the flag.
    fn wait(&self, timeout: Duration) -> bool {
        let guard = self.set.lock().unwrap_or_else(PoisonError::into_inner);
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, timeout, |set| !*set)
            .unwrap_or_else(PoisonError::into_inner);
        *guard
    }
}

/// A process in a PTY. Dropping it kills the process.
pub struct PtyProcess {
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    pid: Option<u32>,
    writer: Option<Box<dyn PtyWriter + Send>>,
    events: Option<Receiver<PtyEvent>>,
    exited: Arc<Flag>,
    /// Set by `Drop` so that the reader stops sending.
    closing: Arc<AtomicBool>,
}

/// Start a process in a new PTY.
pub fn spawn(spec: SpawnSpec) -> Result<PtyProcess, PtyError> {
    if !spec.program.is_absolute() {
        return Err(PtyError::ProgramNotAbsolute(spec.program));
    }
    if !spec.cwd.is_dir() {
        return Err(PtyError::CwdMissing(spec.cwd));
    }
    let log = match &spec.log {
        Some(config) => Some(LogFile::open(config)?),
        None => None,
    };

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: spec.rows,
            cols: spec.cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| PtyError::Open(format!("{error:#}")))?;

    let mut command = CommandBuilder::new(&spec.program);
    command.args(&spec.args);
    command.env_clear();
    for (name, value) in &spec.env {
        command.env(name, value);
    }
    command.cwd(&spec.cwd);

    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| PtyError::Spawn {
            program: spec.program.clone(),
            message: format!("{error:#}"),
        })?;
    // The master reads EOF only when no slave handle is open. Close ours.
    drop(pair.slave);

    let master = pair.master;
    let reader = master
        .try_clone_reader()
        .map_err(|error| PtyError::Open(format!("{error:#}")))?;
    let writer = master
        .take_writer()
        .map_err(|error| PtyError::Open(format!("{error:#}")))?;
    let killer = child.clone_killer();
    let pid = child.process_id();

    let (sender, receiver) = mpsc::channel();
    let started = Instant::now();
    let exited = Arc::new(Flag::default());
    let closing = Arc::new(AtomicBool::new(false));
    let reader_done = Arc::new(Flag::default());

    {
        let sender = sender.clone();
        let reader_done = Arc::clone(&reader_done);
        let closing = Arc::clone(&closing);
        thread::Builder::new()
            .name("pty-reader".into())
            .spawn(move || {
                read_loop(reader, log, &sender, started, &closing);
                reader_done.set();
            })?;
    }
    {
        let exited = Arc::clone(&exited);
        thread::Builder::new()
            .name("pty-waiter".into())
            .spawn(move || {
                let status = match child.wait() {
                    Ok(status) => PtyExit {
                        code: status.exit_code(),
                        signal: status.signal().map(str::to_owned),
                    },
                    Err(error) => PtyExit {
                        code: 1,
                        signal: Some(format!("wait failed: {error}")),
                    },
                };
                exited.set();
                reader_done.wait(EXIT_GRACE);
                let _ = sender.send(PtyEvent::Exit(status));
            })?;
    }

    Ok(PtyProcess {
        master,
        killer,
        pid,
        writer: Some(Box::new(PtyMasterWriter { inner: writer })),
        events: Some(receiver),
        exited,
        closing,
    })
}

impl PtyProcess {
    /// The process id, if the system gives one.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// True until the process has ended.
    pub fn is_running(&self) -> bool {
        !self.exited.is_set()
    }

    /// The channel of events. It can be taken one time. `Exit` is the last event.
    pub fn take_events(&mut self) -> Option<Receiver<PtyEvent>> {
        self.events.take()
    }

    /// The writer. It can be taken one time.
    pub fn take_writer(&mut self) -> Option<Box<dyn PtyWriter + Send>> {
        self.writer.take()
    }

    /// Change the size of the terminal. The child gets `SIGWINCH`.
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), PtyError> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| PtyError::Open(format!("resize failed: {error:#}")))
    }

    /// Ask the process to end: `SIGHUP` (portable-pty). A process that ignores it stays.
    /// `Exit` arrives on the channel when it ends. Does nothing if it has ended.
    pub fn kill(&mut self) -> io::Result<()> {
        if self.exited.is_set() {
            return Ok(());
        }
        self.killer.kill()
    }
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::SeqCst);
        if !self.exited.is_set() {
            let _ = self.killer.kill();
            if !self.exited.wait(KILL_GRACE) {
                if let Some(pid) = self.pid {
                    // SAFETY: plain `kill(2)`. The waiter has not reaped the process yet
                    // (`exited` is not set), so the id still belongs to our child.
                    unsafe {
                        libc::kill(pid as i32, libc::SIGKILL);
                    }
                }
                self.exited.wait(KILL_GRACE);
            }
        }
        // The threads end alone: the waiter has the process status, and the reader reads EOF
        // when the last slave handle closes. We do not join them, so a grandchild that holds
        // the PTY cannot make `drop` hang.
    }
}

fn read_loop(
    mut reader: Box<dyn Read + Send>,
    mut log: Option<LogFile>,
    sender: &Sender<PtyEvent>,
    started: Instant,
    closing: &AtomicBool,
) {
    let mut buffer = vec![0u8; READ_CHUNK];
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let chunk = &buffer[..count];
        if let Some(file) = log.as_mut() {
            // The log is a help for the operator. A write error turns it off. It does not
            // stop the session.
            if file.append(chunk).is_err() {
                log = None;
            }
        }
        if !closing.load(Ordering::SeqCst) {
            // If the receiver is gone, go on reading: the child must not block on a full PTY.
            let _ = sender.send(PtyEvent::Output {
                bytes: chunk.to_vec(),
                at_ms: started.elapsed().as_millis() as u64,
            });
        }
    }
}

/// The log file, with its size and the rotation.
struct LogFile {
    path: PathBuf,
    max_bytes: u64,
    file: File,
    size: u64,
}

impl LogFile {
    fn open(config: &LogConfig) -> Result<LogFile, PtyError> {
        let error = |source| PtyError::Log {
            path: config.path.clone(),
            source,
        };
        if config.max_bytes == 0 {
            return Err(error(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the size limit is 0",
            )));
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&config.path)
            .map_err(error)?;
        let size = file.metadata().map_err(error)?.len();
        Ok(LogFile {
            path: config.path.clone(),
            max_bytes: config.max_bytes,
            file,
            size,
        })
    }

    /// The path of the old file.
    fn old_path(path: &Path) -> PathBuf {
        let mut name = path.as_os_str().to_owned();
        name.push(".1");
        PathBuf::from(name)
    }

    /// Append, and rotate when the file is full. A file never passes the limit.
    fn append(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            if self.size >= self.max_bytes {
                self.rotate()?;
            }
            let room = (self.max_bytes - self.size) as usize;
            let take = room.min(bytes.len());
            self.file.write_all(&bytes[..take])?;
            self.size += take as u64;
            bytes = &bytes[take..];
        }
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        fs::rename(&self.path, Self::old_path(&self.path))?;
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.size = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_writer_keeps_each_call() {
        let recorder = RecordingWriter::new();
        let mut writer = recorder.clone();
        writer.write_all(b"ab").unwrap();
        writer.write_all(b"").unwrap();
        writer.write_all(b"c").unwrap();
        assert_eq!(recorder.write_count(), 3);
        assert_eq!(
            recorder.writes(),
            vec![b"ab".to_vec(), vec![], b"c".to_vec()]
        );
        assert_eq!(recorder.all_bytes(), b"abc".to_vec());
    }

    #[test]
    fn log_file_rotates_and_never_passes_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.log");
        let mut log = LogFile::open(&LogConfig {
            path: path.clone(),
            max_bytes: 10,
        })
        .unwrap();
        log.append(b"0123456").unwrap();
        log.append(b"789ABCDEFGHIJKLMNOP").unwrap();
        // 26 bytes in total: "0123456789" | "ABCDEFGHIJ" | "KLMNOP"
        let old = fs::read(LogFile::old_path(&path)).unwrap();
        let new = fs::read(&path).unwrap();
        assert_eq!(old, b"ABCDEFGHIJ");
        assert_eq!(new, b"KLMNOP");
    }

    #[test]
    fn log_file_continues_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.log");
        fs::write(&path, b"12345678").unwrap();
        let mut log = LogFile::open(&LogConfig {
            path: path.clone(),
            max_bytes: 10,
        })
        .unwrap();
        log.append(b"ABCD").unwrap();
        assert_eq!(fs::read(LogFile::old_path(&path)).unwrap(), b"12345678AB");
        assert_eq!(fs::read(&path).unwrap(), b"CD");
    }

    #[test]
    fn spawn_refuses_a_relative_program_and_a_zero_limit() {
        let dir = tempfile::tempdir().unwrap();
        let spec = |program: &str, log: Option<LogConfig>| SpawnSpec {
            program: program.into(),
            args: vec![],
            env: BTreeMap::new(),
            cwd: dir.path().to_path_buf(),
            rows: 24,
            cols: 80,
            log,
        };
        assert!(matches!(
            spawn(spec("pwd", None)),
            Err(PtyError::ProgramNotAbsolute(_))
        ));
        assert!(matches!(
            spawn(spec(
                "/bin/pwd",
                Some(LogConfig {
                    path: dir.path().join("x.log"),
                    max_bytes: 0
                })
            )),
            Err(PtyError::Log { .. })
        ));
        assert!(matches!(
            spawn(spec("/no/such/program", None)),
            Err(PtyError::Spawn { .. })
        ));
    }
}
