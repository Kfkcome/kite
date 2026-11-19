//! Local shell PTY backed by `alacritty_terminal::tty`.

use std::fs::File;
use std::io::{self, Write as _};
use std::sync::Mutex;

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::tty::{self, Options, Pty};

/// A spawned local login shell attached to a PTY.
pub struct LocalPty {
    writer: Mutex<Option<File>>,
    pty: Mutex<Option<Pty>>,
}

impl std::fmt::Debug for LocalPty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalPty").finish_non_exhaustive()
    }
}

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
    /// Spawns the user's default login shell at `cols` x `rows`.
    pub fn spawn(cols: u16, rows: u16) -> io::Result<Self> {
        let mut opts = Options::default();
        opts.working_directory = std::env::home_dir();
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

    /// Clones the underlying PTY file descriptor for a blocking reader thread.
    pub fn reader(&self) -> io::Result<File> {
        let guard = self.pty.lock().unwrap_or_else(|e| e.into_inner());
        let pty = guard
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "pty closed"))?;
        pty.file().try_clone()
    }

    /// Writes user input bytes to the local shell.
    pub fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let mut guard = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        let Some(writer) = guard.as_mut() else {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "pty closed"));
        };
        writer.write_all(bytes)?;
        writer.flush()
    }

    /// Updates the PTY window size.
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
}
