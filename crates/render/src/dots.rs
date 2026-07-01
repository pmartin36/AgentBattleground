//! Dot-level intermediate representation for the braille rendering pipeline.
//!
//! `Dot` is the sub-cell pixel type; `DotBuffer` is a cols×rows grid of dots
//! in dot units (1 dot = 1 sub-cell pixel, 2 dots wide × 4 dots tall per braille cell).

use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView};
use scene_core::color::Rgba;

use crate::grid::Grid;

/// One sub-cell dot: transparent (reveals what is behind) or a lit color.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Dot {
    #[default]
    Transparent,
    Lit(Rgba),
}

/// A cols×rows buffer of dots in dot units, row-major.
/// `dots[row * cols + col]` addresses dot (col, row).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DotBuffer {
    cols: usize,
    rows: usize,
    dots: Vec<Dot>,
}

impl DotBuffer {
    /// Creates an all-`Transparent` buffer of the given size.
    pub fn new(cols: usize, rows: usize) -> Self {
        DotBuffer {
            cols,
            rows,
            dots: vec![Dot::Transparent; cols * rows],
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Returns the dot at `(col, row)`. Panics if out of bounds.
    pub fn get(&self, col: usize, row: usize) -> Dot {
        assert!(col < self.cols, "col {col} out of bounds (cols={})", self.cols);
        assert!(row < self.rows, "row {row} out of bounds (rows={})", self.rows);
        self.dots[row * self.cols + col]
    }

    /// Sets the dot at `(col, row)`.  Silent no-op when `col >= self.cols || row >= self.rows`.
    pub fn set(&mut self, col: usize, row: usize, dot: Dot) {
        if col >= self.cols || row >= self.rows {
            return;
        }
        self.dots[row * self.cols + col] = dot;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Braille dot constants and helpers
// ─────────────────────────────────────────────────────────────────────────────

/// (dx, dy, bit-position) for each of the 8 braille dots in a 2×4 dot block.
/// The bit index equals the array index: `DOTS[k].2 == k` for all k.
/// Moved from `convert.rs` so both `dots_to_grid` and the old convert path
/// share one definition.
const DOTS: [(u32, u32, u8); 8] = [
    (0, 0, 0),
    (0, 1, 1),
    (0, 2, 2),
    (1, 0, 3),
    (1, 1, 4),
    (1, 2, 5),
    (0, 3, 6),
    (1, 3, 7),
];

/// Perceived brightness from an RGB triplet (truncating float→u8).
fn luma(r: u8, g: u8, b: u8) -> u8 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) as u8
}

/// Map 8 dots (in `DOTS` order, index k → bit k) to a single braille [`Cell`].
///
/// - If no dot is `Lit` → `Cell::Transparent`.
/// - Otherwise: `avg = Σluma(visible) / count` (integer); a visible dot with
///   `luma >= avg` sets its bit; `ch = U+2800 + mask`; `color` = integer mean
///   RGB over all visible dots.
fn cell_from_dots(dots: &[Dot; 8]) -> crate::grid::Cell {
    use crate::grid::Cell;

    // Per-dot luma: None for Transparent, Some(luma) for Lit.
    let lumas: [Option<u8>; 8] =
        std::array::from_fn(|k| match dots[k] {
            Dot::Lit(c) => Some(luma(c.r, c.g, c.b)),
            Dot::Transparent => None,
        });

    let visible_lumas: Vec<u8> = lumas.iter().filter_map(|l| *l).collect();
    if visible_lumas.is_empty() {
        return Cell::Transparent;
    }

    // Integer mean luma of visible dots.
    let avg = visible_lumas.iter().map(|&l| l as u32).sum::<u32>()
        / visible_lumas.len() as u32;

    // Bit mask: set bit k when dot k is visible AND luma >= avg.
    // DOTS[k].2 == k, so mask bit index equals array index.
    let mut mask = 0u32;
    for (k, &maybe_luma) in lumas.iter().enumerate() {
        if let Some(l) = maybe_luma {
            if l as u32 >= avg {
                mask |= 1 << k;
            }
        }
    }

    if mask == 0 {
        // Defensive guard; unreachable when visible is non-empty.
        return Cell::Transparent;
    }

    let ch = char::from_u32(0x2800 + mask).unwrap_or(' ');

    // Foreground = integer mean RGB over all visible (Lit) dots.
    let visible_colors: Vec<Rgba> = dots
        .iter()
        .filter_map(|d| if let Dot::Lit(c) = d { Some(*c) } else { None })
        .collect();
    let n = visible_colors.len() as u32;
    let (r, g, b) = visible_colors.iter().fold((0u32, 0u32, 0u32), |(ar, ag, ab), c| {
        (ar + c.r as u32, ag + c.g as u32, ab + c.b as u32)
    });
    let color = Rgba::rgb((r / n) as u8, (g / n) as u8, (b / n) as u8);

    Cell::Glyph { ch, color }
}

// ─────────────────────────────────────────────────────────────────────────────
// sprite_to_dots
// ─────────────────────────────────────────────────────────────────────────────

/// Rasterize `img` into a `DotBuffer` of `dot_cols × dot_rows` dots.
///
/// The source is resized (Lanczos3) to exactly the dot dimensions; each
/// resulting pixel becomes one dot: `Lit(rgb)` when alpha >= 128, else
/// `Transparent`. Caller supplies fitted dot dims (typically `cols*2 × rows*4`).
pub fn sprite_to_dots(img: &DynamicImage, dot_cols: u32, dot_rows: u32) -> DotBuffer {
    if dot_cols == 0 || dot_rows == 0 {
        return DotBuffer::new(dot_cols as usize, dot_rows as usize);
    }
    let src = img.resize_exact(dot_cols, dot_rows, FilterType::Lanczos3);
    let mut buf = DotBuffer::new(dot_cols as usize, dot_rows as usize);
    for y in 0..dot_rows {
        for x in 0..dot_cols {
            let p = src.get_pixel(x, y).0; // [r, g, b, a]
            if p[3] >= 128 {
                buf.set(x as usize, y as usize, Dot::Lit(Rgba::rgb(p[0], p[1], p[2])));
            }
            // else: leave default Dot::Transparent
        }
    }
    buf
}

// ─────────────────────────────────────────────────────────────────────────────
// tint
// ─────────────────────────────────────────────────────────────────────────────

/// Recolor every `Lit` dot in `buf` to `color`; `Transparent` dots pass
/// through unchanged. Same dims as `buf`. General-purpose (no "team" concept
/// baked in) — purely a function of each dot's own state, position-independent.
pub fn tint(buf: &DotBuffer, color: Rgba) -> DotBuffer {
    let mut out = DotBuffer::new(buf.cols(), buf.rows());
    for row in 0..buf.rows() {
        for col in 0..buf.cols() {
            let dot = match buf.get(col, row) {
                Dot::Lit(_) => Dot::Lit(color),
                Dot::Transparent => Dot::Transparent,
            };
            out.set(col, row, dot);
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// dots_to_grid  (stub — implementation by code-writer)
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a `DotBuffer` into a braille [`Grid`].
///
/// Grid dims = `(buf.cols()/2, buf.rows()/4)`.  Each 2×4 dot block maps to one
/// braille cell via the adaptive-luma rule (see `cell_from_dots`).
/// Out-of-range dot reads are treated as `Dot::Transparent`.
pub fn dots_to_grid(buf: &DotBuffer) -> Grid {
    let cell_cols = buf.cols() / 2;
    let cell_rows = buf.rows() / 4;
    let mut grid = Grid::new(cell_cols, cell_rows);

    for ty in 0..cell_rows {
        for tx in 0..cell_cols {
            // Gather the 8 dots for this 2×4 block in DOTS order.
            // dots[k] corresponds to DOTS[k], contributing bit k.
            let mut arr = [Dot::Transparent; 8];
            for k in 0..8 {
                let (dx, dy, _bit) = DOTS[k];
                let col = tx * 2 + dx as usize;
                let row = ty * 4 + dy as usize;
                arr[k] = if col < buf.cols() && row < buf.rows() {
                    buf.get(col, row)
                } else {
                    Dot::Transparent
                };
            }
            let cell = cell_from_dots(&arr);
            if cell != crate::grid::Cell::Transparent {
                grid.set(tx, ty, cell);
            }
        }
    }

    grid
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Cell;
    use image::{DynamicImage, Rgba as PixelRgba, RgbaImage};
    use scene_core::color::Rgba;

    // ── helper ────────────────────────────────────────────────────────────────

    /// Build a uniform RGBA image where every pixel is `(r, g, b, a)`.
    fn solid_image(w: u32, h: u32, r: u8, g: u8, b: u8, a: u8) -> DynamicImage {
        let mut raw = RgbaImage::new(w, h);
        for p in raw.pixels_mut() {
            *p = PixelRgba([r, g, b, a]);
        }
        DynamicImage::from(raw)
    }

    /// Build an image that is opaque on the left half and fully transparent on
    /// the right half.  Source width == `dot_cols` so the resize is an identity
    /// and Lanczos3 ringing cannot bleed across the seam.
    fn half_opaque_image(dot_cols: u32, dot_rows: u32, r: u8, g: u8, b: u8) -> DynamicImage {
        let mut raw = RgbaImage::new(dot_cols, dot_rows);
        for y in 0..dot_rows {
            for x in 0..dot_cols {
                let a = if x < dot_cols / 2 { 255u8 } else { 0u8 };
                raw.put_pixel(x, y, PixelRgba([r, g, b, a]));
            }
        }
        DynamicImage::from(raw)
    }

    // ── sprite_to_dots tests ──────────────────────────────────────────────────

    /// A uniform opaque image must produce a buffer where every dot is
    /// `Lit` carrying the source RGB, and dims must match the requested
    /// dot_cols × dot_rows.  A uniform source is color-preserving under
    /// Lanczos3, so the exact color assert is safe.
    #[test]
    fn sprite_to_dots_solid_opaque_all_lit() {
        let (r, g, b) = (100u8, 150u8, 200u8);
        let img = solid_image(8, 8, r, g, b, 255);
        let buf = sprite_to_dots(&img, 4, 8);

        assert_eq!(buf.cols(), 4, "cols must equal dot_cols");
        assert_eq!(buf.rows(), 8, "rows must equal dot_rows");

        for row in 0..buf.rows() {
            for col in 0..buf.cols() {
                let dot = buf.get(col, row);
                match dot {
                    Dot::Lit(c) => {
                        assert_eq!(c.r, r, "dot ({col},{row}): red mismatch");
                        assert_eq!(c.g, g, "dot ({col},{row}): green mismatch");
                        assert_eq!(c.b, b, "dot ({col},{row}): blue mismatch");
                    }
                    Dot::Transparent => panic!(
                        "dot ({col},{row}) must be Lit for a fully opaque source; got Transparent"
                    ),
                }
            }
        }
    }

    /// A uniform transparent image (alpha = 0) must produce a buffer where
    /// every dot is `Dot::Transparent`; dimensions still match the request.
    #[test]
    fn sprite_to_dots_fully_transparent_all_transparent() {
        let img = solid_image(8, 8, 255, 0, 0, 0);
        let buf = sprite_to_dots(&img, 6, 4);

        assert_eq!(buf.cols(), 6, "cols must equal dot_cols");
        assert_eq!(buf.rows(), 4, "rows must equal dot_rows");

        for row in 0..buf.rows() {
            for col in 0..buf.cols() {
                assert_eq!(
                    buf.get(col, row),
                    Dot::Transparent,
                    "dot ({col},{row}) must be Transparent for a fully transparent source"
                );
            }
        }
    }

    /// Left half opaque, right half transparent.  The leftmost column must be
    /// `Lit` and the rightmost column must be `Transparent`.  Source width ==
    /// dot_cols so no Lanczos ringing crosses the seam.
    #[test]
    fn sprite_to_dots_half_opaque_split() {
        let (r, g, b) = (200u8, 100u8, 50u8);
        let dot_cols = 8u32;
        let dot_rows = 4u32;
        let img = half_opaque_image(dot_cols, dot_rows, r, g, b);
        let buf = sprite_to_dots(&img, dot_cols, dot_rows);

        assert_eq!(buf.cols(), dot_cols as usize);
        assert_eq!(buf.rows(), dot_rows as usize);

        // Leftmost column (col 0): fully opaque in source → must be Lit.
        for row in 0..buf.rows() {
            match buf.get(0, row) {
                Dot::Lit(_) => {}
                Dot::Transparent => panic!(
                    "dot (0,{row}) must be Lit (left half is opaque)"
                ),
            }
        }

        // Rightmost column (col dot_cols-1): fully transparent in source → must be Transparent.
        let last_col = buf.cols() - 1;
        for row in 0..buf.rows() {
            assert_eq!(
                buf.get(last_col, row),
                Dot::Transparent,
                "dot ({last_col},{row}) must be Transparent (right half is transparent)"
            );
        }
    }

    /// `DotBuffer::new` must produce the given dimensions and every dot must
    /// start as `Dot::Transparent`.
    #[test]
    fn dotbuffer_new_all_transparent_with_correct_dims() {
        let buf = DotBuffer::new(3, 2);
        assert_eq!(buf.cols(), 3, "cols must match constructor arg");
        assert_eq!(buf.rows(), 2, "rows must match constructor arg");
        for row in 0..2 {
            for col in 0..3 {
                assert_eq!(
                    buf.get(col, row),
                    Dot::Transparent,
                    "dot ({col},{row}) must start Transparent"
                );
            }
        }
    }

    /// `set` then `get` round-trips a `Dot::Lit`; all neighbours stay `Transparent`.
    #[test]
    fn dotbuffer_get_set_round_trip() {
        let mut buf = DotBuffer::new(3, 2);
        let color = Rgba::rgb(0xAB, 0xCD, 0xEF);
        let lit = Dot::Lit(color);
        buf.set(1, 0, lit);
        assert_eq!(buf.get(1, 0), lit, "set then get must return the same Dot");
        // Neighbours must be untouched.
        assert_eq!(buf.get(0, 0), Dot::Transparent);
        assert_eq!(buf.get(2, 0), Dot::Transparent);
        assert_eq!(buf.get(0, 1), Dot::Transparent);
    }

    /// `set` with an out-of-bounds coordinate must be a silent no-op:
    /// no panic and the buffer is unchanged from a freshly-constructed one.
    #[test]
    fn dotbuffer_set_out_of_bounds_is_noop() {
        let cols = 2usize;
        let rows = 2usize;
        let mut buf = DotBuffer::new(cols, rows);
        let color = Rgba::rgb(0xFF, 0x00, 0x00);
        // Both col and row out of bounds — must not panic.
        buf.set(999, 999, Dot::Lit(color));
        // col out of bounds only.
        buf.set(cols, 0, Dot::Lit(color));
        // row out of bounds only.
        buf.set(0, rows, Dot::Lit(color));
        // Entire buffer must still equal a fresh buffer of the same size.
        let expected = DotBuffer::new(cols, rows);
        assert_eq!(
            buf, expected,
            "out-of-bounds set must leave the buffer unchanged"
        );
    }

    // ── dots_to_grid tests ────────────────────────────────────────────────────

    /// A 2×4 DotBuffer (= one braille cell) with all dots Transparent must
    /// produce a 1×1 Grid whose only cell is Cell::Transparent.
    #[test]
    fn dots_to_grid_all_transparent_yields_transparent_cell() {
        let buf = DotBuffer::new(2, 4); // all Transparent by default
        let grid = dots_to_grid(&buf);
        assert_eq!(grid.cols(), 1, "2 dot-cols → 1 cell-col");
        assert_eq!(grid.rows(), 1, "4 dot-rows → 1 cell-row");
        assert_eq!(
            grid.get(0, 0),
            Cell::Transparent,
            "all-transparent 2×4 block must produce Cell::Transparent"
        );
    }

    /// `dots_to_grid` must produce a Grid whose dimensions are (cols/2, rows/4).
    #[test]
    fn dots_to_grid_dims_are_halved() {
        let buf = DotBuffer::new(4, 8); // 4 dot-cols, 8 dot-rows
        let grid = dots_to_grid(&buf);
        assert_eq!(grid.cols(), 2, "4 dot-cols → 2 cell-cols");
        assert_eq!(grid.rows(), 2, "8 dot-rows → 2 cell-rows");
    }

    /// Hand-derived oracle from golden case (a): 8 specific Lit dots in a 2×4
    /// buffer must produce glyph U+28C8 ('⣈') with fg Rgba::rgb(91, 98, 106).
    ///
    /// DOTS order: (dx,dy,bit) = (0,0,0),(0,1,1),(0,2,2),(1,0,3),(1,1,4),(1,2,5),(0,3,6),(1,3,7)
    /// Mapping dot positions in DotBuffer(cols=2, rows=4):
    ///   bit 0 → (col=0,row=0): Lit([10,20,30])   luma=18
    ///   bit 1 → (col=0,row=1): Lit([40,50,60])   luma=48
    ///   bit 2 → (col=0,row=2): Lit([70,80,90])   luma=78
    ///   bit 3 → (col=1,row=0): Lit([200,210,220]) luma=208
    ///   bit 4 → (col=1,row=1): Lit([15,25,35])   luma=23
    ///   bit 5 → (col=1,row=2): Lit([45,55,65])   luma=53
    ///   bit 6 → (col=0,row=3): Lit([100,110,120]) luma=108
    ///   bit 7 → (col=1,row=3): Lit([250,240,230]) luma=241
    /// avg = 777/8 = 97; lit (≥97): bits 3,6,7 → mask=0xC8 → U+28C8
    /// fg = mean of 8: R=91, G=98, B=106
    #[test]
    fn dots_to_grid_case_a_exact_glyph_and_fg() {
        let mut buf = DotBuffer::new(2, 4);
        // bit 0 (col=0,row=0)
        buf.set(0, 0, Dot::Lit(Rgba::rgb(10,  20,  30)));
        // bit 1 (col=0,row=1)
        buf.set(0, 1, Dot::Lit(Rgba::rgb(40,  50,  60)));
        // bit 2 (col=0,row=2)
        buf.set(0, 2, Dot::Lit(Rgba::rgb(70,  80,  90)));
        // bit 3 (col=1,row=0)
        buf.set(1, 0, Dot::Lit(Rgba::rgb(200, 210, 220)));
        // bit 4 (col=1,row=1)
        buf.set(1, 1, Dot::Lit(Rgba::rgb(15,  25,  35)));
        // bit 5 (col=1,row=2)
        buf.set(1, 2, Dot::Lit(Rgba::rgb(45,  55,  65)));
        // bit 6 (col=0,row=3)
        buf.set(0, 3, Dot::Lit(Rgba::rgb(100, 110, 120)));
        // bit 7 (col=1,row=3)
        buf.set(1, 3, Dot::Lit(Rgba::rgb(250, 240, 230)));

        let grid = dots_to_grid(&buf);
        assert_eq!(grid.cols(), 1, "case (a): expected 1 col");
        assert_eq!(grid.rows(), 1, "case (a): expected 1 row");
        assert_eq!(
            grid.get(0, 0),
            Cell::Glyph {
                ch: '\u{28C8}', // ⣈ — bits 3,6,7 lit
                color: Rgba::rgb(91, 98, 106),
            },
            "case (a): glyph+fg must match hand-derived oracle (mask=0xC8)"
        );
    }

    /// Hand-derived oracle from golden case (d2): left dot-column Lit blue,
    /// right dot-column Transparent → glyph U+2847 ('⡇') in blue.
    ///
    /// DOTS order places bits 0,1,2,6 in the left column (dx=0) and
    /// bits 3,4,5,7 in the right column (dx=1).
    /// All visible lumas = trunc(0.114*200) = 22; avg = 22; all ≥ avg → all lit.
    /// mask = 0x01|0x02|0x04|0x40 = 0x47 → U+2847
    /// fg = Rgba::rgb(0, 0, 200)
    #[test]
    fn dots_to_grid_case_d2_intracell_dot_split() {
        let mut buf = DotBuffer::new(2, 4);
        // Left column (col=0) → bits 0,1,2,6
        buf.set(0, 0, Dot::Lit(Rgba::rgb(0, 0, 200)));
        buf.set(0, 1, Dot::Lit(Rgba::rgb(0, 0, 200)));
        buf.set(0, 2, Dot::Lit(Rgba::rgb(0, 0, 200)));
        buf.set(0, 3, Dot::Lit(Rgba::rgb(0, 0, 200)));
        // Right column (col=1) → bits 3,4,5,7 — leave as Transparent (default)

        let grid = dots_to_grid(&buf);
        assert_eq!(grid.cols(), 1, "d2: expected 1 col");
        assert_eq!(grid.rows(), 1, "d2: expected 1 row");
        assert_eq!(
            grid.get(0, 0),
            Cell::Glyph {
                ch: '\u{2847}', // ⡇ — bits 0,1,2,6
                color: Rgba::rgb(0, 0, 200),
            },
            "d2: intra-cell dot split must produce mask=0x47 glyph in blue"
        );
    }

    // ── tint tests ────────────────────────────────────────────────────────────

    /// An all-`Lit` buffer of mixed source colors, tinted to a target color,
    /// must yield every dot `Lit(target)` with unchanged dims.
    #[test]
    fn tint_all_lit_buffer_becomes_uniform_target_color() {
        let mut buf = DotBuffer::new(2, 2);
        buf.set(0, 0, Dot::Lit(Rgba::rgb(10, 20, 30)));
        buf.set(1, 0, Dot::Lit(Rgba::rgb(200, 100, 50)));
        buf.set(0, 1, Dot::Lit(Rgba::rgb(0, 255, 0)));
        buf.set(1, 1, Dot::Lit(Rgba::rgb(255, 255, 255)));

        let target = Rgba::rgb(9, 8, 7);
        let out = tint(&buf, target);

        assert_eq!(out.cols(), buf.cols(), "tint must preserve cols");
        assert_eq!(out.rows(), buf.rows(), "tint must preserve rows");
        for row in 0..out.rows() {
            for col in 0..out.cols() {
                assert_eq!(
                    out.get(col, row),
                    Dot::Lit(target),
                    "dot ({col},{row}) must become Lit(target) after tint"
                );
            }
        }
    }

    /// A buffer with some `Transparent` dots must keep those dots
    /// `Transparent` after tinting — only `Lit` dots are touched.
    #[test]
    fn tint_preserves_transparent_dots() {
        let mut buf = DotBuffer::new(2, 2);
        buf.set(0, 0, Dot::Lit(Rgba::rgb(10, 20, 30)));
        // (1,0), (0,1), (1,1) left as default Transparent.
        buf.set(1, 1, Dot::Lit(Rgba::rgb(1, 2, 3)));

        let target = Rgba::rgb(77, 88, 99);
        let out = tint(&buf, target);

        assert_eq!(out.get(0, 0), Dot::Lit(target), "(0,0) was Lit, must be tinted");
        assert_eq!(out.get(1, 0), Dot::Transparent, "(1,0) was Transparent, must stay Transparent");
        assert_eq!(out.get(0, 1), Dot::Transparent, "(0,1) was Transparent, must stay Transparent");
        assert_eq!(out.get(1, 1), Dot::Lit(target), "(1,1) was Lit, must be tinted");
    }

    /// Tinting is purely a function of each dot's own state (position-
    /// independent): two structurally-identical Lit/Transparent patterns with
    /// different source colors, tinted to the same target, must produce the
    /// same Transparent-mask and the same Lit color at every position.
    #[test]
    fn tint_is_position_independent_of_source_color() {
        let mut buf_a = DotBuffer::new(2, 2);
        buf_a.set(0, 0, Dot::Lit(Rgba::rgb(1, 1, 1)));
        buf_a.set(1, 1, Dot::Lit(Rgba::rgb(254, 254, 254)));
        // (1,0), (0,1) left Transparent.

        let mut buf_b = DotBuffer::new(2, 2);
        buf_b.set(0, 0, Dot::Lit(Rgba::rgb(9, 8, 7)));
        buf_b.set(1, 1, Dot::Lit(Rgba::rgb(200, 199, 198)));
        // Same Transparent positions as buf_a, but different source Lit colors.

        let target = Rgba::rgb(50, 60, 70);
        let out_a = tint(&buf_a, target);
        let out_b = tint(&buf_b, target);

        assert_eq!(
            out_a, out_b,
            "tint output must depend only on each dot's own Lit/Transparent state, not its source color"
        );
    }
}
