//! Local shell PTY backed by `alacritty_terminal::tty`.

use std::fs::File;
use std::io;
#[cfg(unix)]
use std::io::Write as _;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::Mutex;

#[cfg(unix)]
use alacritty_terminal::event::{OnResize, WindowSize};
#[cfg(unix)]
use alacritty_terminal::tty::{self, Options, Pty};

/// A spawned local login shell attached to a PTY.
pub struct LocalPty {
    #[cfg(unix)]
    writer: Mutex<Option<File>>,
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
        // Close the cloned master fd first, then wait on the child off the UI thread
        // (alacritty_terminal's `Pty::drop` calls `child.wait()` synchronously).
        let _ = self
            .writer
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(pty) = self.pty.get_mut().unwrap_or_else(|e| e.into_inner()).take() {
            std::thread::spawn(move || drop(pty));
        }
    }
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
        let pty = tty::new(&opts, size, 0)?;
        let writer = pty.file().try_clone()?;
        Ok(Self {
            writer: Mutex::new(Some(writer)),
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

    /// Writes user input bytes to the local shell.
    ///
    /// # Errors
    /// Returns an error if the PTY is closed, the underlying write fails, or the platform is not Unix.
    #[cfg(unix)]
    pub fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let mut guard = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        let Some(writer) = guard.as_mut() else {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "pty closed"));
        };
        writer.write_all(bytes)?;
        writer.flush()
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
mod tests {
    use std::path::Path;

    use super::resolve_working_directory;

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
}
