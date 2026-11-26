// Adapted from zeron crates/ui/src/motion.rs (CubicBezier) (MIT) and seed-design
// (daangn/seed-design, MIT) for snackbar timing (4 s, pause while held, queued).
//! Floating notifications anchored at the bottom right above the status bar, one at a time,
//! styled with the active [`Theme`] popup surface.

use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, deferred, div, px,
};

use super::Shell;
use crate::hover::{Accessible as _, HoverFade as _};

/// seed-design: "기본적으로 4초 동안 표시됩니다."
pub const DEFAULT_DURATION: Duration = Duration::from_secs(4);
/// `--seed-duration-d3`, `--seed-timing-function-enter`.
const ENTER: Duration = Duration::from_millis(150);
/// `--seed-duration-d2`, `--seed-timing-function-exit`.
const EXIT: Duration = Duration::from_millis(100);
const ENTER_CURVE: CubicBezier = CubicBezier::new(0.0, 0.0, 0.15, 1.0);
const EXIT_CURVE: CubicBezier = CubicBezier::new(0.35, 0.0, 1.0, 1.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Default,
    Positive,
    Critical,
}

type Action = Rc<dyn Fn(&mut Shell, &mut Window, &mut Context<Shell>)>;

#[derive(Clone)]
pub(crate) struct Toast {
    kind: Kind,
    message: SharedString,
    action: Option<(SharedString, Action)>,
    duration: Duration,
    /// Follows a transfer: stays until [`Shell::end_progress_toast`], its text replaced in place.
    progress: bool,
    /// Host alias for a background disconnect notice, pruned when reconnected, viewed, or closed.
    disconnect_alias: Option<SharedString>,
}

impl Toast {
    pub(crate) fn new(kind: Kind, message: impl Into<SharedString>) -> Self {
        Self {
            kind,
            message: message.into(),
            action: None,
            duration: DEFAULT_DURATION,
            progress: false,
            disconnect_alias: None,
        }
    }

    /// Stays up for as long as the transfer it reports runs.
    pub(crate) fn lasting(mut self) -> Self {
        self.duration = Duration::from_secs(3600);
        self.progress = true;
        self
    }

    /// Marks this toast as reporting a background disconnect for `alias`.
    pub(crate) fn for_disconnect(mut self, alias: impl Into<SharedString>) -> Self {
        self.disconnect_alias = Some(alias.into());
        self
    }

    /// One short, specific action ("Undo", "Show"), as seed-design allows at most one.
    pub(crate) fn action(
        mut self,
        label: impl Into<SharedString>,
        run: impl Fn(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(run)));
        self
    }
}

/// Where a shown snackbar is in its life, from its own clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Phase {
    Entering(f32),
    Shown,
    Exiting(f32),
    Gone,
}

/// The timing of the snackbar on screen. Time spent under the pointer does not count towards
/// its duration.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Clock {
    started: Instant,
    duration: Duration,
    paused: Duration,
    hovered_since: Option<Instant>,
    /// Set when it starts leaving, early (dismissed) or on time.
    leaving_at: Option<Instant>,
}

impl Clock {
    pub(crate) fn new(now: Instant, duration: Duration) -> Self {
        Self {
            started: now,
            duration,
            paused: Duration::ZERO,
            hovered_since: None,
            leaving_at: None,
        }
    }

    pub(crate) fn hover(&mut self, hovered: bool, now: Instant) {
        match (hovered, self.hovered_since) {
            (true, None) => self.hovered_since = Some(now),
            (false, Some(since)) => {
                self.paused += now.saturating_duration_since(since);
                self.hovered_since = None;
            }
            _ => {}
        }
    }

    pub(crate) fn dismiss(&mut self, now: Instant) {
        self.leaving_at.get_or_insert(now);
    }

    fn visible_for(&self, now: Instant) -> Duration {
        let held = self
            .hovered_since
            .map_or(Duration::ZERO, |since| now.saturating_duration_since(since));
        now.saturating_duration_since(self.started)
            .saturating_sub(self.paused + held)
    }

    pub(crate) fn phase(&mut self, now: Instant) -> Phase {
        if self.leaving_at.is_none() && self.visible_for(now) >= ENTER + self.duration {
            self.leaving_at = Some(now);
        }
        if let Some(at) = self.leaving_at {
            let t = now.saturating_duration_since(at).as_secs_f32() / EXIT.as_secs_f32();
            return if t >= 1.0 {
                Phase::Gone
            } else {
                Phase::Exiting(EXIT_CURVE.eval(t))
            };
        }
        let t = now.saturating_duration_since(self.started).as_secs_f32() / ENTER.as_secs_f32();
        if t < 1.0 {
            Phase::Entering(ENTER_CURVE.eval(t))
        } else {
            Phase::Shown
        }
    }
}

pub(super) struct Toasts {
    queue: VecDeque<Toast>,
    showing: Option<(Toast, Clock)>,
}

impl Toasts {
    pub(super) fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            showing: None,
        }
    }

    pub(crate) fn has_disconnect(&self, alias: &str) -> bool {
        let in_showing = self.showing.as_ref().is_some_and(|(t, c)| {
            c.leaving_at.is_none() && t.disconnect_alias.as_deref() == Some(alias)
        });
        in_showing
            || self
                .queue
                .iter()
                .any(|t| t.disconnect_alias.as_deref() == Some(alias))
    }

    pub(crate) fn has_any_disconnect(&self) -> bool {
        self.showing
            .as_ref()
            .is_some_and(|(t, c)| c.leaving_at.is_none() && t.disconnect_alias.is_some())
            || self.queue.iter().any(|t| t.disconnect_alias.is_some())
    }

    /// Removes queued disconnect toasts and dismisses the active one when `keep(alias)` is false.
    /// Returns `true` if anything changed.
    pub(crate) fn prune_disconnects(
        &mut self,
        now: Instant,
        mut keep: impl FnMut(&str) -> bool,
    ) -> bool {
        let before = self.queue.len();
        self.queue.retain(|t| match t.disconnect_alias.as_deref() {
            Some(alias) => keep(alias),
            None => true,
        });
        let mut changed = self.queue.len() != before;
        if let Some((toast, clock)) = self.showing.as_mut()
            && clock.leaving_at.is_none()
            && let Some(alias) = toast.disconnect_alias.as_deref()
            && !keep(alias)
        {
            clock.dismiss(now);
            changed = true;
        }
        changed
    }
}

impl Shell {
    /// Shows a snackbar now, or after the current one: one at a time, in order.
    pub(crate) fn toast(&mut self, toast: Toast, cx: &mut Context<Self>) {
        self.toasts.queue.push_back(toast);
        if self.toasts.showing.is_none() {
            self.next_toast(cx);
        }
    }

    /// Notifies once per host alias when a connected background pane disconnects.
    pub(crate) fn disconnect_toast(&mut self, alias: &str, cx: &mut Context<Self>) {
        if self.toasts.has_disconnect(alias) {
            return;
        }
        let target_alias = alias.to_owned();
        self.toast(
            Toast::new(
                Kind::Critical,
                crate::i18n::fill("{} disconnected", &[alias]),
            )
            .for_disconnect(alias.to_owned())
            .action(crate::i18n::tr("Show"), move |shell, window, cx| {
                let disconnected = shell.tabs.iter().enumerate().find_map(|(ix, tab)| {
                    tab.panes.iter().find_map(|pane| {
                        (pane.alias == target_alias
                            && pane.session.read(cx).status != crate::session::Status::Connected)
                            .then_some((ix, pane.id))
                    })
                });
                let hit = disconnected.or_else(|| {
                    shell.tabs.iter().enumerate().find_map(|(ix, tab)| {
                        tab.panes
                            .iter()
                            .find_map(|pane| (pane.alias == target_alias).then_some((ix, pane.id)))
                    })
                });
                if let Some((ix, pane_id)) = hit {
                    shell.show_pane(ix, pane_id, window, cx);
                }
            }),
            cx,
        );
    }

    /// Dismisses or drops disconnect toasts whose host has reconnected, is active, or was closed.
    pub(crate) fn reconcile_disconnect_toasts(&mut self, cx: &mut Context<Self>) {
        if !self.toasts.has_any_disconnect() {
            return;
        }
        let tabs = &self.tabs;
        let active = self.active;
        let settings_open = self.settings_page.is_some();
        let changed = self.toasts.prune_disconnects(Instant::now(), |alias| {
            tabs.iter().enumerate().any(|(ix, tab)| {
                (ix != active || settings_open)
                    && tab.panes.iter().any(|p| {
                        p.alias == alias
                            && p.session.read(cx).status != crate::session::Status::Connected
                    })
            })
        });
        if changed {
            cx.notify();
        }
    }

    /// Shows or updates the one snackbar that follows a transfer ("Downloading a.txt… 40%").
    pub(crate) fn progress_toast(
        &mut self,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let message = message.into();
        if let Some((toast, _)) = self.toasts.showing.as_mut().filter(|(t, _)| t.progress) {
            toast.message = message;
            return cx.notify();
        }
        if let Some(queued) = self.toasts.queue.iter_mut().find(|t| t.progress) {
            queued.message = message;
            return;
        }
        self.toast(Toast::new(Kind::Default, message).lasting(), cx);
    }

    /// Takes the transfer snackbar down, so its result can be shown next.
    pub(crate) fn end_progress_toast(&mut self, cx: &mut Context<Self>) {
        self.toasts.queue.retain(|t| !t.progress);
        if let Some((_, clock)) = self.toasts.showing.as_mut().filter(|(t, _)| t.progress) {
            clock.dismiss(Instant::now());
        }
        cx.notify();
    }

    pub(crate) fn notify_toast(
        &mut self,
        kind: Kind,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.toast(Toast::new(kind, message), cx);
    }

    fn next_toast(&mut self, cx: &mut Context<Self>) {
        let Some(toast) = self.toasts.queue.pop_front() else {
            self.toasts.showing = None;
            return cx.notify();
        };
        let clock = Clock::new(Instant::now(), toast.duration);
        self.toasts.showing = Some((toast, clock));
        // The clock decides; this task only wakes the view so the phase is read again.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let gone = this
                    .update(cx, |s, cx| {
                        let gone = match s.toasts.showing.as_mut() {
                            Some((_, clock)) => clock.phase(Instant::now()) == Phase::Gone,
                            None => true,
                        };
                        if gone {
                            s.next_toast(cx);
                        } else {
                            cx.notify();
                        }
                        gone
                    })
                    .unwrap_or(true);
                if gone {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn render_toast(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let now = Instant::now();
        let (toast, clock) = self.toasts.showing.as_mut()?;
        let phase = clock.phase(now);
        let (opacity, lift) = match phase {
            Phase::Gone => return None,
            Phase::Entering(p) => {
                window.request_animation_frame();
                (p, (p - 1.0) * 6.0)
            }
            Phase::Shown => (1.0, 0.0),
            Phase::Exiting(p) => {
                window.request_animation_frame();
                (1.0 - p, -p * 6.0)
            }
        };
        let toast = toast.clone();
        let t = self.theme;
        let dot_color = match toast.kind {
            Kind::Default => t.accent,
            Kind::Positive => t.success,
            Kind::Critical => t.danger,
        };
        let viewport = window.viewport_size();
        let card = div()
            .id("snackbar")
            .occlude()
            .opacity(opacity)
            .min_w(px(200.))
            .max_w(px(
                360.0_f32.min((f32::from(viewport.width) - 24.0).max(160.0))
            ))
            .pl(px(12.))
            .pr(px(6.))
            .py(px(8.))
            .rounded(px(10.))
            .border_1()
            .border_color(t.border)
            .bg(t.popup)
            .shadow_lg()
            .flex()
            .items_center()
            .gap(px(8.))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_hover(cx.listener(|s, hovered: &bool, _, cx| {
                if let Some((_, clock)) = s.toasts.showing.as_mut() {
                    clock.hover(*hovered, Instant::now());
                }
                cx.notify();
            }))
            .child(div().flex_none().size(px(7.)).rounded_full().bg(dot_color))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(t.text)
                    .child(toast.message.clone()),
            )
            .when_some(toast.action.clone(), |el, (label, run)| {
                el.child(
                    div()
                        .id("snackbar-action")
                        .flex_none()
                        .px(px(8.))
                        .py(px(3.))
                        .rounded(px(5.))
                        .cursor_pointer()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.accent)
                        .hover_fade(
                            "snackbar-action",
                            t.accent.opacity(0.14),
                            t.accent.opacity(0.24),
                        )
                        .on_click(cx.listener(move |s, _, window, cx| {
                            run(s, window, cx);
                            if let Some((_, clock)) = s.toasts.showing.as_mut() {
                                clock.dismiss(Instant::now());
                            }
                            cx.notify();
                        }))
                        .child(label),
                )
            })
            .child(
                div()
                    .id("snackbar-close")
                    .size(px(22.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(5.))
                    .cursor_pointer()
                    .hover_fade("snackbar-close", gpui::transparent_black(), t.row_hover)
                    .icon_button(crate::i18n::tr("Close"), &t)
                    .on_click(cx.listener(|s, _, _, cx| {
                        if let Some((_, clock)) = s.toasts.showing.as_mut() {
                            clock.dismiss(Instant::now());
                        }
                        cx.notify();
                    }))
                    .child(
                        crate::icons::icon(crate::icons::CLOSE)
                            .size(px(14.))
                            .text_color(crate::hover::blend("snackbar-close", t.muted, t.text)),
                    ),
            );
        let base_bottom = if self.settings_page.is_some() {
            14.0
        } else {
            crate::statusline::HEIGHT + 10.0
        };
        Some(
            deferred(
                div()
                    .absolute()
                    .right(px(12.))
                    .bottom(px(base_bottom + lift))
                    .child(card),
            )
            .priority(3)
            .into_any_element(),
        )
    }
}

/// CSS `cubic-bezier()` evaluated exactly (zeron `motion::CubicBezier`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct CubicBezier {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
}

impl CubicBezier {
    pub(crate) const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    fn coefficients(a: f32, b: f32) -> (f32, f32, f32) {
        let c = 3.0 * a;
        let bb = 3.0 * (b - a) - c;
        (1.0 - c - bb, bb, c)
    }

    fn sample(a: f32, b: f32, t: f32) -> f32 {
        let (ca, cb, cc) = Self::coefficients(a, b);
        ((ca * t + cb) * t + cc) * t
    }

    fn solve_t(&self, x: f32) -> f32 {
        let mut t = x;
        for _ in 0..8 {
            let err = Self::sample(self.x1, self.x2, t) - x;
            if err.abs() < 1e-6 {
                return t;
            }
            let (a, b, c) = Self::coefficients(self.x1, self.x2);
            let d = (3.0 * a * t + 2.0 * b) * t + c;
            if d.abs() < 1e-6 {
                break;
            }
            t -= err / d;
        }
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..32 {
            let mid = (lo + hi) / 2.0;
            if Self::sample(self.x1, self.x2, mid) < x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (lo + hi) / 2.0
    }

    pub(crate) fn eval(&self, x: f32) -> f32 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        Self::sample(self.y1, self.y2, self.solve_t(x)).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_snackbar_enters_stays_four_seconds_then_leaves() {
        let t0 = Instant::now();
        let mut c = Clock::new(t0, DEFAULT_DURATION);
        assert!(matches!(c.phase(t0 + ms(50)), Phase::Entering(p) if p > 0.0 && p < 1.0));
        assert_eq!(c.phase(t0 + ms(2000)), Phase::Shown);
        assert_eq!(c.phase(t0 + ENTER + DEFAULT_DURATION - ms(1)), Phase::Shown);
        assert!(matches!(
            c.phase(t0 + ENTER + DEFAULT_DURATION + ms(10)),
            Phase::Exiting(_)
        ));
        assert_eq!(
            c.phase(t0 + ENTER + DEFAULT_DURATION + ms(200)),
            Phase::Gone
        );
    }

    #[test]
    fn time_under_the_pointer_does_not_count() {
        let t0 = Instant::now();
        let mut c = Clock::new(t0, DEFAULT_DURATION);
        c.hover(true, t0 + ms(1000));
        // Held for ten seconds: still shown.
        assert_eq!(c.phase(t0 + ms(11_000)), Phase::Shown);
        c.hover(false, t0 + ms(11_000));
        // 1 s before the hold + 3.1 s after it is the full 4 s + enter.
        assert_eq!(c.phase(t0 + ms(14_000)), Phase::Shown);
        assert!(matches!(c.phase(t0 + ms(14_200)), Phase::Exiting(_)));
    }

    #[test]
    fn dismiss_leaves_at_once() {
        let t0 = Instant::now();
        let mut c = Clock::new(t0, DEFAULT_DURATION);
        c.dismiss(t0 + ms(500));
        assert!(matches!(c.phase(t0 + ms(550)), Phase::Exiting(_)));
        assert_eq!(c.phase(t0 + ms(650)), Phase::Gone);
    }

    #[test]
    fn curves_match_css() {
        // cubic-bezier(0, 0, .15, 1) is front-loaded; (.35, 0, 1, 1) is back-loaded.
        assert!(ENTER_CURVE.eval(0.5) > 0.8);
        assert!(EXIT_CURVE.eval(0.5) < 0.4);
        assert_eq!(ENTER_CURVE.eval(1.0), 1.0);
    }

    #[test]
    fn has_disconnect_checks_active_non_leaving_and_queued_only() {
        let t0 = Instant::now();
        let mut toasts = Toasts::new();
        toasts.queue.push_back(Toast::new(Kind::Default, "Synced"));
        toasts
            .queue
            .push_back(Toast::new(Kind::Critical, "prod disconnected").for_disconnect("prod"));
        assert!(toasts.has_disconnect("prod"));
        assert!(!toasts.has_disconnect("staging"));
        assert!(toasts.has_any_disconnect());

        let active = toasts.queue.pop_back().unwrap();
        let mut clock = Clock::new(t0, DEFAULT_DURATION);
        toasts.showing = Some((active, clock));
        assert!(toasts.has_disconnect("prod"));

        clock.dismiss(t0 + ms(100));
        toasts.showing.as_mut().unwrap().1 = clock;
        assert!(!toasts.has_disconnect("prod"));
        assert!(!toasts.has_any_disconnect());
    }

    #[test]
    fn prune_disconnects_dismisses_resolved_and_keeps_unrelated_toasts() {
        let t0 = Instant::now();
        let mut toasts = Toasts::new();
        toasts.showing = Some((
            Toast::new(Kind::Critical, "prod disconnected").for_disconnect("prod"),
            Clock::new(t0, DEFAULT_DURATION),
        ));
        toasts.queue.push_back(Toast::new(Kind::Positive, "Saved"));
        toasts.queue.push_back(
            Toast::new(Kind::Critical, "staging disconnected").for_disconnect("staging"),
        );
        toasts
            .queue
            .push_back(Toast::new(Kind::Critical, "db disconnected").for_disconnect("db"));

        // `prod` and `staging` recovered, `db` is still disconnected in the background.
        let changed = toasts.prune_disconnects(t0 + ms(500), |alias| alias == "db");
        assert!(changed);
        assert!(matches!(
            toasts.showing.as_mut().unwrap().1.phase(t0 + ms(520)),
            Phase::Exiting(_)
        ));
        let remaining: Vec<&str> = toasts.queue.iter().map(|t| t.message.as_ref()).collect();
        assert_eq!(remaining, ["Saved", "db disconnected"]);

        // Calling again with no further changes is a no-op and never touches normal toasts.
        assert!(!toasts.prune_disconnects(t0 + ms(530), |alias| alias == "db"));
    }
}
