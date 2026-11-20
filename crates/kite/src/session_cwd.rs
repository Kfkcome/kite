//! The local shell's process id and last directory. Both live on the session so a
//! reconnect starts where that shell was, and so the tab file can be written without
//! holding the PTY.

use std::path::{Path, PathBuf};

use super::{Launch, Session};

impl Launch {
    /// A local terminal, new or restored.
    pub(crate) fn is_local(&self) -> bool {
        matches!(self, Launch::Local | Launch::LocalIn(_))
    }
}

impl Session {
    /// PTY child of a running local shell.
    pub(crate) fn local_pid(&self) -> Option<u32> {
        self.link.local.as_ref().and_then(|pty| pty.child_pid())
    }

    /// Directory the local shell was last known to be in.
    pub(crate) fn local_cwd(&self) -> Option<&Path> {
        self.link.cwd.as_deref()
    }

    pub(crate) fn set_local_cwd(&mut self, cwd: PathBuf) {
        if cwd.is_dir() {
            self.link.cwd = Some(cwd);
        }
    }
}
