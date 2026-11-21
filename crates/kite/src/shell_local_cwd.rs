//! Reads local-shell directories off the UI thread, and writes them before quit.
//!
//! GPUI drops every window before it polls the future `on_app_quit` returns, and
//! closing the window drops the PTY. The directory is therefore read inside the
//! callback and inside `on_window_should_close`, while the shell is still alive.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{Context, Window};

use super::Shell;
use crate::platform::SshRuntime;

const REFRESH: Duration = Duration::from_secs(5);
const FLUSH_COALESCE: Duration = Duration::from_secs(1);

impl Shell {
    pub(super) fn install_local_cwd_hooks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let last_flush = Rc::new(Cell::new(None::<Instant>));
        let close_flush = Rc::clone(&last_flush);
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |_window, cx| {
            let _ = weak.update(cx, |shell, cx| {
                shell.flush_local_cwds_coalesced(&close_flush, cx);
            });
            true
        });
        let quit_flush = Rc::clone(&last_flush);
        cx.on_app_quit(move |shell, cx| {
            shell.flush_local_cwds_coalesced(&quit_flush, cx);
            async {}
        })
        .detach();
        cx.observe_window_activation(window, |shell, window, cx| {
            if !window.is_window_active() {
                shell.refresh_local_cwds(cx);
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let mut last_activity: Vec<(u32, u64)> = Vec::new();
            loop {
                let outcome = this.update(cx, |shell, cx| {
                    let activity = local_activity(shell, cx);
                    let dirty = activity != last_activity;
                    if dirty {
                        last_activity = activity;
                        shell.refresh_local_cwds(cx);
                    }
                });
                if outcome.is_err() {
                    break;
                }
                cx.background_executor().timer(REFRESH).await;
            }
        })
        .detach();
    }

    pub(crate) fn refresh_local_cwds(&mut self, cx: &mut Context<Self>) {
        let pids = local_root_pids(self, cx);
        if pids.is_empty() {
            return;
        }
        let handle = SshRuntime::handle(cx);
        cx.spawn(async move |this, cx| {
            let found = crate::local_cwd::query(&handle, &pids).await;
            let _ = this.update(cx, |shell, cx| {
                if apply_local_cwds(shell, &found, cx) {
                    shell.persist_tabs();
                }
            });
        })
        .detach();
    }

    fn flush_local_cwds_coalesced(
        &mut self,
        last_flush: &Cell<Option<Instant>>,
        cx: &mut Context<Self>,
    ) {
        let now = Instant::now();
        if last_flush
            .get()
            .is_some_and(|prev| now.saturating_duration_since(prev) < FLUSH_COALESCE)
        {
            self.persist_tabs();
            return;
        }
        last_flush.set(Some(now));
        self.flush_local_cwds_blocking(cx);
    }

    /// Queries every live local shell, then writes `tabs.json`.
    ///
    /// The call blocks the UI thread for at most ~500ms. Quit has to finish the
    /// read before this shell, and its PTY, is dropped.
    pub(super) fn flush_local_cwds_blocking(&mut self, cx: &mut Context<Self>) {
        let pids = local_root_pids(self, cx);
        if !pids.is_empty() {
            let handle = SshRuntime::handle(cx);
            let found = crate::local_cwd::query_blocking(&handle, &pids);
            apply_local_cwds(self, &found, cx);
        }
        self.persist_tabs();
    }
}

fn local_activity(shell: &Shell, cx: &gpui::App) -> Vec<(u32, u64)> {
    shell
        .tabs
        .iter()
        .flat_map(|tab| &tab.panes)
        .filter_map(|pane| {
            let session = pane.session.read(cx);
            session.local_pid().map(|pid| (pid, session.output_seq()))
        })
        .collect()
}

fn local_root_pids(shell: &Shell, cx: &gpui::App) -> Vec<u32> {
    shell
        .tabs
        .iter()
        .flat_map(|tab| &tab.panes)
        .filter_map(|pane| pane.session.read(cx).local_pid())
        .collect()
}

fn apply_local_cwds(
    shell: &mut Shell,
    found: &HashMap<u32, PathBuf>,
    cx: &mut Context<Shell>,
) -> bool {
    let mut changed = false;
    for tab in &mut shell.tabs {
        for pane in &mut tab.panes {
            let Some(pid) = pane.session.read(cx).local_pid() else {
                continue;
            };
            let Some(path) = found.get(&pid) else {
                continue;
            };
            if pane.cwd.as_deref() != Some(path.as_path()) {
                pane.cwd = Some(path.clone());
                changed = true;
            }
            let path = path.clone();
            pane.session
                .update(cx, |session, _| session.set_local_cwd(path));
        }
    }
    changed
}
