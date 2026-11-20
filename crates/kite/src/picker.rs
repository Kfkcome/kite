// Adapted from zeron crates/ui/src/shell/command_palette.rs (palette card, header, footer,
// overlay) and crates/engine/src/repos.rs (nucleo ranking) (MIT).
//! ⌘K picker: type to fuzzy-filter connections and saved layouts. Enter connects a host or
//! opens a layout. An empty query lists recents first, then layouts; text that matches
//! nothing connects as a typed target.

use crate::hover::HoverFade as _;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, Context, FocusHandle, FontWeight, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Pixels, ScrollHandle, SharedString, Size,
    StatefulInteractiveElement, Styled, Window, actions, anchored, deferred, div, point, px,
};
use kite_ssh::HostEntry;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::empty_state;
use crate::i18n::{fill, tr};
use crate::shell::Shell;
use crate::shell::connections_ui::button;
use crate::sidebar;
use crate::theme::Theme;

actions!(tern, [ToggleHostPicker]);

pub struct Picker {
    query: String,
    active: usize,
    focus: FocusHandle,
    scroll: ScrollHandle,
}

impl Picker {
    pub fn new(cx: &mut App) -> Self {
        Self {
            query: String::new(),
            active: 0,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
        }
    }

    pub fn focus(&self) -> &FocusHandle {
        &self.focus
    }

    pub fn query(&self) -> &str {
        &self.query
    }
}

/// What a keystroke does to an open picker.
#[derive(Debug, PartialEq, Eq)]
pub enum Key {
    Move(isize),
    Pick,
    Close,
    Erase,
    Type(String),
    Ignore,
}

/// Arrows and readline's ⌃N/⌃P move, as in zeron's `classify_key`; any other keystroke that
/// produces text without ⌘ or ⌃ is typed into the query.
pub fn classify(event: &KeyDownEvent) -> Key {
    let ks = &event.keystroke;
    let mods = &ks.modifiers;
    match ks.key.as_str() {
        "up" => Key::Move(-1),
        "down" => Key::Move(1),
        "p" if mods.control => Key::Move(-1),
        "n" if mods.control => Key::Move(1),
        "enter" => Key::Pick,
        "escape" => Key::Close,
        "backspace" => Key::Erase,
        _ if mods.platform || mods.control => Key::Ignore,
        _ => match &ks.key_char {
            Some(text) if !text.chars().any(char::is_control) => Key::Type(text.clone()),
            _ => Key::Ignore,
        },
    }
}

/// Indices into `hosts` that match `query`, best first. An empty query keeps config order.
/// Each host is matched as "alias user@hostname", so either the name or the address finds it.
#[cfg(test)]
pub fn rank(query: &str, hosts: &[HostEntry]) -> Vec<usize> {
    let labels: Vec<String> = hosts
        .iter()
        .map(|host| format!("{} {}", host.alias, sidebar::address(host)))
        .collect();
    rank_labels(query, &labels)
}

/// What the picker lists: the ranked matches for a query; with none typed, the recent hosts
/// (newest first) and then everyone else in list order. `labels[i]` and `aliases[i]` describe
/// host `i`.
pub fn order(query: &str, labels: &[String], aliases: &[&str], recent: &[&str]) -> Vec<usize> {
    if !query.trim().is_empty() {
        return rank_named(query, aliases, labels);
    }
    let mut out: Vec<usize> = recent
        .iter()
        .filter_map(|r| aliases.iter().position(|a| a == r))
        .collect();
    let rest: Vec<usize> = (0..labels.len()).filter(|i| !out.contains(i)).collect();
    out.extend(rest);
    out
}

/// Hosts whose name matches come first (best first), then the ones that only match through
/// their address, group or tags, so a tag never outranks a host named for the query.
pub fn rank_named<N: AsRef<str>, L: AsRef<str>>(
    query: &str,
    names: &[N],
    labels: &[L],
) -> Vec<usize> {
    let mut out = rank_labels(query, names);
    let by_name = out.clone();
    out.extend(
        rank_labels(query, labels)
            .into_iter()
            .filter(|ix| !by_name.contains(ix)),
    );
    out
}

/// Whether the pinned "Local Terminal" row matches the current picker query.
fn includes_local(query: &str) -> bool {
    let q = query.trim();
    q.is_empty() || !rank_labels(q, &["local terminal 本地终端 zsh bash shell"]).is_empty()
}

/// One picker row: a saved host, or a saved layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Host(usize),
    Layout(usize),
}

/// Hosts and layouts the picker lists. An empty query lists recent hosts first, then the
/// other hosts, then every layout. A query ranks a name match ahead of an address or leaf.
pub fn hits(
    query: &str,
    host_names: &[&str],
    host_labels: &[String],
    layout_names: &[&str],
    layout_labels: &[String],
    recent: &[&str],
) -> Vec<Hit> {
    if query.trim().is_empty() {
        let mut out: Vec<Hit> = order(query, host_labels, host_names, recent)
            .into_iter()
            .map(Hit::Host)
            .collect();
        out.extend((0..layout_names.len()).map(Hit::Layout));
        return out;
    }
    let hosts = host_names.len();
    let mut names: Vec<&str> = host_names.to_vec();
    names.extend_from_slice(layout_names);
    let mut labels: Vec<&str> = host_labels.iter().map(String::as_str).collect();
    labels.extend(layout_labels.iter().map(String::as_str));
    rank_named(query, &names, &labels)
        .into_iter()
        .map(|ix| {
            if ix < hosts {
                Hit::Host(ix)
            } else {
                Hit::Layout(ix - hosts)
            }
        })
        .collect()
}

/// Fuzzy-ranks any labels: indices of the matches, best first, input order on an empty
/// query or a tie.
pub fn rank_labels<S: AsRef<str>>(query: &str, labels: &[S]) -> Vec<usize> {
    if query.trim().is_empty() {
        return (0..labels.len()).collect();
    }
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, usize)> = labels
        .iter()
        .enumerate()
        .filter_map(|(ix, label)| {
            pattern
                .score(Utf32Str::new(label.as_ref(), &mut buf), &mut matcher)
                .map(|score| (score, ix))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, ix)| ix).collect()
}

impl Shell {
    pub fn on_picker_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = classify(event);
        if key == Key::Ignore {
            return;
        }
        cx.stop_propagation();
        let hits = self.picker_hits();
        let query = self.picker_query();
        let show_local = includes_local(&query);
        let local_offset = usize::from(show_local);
        let total = local_offset + hits.len();
        let Some(picker) = self.picker_mut() else {
            return;
        };
        match key {
            Key::Move(delta) => {
                if total > 0 {
                    picker.active = crate::tabs::step(picker.active, total, delta);
                    picker.scroll.scroll_to_item(picker.active);
                }
            }
            Key::Type(text) => {
                picker.query.push_str(&text);
                picker.active = 0;
            }
            Key::Erase => {
                picker.query.pop();
                picker.active = 0;
            }
            Key::Pick => {
                let active = picker.active;
                if show_local && active == 0 {
                    self.close_picker(window, cx);
                    self.open_local_terminal(window, cx);
                    return;
                }
                let chosen = hits.get(active.saturating_sub(local_offset)).copied();
                self.close_picker(window, cx);
                match chosen {
                    Some(Hit::Host(ix)) => {
                        if let Some(host) = self.hosts().get(ix).cloned() {
                            self.connect_host(&host, window, cx);
                        }
                    }
                    Some(Hit::Layout(ix)) => self.open_layout_at(ix, window, cx),
                    // Nothing listed matches: treat the text as a target, so `user@host:port`
                    // and `~/.ssh/config` aliases that were never imported still connect.
                    None if !query.trim().is_empty() => {
                        self.connect_target(query.trim(), window, cx)
                    }
                    None => {}
                }
                return;
            }
            Key::Close => return self.close_picker(window, cx),
            Key::Ignore => {}
        }
        cx.notify();
    }
}

/// Results list height: zeron's `palette_results_height`.
pub(crate) fn results_height(viewport: Size<Pixels>) -> f32 {
    (f32::from(viewport.height) - 180.0).clamp(100.0, 360.0)
}

/// The rows an open picker draws, besides its query.
pub struct Listed<'a> {
    pub hosts: &'a [HostEntry],
    pub layouts: &'a [crate::layouts::SavedLayout],
    pub hits: &'a [Hit],
    pub recent: &'a [String],
}

pub fn render(
    picker: &Picker,
    listed: &Listed<'_>,
    viewport: Size<Pixels>,
    t: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let Listed {
        hosts,
        layouts,
        hits,
        recent,
    } = listed;
    let show_local = includes_local(&picker.query);
    let local_offset = usize::from(show_local);
    let total = local_offset + hits.len();
    let active = picker.active.min(total.saturating_sub(1));
    let local_row = show_local.then(|| {
        div()
            .id("pick-local-terminal")
            .mx(px(4.))
            .px(px(12.))
            .py(px(4.))
            .min_h(px(30.))
            .rounded(px(10.))
            .flex()
            .items_center()
            .gap(px(10.))
            .cursor_pointer()
            .hover_fade("pick-local", gpui::transparent_black(), t.row_active)
            .when(active == 0, |el| el.bg(t.row_active))
            .on_click(cx.listener(|shell, _, window, cx| {
                shell.close_picker(window, cx);
                shell.open_local_terminal(window, cx);
            }))
            .child(
                crate::icons::icon(crate::icons::TERMINAL)
                    .size(px(14.))
                    .text_color(t.accent),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(tr("Local Terminal")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(t.muted)
                    .child(tr("Open a local shell")),
            )
    });
    let rows = hits.iter().enumerate().map(|(i, hit)| {
        let row = i + local_offset;
        let frame = div()
            .id(("pick", row))
            .mx(px(4.))
            .px(px(12.))
            .py(px(4.))
            .min_h(px(30.))
            .rounded(px(10.))
            .flex()
            .items_center()
            .gap(px(10.))
            .cursor_pointer()
            .hover_fade(
                format!("pick-{row}"),
                gpui::transparent_black(),
                t.row_active,
            )
            .when(row == active, |el| el.bg(t.row_active));
        match *hit {
            Hit::Host(ix) => {
                let Some(host) = hosts.get(ix) else {
                    return frame;
                };
                let target = host.clone();
                let alias = host.alias.clone();
                let address = sidebar::address(host);
                frame
                    .on_click(cx.listener(move |shell, _, window, cx| {
                        shell.close_picker(window, cx);
                        shell.connect_host(&target, window, cx);
                    }))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(SharedString::from(alias.clone())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(t.muted)
                            .child(SharedString::from(address)),
                    )
                    .when(picker.query.is_empty() && recent.contains(&alias), |el| {
                        el.child(div().text_xs().text_color(t.faint).child(tr("Recent")))
                    })
            }
            Hit::Layout(ix) => {
                let name = layouts
                    .get(ix)
                    .map(|layout| layout.name.clone())
                    .unwrap_or_default();
                frame
                    .on_click(cx.listener(move |shell, _, window, cx| {
                        shell.close_picker(window, cx);
                        shell.open_layout_at(ix, window, cx);
                    }))
                    .child(
                        crate::icons::icon(crate::icons::LAYOUT)
                            .size(px(14.))
                            .text_color(t.accent),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(SharedString::from(name)),
                    )
                    .child(div().text_xs().text_color(t.faint).child(tr("Layout")))
            }
        }
    });
    let query: SharedString = if picker.query.is_empty() {
        tr("Connect to a host…").into()
    } else {
        picker.query.clone().into()
    };
    let card = div()
        .id("host-picker")
        .track_focus(&picker.focus)
        .w(px(560.0_f32.min(f32::from(viewport.width) - 32.0)))
        .flex()
        .flex_col()
        .rounded(px(16.))
        .border_1()
        .border_color(t.border)
        .bg(t.popup)
        .text_color(t.text)
        .on_key_down(cx.listener(Shell::on_picker_key))
        .on_mouse_down_out(cx.listener(|shell, _, window, cx| shell.close_picker(window, cx)))
        .child(
            div()
                .min_h(px(44.))
                .px(px(16.))
                .py(px(8.))
                .flex()
                .items_center()
                .gap(px(10.))
                .border_b_1()
                .border_color(t.hairline)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(14.))
                        .when(picker.query.is_empty(), |el| el.text_color(t.faint))
                        .child(query),
                )
                .child(kbd(
                    SharedString::from(crate::keymap::badge(
                        cx.global::<crate::keymap::Keymap>()
                            .combo(crate::keymap::ShortcutId::HostPicker),
                    )),
                    t,
                )),
        )
        .child(
            div()
                .id("picker-results")
                .py(px(8.))
                .max_h(px(results_height(viewport)))
                .overflow_y_scroll()
                .track_scroll(&picker.scroll)
                .flex()
                .flex_col()
                .gap(px(2.))
                .children(local_row)
                .children(rows)
                .when(hits.is_empty() && !show_local, |el| {
                    if empty_state::picker_is_empty(
                        &picker.query,
                        hosts.len() + layouts.len(),
                        hits.len(),
                    ) {
                        return el.child(
                            div().py(px(24.)).child(
                                empty_state::view(t, empty_state::HOSTS, tr("No hosts yet."))
                                    .gap(px(12.))
                                    .child(
                                        button("picker-empty-new", tr("New connection"), true, t)
                                            .on_click(cx.listener(|s, _, w, cx| {
                                                s.close_picker(w, cx);
                                                s.open_form(None, None, w, cx);
                                            })),
                                    ),
                            ),
                        );
                    }
                    el.child(
                        div()
                            .py(px(24.))
                            .flex()
                            .justify_center()
                            .text_sm()
                            .text_color(t.muted)
                            .child(SharedString::from(fill(
                                "No hosts match. Enter connects to \"{}\".",
                                &[picker.query.trim()],
                            ))),
                    )
                }),
        )
        .child(
            div()
                .px(px(16.))
                .py(px(7.))
                .border_t_1()
                .border_color(t.hairline)
                .flex()
                .items_center()
                .gap(px(12.))
                .child(hint("↑ ↓", tr("Navigate"), t))
                .child(hint("↵", tr("Connect"), t))
                .child(hint("Esc", tr("Close"), t)),
        );
    deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .occlude()
                .w(viewport.width)
                .h(viewport.height)
                .bg(t.scrim(0.35))
                .flex()
                .items_center()
                .justify_center()
                .child(card),
        ),
    )
    .priority(2)
    .into_any_element()
}

pub(crate) fn kbd(keys: SharedString, t: &Theme) -> impl IntoElement + use<> {
    div()
        .px(px(5.))
        .rounded(px(4.))
        .bg(t.row_hover)
        .text_size(px(10.))
        .text_color(t.muted)
        .child(keys)
}

pub(crate) fn hint(keys: &'static str, label: &'static str, t: &Theme) -> impl IntoElement + use<> {
    div()
        .flex()
        .items_center()
        .gap(px(5.))
        .child(kbd(keys.into(), t))
        .child(div().text_size(px(10.)).text_color(t.muted).child(label))
}

#[cfg(test)]
mod tests {
    use super::{Hit, hits, order, rank};
    use kite_ssh::HostEntry;

    fn host(alias: &str, host_name: &str) -> HostEntry {
        HostEntry {
            alias: alias.into(),
            host_name: host_name.into(),
            port: 22,
            user: Some("deploy".into()),
            identity_files: Vec::new(),
            proxy_command: None,
            ..Default::default()
        }
    }

    fn hosts() -> Vec<HostEntry> {
        vec![
            host("grape", "203.0.113.40"),
            host("strong-b", "192.168.1.40"),
            host("grape-2", "203.0.113.41"),
            host("tino_charge", "10.0.0.7"),
        ]
    }

    #[test]
    fn empty_query_keeps_config_order() {
        assert_eq!(rank("", &hosts()), vec![0, 1, 2, 3]);
        assert_eq!(rank("  ", &hosts()), vec![0, 1, 2, 3]);
    }

    #[test]
    fn fuzzy_query_finds_scattered_letters_and_drops_the_rest() {
        // "stb" is not a substring of anything; only a fuzzy match finds strong-b.
        assert_eq!(rank("stb", &hosts()), vec![1]);
        assert_eq!(rank("tch", &hosts()), vec![3]);
    }

    #[test]
    fn address_is_searchable() {
        assert_eq!(rank("113.41", &hosts()), vec![2]);
    }

    #[test]
    fn better_match_ranks_first_regardless_of_config_order() {
        // Both match "strong"; the scattered one is listed first, the contiguous one wins.
        let hosts = vec![
            host("s-t-r-o-n-g", "10.0.0.1"),
            host("strong-b", "10.0.0.2"),
        ];
        assert_eq!(rank("strong", &hosts), vec![1, 0]);
    }

    #[test]
    fn an_empty_query_lists_recents_first_then_the_rest_in_order() {
        let hosts = hosts();
        let labels: Vec<String> = hosts.iter().map(|h| h.alias.clone()).collect();
        let aliases: Vec<&str> = hosts.iter().map(|h| h.alias.as_str()).collect();
        // newest first; "gone" no longer exists and is skipped
        let recent = ["tino_charge", "gone", "grape"];
        assert_eq!(order("", &labels, &aliases, &recent), vec![3, 0, 1, 2]);
        assert_eq!(order("", &labels, &aliases, &[]), vec![0, 1, 2, 3]);
        // A query ignores recency and ranks.
        assert_eq!(order("stb", &labels, &aliases, &recent), vec![1]);
    }

    #[test]
    fn layouts_match_by_name_and_by_leaf_and_follow_hosts_when_idle() {
        let host_names = ["grape", "db"];
        let host_labels = vec!["grape 203.0.113.40".into(), "db 10.0.0.1".into()];
        let layout_names = ["生产监控", "notes"];
        let layout_labels = vec!["生产监控 prod staging".into(), "notes local".into()];
        let recent = ["db"];
        assert_eq!(
            hits(
                "",
                &host_names,
                &host_labels,
                &layout_names,
                &layout_labels,
                &recent
            ),
            vec![Hit::Host(1), Hit::Host(0), Hit::Layout(0), Hit::Layout(1)]
        );
        assert_eq!(
            hits(
                "生产监控",
                &host_names,
                &host_labels,
                &layout_names,
                &layout_labels,
                &recent
            ),
            vec![Hit::Layout(0)]
        );
        assert_eq!(
            hits(
                "staging",
                &host_names,
                &host_labels,
                &layout_names,
                &layout_labels,
                &recent
            ),
            vec![Hit::Layout(0)]
        );
        // A host named for the query outranks a layout that only mentions it in a leaf.
        let host_names = ["prod"];
        let host_labels = vec!["prod 10.0.0.1".into()];
        let layout_names = ["ops"];
        let layout_labels = vec!["ops prod-backup".into()];
        assert_eq!(
            hits(
                "prod",
                &host_names,
                &host_labels,
                &layout_names,
                &layout_labels,
                &[]
            ),
            vec![Hit::Host(0), Hit::Layout(0)]
        );
    }
}
