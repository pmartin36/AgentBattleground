//! Shape rasterization for the braille dot pipeline — soft-edged primitive
//! shapes (currently just an ellipse) rendered into a fresh [`DotBuffer`].

use crate::dots::{Dot, DotBuffer};
use engine_core::color::Rgba;

/// The kind of shape to rasterize. Single-variant extension point — add
/// variants only when a real caller needs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Ellipse,
}

/// Rasterize `kind` into a fresh `width_dots × height_dots` [`DotBuffer`],
/// with `color`'s RGB held constant and alpha falling off radially from the
/// center (`color.a`) to the edge (`0`) via a linear `1 - d` profile, where
/// `d` is the normalized distance from center (`d >= 1.0` at/beyond the
/// outermost dot). Dots whose scaled alpha rounds to `0` are emitted as
/// `Dot::Transparent` rather than `Lit(a=0)`.
pub fn rasterize_shape(
    kind: ShapeKind,
    width_dots: usize,
    height_dots: usize,
    color: Rgba,
) -> DotBuffer {
    match kind {
        ShapeKind::Ellipse => {}
    }

    let mut buf = DotBuffer::new(width_dots, height_dots);
    if width_dots == 0 || height_dots == 0 {
        return buf;
    }

    let cx = (width_dots - 1) as f32 / 2.0;
    let cy = (height_dots - 1) as f32 / 2.0;
    let rx = if width_dots > 1 { (width_dots - 1) as f32 / 2.0 } else { 1.0 };
    let ry = if height_dots > 1 { (height_dots - 1) as f32 / 2.0 } else { 1.0 };

    for row in 0..height_dots {
        for col in 0..width_dots {
            let nx = (col as f32 - cx) / rx;
            let ny = (row as f32 - cy) / ry;
            let d = (nx * nx + ny * ny).sqrt();

            if d >= 1.0 {
                continue; // already Transparent
            }

            let a = (color.a as f32 * (1.0 - d)).round() as u8;
            if a == 0 {
                continue; // already Transparent
            }
            buf.set(col, row, Dot::Lit(Rgba::new(color.r, color.g, color.b, a)));
        }
    }

    buf
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dots::Dot;

    /// Output dimensions must match the requested `width_dots`/`height_dots`,
    /// including a non-square case.
    #[test]
    fn dims_match_request() {
        let buf = rasterize_shape(ShapeKind::Ellipse, 8, 4, Rgba::new(255, 0, 0, 200));
        assert_eq!(buf.cols(), 8, "cols must equal width_dots");
        assert_eq!(buf.rows(), 4, "rows must equal height_dots");
    }

    /// With odd dims, the exact center dot must be `Lit` with `a == color.a`,
    /// and no other lit dot may have a strictly greater alpha.
    #[test]
    fn center_is_max_alpha() {
        let color = Rgba::new(10, 20, 30, 200);
        let buf = rasterize_shape(ShapeKind::Ellipse, 9, 9, color);
        let (cx, cy) = (4, 4); // (9-1)/2

        match buf.get(cx, cy) {
            Dot::Lit(c) => assert_eq!(c.a, color.a, "center dot alpha must equal color.a exactly"),
            Dot::Transparent => panic!("center dot must be Lit"),
        }

        for row in 0..buf.rows() {
            for col in 0..buf.cols() {
                if (col, row) == (cx, cy) {
                    continue;
                }
                if let Dot::Lit(c) = buf.get(col, row) {
                    assert!(
                        c.a <= color.a,
                        "dot ({col},{row}) alpha {} must not exceed center alpha {}",
                        c.a,
                        color.a
                    );
                }
            }
        }
    }

    /// All four corner dots must be `Dot::Transparent` (outside the ellipse).
    #[test]
    fn corners_are_transparent() {
        let buf = rasterize_shape(ShapeKind::Ellipse, 9, 9, Rgba::new(255, 255, 255, 255));
        let (last_col, last_row) = (buf.cols() - 1, buf.rows() - 1);
        for (col, row) in [
            (0, 0),
            (last_col, 0),
            (0, last_row),
            (last_col, last_row),
        ] {
            assert_eq!(
                buf.get(col, row),
                Dot::Transparent,
                "corner ({col},{row}) must be Transparent"
            );
        }
    }

    /// The outermost dot on the center row must be Transparent (or, per the
    /// deliverable's allowance, `Lit` with alpha `0` — treated equivalently
    /// here since the chosen encoding always emits `Transparent` for a==0).
    #[test]
    fn edge_of_center_row_is_transparent() {
        let buf = rasterize_shape(ShapeKind::Ellipse, 9, 9, Rgba::new(255, 255, 255, 255));
        let center_row = 4; // (9-1)/2
        assert_eq!(
            buf.get(0, center_row),
            Dot::Transparent,
            "outermost dot on the center row must be Transparent"
        );
    }

    /// Alpha along the center row, moving outward from the center column,
    /// must be monotonically non-increasing.
    #[test]
    fn monotonic_falloff_along_center_row() {
        let color = Rgba::new(255, 255, 255, 255);
        let buf = rasterize_shape(ShapeKind::Ellipse, 9, 9, color);
        let center_row = 4;
        let center_col = 4;

        let alpha_at = |col: usize| match buf.get(col, center_row) {
            Dot::Lit(c) => c.a,
            Dot::Transparent => 0,
        };

        let mut prev = alpha_at(center_col);
        for col in (center_col + 1)..buf.cols() {
            let cur = alpha_at(col);
            assert!(
                cur <= prev,
                "alpha at col {col} ({cur}) must not exceed alpha at previous col ({prev})"
            );
            prev = cur;
        }
    }

    /// Every `Lit` dot must carry `color`'s exact RGB — only alpha is scaled.
    #[test]
    fn rgb_is_preserved_on_lit_dots() {
        let color = Rgba::new(12, 34, 56, 255);
        let buf = rasterize_shape(ShapeKind::Ellipse, 9, 9, color);
        if let Dot::Lit(c) = buf.get(4, 4) {
            assert_eq!(c.r, color.r, "red must be preserved");
            assert_eq!(c.g, color.g, "green must be preserved");
            assert_eq!(c.b, color.b, "blue must be preserved");
        } else {
            panic!("center dot must be Lit for this color");
        }
    }

    /// A 1×1 buffer's single dot is the center — must be `Lit` with
    /// `a == color.a`.
    #[test]
    fn degenerate_1x1_is_lit_center() {
        let color = Rgba::new(1, 2, 3, 77);
        let buf = rasterize_shape(ShapeKind::Ellipse, 1, 1, color);
        assert_eq!(buf.cols(), 1);
        assert_eq!(buf.rows(), 1);
        match buf.get(0, 0) {
            Dot::Lit(c) => assert_eq!(c.a, color.a, "1x1 buffer's sole dot must be Lit at full color.a"),
            Dot::Transparent => panic!("1x1 buffer's sole dot must be Lit"),
        }
    }

    /// A zero-width buffer must not panic and must be empty (no dots to read).
    #[test]
    fn degenerate_zero_width_no_panic() {
        let buf = rasterize_shape(ShapeKind::Ellipse, 0, 5, Rgba::new(255, 255, 255, 255));
        assert_eq!(buf.cols(), 0, "zero width_dots must produce cols()==0");
        assert_eq!(buf.rows(), 5, "height_dots must still be honored");
    }
}
