//! Braille image conversion.
//!
//! Faithful port of `experiments/ascii_test/src/downrez.rs` into a pure
//! function that returns a [`Grid`] instead of an ANSI string, and fits
//! to a two-axis `area` instead of a single `--width`.

use image::DynamicImage;
use ratatui::layout::Rect;

use crate::grid::Grid;

/// Convert `img` into a braille [`Grid`] aspect-preserving fitted into `area`.
///
/// Returns `Grid::new(0, 0)` when `area.width == 0` or `area.height == 0`.
pub fn convert(img: &DynamicImage, area: Rect) -> Grid {
    if area.width == 0 || area.height == 0 {
        return Grid::new(0, 0);
    }

    // Guard against a degenerate source (should never happen with real images).
    let sh = img.height().max(1) as f32;
    let aspect = img.width() as f32 / sh;

    // Aspect-preserving fit.  Braille dots are square: one cell = 2 px wide × 4 px tall,
    // so the pixel aspect ratio equals cols*2 / (rows*4) = cols/(2*rows) = image aspect.
    // Fit width-first; if the resulting row count overflows area height, fit height-first.
    let mut cols = area.width as u32;
    let mut rows = (cols as f32 / (2.0 * aspect)).round().max(1.0) as u32;

    if rows > area.height as u32 {
        rows = area.height as u32;
        cols = (rows as f32 * 2.0 * aspect).round().max(1.0) as u32;
    }

    // Clamp to area bounds.
    cols = cols.clamp(1, area.width as u32);
    rows = rows.clamp(1, area.height as u32);

    crate::dots::dots_to_grid(&crate::dots::sprite_to_dots(img, cols * 2, rows * 4))
}

#[cfg(test)]
mod tests {
    use super::convert;
    use crate::grid::Cell;
    use image::{DynamicImage, Rgba as PixelRgba, RgbaImage};
    use ratatui::layout::Rect;

    // ── helper ────────────────────────────────────────────────────────────────

    /// Build a uniform RGBA image where every pixel is (r, g, b, a).
    fn solid_image(w: u32, h: u32, r: u8, g: u8, b: u8, a: u8) -> DynamicImage {
        let mut raw = RgbaImage::new(w, h);
        for p in raw.pixels_mut() {
            *p = PixelRgba([r, g, b, a]);
        }
        DynamicImage::from(raw)
    }

    // ── smoke tests (this task's deliverable) ─────────────────────────────────

    /// A solid opaque single-color 16×16 image must convert to a non-empty
    /// grid where every cell is the fully-lit glyph '⣿' (all 8 dots set,
    /// mask = 0xFF) in exactly the source color.
    ///
    /// Hand-derivation for area = Rect(0,0,8,4) with a 16×16 source (aspect=1):
    ///   cols = 8, rows = round(8 / (2·1)) = 4
    ///   resize_exact(16,16) → identity (source is already 16×16).
    ///   Per 2×4 cell: all 8 dots opaque; all same luma → all ≥ mean → mask=0xFF.
    ///   fg = integer mean of 8 identical pixels = source color exactly.
    #[test]
    fn convert_solid_opaque_produces_full_glyph() {
        let (r, g, b) = (200u8, 100u8, 50u8);
        let img = solid_image(16, 16, r, g, b, 255);
        let area = Rect::new(0, 0, 8, 4);
        let grid = convert(&img, area);

        assert!(
            grid.cols() > 0 && grid.rows() > 0,
            "non-zero area must produce a non-empty grid"
        );

        for row in 0..grid.rows() {
            for col in 0..grid.cols() {
                match grid.get(col, row) {
                    Cell::Glyph { ch, color } => {
                        assert_eq!(
                            ch, '⣿',
                            "cell ({col},{row}): flat solid region must produce the fully-lit glyph"
                        );
                        assert_eq!(color.r, r, "cell ({col},{row}): red mismatch");
                        assert_eq!(color.g, g, "cell ({col},{row}): green mismatch");
                        assert_eq!(color.b, b, "cell ({col},{row}): blue mismatch");
                    }
                    Cell::Transparent => {
                        panic!(
                            "cell ({col},{row}) is Transparent, expected Glyph for solid opaque image"
                        );
                    }
                }
            }
        }
    }

    /// A fully transparent (alpha = 0) image must convert to a grid whose
    /// every cell is `Cell::Transparent`.  The grid must not be empty —
    /// the dimensions are set by the aspect fit, not by pixel alpha.
    #[test]
    fn convert_fully_transparent_produces_transparent_grid() {
        let img = solid_image(16, 16, 255, 255, 255, 0);
        let area = Rect::new(0, 0, 8, 4);
        let grid = convert(&img, area);

        assert!(
            grid.cols() > 0 && grid.rows() > 0,
            "non-zero area transparent image must produce a non-empty grid \
             (all Cell::Transparent, not a 0×0 empty grid)"
        );

        for row in 0..grid.rows() {
            for col in 0..grid.cols() {
                assert_eq!(
                    grid.get(col, row),
                    Cell::Transparent,
                    "cell ({col},{row}) must be Transparent for a fully transparent source"
                );
            }
        }
    }

    /// A zero-dimension area (width = 0 or height = 0) must return `Grid::new(0, 0)`.
    #[test]
    fn convert_zero_area_returns_empty_grid() {
        let img = solid_image(16, 16, 200, 100, 50, 255);

        let g_zero_w = convert(&img, Rect::new(0, 0, 0, 4));
        assert_eq!(g_zero_w.cols(), 0, "zero-width area: expected cols = 0");
        assert_eq!(g_zero_w.rows(), 0, "zero-width area: expected rows = 0");

        let g_zero_h = convert(&img, Rect::new(0, 0, 8, 0));
        assert_eq!(g_zero_h.cols(), 0, "zero-height area: expected cols = 0");
        assert_eq!(g_zero_h.rows(), 0, "zero-height area: expected rows = 0");
    }
}
