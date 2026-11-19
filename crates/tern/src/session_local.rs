//! Local shell PTY session driver for `Session` (`Launch::Local`).

use std::io::Read as _;
use std::sync::Arc;

use gpui::{Context, Task};
use tern_ssh::{Disconnect, SessionEvent, TermSize};
use tern_term::LocalPty;

use crate::session::Session;

pub(super) fn spawn(
    size: TermSize,
    cx: &mut Context<Session>,
) -> (Option<Arc<LocalPty>>, Task<()>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let local = match LocalPty::spawn(size.cols, size.rows) {
        Ok(pty) => {
            let pty = Arc::new(pty);
            let _ = tx.send(SessionEvent::Connected);
            if let Ok(mut reader) = pty.reader() {
                std::thread::spawn(move || {
                    let mut buf = [0u8; 8192];
                    let mut idle_ms = 2_u64;
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
                                std::thread::sleep(std::time::Duration::from_millis(idle_ms));
                                idle_ms = (idle_ms + 2).min(15);
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
                                idle_ms = 2;
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
        while let Some(event) = rx.recv().await {
            if this
                .update(cx, |s, cx| s.on_session_event(event, cx))
                .is_err()
            {
                break;
            }
        }
    });
    (local, task)
}
