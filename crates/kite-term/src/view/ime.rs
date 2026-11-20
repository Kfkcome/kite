//! Input-method composition for [`TerminalView`].
//!
//! The preedit lives only in this view. It is sent to the remote when the
//! input method commits (`insertText`), and dropped when the composition is
//! cancelled. `unmarkText` does not send: on macOS the commit arrives as
//! `insertText`, and sending both would write the composition twice.

use std::ops::Range;

use alacritty_terminal::term::TermMode;
use gpui::{
    App, Bounds, ClipboardItem, Context, ElementInputHandler, Entity, EntityInputHandler, Pixels,
    UTF16Selection, Window, point, px, size,
};

use super::{GridGeometry, TerminalView};

#[derive(Debug, Default)]
pub(super) struct Composition {
    text: String,
    /// Selection inside `text`, in UTF-16 code units.
    selected: Range<usize>,
}

impl Composition {
    pub(super) fn clear(&mut self) {
        self.text.clear();
        self.selected = 0..0;
    }

    pub(super) fn preedit(&self) -> Option<&str> {
        if self.text.is_empty() {
            None
        } else {
            Some(self.text.as_str())
        }
    }

    fn marked_utf16(&self) -> Option<Range<usize>> {
        if self.text.is_empty() {
            None
        } else {
            Some(0..utf16_len(&self.text))
        }
    }

    /// Replace the composition and return the bytes that should hit the remote.
    /// The composition itself was never sent, so the returned string is the
    /// whole result, not only `new_text`.
    fn commit(&mut self, range: Option<Range<usize>>, new_text: &str) -> Vec<u8> {
        let updated = self.splice(range, new_text, None);
        self.clear();
        updated.text.into_bytes()
    }

    fn mark(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        selected: Option<Range<usize>>,
    ) {
        let updated = self.splice(range, new_text, selected);
        if updated.text.is_empty() {
            self.clear();
        } else {
            *self = updated;
        }
    }

    fn splice(
        &self,
        range: Option<Range<usize>>,
        new_text: &str,
        selected: Option<Range<usize>>,
    ) -> Self {
        let byte_range = self.byte_range(range);
        let mut text = String::with_capacity(self.text.len() + new_text.len());
        text.push_str(&self.text[..byte_range.start]);
        text.push_str(new_text);
        text.push_str(&self.text[byte_range.end..]);

        let inserted_at = utf16_len(&self.text[..byte_range.start]);
        let sel = selected.unwrap_or_else(|| {
            let n = utf16_len(new_text);
            n..n
        });
        let len = utf16_len(&text);
        let start = inserted_at.saturating_add(sel.start).min(len);
        let end = inserted_at.saturating_add(sel.end).min(len).max(start);
        Self {
            text,
            selected: start..end,
        }
    }

    /// UTF-8 span for a UTF-16 range. `None` replaces the whole composition,
    /// which is the input method's "current marked range" when it passes
    /// `NSNotFound`.
    fn byte_range(&self, range: Option<Range<usize>>) -> Range<usize> {
        match range {
            Some(range) => {
                let len = utf16_len(&self.text);
                let start = range.start.min(len);
                let end = range.end.min(len).max(start);
                byte_at(&self.text, start)..byte_at(&self.text, end)
            }
            None if !self.text.is_empty() => 0..self.text.len(),
            None => {
                let at = byte_at(&self.text, self.selected.start);
                at..at
            }
        }
    }
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Rectangle the input method uses to place its candidate window.
///
/// `cursor` is the viewport cell from [`crate::terminal::Terminal::ime_cursor`].
/// A cell that has scrolled away yields no rectangle, so the window is not
/// pinned to the corner of the pane. The column is clamped into the grid: a
/// composition caret past the right margin stays on the last cell.
fn ime_candidate_bounds(
    geo: GridGeometry,
    cursor: Option<(usize, usize)>,
    range_utf16: Range<usize>,
) -> Option<Bounds<Pixels>> {
    if geo.cols == 0 || geo.rows == 0 {
        return None;
    }
    let (col, row) = cursor?;
    let start = range_utf16.start;
    let span = range_utf16.end.saturating_sub(start).max(1);
    let anchor_col = col
        .saturating_add(start)
        .min(usize::from(geo.cols.saturating_sub(1)));
    Some(Bounds::new(
        point(
            geo.origin.x + px(geo.cell_w * anchor_col as f32),
            geo.origin.y + px(geo.line_h * row as f32),
        ),
        size(px(geo.cell_w * span as f32), px(geo.line_h)),
    ))
}

fn byte_at(text: &str, utf16_index: usize) -> usize {
    let mut seen = 0;
    for (byte, ch) in text.char_indices() {
        if seen >= utf16_index {
            return byte;
        }
        seen += ch.len_utf16();
    }
    text.len()
}

impl TerminalView {
    pub(crate) fn ime_preedit(&self) -> Option<&str> {
        self.ime.preedit()
    }

    pub(crate) fn attach_input(
        &self,
        bounds: Bounds<Pixels>,
        entity: Entity<Self>,
        window: &mut Window,
        cx: &App,
    ) {
        window.handle_input(
            &self.focus_handle,
            ElementInputHandler::new(bounds, entity),
            cx,
        );
    }
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let len = utf16_len(&self.ime.text);
        let start = range_utf16.start.min(len);
        let end = range_utf16.end.min(len).max(start);
        *adjusted_range = Some(start..end);
        let bytes = byte_at(&self.ime.text, start)..byte_at(&self.ime.text, end);
        Some(self.ime.text[bytes].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.ime.selected.clone(),
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.ime.marked_utf16()
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.ime.clear();
        cx.notify();
    }

    fn paste(&mut self, item: ClipboardItem, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = item.text() else {
            return;
        };
        let bracketed = self.mode(cx).contains(TermMode::BRACKETED_PASTE);
        self.ime.clear();
        self.send_input(super::paste_bytes(&text, bracketed), cx);
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bytes = self.ime.commit(range_utf16, text);
        if !bytes.is_empty() {
            self.send_input(bytes, cx);
        } else {
            cx.notify();
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ime.mark(range_utf16, new_text, new_selected_range);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let geo = self.geometry?;
        let cursor = self.terminal.read(cx).ime_cursor();
        ime_candidate_bounds(geo, cursor, range_utf16)
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.ime.selected.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_replaces_pinyin_with_the_candidate() {
        let mut ime = Composition::default();
        assert_eq!(ime.commit(None, "a"), b"a");
        assert!(ime.preedit().is_none());

        ime.mark(None, "ni", Some(2..2));
        assert_eq!(ime.preedit(), Some("ni"));
        assert_eq!(ime.marked_utf16(), Some(0..2));
        assert_eq!(ime.commit(None, "你"), "你".as_bytes());
        assert!(ime.preedit().is_none());
    }

    #[test]
    fn marking_stays_local_until_commit() {
        let mut ime = Composition::default();
        ime.mark(None, "ni", None);
        ime.mark(None, "nih", Some(3..3));
        assert_eq!(ime.preedit(), Some("nih"));
        ime.mark(Some(2..3), "好", Some(1..1));
        assert_eq!(ime.preedit(), Some("ni好"));
        ime.mark(None, "", None);
        assert!(ime.preedit().is_none());
    }

    #[test]
    fn utf16_ranges_do_not_split_a_scalar() {
        let mut ime = Composition::default();
        ime.mark(None, "😀", None);
        assert_eq!(ime.marked_utf16(), Some(0..2));
        assert_eq!(ime.commit(Some(0..2), "x"), b"x");
    }

    fn grid(cols: u16, rows: u16) -> GridGeometry {
        GridGeometry {
            bounds: Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(600.0))),
            origin: point(px(8.0), px(16.0)),
            cell_w: 10.0,
            line_h: 20.0,
            cols,
            rows,
        }
    }

    #[test]
    fn candidate_bounds_follow_the_ime_cell_and_clamp_at_the_right_margin() {
        let geo = grid(80, 24);
        assert_eq!(
            ime_candidate_bounds(geo, Some((2, 21)), 0..1),
            Some(Bounds::new(
                point(px(28.0), px(436.0)),
                size(px(10.0), px(20.0)),
            ))
        );
        // Column 78 plus a caret 5 cells into the composition is 83, past the
        // last cell. The window stays on column 79 instead of leaving the pane.
        assert_eq!(
            ime_candidate_bounds(geo, Some((78, 3)), 5..8),
            Some(Bounds::new(
                point(px(798.0), px(76.0)),
                size(px(30.0), px(20.0)),
            ))
        );
        assert_eq!(ime_candidate_bounds(geo, None, 0..1), None);
        assert_eq!(ime_candidate_bounds(grid(0, 24), Some((0, 0)), 0..1), None);
        assert_eq!(ime_candidate_bounds(grid(80, 0), Some((0, 0)), 0..1), None);
    }
}
