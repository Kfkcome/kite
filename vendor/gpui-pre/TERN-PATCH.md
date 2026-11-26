# tern patch to gpui-pre 0.3.8

Prevents 4x monochrome glyph atlas bloat for CJK and fullwidth characters without changing any
public API (adapted from DEEIX-AI/Winc `crates/winc-render/src/draw/frame.rs`).

## Differences

`src/window.rs`
- Adds `pub(crate) fn paint_glyph_with_subpixel(..., use_subpixel_x: bool)` behind the existing
  `paint_glyph` method. When `use_subpixel_x` is `false`, horizontal placement snaps to whole device
  pixels with `subpixel_variant.x = 0` (matching `paint_emoji`).

`src/text_system/line.rs`
- Passes `Option<&str>` line text into `paint_line` and snaps CJK / Hangul / Kana / fullwidth
  characters (`is_full_width_char`) to whole horizontal device pixels so each character occupies 1
  atlas tile instead of up to 4.
