//! One set of port forwards per SSH target, shared by every open tab and pane.
//!
//! Each forward is started on one connected session. Another connection to the same host
//! does not bind it again. When the session that holds the sockets closes or drops, they
//! are started on a connection that is still up.

use gpui::{App, Context, Entity};
use kite_ssh::Forward;

use super::Shell;
use super::forwards_ui::TunnelRow;
use crate::forward_spec;
use crate::i18n::tr;
use crate::session::{HostKey, Session, Status};

pub(super) struct Claim {
    key: HostKey,
    forward: Forward,
    session: Entity<Session>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HostSnap {
    key: Option<HostKey>,
    connected: bool,
    forwards: Vec<Forward>,
    starting: Vec<Forward>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForwardMove {
    to: usize,
    forwards: Vec<Forward>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveForward {
    forward: Forward,
    id: u64,
    bound_port: u16,
    owner: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TunnelDraft {
    forward: Forward,
    running_id: Option<u64>,
    bound_port: Option<u16>,
    owner: Option<usize>,
    saved: bool,
    auto_start: bool,
}

struct OpenSession {
    entity: Entity<Session>,
    snap: HostSnap,
}

/// Forwards from `wanted` that no connected session is running or already opening.
fn forwards_to_start(wanted: &[Forward], active: &[Forward], starting: &[Forward]) -> Vec<Forward> {
    let mut out = Vec::new();
    for forward in wanted {
        if active.contains(forward) || starting.contains(forward) || out.contains(forward) {
            continue;
        }
        out.push(forward.clone());
    }
    out
}

/// Running forwards across sessions, then saved ones that are not running. The first session
/// that has a forward is its owner.
fn aggregate_tunnels(
    saved: &[Forward],
    manual: &[Forward],
    live: &[LiveForward],
) -> Vec<TunnelDraft> {
    let mut rows = Vec::new();
    for item in live {
        if rows
            .iter()
            .any(|row: &TunnelDraft| row.forward == item.forward)
        {
            continue;
        }
        let is_saved = saved.contains(&item.forward);
        rows.push(TunnelDraft {
            forward: item.forward.clone(),
            running_id: Some(item.id),
            bound_port: Some(item.bound_port),
            owner: Some(item.owner),
            saved: is_saved,
            auto_start: is_saved && !manual.contains(&item.forward),
        });
    }
    for forward in saved {
        if rows.iter().any(|row| row.forward == *forward) {
            continue;
        }
        rows.push(TunnelDraft {
            forward: forward.clone(),
            running_id: None,
            bound_port: None,
            owner: None,
            saved: true,
            auto_start: !manual.contains(forward),
        });
    }
    rows
}

/// Moves each dying session's running and in-flight forwards onto one other connected
/// session of the same host. A forward already running or opening there is left in place.
fn plan_handoff(sessions: &[HostSnap], dying: &[usize]) -> Vec<ForwardMove> {
    let mut taken: Vec<(HostKey, Forward)> = Vec::new();
    for (ix, session) in sessions.iter().enumerate() {
        if dying.contains(&ix) {
            continue;
        }
        let Some(key) = session.key.clone() else {
            continue;
        };
        if !session.connected {
            continue;
        }
        for forward in session.forwards.iter().chain(&session.starting) {
            taken.push((key.clone(), forward.clone()));
        }
    }
    let mut moves = Vec::new();
    for &from in dying {
        let Some(session) = sessions.get(from) else {
            continue;
        };
        let Some(key) = session.key.clone() else {
            continue;
        };
        let Some(to) = sessions.iter().enumerate().find_map(|(ix, other)| {
            (other.connected && !dying.contains(&ix) && other.key.as_ref() == Some(&key))
                .then_some(ix)
        }) else {
            continue;
        };
        let mut forwards = Vec::new();
        for forward in session.forwards.iter().chain(&session.starting) {
            if taken
                .iter()
                .any(|(held, item)| held == &key && item == forward)
                || forwards.contains(forward)
            {
                continue;
            }
            forwards.push(forward.clone());
            taken.push((key.clone(), forward.clone()));
        }
        if !forwards.is_empty() {
            moves.push(ForwardMove { to, forwards });
        }
    }
    moves
}

impl Shell {
    pub(super) fn tunnel_rows(
        &self,
        session: &Entity<Session>,
        host_alias: &str,
        cx: &App,
    ) -> Vec<TunnelRow> {
        let hosts = session
            .read(cx)
            .host_key()
            .map(|key| self.sessions_for(&key, cx))
            .unwrap_or_default();
        let mut live = Vec::new();
        for (owner, host_session) in hosts.iter().enumerate() {
            for info in host_session.read(cx).active_forwards() {
                live.push(LiveForward {
                    forward: info.forward,
                    id: info.id,
                    bound_port: info.bound_port,
                    owner,
                });
            }
        }
        let (saved, manual) = self.saved_forwards(host_alias);
        aggregate_tunnels(&saved, &manual, &live)
            .into_iter()
            .map(|row| TunnelRow {
                forward: row.forward,
                running_id: row.running_id,
                bound_port: row.bound_port,
                saved: row.saved,
                auto_start: row.auto_start,
                owner: row.owner.and_then(|ix| hosts.get(ix).cloned()),
            })
            .collect()
    }

    pub(super) fn shared_forward_count(&self, session: &Entity<Session>, cx: &App) -> usize {
        session
            .read(cx)
            .host_key()
            .map(|key| self.active_forwards_of(&key, cx).len())
            .unwrap_or(0)
    }

    pub(super) fn autostart_forwards(&mut self, session: &Entity<Session>, cx: &mut Context<Self>) {
        let wanted = session.read(cx).spec_forwards();
        self.launch_on(session, &wanted, false, cx);
    }

    /// Stops `dying` sessions' listeners and starts those forwards on a session that stays.
    pub(super) fn migrate_forwards(&mut self, dying: &[Entity<Session>], cx: &mut Context<Self>) {
        if dying.is_empty() {
            return;
        }
        let open = self.open_sessions(cx);
        let dying_at: Vec<usize> = open
            .iter()
            .enumerate()
            .filter(|(_, row)| dying.contains(&row.entity))
            .map(|(ix, _)| ix)
            .collect();
        let snaps: Vec<HostSnap> = open.iter().map(|row| row.snap.clone()).collect();
        let moves = plan_handoff(&snaps, &dying_at);
        for &ix in &dying_at {
            let Some(row) = open.get(ix) else { continue };
            row.entity
                .update(cx, |session, cx| session.release_forwards(cx));
            self.forward_claims
                .retain(|claim| claim.session != row.entity);
        }
        for step in moves {
            let Some(row) = open.get(step.to) else {
                continue;
            };
            self.launch_on(&row.entity, &step.forwards, true, cx);
        }
    }

    /// `forwards` were released as `session` disconnected. Start them on another connection.
    pub(super) fn migrate_released(
        &mut self,
        session: &Entity<Session>,
        forwards: &[Forward],
        cx: &mut Context<Self>,
    ) {
        let mut open = self.open_sessions(cx);
        let Some(ix) = open.iter().position(|row| &row.entity == session) else {
            self.forward_claims
                .retain(|claim| &claim.session != session);
            return;
        };
        let starting: Vec<Forward> = self
            .forward_claims
            .iter()
            .filter(|claim| &claim.session == session)
            .map(|claim| claim.forward.clone())
            .collect();
        self.forward_claims
            .retain(|claim| &claim.session != session);
        open[ix].snap.forwards = forwards.to_vec();
        open[ix].snap.starting = starting;
        open[ix].snap.connected = false;
        let snaps: Vec<HostSnap> = open.iter().map(|row| row.snap.clone()).collect();
        for step in plan_handoff(&snaps, &[ix]) {
            let Some(row) = open.get(step.to) else {
                continue;
            };
            self.launch_on(&row.entity, &step.forwards, true, cx);
        }
    }

    pub(super) fn finish_claim(&mut self, session: &Entity<Session>, forward: &Forward) {
        if let Some(at) = self
            .forward_claims
            .iter()
            .position(|claim| &claim.session == session && &claim.forward == forward)
        {
            self.forward_claims.remove(at);
        }
    }

    /// The connection is already gone and another session has the forward, so the failure is
    /// the handoff race rather than a forward the person needs to see.
    pub(super) fn failure_already_covered(
        &self,
        session: &Entity<Session>,
        forward: &Forward,
        cx: &App,
    ) -> bool {
        if session.read(cx).status == Status::Connected {
            return false;
        }
        let Some(key) = session.read(cx).host_key() else {
            return false;
        };
        self.active_forwards_of(&key, cx).contains(forward)
            || self.starting_of(&key).contains(forward)
    }

    pub(super) fn forward_taken(
        &self,
        session: &Entity<Session>,
        forward: &Forward,
        cx: &App,
    ) -> bool {
        let Some(key) = session.read(cx).host_key() else {
            return false;
        };
        self.active_forwards_of(&key, cx).contains(forward)
            || self.starting_of(&key).contains(forward)
    }

    /// `preferred` when it is up, otherwise another connected session of the same host.
    pub(super) fn connected_session(
        &self,
        preferred: &Entity<Session>,
        cx: &App,
    ) -> Option<Entity<Session>> {
        let key = preferred.read(cx).host_key()?;
        if preferred.read(cx).status == Status::Connected {
            return Some(preferred.clone());
        }
        self.sessions_for(&key, cx)
            .into_iter()
            .find(|session| session.read(cx).status == Status::Connected)
    }

    pub(super) fn start_all_panel_forwards(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.panels.forwards.as_ref() else {
            return;
        };
        let session = panel.session.clone();
        let host_alias = self
            .tab_for_session(&session)
            .and_then(|tab| tab.panes.iter().find(|pane| pane.session == session))
            .map(|pane| pane.alias.clone())
            .unwrap_or_default();
        let wanted: Vec<Forward> = self
            .tunnel_rows(&session, &host_alias, cx)
            .into_iter()
            .filter(|row| row.running_id.is_none())
            .map(|row| row.forward)
            .collect();
        if wanted.is_empty() {
            return;
        }
        self.launch_preferred(&session, &wanted, cx);
    }

    pub(super) fn stop_all_panel_forwards(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.panels.forwards.as_ref() else {
            return;
        };
        let session = panel.session.clone();
        let Some(key) = session.read(cx).host_key() else {
            return;
        };
        let stops: Vec<(Entity<Session>, u64, Forward)> = self
            .sessions_for(&key, cx)
            .into_iter()
            .flat_map(|owner| {
                owner
                    .read(cx)
                    .active_forwards()
                    .into_iter()
                    .map(|info| (owner.clone(), info.id, info.forward))
                    .collect::<Vec<_>>()
            })
            .collect();
        for (owner, id, forward) in &stops {
            owner.update(cx, |session, cx| session.stop_forward(*id, cx));
            self.finish_claim(owner, forward);
            self.forget_forward_on_host(&key, forward, cx);
        }
    }

    pub(super) fn toggle_shared_forward(
        &mut self,
        owner: Option<Entity<Session>>,
        forward: &Forward,
        running_id: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = running_id {
            let Some(owner) = owner else { return };
            let key = owner.read(cx).host_key();
            owner.update(cx, |session, cx| session.stop_forward(id, cx));
            self.finish_claim(&owner, forward);
            if let Some(key) = key {
                self.forget_forward_on_host(&key, forward, cx);
            }
            return;
        }
        let Some(panel) = self.panels.forwards.as_ref() else {
            return;
        };
        let preferred = panel.session.clone();
        if self.forward_taken(&preferred, forward, cx) {
            return;
        }
        self.launch_preferred(&preferred, std::slice::from_ref(forward), cx);
    }

    fn forget_forward_on_host(&mut self, key: &HostKey, forward: &Forward, cx: &mut Context<Self>) {
        for session in self.sessions_for(key, cx) {
            session.update(cx, |open, _| open.forget_spec_forward(forward));
        }
    }

    pub(super) fn launch_preferred(
        &mut self,
        preferred: &Entity<Session>,
        forwards: &[Forward],
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.connected_session(preferred, cx) else {
            self.panel_forward_error(tr("Not connected."), cx);
            return;
        };
        self.launch_on(&target, forwards, false, cx);
    }

    fn launch_on(
        &mut self,
        target: &Entity<Session>,
        forwards: &[Forward],
        handoff: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = target.read(cx).host_key() else {
            return;
        };
        if target.read(cx).status != Status::Connected {
            return;
        }
        let active = self.active_forwards_of(&key, cx);
        let starting = self.starting_of(&key);
        for forward in forwards_to_start(forwards, &active, &starting) {
            self.forward_claims.push(Claim {
                key: key.clone(),
                forward: forward.clone(),
                session: target.clone(),
            });
            target.update(cx, |session, cx| {
                if handoff {
                    session.launch_forward(forward, cx);
                } else {
                    session.add_forward(forward, cx);
                }
            });
        }
    }

    fn panel_forward_error(&mut self, message: &str, cx: &mut Context<Self>) {
        if let Some(panel) = self.panels.forwards.as_mut() {
            panel.error = Some(message.to_owned());
        }
        cx.notify();
    }

    fn sessions_for(&self, key: &HostKey, cx: &App) -> Vec<Entity<Session>> {
        self.tabs
            .iter()
            .flat_map(|tab| &tab.panes)
            .filter(|pane| pane.session.read(cx).host_key().as_ref() == Some(key))
            .map(|pane| pane.session.clone())
            .collect()
    }

    fn active_forwards_of(&self, key: &HostKey, cx: &App) -> Vec<Forward> {
        let mut out = Vec::new();
        for session in self.sessions_for(key, cx) {
            if session.read(cx).status != Status::Connected {
                continue;
            }
            for info in session.read(cx).active_forwards() {
                if !out.contains(&info.forward) {
                    out.push(info.forward);
                }
            }
        }
        out
    }

    fn starting_of(&self, key: &HostKey) -> Vec<Forward> {
        self.forward_claims
            .iter()
            .filter(|claim| &claim.key == key)
            .map(|claim| claim.forward.clone())
            .collect()
    }

    fn saved_forwards(&self, host_alias: &str) -> (Vec<Forward>, Vec<Forward>) {
        let Some(conn) = self.connections.iter().find(|conn| conn.name == host_alias) else {
            return (Vec::new(), Vec::new());
        };
        let parse = |list: &[String]| -> Vec<Forward> {
            list.iter()
                .filter_map(|text| forward_spec::parse(text).ok())
                .collect()
        };
        (parse(&conn.forwards), parse(&conn.manual_forwards))
    }

    fn open_sessions(&self, cx: &App) -> Vec<OpenSession> {
        self.tabs
            .iter()
            .flat_map(|tab| &tab.panes)
            .map(|pane| {
                let session = pane.session.read(cx);
                let starting = self
                    .forward_claims
                    .iter()
                    .filter(|claim| claim.session == pane.session)
                    .map(|claim| claim.forward.clone())
                    .collect();
                OpenSession {
                    entity: pane.session.clone(),
                    snap: HostSnap {
                        key: session.host_key(),
                        connected: session.status == Status::Connected,
                        forwards: session
                            .active_forwards()
                            .into_iter()
                            .map(|info| info.forward)
                            .collect(),
                        starting,
                    },
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{HostSnap, LiveForward, aggregate_tunnels, forwards_to_start, plan_handoff};
    use crate::session::HostKey;
    use kite_ssh::{ConnectSpec, Forward, JumpHop};

    fn key(host: &str, port: u16, user: &str, proxy: Option<&str>, jump: Option<&str>) -> HostKey {
        HostKey::from_spec(&ConnectSpec {
            host: host.into(),
            port,
            user: user.into(),
            proxy_command: proxy.map(str::to_owned),
            proxy_jump: jump
                .map(|hop| {
                    vec![JumpHop {
                        host: hop.into(),
                        port: 22,
                        ..JumpHop::default()
                    }]
                })
                .unwrap_or_default(),
            ..ConnectSpec::default()
        })
    }

    fn local(port: u16) -> Forward {
        Forward::Local {
            bind_host: "127.0.0.1".into(),
            bind_port: port,
            dest_host: "127.0.0.1".into(),
            dest_port: 80,
        }
    }

    fn remote(port: u16) -> Forward {
        Forward::Remote {
            bind_host: "localhost".into(),
            bind_port: port,
            dest_host: "127.0.0.1".into(),
            dest_port: port,
        }
    }

    fn snap(
        key: Option<HostKey>,
        connected: bool,
        forwards: Vec<Forward>,
        starting: Vec<Forward>,
    ) -> HostSnap {
        HostSnap {
            key,
            connected,
            forwards,
            starting,
        }
    }

    #[test]
    fn host_key_matches_the_target_and_ignores_the_key_file() {
        let same = key("server", 2222, "root", None, None);
        assert_eq!(same, key("server", 2222, "root", None, None));
        assert_ne!(same, key("server", 22, "root", None, None));
        assert_ne!(same, key("server", 2222, "deploy", None, None));
        assert_ne!(same, key("other", 2222, "root", None, None));
        assert_ne!(
            same,
            key("server", 2222, "root", Some("nc bastion 22"), None)
        );
        assert_ne!(same, key("server", 2222, "root", None, Some("bastion")));
        let mut spec = ConnectSpec {
            host: "server".into(),
            port: 2222,
            user: "root".into(),
            identity_files: vec!["/tmp/a".into()],
            ..ConnectSpec::default()
        };
        let with_key = HostKey::from_spec(&spec);
        spec.identity_files = vec!["/tmp/b".into()];
        assert_eq!(with_key, HostKey::from_spec(&spec));
    }

    #[test]
    fn a_forward_already_running_or_starting_is_not_started_again() {
        let wanted = vec![local(7890), remote(7897), local(7890)];
        let active = vec![local(7890)];
        let starting = vec![remote(7897)];
        assert!(forwards_to_start(&wanted, &active, &starting).is_empty());
        assert_eq!(
            forwards_to_start(&wanted, &[], &[]),
            vec![local(7890), remote(7897)]
        );
        assert_eq!(forwards_to_start(&wanted, &active, &[]), vec![remote(7897)]);
    }

    #[test]
    fn rows_keep_one_owner_and_still_list_a_saved_forward_that_is_stopped() {
        let saved = vec![local(7890), remote(7897)];
        let manual = vec![remote(7897)];
        let live = vec![
            LiveForward {
                forward: local(7890),
                id: 4,
                bound_port: 7890,
                owner: 0,
            },
            LiveForward {
                forward: local(7890),
                id: 9,
                bound_port: 7890,
                owner: 1,
            },
        ];
        let rows = aggregate_tunnels(&saved, &manual, &live);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].running_id, Some(4));
        assert_eq!(rows[0].owner, Some(0));
        assert!(rows[0].auto_start);
        assert_eq!(rows[1].forward, remote(7897));
        assert_eq!(rows[1].running_id, None);
        assert_eq!(rows[1].owner, None);
        assert!(!rows[1].auto_start);
    }

    #[test]
    fn handoff_moves_forwards_onto_the_other_connected_session() {
        let host = key("server", 2222, "root", None, None);
        let other = key("server", 22, "root", None, None);
        let running = snap(
            Some(host.clone()),
            true,
            vec![local(7890), remote(7897)],
            vec![],
        );
        let idle = snap(Some(host.clone()), true, vec![], vec![]);
        let moves = plan_handoff(&[running.clone(), idle], &[0]);
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].to, 1);
        assert_eq!(moves[0].forwards, vec![local(7890), remote(7897)]);

        let busy = snap(
            Some(host.clone()),
            true,
            vec![local(7890)],
            vec![remote(7897)],
        );
        assert!(plan_handoff(&[running, busy], &[0]).is_empty());

        let down = snap(Some(host.clone()), false, vec![], vec![]);
        assert!(plan_handoff(&[running_snap(&host), down], &[0]).is_empty());
        assert!(
            plan_handoff(
                &[running_snap(&host), snap(Some(other), true, vec![], vec![])],
                &[0]
            )
            .is_empty()
        );
        assert!(
            plan_handoff(
                &[
                    snap(None, true, vec![local(1)], vec![]),
                    running_snap(&host)
                ],
                &[0]
            )
            .is_empty()
        );
    }

    #[test]
    fn handoff_includes_a_forward_still_starting_and_skips_a_session_that_is_also_closing() {
        let host = key("server", 2222, "root", None, None);
        let opening = snap(
            Some(host.clone()),
            true,
            vec![local(7890)],
            vec![remote(7897)],
        );
        let stay = snap(Some(host.clone()), true, vec![], vec![]);
        let also = snap(Some(host.clone()), true, vec![local(1)], vec![]);
        let moves = plan_handoff(&[opening, stay, also], &[0, 2]);
        assert!(moves.iter().all(|step| step.to == 1));
        assert_eq!(moves[0].forwards, vec![local(7890), remote(7897)]);
        assert_eq!(moves[1].forwards, vec![local(1)]);
    }

    fn running_snap(host: &HostKey) -> HostSnap {
        snap(Some(host.clone()), true, vec![local(7890)], vec![])
    }
}
