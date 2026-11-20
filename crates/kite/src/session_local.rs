//! Local shell PTY session driver for `Session` (`Launch::Local`).

use std::io::Read as _;
use std::sync::Arc;

use gpui::{Context, Task};
use kite_ssh::{Disconnect, SessionEvent, TermSize};
use kite_term::LocalPty;

use crate::session::Session;

const BATCH_CAP: usize = 256 * 1024;

#[cfg(unix)]
fn wait_readable(file: &std::fs::File) {
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    use std::os::fd::AsFd as _;
    let mut fds = [PollFd::new(file.as_fd(), PollFlags::POLLIN)];
    let _ = poll(&mut fds, PollTimeout::NONE);
}

#[cfg(not(unix))]
fn wait_readable(_file: &std::fs::File) {
    std::thread::sleep(std::time::Duration::from_millis(8));
}

pub(super) fn spawn(
    size: TermSize,
    cwd: Option<&std::path::Path>,
    cx: &mut Context<Session>,
) -> (Option<Arc<LocalPty>>, Task<()>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let local = match LocalPty::spawn_in(size.cols, size.rows, cwd) {
        Ok(pty) => {
            let pty = Arc::new(pty);
            let _ = tx.send(SessionEvent::Connected);
            if let Ok(mut reader) = pty.reader() {
                std::thread::spawn(move || {
                    let mut buf = [0u8; 8192];
                    loop {
                        match reader.read(&mut buf) {
                            Ok(0) => {
                                let _ = tx.send(SessionEvent::Closed {
                                    reason: Disconnect::Local,
                                    error: None,
                                    exit_status: None,
                                });
                                break;
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                wait_readable(&reader);
                                continue;
                            }
                            Err(_) => {
                                let _ = tx.send(SessionEvent::Closed {
                                    reason: Disconnect::Local,
                                    error: None,
                                    exit_status: None,
                                });
                                break;
                            }
                            Ok(n) => {
                                if tx.send(SessionEvent::Data(buf[..n].to_vec())).is_err() {
                                    break;
                                }
                            }
                        }
                    }
                });
            }
            Some(pty)
        }
        Err(e) => {
            let _ = tx.send(SessionEvent::Closed {
                reason: Disconnect::Local,
                error: Some(e.to_string()),
                exit_status: None,
            });
            None
        }
    };
    let task = cx.spawn(async move |this, cx| {
        while let Some(first) = rx.recv().await {
            let (batch, trailing) = coalesce_events(first, &mut rx);
            if let Some(data) = batch
                && this
                    .update(cx, |s, cx| s.on_session_event(SessionEvent::Data(data), cx))
                    .is_err()
            {
                break;
            }
            if let Some(ev) = trailing
                && this.update(cx, |s, cx| s.on_session_event(ev, cx)).is_err()
            {
                break;
            }
        }
    });
    (local, task)
}

fn coalesce_events(
    first: SessionEvent,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<SessionEvent>,
) -> (Option<Vec<u8>>, Option<SessionEvent>) {
    let mut buf = match first {
        SessionEvent::Data(bytes) => bytes,
        other => return (None, Some(other)),
    };
    while buf.len() < BATCH_CAP {
        match rx.try_recv() {
            Ok(SessionEvent::Data(more)) => buf.extend_from_slice(&more),
            Ok(other) => return (Some(buf), Some(other)),
            Err(_) => break,
        }
    }
    (Some(buf), None)
}
