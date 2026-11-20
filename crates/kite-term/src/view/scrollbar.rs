use std::time::{Duration, Instant};

use alacritty_terminal::term::TermMode;
use gpui::{
    App, AppContext as _, Bounds, Context, Empty, Entity, InteractiveElement as _, IntoElement,
    MouseButton, MouseUpEvent, Pixels, Render, StatefulInteractiveElement as _, Styled as _, Task,
    TouchPhase, Window, div, px,
};

use crate::terminal::Terminal;
use crate::view::TerminalView;

pub(crate) const SCROLLBAR_HIDE_DELAY_MS: u64 = 1200;
const TRACK_INSET_Y: f32 = 4.0;
const MIN_THUMB_H: f32 = 24.0;
const HIT_STRIP_W: f32 = 16.0;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollbarDrag {
    pub grab_offset_y: f32,
}

pub(crate) struct ScrollbarDragHandle;

pub(crate) struct ScrollbarDragGhost;

impl Render for ScrollbarDragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Default)]
pub(crate) struct ScrollbarState {
    pub visible: bool,
    pub hover: bool,
    pub drag: Option<ScrollbarDrag>,
    hide_task: Option<Task<()>>,
    pub remainder: f32,
    /// Fractional wheel lines for mouse-reporting and alternate scroll. Kept
    /// off `remainder`, which is the pixel shift painted between rows.
    report_remainder: f32,
    smooth_pending: f32,
    smooth_last_tick: Option<Instant>,
}

impl ScrollbarState {
    pub(crate) fn should_paint(&self) -> bool {
        self.visible || self.hover || self.drag.is_some()
    }

    pub(crate) fn active(&self) -> bool {
        self.hover || self.drag.is_some()
    }

    pub(crate) fn show(&mut self, cx: &mut Context<TerminalView>) {
        if !self.visible {
            self.visible = true;
            cx.notify();
        }
        self.schedule_hide(cx);
    }

    pub(crate) fn schedule_hide(&mut self, cx: &mut Context<TerminalView>) {
        self.hide_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(SCROLLBAR_HIDE_DELAY_MS))
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.scrollbar.drag.is_none() && !view.scrollbar.hover {
                    view.scrollbar.visible = false;
                    cx.notify();
                }
            });
        }));
    }

    pub(crate) fn drag_to(
        &mut self,
        y: f32,
        grab_offset_y: f32,
        m: ScrollbarMetrics,
        terminal: &Entity<Terminal>,
        cx: &mut Context<TerminalView>,
    ) {
        let history = terminal.read(cx).history_size();
        let target = m.offset_for_thumb_top(y - grab_offset_y, history);
        terminal.update(cx, |t, cx| {
            t.scroll_to_offset(target);
            cx.notify();
        });
        self.show(cx);
    }

    pub(crate) fn reset_smooth(&mut self) {
        self.remainder = 0.0;
        self.smooth_pending = 0.0;
        self.smooth_last_tick = None;
    }

    /// Whole lines to send for one wheel event in mouse-reporting or alternate
    /// scroll. Clears the painted smooth-scroll residue, and drops a stale
    /// fraction when a gesture starts or the wheel reverses.
    pub(crate) fn take_report_step(&mut self, lines: f32, phase: TouchPhase) -> i32 {
        self.reset_smooth();
        if phase == TouchPhase::Started || report_direction_flipped(self.report_remainder, lines) {
            self.report_remainder = 0.0;
        }
        self.report_remainder += lines;
        let step = self.report_remainder.trunc() as i32;
        self.report_remainder -= step as f32;
        step
    }

    fn advance_smooth_lines(
        &mut self,
        lines: f32,
        terminal: &Entity<Terminal>,
        notify: bool,
        cx: &mut Context<TerminalView>,
    ) {
        self.remainder += lines;
        let step = self.remainder.trunc() as i32;
        self.remainder -= step as f32;
        terminal.update(cx, |t, cx| {
            if step != 0 {
                t.scroll(step);
            }
            if t.display_offset() == 0 {
                self.remainder = self.remainder.max(0.0);
            }
            if t.display_offset() >= t.history_size() {
                self.remainder = self.remainder.min(0.0);
            }
            if notify {
                cx.notify();
            }
        });
        if notify {
            self.show(cx);
        } else {
            self.visible = true;
            self.hide_task = None;
        }
    }

    pub(crate) fn apply_smooth_lines(
        &mut self,
        lines: f32,
        terminal: &Entity<Terminal>,
        cx: &mut Context<TerminalView>,
    ) {
        self.advance_smooth_lines(lines, terminal, true, cx);
    }

    pub(crate) fn queue_smooth_lines(
        &mut self,
        lines: f32,
        window: &mut Window,
        cx: &mut Context<TerminalView>,
    ) {
        self.smooth_pending += lines;
        if self.smooth_last_tick.is_none() {
            self.smooth_last_tick = Some(Instant::now());
        }
        window.request_animation_frame();
        cx.notify();
    }

    pub(crate) fn step_smooth(
        &mut self,
        terminal: &Entity<Terminal>,
        window: &mut Window,
        cx: &mut Context<TerminalView>,
    ) {
        if self.smooth_pending.abs() < 0.001 {
            self.smooth_pending = 0.0;
            self.smooth_last_tick = None;
            return;
        }
        let now = Instant::now();
        let dt_ms = self
            .smooth_last_tick
            .replace(now)
            .map_or(8.0, |prev| {
                now.saturating_duration_since(prev).as_secs_f32() * 1000.0
            })
            .clamp(1.0, 50.0);
        let alpha = 1.0 - 0.72_f32.powf(dt_ms / 8.0);
        let rem = self.smooth_pending;
        let step = if rem.abs() < 0.03 { rem } else { rem * alpha };
        self.smooth_pending -= step;
        self.advance_smooth_lines(step, terminal, false, cx);
        if self.smooth_pending.abs() < 0.001 {
            self.smooth_pending = 0.0;
            self.smooth_last_tick = None;
            self.schedule_hide(cx);
        } else {
            window.request_animation_frame();
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollbarMetrics {
    pub track_top: f32,
    pub track_h: f32,
    pub thumb_top: f32,
    pub thumb_h: f32,
    pub thumb_x: f32,
    pub thumb_w: f32,
}

impl ScrollbarMetrics {
    pub(crate) fn contains_thumb_y(self, y: f32) -> bool {
        y >= self.thumb_top && y <= self.thumb_top + self.thumb_h
    }

    pub(crate) fn offset_for_thumb_top(self, thumb_top: f32, history: usize) -> usize {
        if history == 0 {
            return 0;
        }
        let scrollable = self.track_h - self.thumb_h;
        if scrollable <= 0.0 {
            return 0;
        }
        let frac = ((thumb_top - self.track_top) / scrollable).clamp(0.0, 1.0);
        ((1.0 - frac) * history as f32).round() as usize
    }
}

/// A fixed TUI viewport must not ride a fractional line. Alternate screen and
/// mouse reporting both own that viewport.
pub(crate) fn painted_sub_line(remainder: f32, mode: TermMode) -> f32 {
    if mode.intersects(TermMode::ALT_SCREEN | TermMode::MOUSE_MODE) {
        0.0
    } else {
        remainder
    }
}

fn report_direction_flipped(remainder: f32, lines: f32) -> bool {
    remainder != 0.0 && lines != 0.0 && (remainder > 0.0) != (lines > 0.0)
}

pub(crate) fn metrics(
    bounds: Bounds<Pixels>,
    rows: usize,
    history: usize,
    offset: usize,
    active: bool,
) -> Option<ScrollbarMetrics> {
    if history == 0 {
        return None;
    }
    let height = f32::from(bounds.size.height);
    let track_top = f32::from(bounds.origin.y) + TRACK_INSET_Y;
    let track_h = (height - 2.0 * TRACK_INSET_Y).max(MIN_THUMB_H);
    let visible_rows = rows.max(1) as f32;
    let total = history as f32 + visible_rows;
    let thumb_h = ((visible_rows / total) * track_h).clamp(MIN_THUMB_H, track_h);
    let frac = 1.0 - (offset as f32 / history as f32).clamp(0.0, 1.0);
    let thumb_top = track_top + frac * (track_h - thumb_h);
    let thumb_w = if active { 7.0 } else { 5.0 };
    let right = f32::from(bounds.origin.x + bounds.size.width);
    Some(ScrollbarMetrics {
        track_top,
        track_h,
        thumb_top,
        thumb_h,
        thumb_x: right - (thumb_w + 3.0),
        thumb_w,
    })
}

pub(crate) fn drag_move(view: &mut TerminalView, y: f32, cx: &mut Context<TerminalView>) {
    let Some(drag) = view.scrollbar.drag else {
        return;
    };
    if let Some(m) = view_metrics(view, cx) {
        view.scrollbar
            .drag_to(y, drag.grab_offset_y, m, &view.terminal, cx);
    }
}

pub(crate) fn view_metrics(view: &TerminalView, cx: &App) -> Option<ScrollbarMetrics> {
    let g = view.geometry?;
    let t = view.terminal.read(cx);
    metrics(
        g.bounds,
        g.rows as usize,
        t.history_size(),
        t.display_offset(),
        view.scrollbar.active(),
    )
}

pub(crate) fn render_strip(cx: &mut Context<TerminalView>) -> impl IntoElement + use<> {
    div()
        .id("terminal-scrollbar")
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w(px(HIT_STRIP_W))
        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            this.scrollbar.hover = *hovered;
            if *hovered {
                this.scrollbar.show(cx);
            } else {
                this.scrollbar.schedule_hide(cx);
            }
            cx.notify();
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(TerminalView::on_scrollbar_mouse_down),
        )
        .on_drag(ScrollbarDragHandle, |_, _, _, cx| {
            cx.stop_propagation();
            cx.new(|_| ScrollbarDragGhost)
        })
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _, cx| this.on_scrollbar_mouse_up(cx)),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _, cx| this.on_scrollbar_mouse_up(cx)),
        )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use alacritty_terminal::term::TermMode;
    use gpui::{TouchPhase, point, px, size};

    #[test]
    fn none_when_no_history() {
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(400.0)));
        assert!(metrics(bounds, 24, 0, 0, false).is_none());
    }

    #[test]
    fn thumb_drag_maps_top_and_bottom_to_history_bounds() {
        let bounds = Bounds::new(point(px(10.0), px(20.0)), size(px(800.0), px(400.0)));
        let m = metrics(bounds, 24, 200, 0, true).expect("metrics");
        assert_eq!(m.offset_for_thumb_top(m.track_top, 200), 200);
        assert_eq!(
            m.offset_for_thumb_top(m.track_top + m.track_h - m.thumb_h, 200),
            0
        );
        assert_eq!(m.thumb_w, 7.0);
    }

    #[test]
    fn report_steps_do_not_move_the_painted_offset_and_match_scroll_direction() {
        use alacritty_terminal::index::{Column, Line, Point};
        use gpui::{ScrollDelta, ScrollWheelEvent};

        use crate::mappings::mouse::scroll_report;

        let mut bar = ScrollbarState {
            remainder: 0.5,
            smooth_pending: 1.0,
            ..ScrollbarState::default()
        };
        assert_eq!(bar.take_report_step(0.25, TouchPhase::Moved), 0);
        assert_eq!(bar.remainder, 0.0);
        assert_eq!(bar.smooth_pending, 0.0);
        assert_eq!(bar.report_remainder, 0.25);
        assert_eq!(painted_sub_line(bar.remainder, TermMode::empty()), 0.0);

        let step = bar.take_report_step(0.75, TouchPhase::Moved);
        assert_eq!(step, 1);
        assert_eq!(bar.remainder, 0.0);
        assert_eq!(bar.report_remainder, 0.0);

        let wheel_down = ScrollWheelEvent {
            delta: ScrollDelta::Lines(point(0.0, -1.0)),
            touch_phase: TouchPhase::Moved,
            ..ScrollWheelEvent::default()
        };
        let up: Vec<Vec<u8>> = scroll_report(
            Point::new(Line(0), Column(0)),
            step,
            &wheel_down,
            TermMode::MOUSE_REPORT_CLICK,
        )
        .expect("mouse mode reports the wheel")
        .collect();
        assert_eq!(up.len(), 1);
        assert_eq!(up[0][3], 32 + 64);

        bar.report_remainder = 0.5;
        assert_eq!(bar.take_report_step(-0.25, TouchPhase::Moved), 0);
        assert_eq!(bar.report_remainder, -0.25);
        assert_eq!(bar.remainder, 0.0);

        bar.report_remainder = 0.75;
        let step = bar.take_report_step(-1.5, TouchPhase::Moved);
        assert_eq!(step, -1);
        assert_eq!(bar.report_remainder, -0.5);
        let wheel_up = ScrollWheelEvent {
            delta: ScrollDelta::Lines(point(0.0, 1.0)),
            touch_phase: TouchPhase::Moved,
            ..ScrollWheelEvent::default()
        };
        let down: Vec<Vec<u8>> = scroll_report(
            Point::new(Line(0), Column(0)),
            step,
            &wheel_up,
            TermMode::MOUSE_REPORT_CLICK,
        )
        .expect("mouse mode reports the wheel")
        .collect();
        assert_eq!(down[0][3], 32 + 65);

        bar.report_remainder = 0.75;
        assert_eq!(bar.take_report_step(0.25, TouchPhase::Started), 0);
        assert_eq!(bar.report_remainder, 0.25);
        assert_eq!(bar.remainder, 0.0);

        bar.report_remainder = 0.75;
        assert_eq!(bar.take_report_step(-1.5, TouchPhase::Started), -1);
        assert_eq!(bar.report_remainder, -0.5);
    }

    #[test]
    fn sub_line_offset_is_zero_while_alt_screen_or_mouse_mode_is_active() {
        let leftover = 0.5;
        for mode in [
            TermMode::ALT_SCREEN,
            TermMode::MOUSE_MODE,
            TermMode::MOUSE_REPORT_CLICK,
            TermMode::MOUSE_MOTION,
            TermMode::MOUSE_DRAG,
            TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL,
        ] {
            assert_eq!(painted_sub_line(leftover, mode), 0.0);
            assert_eq!(painted_sub_line(-0.25, mode), 0.0);
        }
        assert_eq!(painted_sub_line(leftover, TermMode::SHOW_CURSOR), leftover);
        assert_eq!(painted_sub_line(leftover, TermMode::empty()), leftover);
    }
}
