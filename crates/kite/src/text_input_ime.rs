//! UTF-8 / UTF-16 boundary clamping and IME text splicing helpers for [`super::TextInput`].

use std::ops::Range;

use gpui::SharedString;

/// Clamps `offset` into `0..=text.len()` on a valid UTF-8 character boundary.
pub(super) fn clamp_boundary(text: &str, offset: usize) -> usize {
    let mut clamped = offset.min(text.len());
    while clamped > 0 && !text.is_char_boundary(clamped) {
        clamped -= 1;
    }
    clamped
}

/// Clamps `range` onto ordered UTF-8 character boundaries within `text`.
pub(super) fn clamp_range(text: &str, range: &Range<usize>) -> Range<usize> {
    let start = clamp_boundary(text, range.start);
    let end = clamp_boundary(text, range.end).max(start);
    start..end
}

/// Converts a UTF-16 code-unit offset inside `text` to a valid UTF-8 byte boundary.
pub(super) fn utf16_offset_to_utf8(text: &str, offset_utf16: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;

    for ch in text.chars() {
        if utf16_count >= offset_utf16 {
            break;
        }
        utf16_count += ch.len_utf16();
        utf8_offset += ch.len_utf8();
    }

    utf8_offset
}

/// Converts a UTF-16 span inside `text` into an ordered UTF-8 byte range.
pub(super) fn utf16_range_to_utf8(text: &str, range_utf16: &Range<usize>) -> Range<usize> {
    let start = utf16_offset_to_utf8(text, range_utf16.start);
    let end = utf16_offset_to_utf8(text, range_utf16.end).max(start);
    start..end
}

fn splice_range(
    content: &str,
    selected_range: &Range<usize>,
    marked_range: &Option<Range<usize>>,
    range_utf16: Option<&Range<usize>>,
) -> Range<usize> {
    let raw = range_utf16
        .map(|r| utf16_range_to_utf8(content, r))
        .or_else(|| marked_range.clone())
        .unwrap_or_else(|| selected_range.clone());
    clamp_range(content, &raw)
}

/// Replaces `range_utf16` (or the current marked/selected range) with `new_text` and clears IME marks.
pub(super) fn apply_replace(
    content: &mut SharedString,
    selected_range: &mut Range<usize>,
    selection_reversed: &mut bool,
    marked_range: &mut Option<Range<usize>>,
    range_utf16: Option<&Range<usize>>,
    new_text: &str,
) {
    let range = splice_range(content, selected_range, marked_range, range_utf16);
    *content = (content[0..range.start].to_owned() + new_text + &content[range.end..]).into();
    let caret = range.start + new_text.len();
    *selected_range = caret..caret;
    *selection_reversed = false;
    *marked_range = None;
}

/// Updates the marked IME preedit with `new_text` and maps `new_selected_range_utf16` relative to `new_text`.
pub(super) fn apply_mark(
    content: &mut SharedString,
    selected_range: &mut Range<usize>,
    selection_reversed: &mut bool,
    marked_range: &mut Option<Range<usize>>,
    range_utf16: Option<&Range<usize>>,
    new_text: &str,
    new_selected_range_utf16: Option<&Range<usize>>,
) {
    let range = splice_range(content, selected_range, marked_range, range_utf16);
    *content = (content[0..range.start].to_owned() + new_text + &content[range.end..]).into();
    *marked_range = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
    // AppKit reports `new_selected_range_utf16` inside `new_text`, so map it in `new_text` and
    // anchor both ends on `range.start`.
    let next_selected = new_selected_range_utf16
        .map(|r| {
            let rel = utf16_range_to_utf8(new_text, r);
            range.start.saturating_add(rel.start)..range.start.saturating_add(rel.end)
        })
        .unwrap_or_else(|| {
            let caret = range.start + new_text.len();
            caret..caret
        });
    *selected_range = clamp_range(content, &next_selected);
    *selection_reversed = false;
}
