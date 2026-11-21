//! Local shell PTY backed by `alacritty_terminal::tty`.

use std::fs::File;
use std::io;
#[cfg(unix)]
use std::io::Write as _;
#[cfg(unix)]
use std::os::fd::AsFd as _;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(unix)]
use std::sync::{Arc, Mutex, mpsc};

#[cfg(unix)]
use alacritty_terminal::event::{OnResize, WindowSize};
#[cfg(unix)]
use alacritty_terminal::tty::{self, Options, Pty};

#[cfg(unix)]
const WRITE_QUEUE_CAP: usize = 256;
#[cfg(unix)]
const WRITE_POLL_MS: u16 = 250;

/// A spawned local login shell attached to a PTY.
pub struct LocalPty {
    #[cfg(unix)]
    writer_tx: Mutex<Option<mpsc::SyncSender<Vec<u8>>>>,
    #[cfg(unix)]
    closed: Arc<AtomicBool>,
    #[cfg(unix)]
    pty: Mutex<Option<Pty>>,
}

impl std::fmt::Debug for LocalPty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalPty").finish_non_exhaustive()
    }
}

#[cfg(unix)]
impl Drop for LocalPty {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Relaxed);
        // Drop the writer channel first so the background writer exits and releases its cloned fd,
        // then wait on the child off the UI thread (Pty::drop calls child.wait() synchronously).
        let _ = self
            .writer_tx
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(pty) = self.pty.get_mut().unwrap_or_else(|e| e.into_inner()).take() {
            std::thread::spawn(move || drop(pty));
        }
    }
}

#[cfg(unix)]
fn wait_writable(file: &File) -> bool {
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    let mut fds = [PollFd::new(file.as_fd(), PollFlags::POLLOUT)];
    match poll(&mut fds, PollTimeout::from(WRITE_POLL_MS)) {
        Ok(0) => true,
        Ok(_) => fds[0]
            .revents()
            .is_none_or(|ev| !ev.intersects(PollFlags::POLLERR | PollFlags::POLLNVAL)),
        Err(nix::errno::Errno::EINTR) => true,
        Err(_) => false,
    }
}

#[cfg(unix)]
fn write_all_nonblocking(writer: &mut File, bytes: &[u8], closed: &AtomicBool) -> bool {
    let mut pos = 0;
    while pos < bytes.len() {
        match writer.write(&bytes[pos..]) {
            Ok(0) => return false,
            Ok(n) => pos += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if closed.load(Ordering::Relaxed) || !wait_writable(writer) {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

#[cfg(unix)]
fn spawn_writer(mut writer: File, rx: mpsc::Receiver<Vec<u8>>, closed: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        while let Ok(chunk) = rx.recv() {
            if !write_all_nonblocking(&mut writer, &chunk, &closed) {
                break;
            }
        }
    });
}

impl LocalPty {
    /// Spawns the user's default login shell at `cols` x `rows`, in `$HOME`.
    ///
    /// # Errors
    /// Returns an error if the PTY cannot be created, its file descriptor cannot be cloned,
    /// or the current platform is not Unix.
    pub fn spawn(cols: u16, rows: u16) -> io::Result<Self> {
        Self::spawn_in(cols, rows, None)
    }

    /// Spawns the user's default login shell at `cols` x `rows`.
    ///
    /// `working_directory` is used when it is an existing directory. Anything else,
    /// including `None` and a path that has gone away, starts in `$HOME`.
    ///
    /// # Errors
    /// Returns an error if the PTY cannot be created, its file descriptor cannot be cloned,
    /// or the current platform is not Unix.
    #[cfg(unix)]
    pub fn spawn_in(cols: u16, rows: u16, working_directory: Option<&Path>) -> io::Result<Self> {
        let mut opts = Options {
            working_directory: resolve_working_directory(working_directory),
            ..Options::default()
        };
        opts.env.insert("TERM".into(), "xterm-256color".into());
        opts.env.insert("COLORTERM".into(), "truecolor".into());
        let size = WindowSize {
            num_lines: rows.max(1),
            num_cols: cols.max(1),
            cell_width: 0,
            cell_height: 0,
        };
        let mut pty = tty::new(&opts, size, 0)?;
        // LocalPty detects exit from PTY master EOF, so keeping an unread SIGCHLD socket pair
        // would only let process-wide child exits fill its buffer.
        pty.detach_signal_pipe();
        let writer = pty.file().try_clone()?;
        let (tx, rx) = mpsc::sync_channel(WRITE_QUEUE_CAP);
        let closed = Arc::new(AtomicBool::new(false));
        spawn_writer(writer, rx, Arc::clone(&closed));
        Ok(Self {
            writer_tx: Mutex::new(Some(tx)),
            closed,
            pty: Mutex::new(Some(pty)),
        })
    }

    #[cfg(not(unix))]
    pub fn spawn_in(_cols: u16, _rows: u16, _working_directory: Option<&Path>) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "local terminal is only supported on Unix",
        ))
    }

    /// Process id of the PTY child, or `None` when this platform has no PTY or it is closed.
    ///
    /// On macOS the child is `login`. The shell `login` started is a child of that process.
    #[must_use]
    pub fn child_pid(&self) -> Option<u32> {
        #[cfg(unix)]
        {
            let guard = self.pty.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().map(|pty| pty.child().id())
        }
        #[cfg(not(unix))]
        {
            let _ = self;
            None
        }
    }

    /// Clones the underlying PTY file descriptor for a blocking reader thread.
    ///
    /// # Errors
    /// Returns an error if the PTY is already closed, `try_clone` fails, or the platform is not Unix.
    #[cfg(unix)]
    pub fn reader(&self) -> io::Result<File> {
        let guard = self.pty.lock().unwrap_or_else(|e| e.into_inner());
        let pty = guard
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "pty closed"))?;
        pty.file().try_clone()
    }

    #[cfg(not(unix))]
    pub fn reader(&self) -> io::Result<File> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "local terminal is only supported on Unix",
        ))
    }

    /// Queues user input bytes to be written to the local shell without blocking the caller.
    ///
    /// # Errors
    /// Returns an error if the PTY is closed, the bounded write queue is full, or the platform
    /// is not Unix.
    #[cfg(unix)]
    pub fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let tx = {
            let guard = self.writer_tx.lock().unwrap_or_else(|e| e.into_inner());
            guard
                .as_ref()
                .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "pty closed"))?
                .clone()
        };
        if bytes.is_empty() {
            return Ok(());
        }
        tx.try_send(bytes.to_vec()).map_err(|err| match err {
            mpsc::TrySendError::Full(_) => {
                io::Error::new(io::ErrorKind::WouldBlock, "pty write queue full")
            }
            mpsc::TrySendError::Disconnected(_) => {
                io::Error::new(io::ErrorKind::BrokenPipe, "pty closed")
            }
        })
    }

    #[cfg(not(unix))]
    pub fn write(&self, _bytes: &[u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "local terminal is only supported on Unix",
        ))
    }

    /// Updates the PTY window size.
    #[cfg(unix)]
    pub fn resize(&self, cols: u16, rows: u16) {
        let size = WindowSize {
            num_lines: rows.max(1),
            num_cols: cols.max(1),
            cell_width: 0,
            cell_height: 0,
        };
        let mut guard = self.pty.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pty) = guard.as_mut() {
            pty.on_resize(size);
        }
    }

    #[cfg(not(unix))]
    pub fn resize(&self, _cols: u16, _rows: u16) {}
}

/// Directory a new local shell should start in.
///
/// An empty path, a missing path, and a path that is not a directory all mean `$HOME`.
#[must_use]
pub fn resolve_working_directory(requested: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = requested
        && !dir.as_os_str().is_empty()
        && dir.is_dir()
    {
        return Some(dir.to_path_buf());
    }
    std::env::home_dir()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::path::Path;

    use super::{LocalPty, resolve_working_directory};

    #[test]
    fn a_missing_or_empty_directory_falls_back_to_home() {
        let home = std::env::home_dir();
        assert_eq!(resolve_working_directory(None), home);
        assert_eq!(resolve_working_directory(Some(Path::new(""))), home);
        assert_eq!(
            resolve_working_directory(Some(Path::new("/no/such/kite-cwd"))),
            home
        );
        let dir = std::env::temp_dir();
        assert_eq!(resolve_working_directory(Some(&dir)), Some(dir));
    }

    #[cfg(unix)]
    #[test]
    fn write_is_nonblocking_for_large_pastes() {
        let pty = LocalPty::spawn(80, 24).expect("spawn local pty");
        let payload = vec![b'a'; 16 * 1024];
        let started = std::time::Instant::now();
        pty.write(&payload).expect("queue large write");
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }
}
