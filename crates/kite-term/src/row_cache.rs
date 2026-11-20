//! Per-row text-shaping cache for [`crate::element::TerminalElement`].
//!
//! Avoids re-shaping unchanged rows on cursor blinks, smooth scrolling,
//! selection drags, scrollbar fades, and partial output.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use gpui::{
    Font, FontStyle, FontWeight, Hsla, Pixels, ShapedLine, SharedString, StrikethroughStyle,
    TextRun, UnderlineStyle, Window, px,
};

use crate::box_drawing;
use crate::config::TerminalTheme;
use crate::element::mono_font;
use crate::snapshot::{CellColor, CellSnapshot};

pub(crate) struct BoxCell {
    pub(crate) col: usize,
    pub(crate) ch: char,
    pub(crate) color: Hsla,
}

pub(crate) struct RowShaped {
    pub(crate) segments: Vec<(usize, ShapedLine)>,
    pub(crate) boxes: Vec<BoxCell>,
}

struct CachedEntry {
    last_gen: u64,
    shaped: Arc<RowShaped>,
}

const SOFT_CAP: usize = 256;
const HARD_CAP: usize = 512;
const RETAIN_GENS: u64 = 4;

#[derive(Default)]
pub(crate) struct RowShapeCache {
    pub(crate) row_buf: Vec<CellSnapshot>,
    theme_sig: u64,
    fonts: Option<[Font; 4]>,
    entries: HashMap<(u128, Option<usize>), CachedEntry>,
    generation: u64,
}

impl RowShapeCache {
    pub(crate) fn begin_frame(
        &mut self,
        theme: &TerminalTheme,
        mono: &Font,
        cols: usize,
        scale_factor: f32,
    ) {
        let sig = theme_signature(theme, cols, scale_factor);
        if self.fonts.is_none() || self.theme_sig != sig {
            self.theme_sig = sig;
            self.entries.clear();
            self.fonts = Some(font_variants(mono));
        }
        self.generation = self.generation.wrapping_add(1);
    }

    pub(crate) fn get_or_shape(
        &mut self,
        row: &[CellSnapshot],
        theme: &TerminalTheme,
        font_size: Pixels,
        block_cursor_col: Option<usize>,
        window: &Window,
    ) -> Arc<RowShaped> {
        let key = (row_signature(row), block_cursor_col);
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_gen = self.generation;
            return Arc::clone(&entry.shaped);
        }
        let fonts = self
            .fonts
            .get_or_insert_with(|| font_variants(&mono_font(theme)));
        let (segments, boxes) = shape_row(row, theme, fonts, font_size, block_cursor_col, window);
        let shaped = Arc::new(RowShaped { segments, boxes });
        self.entries.insert(
            key,
            CachedEntry {
                last_gen: self.generation,
                shaped: Arc::clone(&shaped),
            },
        );
        shaped
    }

    pub(crate) fn end_frame(&mut self) {
        if self.entries.len() <= SOFT_CAP {
            return;
        }
        let cur = self.generation;
        self.entries
            .retain(|_, entry| cur.wrapping_sub(entry.last_gen) <= RETAIN_GENS);
        if self.entries.len() > HARD_CAP {
            self.entries.retain(|_, entry| entry.last_gen == cur);
        }
    }
}

fn font_variants(mono: &Font) -> [Font; 4] {
    let mut normal = mono.clone();
    normal.weight = FontWeight::NORMAL;
    normal.style = FontStyle::Normal;

    let mut bold = mono.clone();
    bold.weight = FontWeight::BOLD;
    bold.style = FontStyle::Normal;

    let mut italic = mono.clone();
    italic.weight = FontWeight::NORMAL;
    italic.style = FontStyle::Italic;

    let mut bold_italic = mono.clone();
    bold_italic.weight = FontWeight::BOLD;
    bold_italic.style = FontStyle::Italic;

    [normal, bold, italic, bold_italic]
}

fn hash_hsla<H: Hasher>(c: Hsla, h: &mut H) {
    c.h.to_bits().hash(h);
    c.s.to_bits().hash(h);
    c.l.to_bits().hash(h);
    c.a.to_bits().hash(h);
}

fn theme_signature(theme: &TerminalTheme, cols: usize, scale_factor: f32) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    theme.font_family.hash(&mut h);
    theme.font_size.to_bits().hash(&mut h);
    hash_hsla(theme.foreground, &mut h);
    hash_hsla(theme.background, &mut h);
    for c in theme.ansi {
        hash_hsla(c, &mut h);
    }
    cols.hash(&mut h);
    scale_factor.to_bits().hash(&mut h);
    h.finish()
}

fn mix_color(fg: CellColor) -> u64 {
    match fg {
        CellColor::Foreground => 1,
        CellColor::Background => 2,
        CellColor::Indexed(ix) => 0x100 | u64::from(ix),
        CellColor::Rgb(r, g, b) => {
            0x200_0000 | (u64::from(r) << 16) | (u64::from(g) << 8) | u64::from(b)
        }
    }
}

fn row_signature(row: &[CellSnapshot]) -> u128 {
    let mut h1: u64 = 0xcbf2_9ce4_8422_2325;
    let mut h2: u64 = 0x9e37_79b9_7f4a_7c15 ^ (row.len() as u64);
    for cell in row {
        let ch = if cell.hidden { ' ' } else { cell.ch };
        let (fg, _) = cell.display_colors();
        let flags = u64::from(cell.bold)
            | (u64::from(cell.dim) << 1)
            | (u64::from(cell.italic) << 2)
            | (u64::from(cell.underline) << 3)
            | (u64::from(cell.strikeout) << 4)
            | (u64::from(cell.wide) << 5)
            | (u64::from(cell.wide_spacer) << 6);
        let packed = (u64::from(ch as u32) << 32) ^ mix_color(fg) ^ (flags << 56);

        h1 ^= packed;
        h1 = h1.wrapping_mul(0x1000_0000_01b3);

        h2 = h2.wrapping_add(packed.wrapping_mul(0xbf58_476d_1ce4_e5b9));
        h2 = (h2 ^ (h2 >> 30)).wrapping_mul(0x94d0_49bb_1331_11eb);
    }
    (u128::from(h1) << 64) | u128::from(h2)
}

#[derive(Clone, Copy, PartialEq)]
struct RunStyle {
    color: Hsla,
    bold: bool,
    italic: bool,
    underline: bool,
    strikeout: bool,
}

/// Shape one grid row into COLUMN-PINNED segments.
///
/// A shaped line places glyphs by font advances, which only match the grid
/// while every glyph is the mono font's width. A glyph that resolves through
/// font fallback (arrows, emoji, CJK) has some other advance and would push
/// the rest of the line out of the grid. So runs of ASCII shape together and
/// every other glyph is its own segment pinned at its own column. Box-drawing
/// cells are not shaped at all; they are returned for programmatic painting.
fn shape_row(
    row: &[CellSnapshot],
    theme: &TerminalTheme,
    fonts: &[Font; 4],
    font_size: Pixels,
    block_cursor_col: Option<usize>,
    window: &Window,
) -> (Vec<(usize, ShapedLine)>, Vec<BoxCell>) {
    fn flush(
        segments: &mut Vec<(usize, ShapedLine)>,
        text: &mut String,
        runs: &mut Vec<TextRun>,
        last_style: &mut Option<RunStyle>,
        seg_col: usize,
        font_size: Pixels,
        window: &Window,
    ) {
        if text.is_empty() {
            return;
        }
        let shaped = window.text_system().shape_line(
            SharedString::from(std::mem::take(text)),
            font_size,
            runs,
            None,
        );
        segments.push((seg_col, shaped));
        runs.clear();
        *last_style = None;
    }

    let mut segments: Vec<(usize, ShapedLine)> = Vec::new();
    let mut boxes = Vec::new();
    let mut text = String::with_capacity(row.len());
    let mut runs: Vec<TextRun> = Vec::new();
    let mut last_style: Option<RunStyle> = None;
    let mut seg_col = 0usize;

    for (col, cell) in row.iter().enumerate() {
        if cell.wide_spacer {
            continue;
        }
        let ch = if cell.hidden { ' ' } else { cell.ch };
        let (fg, _) = cell.display_colors();
        let mut color = theme.resolve(fg);
        if cell.dim {
            color.a *= 0.6;
        }
        if block_cursor_col == Some(col) {
            color = theme.background;
        }
        if box_drawing::get_box_segments(ch).is_some() {
            flush(
                &mut segments,
                &mut text,
                &mut runs,
                &mut last_style,
                seg_col,
                font_size,
                window,
            );
            boxes.push(BoxCell { col, ch, color });
            continue;
        }
        // Anything that can leave the mono font gets its own pinned segment.
        let pinned = !ch.is_ascii() || cell.wide;
        if pinned {
            flush(
                &mut segments,
                &mut text,
                &mut runs,
                &mut last_style,
                seg_col,
                font_size,
                window,
            );
        }
        if text.is_empty() {
            seg_col = col;
        }
        let style = RunStyle {
            color,
            bold: cell.bold,
            italic: cell.italic,
            underline: cell.underline,
            strikeout: cell.strikeout,
        };
        let len = ch.len_utf8();
        text.push(ch);
        if last_style == Some(style)
            && let Some(last) = runs.last_mut()
        {
            last.len += len;
        } else {
            let font_ix = usize::from(cell.bold) | (usize::from(cell.italic) << 1);
            runs.push(TextRun {
                len,
                font: fonts[font_ix].clone(),
                color,
                background_color: None,
                underline: cell.underline.then_some(UnderlineStyle {
                    color: Some(color),
                    thickness: px(1.0),
                    wavy: false,
                }),
                strikethrough: cell.strikeout.then_some(StrikethroughStyle {
                    color: Some(color),
                    thickness: px(1.0),
                }),
            });
            last_style = Some(style);
        }
        if pinned {
            flush(
                &mut segments,
                &mut text,
                &mut runs,
                &mut last_style,
                seg_col,
                font_size,
                window,
            );
        }
    }
    flush(
        &mut segments,
        &mut text,
        &mut runs,
        &mut last_style,
        seg_col,
        font_size,
        window,
    );
    (segments, boxes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::SearchMark;

    fn sample_cell(ch: char) -> CellSnapshot {
        CellSnapshot {
            ch,
            fg: CellColor::Foreground,
            bg: CellColor::Background,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            strikeout: false,
            inverse: false,
            hidden: false,
            wide: false,
            wide_spacer: false,
            selected: false,
            search: SearchMark::None,
        }
    }

    #[test]
    fn selection_and_search_do_not_invalidate_row_signature() {
        let mut a = vec![sample_cell('h'), sample_cell('i')];
        let mut b = a.clone();
        b[0].selected = true;
        b[1].search = SearchMark::Current;
        assert_eq!(row_signature(&a), row_signature(&b));

        a[0].ch = 'x';
        assert_ne!(row_signature(&a), row_signature(&b));
    }
}
