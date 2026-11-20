// Adapted from zeron crates/ui/src/terminal/view.rs (MIT) and zed
// crates/terminal_view/src/terminal_element.rs (GPL-3.0-or-later); box-drawing
// pass follows gpui-terminal src/render.rs (MIT OR Apache-2.0).
//
//! The grid-painting element. Measures cell metrics from the real mono font,
//! reports the resulting grid to the owning [`TerminalView`], and paints:
//! background quads for non-default cells, the selection wash, one shaped
//! segment per run of same-width glyphs, programmatic box-drawing, and the
//! cursor.

use std::sync::Arc;

use alacritty_terminal::vte::ansi::CursorShape;
use gpui::{
    App, Bounds, Entity, GlobalElementId, Hsla, LayoutId, PaintQuad, Pixels, SharedString, Style,
    TextRun, Window, fill, font, outline, point, px, relative, size,
};

use crate::box_drawing;
use crate::config::TerminalTheme;
use crate::row_cache::{BoxCell, RowShaped};
use crate::search::SearchMark;
use crate::snapshot::{CellColor, CellSnapshot, CursorSnapshot};
use crate::view::{GridGeometry, TERM_PADDING, TerminalView};

pub struct TerminalElement {
    view: Entity<TerminalView>,
    focused: bool,
}

impl TerminalElement {
    pub fn new(view: Entity<TerminalView>, focused: bool) -> Self {
        Self { view, focused }
    }
}

pub struct TerminalPrepaint {
    bg_quads: Vec<PaintQuad>,
    /// Painted after `bg_quads`, before glyphs: it tints a cell's own
    /// background and must not wash out the text it highlights.
    sel_quads: Vec<PaintQuad>,
    /// Per row, cached shaped segments and box-drawing cells.
    lines: Vec<Arc<RowShaped>>,
    start_row: i32,
    grid_rows: usize,
    cell_w: Pixels,
    line_h: Pixels,
    origin: gpui::Point<Pixels>,
    cursor_under: Option<PaintQuad>,
    cursor_over: Option<PaintQuad>,
    /// Underline under the hovered link, painted over the glyphs.
    link_quads: Vec<PaintQuad>,
}

impl gpui::IntoElement for TerminalElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = TerminalPrepaint;

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = self.view.read(cx).terminal.read(cx).theme().clone();
        let mono = mono_font(&theme);
        let font_size = px(theme.font_size);
        // Font probe: measure the resolved font's real advance so cols/rows
        // track glyph metrics, not a guessed aspect ratio.
        let font_id = window.text_system().resolve_font(&mono);
        let cell_w = window
            .text_system()
            .em_advance(font_id, font_size)
            .unwrap_or(px(theme.font_size * 0.6));
        let nominal = (theme.font_size * theme.line_height_ratio).round().max(1.0);

        let inner_w = f32::from(bounds.size.width) - 2.0 * TERM_PADDING;
        // Top inset only. The line box then grows so the rows reach the bottom
        // edge. A reserved bottom inset is not a cell, and with a translucent
        // terminal the wallpaper shows through it as a blank band.
        let avail_h = f32::from(bounds.size.height) - TERM_PADDING;
        let cols = ((inner_w / f32::from(cell_w)).floor() as i64).clamp(2, 1000) as u16;
        let (rows, filled_h) = filled_rows(avail_h, nominal);
        let line_h = px(filled_h);
        let origin = point(
            bounds.left() + px(TERM_PADDING),
            bounds.top() + px(TERM_PADDING),
        );

        // Report the measured grid before snapshotting so this frame paints
        // the resized grid. The view is not borrowed during prepaint.
        let (terminal, mut cache) = self.view.update(cx, |view, cx| {
            view.on_grid_metrics(
                GridGeometry {
                    bounds,
                    origin,
                    cell_w: f32::from(cell_w),
                    line_h: f32::from(line_h),
                    cols,
                    rows,
                },
                cx,
            );
            (view.terminal.clone(), std::mem::take(&mut view.row_cache))
        });
        let link_segments = self
            .view
            .read(cx)
            .hover_link()
            .map(|link| link.segments.clone())
            .unwrap_or_default();
        // A focused blinking cursor is dark in its off phase; an unfocused
        // one is a steady hollow box.
        let cursor_on = !self.focused || self.view.read(cx).cursor_on();
        let sub_line = self.view.read(cx).sub_line_offset();
        let origin = point(origin.x, origin.y + px(sub_line * f32::from(line_h)));

        let mut bg_quads = Vec::new();
        let mut sel_quads = Vec::new();
        let block_cursor_col;
        let cursor;
        let start_row;
        let mut lines;
        let mut cursor_wide = false;

        {
            let t = terminal.read(cx);
            cursor = t.cursor().filter(|_| cursor_on);
            block_cursor_col =
                cursor.filter(|c| self.focused && matches!(c.shape, CursorShape::Block));
            let viewport = t.scroll_viewport(sub_line);
            start_row = viewport.start_row;
            let row_count = (viewport.end_row - viewport.start_row).max(0) as usize;
            lines = Vec::with_capacity(row_count);
            let mut row_buf = std::mem::take(&mut cache.row_buf);
            cache.begin_frame(&theme, &mono, cols as usize, window.scale_factor());

            for row_pos in viewport.start_row..viewport.end_row {
                t.fill_scroll_row(&viewport, row_pos, &mut row_buf);
                let row = &row_buf;
                if cursor.is_some_and(|c| c.row as i32 == row_pos)
                    && let Some(c) = cursor
                {
                    cursor_wide = row.get(c.col).is_some_and(|cell| cell.wide);
                }
                let y = origin.y + line_h * row_pos as f32;
                let quad_at = |start: usize, end: usize, color: Hsla| {
                    fill(
                        Bounds::new(
                            point(origin.x + cell_w * start as f32, y),
                            size(cell_w * (end - start) as f32, line_h),
                        ),
                        color,
                    )
                };
                // Selected and found runs, one quad per contiguous span.
                for (start, end) in spans(row, |cell| cell.selected) {
                    sel_quads.push(quad_at(start, end, theme.selection));
                }
                for (start, end) in spans(row, |cell| cell.search == SearchMark::Match) {
                    sel_quads.push(quad_at(start, end, theme.search_match));
                }
                for (start, end) in spans(row, |cell| cell.search == SearchMark::Current) {
                    sel_quads.push(quad_at(start, end, theme.search_current));
                }
                // Merge consecutive non-default background cells into quads.
                let mut run_start: Option<(usize, Hsla)> = None;
                for (col, color) in row
                    .iter()
                    .map(|cell| cell.display_colors().1)
                    .chain(std::iter::once(CellColor::Background))
                    .enumerate()
                {
                    let paint = match color {
                        CellColor::Background => None,
                        other => Some(theme.resolve(other)),
                    };
                    match (&run_start, paint) {
                        (None, Some(color)) => run_start = Some((col, color)),
                        (Some((start, current)), next) if next != Some(*current) => {
                            bg_quads.push(quad_at(*start, col, *current));
                            run_start = next.map(|color| (col, color));
                        }
                        _ => {}
                    }
                }
                let cursor_col = block_cursor_col
                    .filter(|c| c.row as i32 == row_pos)
                    .map(|c| c.col);
                lines.push(cache.get_or_shape(row, &theme, font_size, cursor_col, window));
            }
            cache.row_buf = row_buf;
        }
        cache.end_frame();
        self.view.update(cx, |view, _| view.row_cache = cache);

        let (cursor_under, cursor_over) = match cursor {
            Some(c) => cursor_quads(
                c,
                cursor_wide,
                &theme,
                self.focused,
                origin,
                cell_w,
                line_h,
            ),
            None => (None, None),
        };

        let link_quads = link_segments
            .iter()
            .map(|&(row, start, end)| {
                fill(
                    Bounds::new(
                        point(
                            origin.x + cell_w * start as f32,
                            origin.y + line_h * row as f32 + line_h - px(2.0),
                        ),
                        size(cell_w * (end - start) as f32, px(1.0)),
                    ),
                    theme.foreground,
                )
            })
            .collect();

        TerminalPrepaint {
            bg_quads,
            sel_quads,
            lines,
            start_row,
            grid_rows: rows as usize,
            cell_w,
            line_h,
            origin,
            cursor_under,
            cursor_over,
            link_quads,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _state: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let view = self.view.clone();
        view.read(cx).attach_input(bounds, view.clone(), window, cx);
        let (cell_w, line_h, origin) = (prepaint.cell_w, prepaint.line_h, prepaint.origin);
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            for quad in prepaint.bg_quads.drain(..) {
                window.paint_quad(quad);
            }
            for quad in prepaint.sel_quads.drain(..) {
                window.paint_quad(quad);
            }
            if let Some(quad) = prepaint.cursor_under.take() {
                window.paint_quad(quad);
            }
            let box_origin = point(origin.x, origin.y + line_h * prepaint.start_row as f32);
            for (ix, row_shaped) in prepaint.lines.iter().enumerate() {
                let y = origin.y + line_h * (prepaint.start_row + ix as i32) as f32;
                for (col, line) in &row_shaped.segments {
                    let _ = line.paint(
                        point(origin.x + cell_w * *col as f32, y),
                        line_h,
                        gpui::TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }
                paint_box_row(&row_shaped.boxes, box_origin, ix, cell_w, line_h, window);
            }
            for quad in prepaint.link_quads.drain(..) {
                window.paint_quad(quad);
            }
            if let Some(quad) = prepaint.cursor_over.take() {
                window.paint_quad(quad);
            }
            paint_ime_preedit(&self.view, origin, cell_w, line_h, window, cx);
            paint_scrollbar(&self.view, bounds, prepaint.grid_rows, window, cx);
        });
    }
}

/// The uncommitted input-method string, drawn over the cursor cell. It is not
/// on the grid: the remote has not been sent it yet.
fn paint_ime_preedit(
    view: &Entity<TerminalView>,
    origin: gpui::Point<Pixels>,
    cell_w: Pixels,
    line_h: Pixels,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(text) = view.read(cx).ime_preedit().map(str::to_string) else {
        return;
    };
    let Some(cursor) = view.read(cx).terminal.read(cx).cursor() else {
        return;
    };
    let theme = view.read(cx).terminal.read(cx).theme().clone();
    let run = TextRun {
        len: text.len(),
        font: mono_font(&theme),
        color: theme.foreground,
        background_color: None,
        underline: Some(gpui::UnderlineStyle {
            color: Some(theme.foreground),
            thickness: px(1.0),
            wavy: false,
        }),
        strikethrough: None,
    };
    let line = window.text_system().shape_line(
        SharedString::from(text),
        px(theme.font_size),
        &[run],
        None,
    );
    let top_left = point(
        origin.x + cell_w * cursor.col as f32,
        origin.y + line_h * cursor.row as f32,
    );
    let mut bg = theme.background;
    bg.a = 1.0;
    window.paint_quad(fill(
        Bounds::new(top_left, size(line.width.max(cell_w), line_h)),
        bg,
    ));
    let _ = line.paint(top_left, line_h, gpui::TextAlign::Left, None, window, cx);
}

fn paint_scrollbar(
    view: &Entity<TerminalView>,
    bounds: Bounds<Pixels>,
    rows: usize,
    window: &mut Window,
    cx: &App,
) {
    let view_ref = view.read(cx);
    if !view_ref.scrollbar.should_paint() {
        return;
    }
    let term = view_ref.terminal.read(cx);
    let active = view_ref.scrollbar.active();
    let Some(m) = crate::view::scrollbar::metrics(
        bounds,
        rows,
        term.history_size(),
        term.display_offset(),
        active,
    ) else {
        return;
    };
    let mut color = term.theme().foreground;
    color.a = if active { 0.55 } else { 0.36 };
    let mut quad = fill(
        Bounds::new(
            point(px(m.thumb_x), px(m.thumb_top)),
            size(px(m.thumb_w), px(m.thumb_h)),
        ),
        color,
    );
    quad.corner_radii = gpui::Corners::all(px(m.thumb_w * 0.5));
    window.paint_quad(quad);
}

/// Rows that fit in `avail`, and the line height that makes them reach it.
///
/// The line box grows instead of leaving a short strip under the last row.
/// Below one nominal line the box stays nominal and the pane clips it.
fn filled_rows(avail: f32, nominal: f32) -> (u16, f32) {
    let nominal = if nominal.is_finite() && nominal >= 1.0 {
        nominal
    } else {
        1.0
    };
    let avail = if avail.is_finite() && avail >= nominal {
        avail
    } else {
        nominal
    };
    let rows = ((avail / nominal).floor() as i64).clamp(1, 1000) as u16;
    (rows, avail / f32::from(rows))
}

/// Contiguous `[start, end)` column runs of cells satisfying `pred`.
fn spans(row: &[CellSnapshot], pred: impl Fn(&CellSnapshot) -> bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for col in 0..=row.len() {
        match (start, row.get(col).is_some_and(&pred)) {
            (None, true) => start = Some(col),
            (Some(s), false) => {
                out.push((s, col));
                start = None;
            }
            _ => {}
        }
    }
    out
}

pub(crate) fn mono_font(theme: &TerminalTheme) -> gpui::Font {
    // Ligatures OFF: a terminal is a fixed grid, and a contextual substitution
    // collapses several cells into fewer glyphs so the row renders short while
    // the cursor stays on the true column.
    let mut mono = font(theme.font_family.clone());
    mono.features = gpui::FontFeatures(std::sync::Arc::new(vec![
        ("liga".into(), 0),
        ("calt".into(), 0),
        ("dlig".into(), 0),
    ]));
    // Geist Mono has no CJK. A fallback has to be listed or macOS never cascades.
    mono.fallbacks = Some(cjk_fallbacks());
    mono
}

fn cjk_fallbacks() -> gpui::FontFallbacks {
    use std::sync::OnceLock;
    static CJK: OnceLock<gpui::FontFallbacks> = OnceLock::new();
    CJK.get_or_init(|| {
        gpui::FontFallbacks::from_fonts(vec![
            "PingFang SC".into(),
            "Hiragino Sans GB".into(),
            "Heiti SC".into(),
            "PingFang TC".into(),
            "Noto Sans CJK SC".into(),
        ])
    })
    .clone()
}

/// The cursor quad, split by z-order: a focused block goes under the glyph
/// (which is recolored to the background), everything else over it.
fn cursor_quads(
    c: CursorSnapshot,
    wide: bool,
    theme: &TerminalTheme,
    focused: bool,
    origin: gpui::Point<Pixels>,
    cell_w: Pixels,
    line_h: Pixels,
) -> (Option<PaintQuad>, Option<PaintQuad>) {
    let w = if wide { cell_w * 2.0 } else { cell_w };
    let top_left = point(
        origin.x + cell_w * c.col as f32,
        origin.y + line_h * c.row as f32,
    );
    let full = Bounds::new(top_left, size(w, line_h));
    match c.shape {
        CursorShape::Hidden => (None, None),
        CursorShape::Block if focused => (Some(fill(full, theme.cursor)), None),
        CursorShape::Block | CursorShape::HollowBlock => (
            None,
            Some(outline(full, theme.cursor, gpui::BorderStyle::Solid)),
        ),
        CursorShape::Underline => (
            None,
            Some(fill(
                Bounds::new(
                    point(top_left.x, top_left.y + line_h - px(2.0)),
                    size(w, px(2.0)),
                ),
                theme.cursor,
            )),
        ),
        CursorShape::Beam => (
            None,
            Some(fill(
                Bounds::new(top_left, size(px(2.0), line_h)),
                theme.cursor,
            )),
        ),
    }
}

/// Paint one row of box-drawing cells: continuous horizontal spans first (no
/// gaps between cells), then the vertical components / whole glyphs.
fn paint_box_row(
    cells: &[BoxCell],
    origin: gpui::Point<Pixels>,
    row: usize,
    cell_w: Pixels,
    line_h: Pixels,
    window: &mut Window,
) {
    if cells.is_empty() {
        return;
    }
    let y_base = origin.y + line_h * row as f32;
    let cy = y_base + line_h / 2.0;
    let mut horizontal = std::collections::HashSet::new();
    let mut i = 0;
    while i < cells.len() {
        let BoxCell { col, ch, color } = &cells[i];
        let Some(weight) = box_drawing::get_horizontal_weight(*ch) else {
            i += 1;
            continue;
        };
        let (start, mut end, mut j) = (*col, *col, i + 1);
        while let Some(next) = cells.get(j) {
            if next.col == end + 1
                && next.color == *color
                && box_drawing::get_horizontal_weight(next.ch) == Some(weight)
            {
                end = next.col;
                j += 1;
            } else {
                break;
            }
        }
        box_drawing::draw_horizontal_span(
            origin.x + cell_w * start as f32,
            origin.x + cell_w * (end + 1) as f32,
            cy,
            weight,
            cell_w,
            *color,
            window,
        );
        horizontal.extend(start..=end);
        i = j;
    }
    for BoxCell { col, ch, color } in cells {
        let bounds = Bounds::new(
            point(origin.x + cell_w * *col as f32, y_base),
            size(cell_w, line_h),
        );
        if horizontal.contains(col) {
            box_drawing::draw_vertical_components(*ch, bounds, *color, cell_w, window);
        } else {
            box_drawing::draw_box_character(*ch, bounds, *color, cell_w, window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::filled_rows;

    #[test]
    fn rows_reach_the_bottom_of_the_pane() {
        let (rows, line_h) = filled_rows(400.0, 18.0);
        assert_eq!(rows, 22);
        assert!((f32::from(rows) * line_h - 400.0).abs() < 1e-3);
        assert!(line_h >= 18.0);
    }

    #[test]
    fn an_exact_multiple_keeps_the_nominal_line() {
        let (rows, line_h) = filled_rows(360.0, 18.0);
        assert_eq!(rows, 20);
        assert!((line_h - 18.0).abs() < 1e-3);
    }

    #[test]
    fn a_short_pane_does_not_squash_the_line() {
        let (rows, line_h) = filled_rows(10.0, 18.0);
        assert_eq!(rows, 1);
        assert!((line_h - 18.0).abs() < 1e-3);
    }
}
