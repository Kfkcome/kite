use std::time::Duration;

use gpui::{
    App, AppContext as _, Bounds, Context, Empty, Entity, InteractiveElement as _, IntoElement,
    MouseButton, MouseUpEvent, Pixels, Point, Render, StatefulInteractiveElement as _, Styled as _,
    Task, Window, div, px,
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
    smooth_pending: f32,
    smooth_task: Option<Task<()>>,
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
        self.smooth_task = None;
    }

    pub(crate) fn apply_smooth_lines(
        &mut self,
        lines: f32,
        terminal: &Entity<Terminal>,
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
            cx.notify();
        });
        self.show(cx);
    }

    pub(crate) fn queue_smooth_lines(&mut self, lines: f32, cx: &mut Context<TerminalView>) {
        self.smooth_pending += lines;
        if self.smooth_task.is_some() {
            return;
        }
        self.smooth_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(8))
                    .await;
                let keep_going = this
                    .update(cx, |view, cx| {
                        let rem = view.scrollbar.smooth_pending;
                        if rem.abs() < 0.001 {
                            view.scrollbar.smooth_pending = 0.0;
                            view.scrollbar.smooth_task = None;
                            return false;
                        }
                        let step = if rem.abs() < 0.03 { rem } else { rem * 0.28 };
                        view.scrollbar.smooth_pending -= step;
                        let terminal = view.terminal.clone();
                        view.scrollbar.apply_smooth_lines(step, &terminal, cx);
                        if view.scrollbar.smooth_pending.abs() < 0.001 {
                            view.scrollbar.smooth_pending = 0.0;
                            view.scrollbar.smooth_task = None;
                            false
                        } else {
                            true
                        }
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        }));
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
    pub hit_left: f32,
    pub hit_right: f32,
    pub hit_top: f32,
    pub hit_bottom: f32,
}

impl ScrollbarMetrics {
    pub(crate) fn contains(self, pos: Point<Pixels>) -> bool {
        let x = f32::from(pos.x);
        let y = f32::from(pos.y);
        x >= self.hit_left && x <= self.hit_right && y >= self.hit_top && y <= self.hit_bottom
    }

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
        hit_left: right - HIT_STRIP_W,
        hit_right: right,
        hit_top: f32::from(bounds.origin.y),
        hit_bottom: f32::from(bounds.origin.y) + height,
    })
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
mod tests {
    use super::*;
    use gpui::{point, px, size};

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
}
