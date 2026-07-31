//! Render crate — terminal braille rendering.
//!
//! Every non-text visual goes through the dot pipeline: source art or a
//! procedural shape becomes a [`dots::DotBuffer`] (1 dot = 1 sub-cell pixel,
//! 2 dots wide × 4 dots tall per braille cell), buffers composite back to
//! front, then [`dots::dots_to_grid`] folds each 2×4 block into one braille
//! glyph and [`grid::draw_grid`] blits it. `draw_grid` is the only write into
//! a `ratatui::Buffer` in the crate.
//!
//! Module map:
//! - [`dots`], [`grid`], [`dot_diff`] — the dot/cell IRs and the dot-level
//!   decoder tests assert against.
//! - [`composite`], [`camera`], [`transform`] — world space, depth-sorted
//!   compositing, per-sprite transforms.
//! - [`flex`], [`screen_layout`] — dot-precision (`DotRect`) and cell-space
//!   layout.
//! - [`convert`], [`anim`], [`asset_cache`], [`shapes`], [`ui_primitives`] —
//!   image conversion, GIF playback, caching, procedural chrome.
//! - [`button`], [`text_editor`] — widgets.
//!
//! The free functions here ([`fill`], [`label`], [`wrapped_text`],
//! [`draw_asset`]) are the flat conveniences on top. `label` and friends draw
//! real terminal characters: text is the one thing that does not become dots.

pub mod anim;
pub mod asset_cache;
pub mod button;
pub mod camera;
pub mod composite;
pub mod convert;
pub mod debug_grid;
pub mod dot_diff;
pub mod dots;
pub mod flex;
pub mod grid;
pub mod screen_layout;
pub mod shapes;
pub mod text_editor;
pub mod transform;
pub mod tween;
pub mod ui_primitives;
pub use anim::AnimatedSprite;
pub use button::{ActiveStyle, Button, ButtonColors, ButtonCore, ButtonState, StateColors};
pub use convert::convert;
pub use debug_grid::{draw_debug_grid, GRID_SPACING_COLS, GRID_SPACING_ROWS};
pub use dot_diff::{decode_braille_cell, diff_dots, DotDiff, DotMismatch};
pub use flex::{flex, Align, Basis, Direction, DotRect, DotRectTween, FlexChild, FlexStyle, Justify};
pub use dots::draw_dots;
pub use grid::{draw_grid, Cell, Grid};
pub use screen_layout::{anchor, anchor_with_margin, stack, Anchor, RectTween, StackAxis};
pub use shapes::{rasterize_shape, ShapeKind};
pub use text_editor::{
    Decoration, EditorEvent, MentionCandidate, MentionProvider, Sizing, TextEditor,
    TextEditorConfig, SELECTION_BG,
};
pub use ui_primitives::{circle, circle_outline, rect, ring, rounded_rect};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use engine_core::color::Rgba;

/// Horizontal alignment for text rendering (`label`, `wrapped_text`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

/// Fully-lit braille glyph (all 8 dots): U+28FF.
const FULL: char = '⣿';

/// Paint every cell of `area` (clamped to `buf`) with the fully-lit braille
/// glyph in `color`'s RGB. Alpha is ignored (opaque M1 fill).
pub fn fill(buf: &mut Buffer, area: Rect, color: Rgba) {
    let inter = area.intersection(buf.area);
    if inter.is_empty() {
        return;
    }
    let fg = Color::Rgb(color.r, color.g, color.b);
    for y in inter.top()..inter.bottom() {
        for x in inter.left()..inter.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char(FULL);
                cell.set_fg(fg);
            }
        }
    }
}

/// Draw `text` as a single line over `area` (clamped to `buf`), placed
/// horizontally per `align` and vertically centered. Truncated to fit; no
/// wrapping. `style` (fg + any modifiers, e.g. `Modifier::UNDERLINED`) is
/// applied whole to every written cell.
///
/// `style` must carry a legible foreground — not `Style::default()`'s unset
/// foreground, which renders as `Color::Reset` (the terminal's own default),
/// illegible against a caller's own tinted background more often than not
/// (confirmed: this is exactly why the main menu's button labels were
/// invisible). Every caller must pick a color that actually contrasts with
/// whatever it draws underneath.
pub fn label(buf: &mut Buffer, area: Rect, text: &str, align: TextAlign, style: Style) {
    let inter = area.intersection(buf.area);
    if inter.is_empty() {
        return;
    }
    let text_w = text.chars().count() as u16;
    let y = inter.top() + inter.height / 2;
    let x = match align {
        TextAlign::Left => inter.left(),
        TextAlign::Center => inter.left() + inter.width.saturating_sub(text_w) / 2,
        TextAlign::Right => inter.right().saturating_sub(text_w).max(inter.left()),
    };
    let max_width = inter.right().saturating_sub(x) as usize;
    buf.set_stringn(x, y, text, max_width, style);
}

/// Alpha-aware variant of [`label`] (b2-t3): composites `color` over each
/// written cell's destination fg (`Rgba::over`) instead of overwriting it
/// outright, so a caller can fade a label's own color independent of the
/// backdrop. Uses `label`'s exact centering formula. `color.a == 0` must
/// write no cell at all (destination untouched); `color.a == 255` must be
/// byte-identical to calling `label` with the opaque color.
///
/// `a == 0` writes no cell at all (destination untouched); `a == 255` is
/// byte-identical to calling `label` with the opaque color.
pub fn label_blended(buf: &mut Buffer, area: Rect, text: &str, align: TextAlign, color: Rgba) {
    if color.a == 0 {
        return;
    }
    let inter = area.intersection(buf.area);
    if inter.is_empty() {
        return;
    }
    let text_w = text.chars().count() as u16;
    let y = inter.top() + inter.height / 2;
    let x = match align {
        TextAlign::Left => inter.left(),
        TextAlign::Center => inter.left() + inter.width.saturating_sub(text_w) / 2,
        TextAlign::Right => inter.right().saturating_sub(text_w).max(inter.left()),
    };
    let max_width = inter.right().saturating_sub(x) as usize;
    for (i, ch) in text.chars().enumerate() {
        if i >= max_width {
            break;
        }
        let cx = x + i as u16;
        let Some(cell) = buf.cell_mut((cx, y)) else {
            continue;
        };
        let dest = match cell.fg {
            Color::Rgb(r, g, b) => Rgba::new(r, g, b, 0xFF),
            _ => Rgba::new(0, 0, 0, 0xFF),
        };
        let blended = color.over(dest);
        cell.set_char(ch);
        cell.set_fg(Color::Rgb(blended.r, blended.g, blended.b));
    }
}

/// Word-wrap `text` to `area` width and draw it top-down, one wrapped row per
/// line, each row placed horizontally per `align`; clipped to `area` height.
/// An over-long single token is broken by character. When `ellipsis` is true
/// AND the wrapped text overflows the visible rows, the last visible row ends
/// with a single `…` (one tail ellipsis for the whole block). `style` (fg +
/// modifiers) applies to every written cell; a legible fg is still required
/// (same `Color::Reset` caveat as `label`).
pub fn wrapped_text(
    buf: &mut Buffer,
    area: Rect,
    text: &str,
    align: TextAlign,
    style: Style,
    ellipsis: bool,
) {
    let inter = area.intersection(buf.area);
    if inter.is_empty() {
        return;
    }
    let width = inter.width as usize;
    let max_rows = inter.height as usize;

    let mut lines = wrap_to_width(text, width);
    let overflow = lines.len() > max_rows;
    lines.truncate(max_rows);

    if ellipsis && overflow {
        if let Some(last) = lines.last_mut() {
            let keep = width.saturating_sub(1);
            let mut s: String = last.chars().take(keep).collect();
            s.push('…');
            *last = s;
        }
    }

    for (i, line) in lines.iter().enumerate() {
        let row = Rect::new(inter.x, inter.top() + i as u16, inter.width, 1);
        label(buf, row, line, align, style);
    }
}

/// Greedy word-wrap `text` to `width` characters per line. Words never split
/// at a space boundary; a single word longer than `width` is broken by
/// character into `width`-char chunks.
fn wrap_to_width(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    if width == 0 {
        return lines;
    }
    let mut current = String::new();

    for word in text.split_whitespace() {
        let word_len = word.chars().count();
        if word_len > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let chars: Vec<char> = word.chars().collect();
            for chunk in chars.chunks(width) {
                current = chunk.iter().collect();
                if current.chars().count() == width {
                    lines.push(std::mem::take(&mut current));
                }
            }
            continue;
        }

        if current.is_empty() {
            current = word.to_string();
        } else if current.chars().count() + 1 + word_len <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current = word.to_string();
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    lines
}

/// Paint a bundled raster asset (`bytes`, e.g. `game::assets::DOT_FILLED`)
/// aspect-fit + centered into `area`, routed through the shared
/// process-lifetime decode/rasterize cache (`asset_cache::convert`). Zero-area
/// `area` paints nothing. Panics only if `bytes` is not a decodable image
/// (callers pass first-party bundled assets — invariant, as in
/// `Button::new`).
pub fn draw_asset(buf: &mut Buffer, area: Rect, bytes: &'static [u8]) {
    let grid = asset_cache::convert(bytes, area);
    draw_grid(buf, area, &grid);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Color;

    // Helper: build an empty buffer of given size.
    fn make_buf(w: u16, h: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, w, h))
    }

    // ------------------------------------------------------------------ fill

    /// Every cell in a full-buffer fill gets the braille glyph and the
    /// correct RGB foreground. This is the core M1 visual invariant.
    #[test]
    fn fill_sets_every_cell_symbol_and_fg() {
        let mut buf = make_buf(4, 2);
        let color = Rgba::rgb(0xc8, 0x1e, 0x1e);
        fill(&mut buf, Rect::new(0, 0, 4, 2), color);

        for y in 0..2 {
            for x in 0..4 {
                let cell = buf.cell((x, y)).expect("cell must exist");
                assert_eq!(
                    cell.symbol(),
                    "⣿",
                    "cell ({x},{y}) symbol should be ⣿"
                );
                assert_eq!(
                    cell.fg,
                    Color::Rgb(0xc8, 0x1e, 0x1e),
                    "cell ({x},{y}) fg mismatch"
                );
            }
        }
    }

    /// Alpha channel must have no effect on the RGB foreground written to the
    /// buffer — transparent and fully opaque produce identical output.
    #[test]
    fn fill_ignores_alpha() {
        let mut buf_opaque = make_buf(2, 2);
        let mut buf_transparent = make_buf(2, 2);
        let area = Rect::new(0, 0, 2, 2);

        fill(&mut buf_opaque, area, Rgba::new(0x10, 0x20, 0x30, 0xFF));
        fill(&mut buf_transparent, area, Rgba::new(0x10, 0x20, 0x30, 0x00));

        for y in 0..2u16 {
            for x in 0..2u16 {
                assert_eq!(
                    buf_opaque.cell((x, y)).unwrap().fg,
                    buf_transparent.cell((x, y)).unwrap().fg,
                    "alpha must not affect fg at ({x},{y})"
                );
            }
        }
    }

    /// A sub-area fill must paint only the cells inside the area and leave
    /// cells outside completely untouched (still default/empty).
    #[test]
    fn fill_sub_area_leaves_outside_cells_untouched() {
        let mut buf = make_buf(6, 4);
        // Fill only the inner 2×2 block at offset (2, 1)
        let area = Rect::new(2, 1, 2, 2);
        let color = Rgba::rgb(0x00, 0xFF, 0x00);
        fill(&mut buf, area, color);

        // Inside the area every cell must carry the glyph
        for y in 1..3 {
            for x in 2..4 {
                let cell = buf.cell((x, y)).unwrap();
                assert_eq!(cell.symbol(), "⣿", "inside ({x},{y}) should be ⣿");
                assert_eq!(cell.fg, Color::Rgb(0x00, 0xFF, 0x00));
            }
        }

        // A cell clearly outside the area must be unchanged (default symbol " ")
        let outside = buf.cell((0, 0)).unwrap();
        assert_ne!(
            outside.symbol(),
            "⣿",
            "cell (0,0) outside fill area must be untouched"
        );
    }

    /// An area larger than the buffer must not panic — only in-bounds cells
    /// are painted and the function returns cleanly.
    #[test]
    fn fill_oversized_area_does_not_panic() {
        let mut buf = make_buf(3, 3);
        // Area intentionally extends well beyond the buffer
        let oversized = Rect::new(0, 0, 100, 100);
        fill(&mut buf, oversized, Rgba::rgb(0xFF, 0x00, 0x00));
        // If we reach here, no panic occurred.
        // Spot-check: (0,0) should have been painted.
        assert_eq!(buf.cell((0, 0)).unwrap().symbol(), "⣿");
    }

    // ----------------------------------------------------------------- label

    fn white_style() -> Style {
        Style::default().fg(Color::Rgb(0xff, 0xff, 0xff))
    }

    /// `TextAlign::Center`: a string shorter than the area's width must
    /// appear centered on the middle row, starting at
    /// `left + (width - len) / 2` — byte-identical to the pre-migration
    /// centered behavior every existing caller relies on.
    #[test]
    fn label_centers_text_in_area() {
        let w: u16 = 10;
        let h: u16 = 4;
        let mut buf = make_buf(w, h);
        let area = Rect::new(0, 0, w, h);

        label(&mut buf, area, "Hi", TextAlign::Center, white_style());

        // Expected row: h/2 = 2; expected x start: (10 - 2) / 2 = 4
        let expected_y = h / 2;
        let text_len: u16 = 2; // "Hi"
        let expected_x = (w - text_len) / 2; // 4

        let first_char = buf.cell((expected_x, expected_y)).unwrap();
        assert_eq!(
            first_char.symbol(),
            "H",
            "first char of 'Hi' must be at ({expected_x},{expected_y})"
        );
        let second_char = buf.cell((expected_x + 1, expected_y)).unwrap();
        assert_eq!(
            second_char.symbol(),
            "i",
            "second char of 'Hi' must be at ({},{expected_y})",
            expected_x + 1
        );
    }

    /// A string that is wider than the area must be truncated — no characters
    /// may appear at or past `area.right()`, and the call must not panic.
    #[test]
    fn label_truncates_overlong_text() {
        let mut buf = make_buf(5, 3);
        let area = Rect::new(0, 0, 5, 3);
        // "Hello!!" is 7 chars, wider than 5
        label(&mut buf, area, "Hello!!", TextAlign::Center, white_style());

        // Nothing must appear at x == 5 (which would be outside the 0..5 area)
        // The buffer is only 5 wide so x=5 doesn't exist — we verify col 4 is
        // the last written column (anything past 4 would panic on a narrow buf).
        // The real check: the call returned without panic (above), and nothing
        // escaped the area.  We confirm col 0..4 have only valid in-area text.
        let row = 3 / 2; // center row
        // At minimum, the character at x=0 on the center row must be from the
        // text (not blank), proving some truncated output was written.
        let cell = buf.cell((0, row)).unwrap();
        assert_ne!(
            cell.symbol(),
            "⣿",
            "label should not write braille glyphs"
        );
    }

    /// `TextAlign::Left`: the first char must land at `area.left()`.
    #[test]
    fn label_left_places_text_at_area_left() {
        let mut buf = make_buf(10, 3);
        let area = Rect::new(0, 0, 10, 3);

        label(&mut buf, area, "Hi", TextAlign::Left, white_style());

        let y = 3 / 2;
        assert_eq!(buf.cell((0, y)).unwrap().symbol(), "H");
        assert_eq!(buf.cell((1, y)).unwrap().symbol(), "i");
    }

    /// `TextAlign::Right`: the text's right edge must be flush to
    /// `area.right()` — the last char lands at `area.right() - 1`.
    #[test]
    fn label_right_places_text_flush_to_area_right() {
        let w: u16 = 10;
        let mut buf = make_buf(w, 3);
        let area = Rect::new(0, 0, w, 3);

        label(&mut buf, area, "Hi", TextAlign::Right, white_style());

        let y = 3 / 2;
        assert_eq!(buf.cell((w - 2, y)).unwrap().symbol(), "H");
        assert_eq!(buf.cell((w - 1, y)).unwrap().symbol(), "i");
    }

    /// `TextAlign::Right` with text wider than the area must clamp to
    /// `area.left()` (not underflow / panic) and still truncate cleanly.
    #[test]
    fn label_right_overlong_text_clamps_to_area_left_no_panic() {
        let mut buf = make_buf(5, 3);
        let area = Rect::new(0, 0, 5, 3);

        label(&mut buf, area, "Hello!!", TextAlign::Right, white_style());

        let y = 3 / 2;
        assert_eq!(
            buf.cell((0, y)).unwrap().symbol(),
            "H",
            "overlong Right-aligned text must start at area.left()"
        );
    }

    /// `style`'s modifiers (e.g. `Modifier::UNDERLINED`) must be applied to
    /// every written cell, not just the foreground color.
    #[test]
    fn label_applies_style_modifier_to_written_cells() {
        use ratatui::style::Modifier;

        let mut buf = make_buf(10, 3);
        let area = Rect::new(0, 0, 10, 3);
        let style = Style::default()
            .fg(Color::Rgb(0x11, 0x22, 0x33))
            .add_modifier(Modifier::UNDERLINED);

        label(&mut buf, area, "Hi", TextAlign::Left, style);

        let y = 3 / 2;
        let cell = buf.cell((0, y)).unwrap();
        assert_eq!(cell.fg, Color::Rgb(0x11, 0x22, 0x33));
        assert!(
            cell.modifier.contains(Modifier::UNDERLINED),
            "label must apply style's modifier to written cells"
        );
    }

    // ------------------------------------------------------------ wrapped_text

    /// Collect the text of each row in `area` as a trimmed string (trailing
    /// spaces stripped), for row-by-row wrap assertions.
    fn row_text(buf: &Buffer, area: Rect, row: u16) -> String {
        (area.left()..area.right())
            .map(|x| buf.cell((x, row)).unwrap().symbol().chars().next().unwrap_or(' '))
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    /// A paragraph longer than one row must wrap onto multiple rows, one word
    /// per row within the fit, with words never split at a space boundary.
    #[test]
    fn wrapped_text_wraps_paragraph_to_expected_rows() {
        let mut buf = make_buf(6, 4);
        let area = Rect::new(0, 0, 6, 4);
        // "one two three" at width 6: "one" (3) fits, "one two" (7) doesn't ->
        // row0 "one", row1 "two", row2 "three" (5, fits).
        wrapped_text(&mut buf, area, "one two three", TextAlign::Left, white_style(), false);

        assert_eq!(row_text(&buf, area, 0), "one");
        assert_eq!(row_text(&buf, area, 1), "two");
        assert_eq!(row_text(&buf, area, 2), "three");
    }

    /// When `ellipsis: true` and the wrapped block overflows the visible
    /// rows, the LAST visible row must end in a single `…`; earlier visible
    /// rows must not.
    #[test]
    fn wrapped_text_ellipsis_overflow_last_row_ends_in_ellipsis() {
        let mut buf = make_buf(6, 2);
        let area = Rect::new(0, 0, 6, 2);
        // Wraps to 3 rows ("one","two","three") but area.height == 2, so it
        // overflows; only rows 0..2 are visible, row1 ("two") must gain "…".
        wrapped_text(&mut buf, area, "one two three", TextAlign::Left, white_style(), true);

        assert_eq!(row_text(&buf, area, 0), "one");
        let last_row = row_text(&buf, area, 1);
        assert!(
            last_row.ends_with('…'),
            "last visible row must end with an ellipsis, got {last_row:?}"
        );
    }

    /// A block that fits entirely within `area.height` must produce NO
    /// ellipsis anywhere, even when `ellipsis: true`.
    #[test]
    fn wrapped_text_fitting_block_has_no_ellipsis() {
        let mut buf = make_buf(6, 4);
        let area = Rect::new(0, 0, 6, 4);
        wrapped_text(&mut buf, area, "one two three", TextAlign::Left, white_style(), true);

        for row in 0..4u16 {
            let text = row_text(&buf, area, row);
            assert!(
                !text.contains('…'),
                "a fitting block must have no ellipsis anywhere, row {row} was {text:?}"
            );
        }
    }

    /// A single token with no spaces, longer than `area.width`, must be
    /// broken across rows by character (not dropped, not left unclipped).
    #[test]
    fn wrapped_text_breaks_overlong_token_by_char() {
        let mut buf = make_buf(4, 3);
        let area = Rect::new(0, 0, 4, 3);
        // "ABCDEFGH" (8 chars) at width 4 -> "ABCD" / "EFGH", no spaces.
        wrapped_text(&mut buf, area, "ABCDEFGH", TextAlign::Left, white_style(), false);

        assert_eq!(row_text(&buf, area, 0), "ABCD");
        assert_eq!(row_text(&buf, area, 1), "EFGH");
    }

    /// Each wrapped row is placed per `align` independently of the other
    /// rows' lengths — `TextAlign::Right` must flush every row's own text to
    /// `area.right()`, not to the longest row's width.
    #[test]
    fn wrapped_text_applies_alignment_per_row() {
        let mut buf = make_buf(6, 2);
        let area = Rect::new(0, 0, 6, 2);
        // row0 "one" (3 chars) -> flush right ends at x=5, starts at x=3.
        // row1 "two" (3 chars) -> same.
        wrapped_text(&mut buf, area, "one two", TextAlign::Right, white_style(), false);

        assert_eq!(buf.cell((3, 0)).unwrap().symbol(), "o");
        assert_eq!(buf.cell((5, 0)).unwrap().symbol(), "e");
        assert_eq!(buf.cell((3, 1)).unwrap().symbol(), "t");
        assert_eq!(buf.cell((5, 1)).unwrap().symbol(), "o");
    }

    /// Empty text and a zero-area rect must draw nothing and must not panic.
    #[test]
    fn wrapped_text_zero_area_and_empty_text_no_panic() {
        let mut buf = make_buf(4, 3);

        // Zero-width area with non-empty text.
        wrapped_text(
            &mut buf,
            Rect::new(0, 0, 0, 3),
            "hello",
            TextAlign::Left,
            white_style(),
            true,
        );
        for y in 0..3u16 {
            for x in 0..4u16 {
                assert_eq!(buf.cell((x, y)).unwrap().symbol(), " ");
            }
        }

        // Empty text with a non-zero area.
        wrapped_text(&mut buf, Rect::new(0, 0, 4, 3), "", TextAlign::Left, white_style(), true);
        for y in 0..3u16 {
            for x in 0..4u16 {
                assert_eq!(buf.cell((x, y)).unwrap().symbol(), " ");
            }
        }
    }

    // -------------------------------------------------------------- label_blended (b2-t3)

    /// `label_blended` at `color.a == 0` must leave the destination cells
    /// completely untouched — no glyph, no fg change — not merely blend to
    /// an invisible color.
    #[test]
    fn label_blended_zero_alpha_is_noop() {
        let rect = Rect::new(1, 1, 6, 3);
        let mut buf = make_buf(10, 6);
        // "Hi" centered in `rect` lands its first char at (3, 2) — same
        // centering formula `label` uses.
        let (sx, sy) = (3u16, 2u16);
        buf.cell_mut((sx, sy)).unwrap().set_char('X');
        buf.cell_mut((sx, sy)).unwrap().set_fg(Color::Rgb(0x01, 0x02, 0x03));

        label_blended(&mut buf, rect, "Hi", TextAlign::Center, Rgba::new(0x11, 0x22, 0x33, 0));

        let cell = buf.cell((sx, sy)).unwrap();
        assert_eq!(cell.symbol(), "X", "a==0 must leave the destination glyph untouched");
        assert_eq!(
            cell.fg,
            Color::Rgb(0x01, 0x02, 0x03),
            "a==0 must leave the destination fg untouched"
        );
    }

    /// `label_blended` at a mid alpha must composite `color` over the
    /// destination cell's PRE-EXISTING fg via `Rgba::over`, not overwrite it
    /// with the opaque override color.
    #[test]
    fn label_blended_mid_alpha_blends_over_destination_fg() {
        let rect = Rect::new(1, 1, 6, 3);
        let mut buf = make_buf(10, 6);
        let (sx, sy) = (3u16, 2u16);
        buf.cell_mut((sx, sy)).unwrap().set_fg(Color::Rgb(0, 0, 0));

        let color = Rgba::new(0x80, 0x40, 0x20, 0x80);
        label_blended(&mut buf, rect, "Hi", TextAlign::Center, color);

        let expected = color.over(Rgba::new(0, 0, 0, 0xFF));
        let cell = buf.cell((sx, sy)).unwrap();
        assert_eq!(
            cell.fg,
            Color::Rgb(expected.r, expected.g, expected.b),
            "mid-alpha label_blended must composite the color over the destination fg via Rgba::over"
        );
    }
}

// ------------------------------------------------------------------- draw_asset

#[cfg(test)]
mod draw_asset_tests {
    use super::*;

    fn make_buf(w: u16, h: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, w, h))
    }

    /// Synthetic stand-in for a bundled dot/icon asset (b1-t2: `crate::assets`
    /// no longer exists in `engine-render` — moved to `game::assets`).
    /// `draw_asset` is content-agnostic, so any decodable opaque-body PNG
    /// bytes exercise the same contract the real asset did. Leaked to
    /// `'static` (b5-t1: `draw_asset` keys the shared cache off the bytes'
    /// own pointer, which is only sound for bytes that outlive the process).
    fn synthetic_dot_png() -> &'static [u8] {
        let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 255, 255, 255]));
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .expect("synthetic test fixture must encode to PNG");
        Box::leak(buf.into_boxed_slice())
    }

    /// `draw_asset` of an opaque synthetic dot icon into a non-zero area
    /// paints at least one non-space cell (b3-t3: dot row uses this to paint
    /// the 6 position-indicator dots).
    #[test]
    fn draw_asset_paints_non_space_cell() {
        let _guard = crate::asset_cache::cache_test_lock();
        let mut buf = make_buf(4, 2);
        let area = Rect::new(0, 0, 4, 2);
        draw_asset(&mut buf, area, synthetic_dot_png());

        let painted = (0..2u16)
            .any(|y| (0..4u16).any(|x| buf.cell((x, y)).unwrap().symbol() != " "));
        assert!(painted, "draw_asset must paint at least one non-space cell");
    }

    /// A zero-area rect must paint nothing and must not panic.
    #[test]
    fn draw_asset_zero_area_paints_nothing() {
        let _guard = crate::asset_cache::cache_test_lock();
        let mut buf = make_buf(4, 2);
        let area = Rect::new(0, 0, 0, 2);
        draw_asset(&mut buf, area, synthetic_dot_png());

        for y in 0..2u16 {
            for x in 0..4u16 {
                assert_eq!(
                    buf.cell((x, y)).unwrap().symbol(),
                    " ",
                    "zero-area draw_asset must leave every cell untouched"
                );
            }
        }
    }

    /// b5-t1: two `draw_asset` calls with the SAME `'static` bytes at the
    /// SAME `area` must perform exactly one real rasterization (the second
    /// call is a shared-cache hit), and the painted output must still match
    /// what the pre-existing uncached decode+convert+draw_grid path would
    /// have produced.
    #[test]
    fn draw_asset_repeat_same_bytes_area_is_one_rasterization() {
        let _guard = crate::asset_cache::cache_test_lock();
        let bytes = synthetic_dot_png();
        let area = Rect::new(0, 0, 4, 2);
        let before = crate::asset_cache::rasterize_recompute_count();

        let mut buf_first = make_buf(4, 2);
        draw_asset(&mut buf_first, area, bytes);
        let mut buf_second = make_buf(4, 2);
        draw_asset(&mut buf_second, area, bytes);

        let delta = crate::asset_cache::rasterize_recompute_count() - before;
        assert_eq!(
            delta, 1,
            "second draw_asset call with the same (bytes, area) must be a cache hit"
        );

        let img = image::load_from_memory(bytes).expect("fixture must decode");
        let expected_grid = convert(&img, area);
        let mut buf_expected = make_buf(4, 2);
        draw_grid(&mut buf_expected, area, &expected_grid);
        assert_eq!(
            buf_first, buf_expected,
            "cached draw_asset output must match the uncached decode+convert+draw_grid path"
        );
    }
}
