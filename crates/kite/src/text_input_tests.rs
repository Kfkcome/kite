#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_pass_by_value,
    clippy::reversed_empty_ranges
)]

use std::ops::Range;

use gpui::{KeyContext, Keystroke, SharedString};

use super::{Backspace, CONTEXT, apply_mark, apply_replace, bindings, clamp_boundary, clamp_range};

#[derive(Default)]
struct State {
    content: SharedString,
    selected: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
}

impl State {
    fn with_text(text: &str, cursor: usize) -> Self {
        Self {
            content: text.to_owned().into(),
            selected: cursor..cursor,
            reversed: false,
            marked: None,
        }
    }

    fn mark(&mut self, range_utf16: Option<Range<usize>>, text: &str, sel: Option<Range<usize>>) {
        apply_mark(
            &mut self.content,
            &mut self.selected,
            &mut self.reversed,
            &mut self.marked,
            range_utf16.as_ref(),
            text,
            sel.as_ref(),
        );
    }

    fn replace(&mut self, range_utf16: Option<Range<usize>>, text: &str) {
        apply_replace(
            &mut self.content,
            &mut self.selected,
            &mut self.reversed,
            &mut self.marked,
            range_utf16.as_ref(),
            text,
        );
    }
}

fn resolves_to<A: gpui::Action>(keys: &str) -> bool {
    let keymap = gpui::Keymap::new(bindings());
    let stroke = Keystroke::parse(keys).expect("valid keystroke");
    let ctx = KeyContext::parse(CONTEXT).expect("valid context");
    let (found, _) = keymap.bindings_for_input(&[stroke], &[ctx]);
    found.iter().any(|b| b.action().as_any().is::<A>())
}

#[test]
fn shift_backspace_edits_like_the_plain_key() {
    assert!(resolves_to::<Backspace>("backspace"));
    assert!(resolves_to::<Backspace>("shift-backspace"));
}

#[test]
fn clearing_marked_text_does_not_leave_selection_past_end() {
    let mut s = State::default();
    s.mark(None, "a", Some(1..1));
    assert_eq!(s.content.as_ref(), "a");
    assert_eq!(s.marked, Some(0..1));
    assert_eq!(s.selected, 1..1);

    // Clearing the preedit with backspace drops marked text back to empty.
    s.mark(None, "", Some(0..0));
    assert_eq!(s.content.as_ref(), "");
    assert_eq!(s.marked, None);
    assert_eq!(s.selected, 0..0);

    // Typing again immediately after clearing must not slice out of bounds.
    s.mark(None, "n", Some(1..1));
    assert_eq!(s.content.as_ref(), "n");
    assert_eq!(s.marked, Some(0..1));
    assert_eq!(s.selected, 1..1);
}

#[test]
fn mark_selected_range_is_measured_within_new_text_after_cjk_prefix() {
    let mut s = State::with_text("中文", 6);
    s.mark(None, "shi'jie", Some(3..3));
    assert_eq!(s.content.as_ref(), "中文shi'jie");
    assert_eq!(s.marked, Some(6..13));
    assert_eq!(s.selected, 9..9);
}

#[test]
fn mark_handles_multibyte_cjk_and_surrogate_pair_offsets() {
    let mut cjk = State::with_text("中文", 6);
    cjk.mark(None, "世界", Some(1..2));
    assert_eq!(cjk.content.as_ref(), "中文世界");
    assert_eq!(cjk.marked, Some(6..12));
    assert_eq!(cjk.selected, 9..12);

    // Offset 1 splits the UTF-16 surrogate pair of "😀"; snaps to end of "😀" (byte 4).
    let mut emoji = State::default();
    emoji.mark(None, "😀a", Some(1..1));
    assert_eq!(emoji.content.as_ref(), "😀a");
    assert_eq!(emoji.marked, Some(0..5));
    assert_eq!(emoji.selected, 4..4);
}

#[test]
fn mark_and_replace_clamp_out_of_bounds_and_reversed_ranges() {
    let mut s = State::with_text("hi", 2);
    s.mark(Some(10..2), "x", Some(99..50));
    assert_eq!(s.content.as_ref(), "hix");
    assert_eq!(s.marked, Some(2..3));
    assert_eq!(s.selected, 3..3);

    s.replace(Some(999..1000), "!");
    assert_eq!(s.content.as_ref(), "hix!");
    assert_eq!(s.marked, None);
    assert_eq!(s.selected, 4..4);
}

#[test]
fn replace_commits_marked_text_and_clears_mark() {
    let mut s = State::with_text("hi", 2);
    s.mark(None, "shijie", Some(6..6));
    assert_eq!(s.marked, Some(2..8));

    s.replace(None, "世界");
    assert_eq!(s.content.as_ref(), "hi世界");
    assert_eq!(s.marked, None);
    assert_eq!(s.selected, 8..8);
}

#[test]
fn clamp_helpers_align_to_char_boundaries() {
    let text = "a中b";
    assert_eq!(clamp_boundary(text, 0), 0);
    assert_eq!(clamp_boundary(text, 1), 1);
    assert_eq!(clamp_boundary(text, 2), 1);
    assert_eq!(clamp_boundary(text, 3), 1);
    assert_eq!(clamp_boundary(text, 4), 4);
    assert_eq!(clamp_boundary(text, 99), 5);
    assert_eq!(clamp_range(text, &(3..2)), 1..1);
    assert_eq!(clamp_range(text, &(2..99)), 1..5);
}
