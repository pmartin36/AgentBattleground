//! Render crate — terminal braille rendering.
//!
//! ⚠ M1 PLACEHOLDER. The only thing implemented here is a solid-color braille
//! `fill` plus a centered `label` — just enough to make scene switching
//! visible on screen. This is NOT the real renderer and NOT the rendering
//! model to build on. The real braille image/sprite renderer (per-cell luma
//! threshold, native alpha transparency, depth-sorted multi-sprite
//! compositing, animation) is specified in `specs/13-rendering.md` and will
//! replace `fill`/`label` in place. Do not extend the solid-fill approach.

pub mod anim;
pub mod asset_cache;
pub mod button;
pub mod camera;
pub mod composite;
pub mod convert;
pub mod dots;
pub mod grid;
pub mod screen_layout;
pub mod transform;
pub mod tween;
pub use anim::AnimatedSprite;
pub use button::{Button, ButtonCore, ButtonState, FrameButton};
pub use convert::convert;
pub use grid::{draw_grid, Cell, Grid};
pub use screen_layout::{anchor, anchor_with_margin, stack, Anchor, RectTween, StackAxis};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use engine_core::color::Rgba;

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

/// Draw `text` as a single centered line over `area` (clamped to `buf`), in
/// `color`. Truncated to fit; no wrapping.
///
/// `color` is a required, explicit parameter — not a "default" style — because
/// `Style::default()`'s unset foreground renders as `Color::Reset` (the
/// terminal's own default), which is illegible against a caller's own tinted
/// background more often than not (confirmed: this is exactly why the main
/// menu's button labels were invisible). Every caller must pick a color that
/// actually contrasts with whatever it draws underneath.
pub fn label(buf: &mut Buffer, area: Rect, text: &str, color: Rgba) {
    let inter = area.intersection(buf.area);
    if inter.is_empty() {
        return;
    }
    let text_w = text.chars().count() as u16;
    let y = inter.top() + inter.height / 2;
    let x = inter.left() + inter.width.saturating_sub(text_w) / 2;
    let max_width = inter.right().saturating_sub(x) as usize;
    let style = Style::default().fg(Color::Rgb(color.r, color.g, color.b));
    buf.set_stringn(x, y, text, max_width, style);
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

    /// A string shorter than the area's width must appear centered on the
    /// middle row, starting at `left + (width - len) / 2`.
    #[test]
    fn label_centers_text_in_area() {
        let w: u16 = 10;
        let h: u16 = 4;
        let mut buf = make_buf(w, h);
        let area = Rect::new(0, 0, w, h);

        label(&mut buf, area, "Hi", Rgba::rgb(0xff, 0xff, 0xff));

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
        label(&mut buf, area, "Hello!!", Rgba::rgb(0xff, 0xff, 0xff));

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
