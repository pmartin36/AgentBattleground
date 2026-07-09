//! Shape rasterization for the braille dot pipeline — soft-edged primitive
//! shapes (an ellipse and an annulus/ring) rendered into a fresh
//! [`DotBuffer`].

use crate::dots::{Dot, DotBuffer};
use engine_core::color::Rgba;

/// The kind of shape to rasterize. Extension point — add variants only when
/// a real caller needs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Ellipse,
    Ring,
}

/// Normalized radius at which `ShapeKind::Ring`'s alpha profile peaks —
/// close to the outer edge (`1.0`) so the lit band hugs the shape's outer
/// boundary rather than sitting mid-radius.
const RING_PEAK: f32 = 0.85;
/// Half-width of `ShapeKind::Ring`'s lit band, in ABSOLUTE dots — not a
/// fraction of the radius. A fixed fraction (e.g. "10% of radius") reads
/// fine at large sizes but is narrower than one dot at the shadow sizes this
/// actually renders at (a real shadow's radius is often only 6-8 dots), so
/// almost no dot's discretized distance lands inside the band and the ring
/// rasterizes to a single fallback point instead of a visible perimeter —
/// this shipped as a real bug ("rings disappeared"). An absolute width
/// guarantees the band always spans enough dots to form a ring at realistic
/// sizes, while still reading as thin at large ones (same absolute width,
/// smaller fraction of a bigger radius).
const RING_BAND_HALF_WIDTH_DOTS: f32 = 1.1;

/// Rasterize `kind` into a fresh `width_dots × height_dots` [`DotBuffer`],
/// with `color`'s RGB held constant and alpha varying radially by `kind`'s
/// profile, where `d` is the normalized distance from center (`d >= 1.0` at/
/// beyond the outermost dot):
/// - `Ellipse`: alpha falls off linearly from the center (`color.a`) to the
///   edge (`0`) via `1 - d`.
/// - `Ring`: a true annulus — alpha peaks (`color.a`) at `d == RING_PEAK` and
///   falls off linearly to `0` over `RING_BAND_HALF_WIDTH_DOTS` dots either
///   side of it (an absolute width, not a fraction of the radius — see that
///   constant's own doc comment), so both the hollow interior and the outer
///   edge are genuinely transparent, not just dim.
///
/// Dots whose scaled alpha rounds to `0` are emitted as `Dot::Transparent`
/// rather than `Lit(a=0)`.
pub fn rasterize_shape(
    kind: ShapeKind,
    width_dots: usize,
    height_dots: usize,
    color: Rgba,
) -> DotBuffer {
    let mut buf = DotBuffer::new(width_dots, height_dots);
    if width_dots == 0 || height_dots == 0 {
        return buf;
    }

    let cx = (width_dots - 1) as f32 / 2.0;
    let cy = (height_dots - 1) as f32 / 2.0;
    let rx = if width_dots > 1 { (width_dots - 1) as f32 / 2.0 } else { 1.0 };
    let ry = if height_dots > 1 { (height_dots - 1) as f32 / 2.0 } else { 1.0 };

    // Convert the Ring's absolute-dot band half-width into normalized `d`
    // units, using the SMALLER of the two radii as the reference (the more
    // constrained axis) so the band stays at least this wide in dots along
    // whichever direction is tightest — and clamp it well under `RING_PEAK`
    // so the band can never swallow the hole entirely on a very small shape.
    let ref_radius = rx.min(ry).max(1.0);
    let ring_band_half_width = (RING_BAND_HALF_WIDTH_DOTS / ref_radius).min(RING_PEAK * 0.95);

    let mut any_lit = false;
    // Tracks the in-disk dot whose `d` is closest to `RING_PEAK` — the
    // fallback below needs it on the rare shape small enough that even the
    // adaptive band above doesn't land on a real dot (a real annulus can
    // otherwise rasterize to fully transparent at tiny sizes, which reads as
    // "the shadow vanished," not "the shadow is small").
    let mut closest_to_peak: Option<(usize, usize, f32)> = None;

    for row in 0..height_dots {
        for col in 0..width_dots {
            let nx = (col as f32 - cx) / rx;
            let ny = (row as f32 - cy) / ry;
            let d = (nx * nx + ny * ny).sqrt();

            if d >= 1.0 {
                continue; // already Transparent
            }

            if kind == ShapeKind::Ring {
                let dist_to_peak = (d - RING_PEAK).abs();
                if closest_to_peak.is_none_or(|(_, _, best)| dist_to_peak < best) {
                    closest_to_peak = Some((col, row, dist_to_peak));
                }
            }

            let alpha_factor = match kind {
                ShapeKind::Ellipse => 1.0 - d,
                ShapeKind::Ring => (1.0 - (d - RING_PEAK).abs() / ring_band_half_width).max(0.0),
            };

            let a = (color.a as f32 * alpha_factor).round() as u8;
            if a == 0 {
                continue; // already Transparent
            }
            buf.set(col, row, Dot::Lit(Rgba::new(color.r, color.g, color.b, a)));
            any_lit = true;
        }
    }

    // A genuine 1×1 buffer's sole dot IS the exact center — that stays the
    // (degenerate) hole, matching Ring's own "center is always the hole"
    // rule, not a candidate for the small-shape fallback below.
    let is_degenerate_1x1 = width_dots == 1 && height_dots == 1;
    if kind == ShapeKind::Ring && !any_lit && !is_degenerate_1x1 {
        if let Some((col, row, _)) = closest_to_peak {
            buf.set(col, row, Dot::Lit(color));
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

    // ── Ring ────────────────────────────────────────────────────────────────
    // `Ring` is an annulus: alpha 0 at the exact center (the hole), 0 at/
    // beyond the outer edge, peaking somewhere in a mid band. This is the
    // opposite of `Ellipse`'s center-max-alpha profile, so these tests
    // deliberately do NOT reuse Ellipse's center/monotonic-from-center
    // assertions.

    fn ring_alpha_at(buf: &DotBuffer, col: usize, row: usize) -> u8 {
        match buf.get(col, row) {
            Dot::Lit(c) => c.a,
            Dot::Transparent => 0,
        }
    }

    #[test]
    fn ring_dims_match_request() {
        let buf = rasterize_shape(ShapeKind::Ring, 8, 4, Rgba::new(255, 0, 0, 200));
        assert_eq!(buf.cols(), 8, "cols must equal width_dots");
        assert_eq!(buf.rows(), 4, "rows must equal height_dots");
    }

    #[test]
    fn ring_center_is_transparent() {
        let color = Rgba::new(10, 20, 30, 200);
        let buf = rasterize_shape(ShapeKind::Ring, 9, 9, color);
        let (cx, cy) = (4, 4); // (9-1)/2
        assert_eq!(
            buf.get(cx, cy),
            Dot::Transparent,
            "ring's exact center (the hole) must be Transparent, not max-alpha like Ellipse"
        );
    }

    #[test]
    fn ring_corners_are_transparent() {
        let buf = rasterize_shape(ShapeKind::Ring, 9, 9, Rgba::new(255, 255, 255, 255));
        let (last_col, last_row) = (buf.cols() - 1, buf.rows() - 1);
        for (col, row) in [(0, 0), (last_col, 0), (0, last_row), (last_col, last_row)] {
            assert_eq!(
                buf.get(col, row),
                Dot::Transparent,
                "corner ({col},{row}) must be Transparent (at/beyond outer edge)"
            );
        }
    }

    #[test]
    fn ring_edge_of_center_row_is_transparent() {
        let buf = rasterize_shape(ShapeKind::Ring, 9, 9, Rgba::new(255, 255, 255, 255));
        let center_row = 4; // (9-1)/2
        assert_eq!(
            buf.get(0, center_row),
            Dot::Transparent,
            "outermost dot on the center row must be Transparent"
        );
    }

    /// The band peak must land strictly between the center column and the
    /// edge column — neither at the center (that's the hole) nor at the
    /// edge (that's outside the annulus).
    #[test]
    fn ring_peak_is_off_center() {
        let color = Rgba::new(255, 255, 255, 255);
        // 25×25 (not 9×9): RING_PEAK sits close enough to the outer edge
        // (0.90) that a 9-dot buffer doesn't have enough resolution to
        // discretize the peak away from the last column — real shadows are
        // dozens of dots wide, so this is representative, not oversized.
        let buf = rasterize_shape(ShapeKind::Ring, 25, 25, color);
        let center_row = (buf.rows() - 1) / 2;
        let center_col = (buf.cols() - 1) / 2;
        let last_col = buf.cols() - 1;

        let (peak_col, peak_alpha) = (0..buf.cols())
            .map(|col| (col, ring_alpha_at(&buf, col, center_row)))
            .max_by_key(|&(_, a)| a)
            .expect("center row has at least one dot");

        assert_ne!(peak_col, center_col, "peak must not be at the center (that's the hole)");
        assert_ne!(peak_col, 0, "peak must not be at the left edge");
        assert_ne!(peak_col, last_col, "peak must not be at the right edge");
        assert!(
            peak_alpha > ring_alpha_at(&buf, center_col, center_row),
            "peak alpha ({peak_alpha}) must exceed the center's alpha ({})",
            ring_alpha_at(&buf, center_col, center_row)
        );
        assert!(
            peak_alpha > ring_alpha_at(&buf, 0, center_row),
            "peak alpha ({peak_alpha}) must exceed the edge's alpha"
        );
    }

    /// From the peak column outward to the edge, alpha must be
    /// monotonically non-increasing (Ring rises then falls — do not assert
    /// monotonic-from-center like Ellipse's test, which would be false here).
    #[test]
    fn ring_monotonic_outward_from_peak() {
        let color = Rgba::new(255, 255, 255, 255);
        let buf = rasterize_shape(ShapeKind::Ring, 9, 9, color);
        let center_row = 4;

        let (peak_col, _) = (0..buf.cols())
            .map(|col| (col, ring_alpha_at(&buf, col, center_row)))
            .max_by_key(|&(_, a)| a)
            .expect("center row has at least one dot");

        let mut prev = ring_alpha_at(&buf, peak_col, center_row);
        for col in (peak_col + 1)..buf.cols() {
            let cur = ring_alpha_at(&buf, col, center_row);
            assert!(
                cur <= prev,
                "alpha at col {col} ({cur}) must not exceed alpha at previous col ({prev}), moving outward from the peak"
            );
            prev = cur;
        }
    }

    #[test]
    fn ring_rgb_preserved() {
        let color = Rgba::new(12, 34, 56, 255);
        // 25×25 for the same resolution reason as `ring_peak_is_off_center`.
        let buf = rasterize_shape(ShapeKind::Ring, 25, 25, color);
        let center_row = (buf.rows() - 1) / 2;
        let mut saw_lit = false;
        for col in 0..buf.cols() {
            if let Dot::Lit(c) = buf.get(col, center_row) {
                saw_lit = true;
                assert_eq!(c.r, color.r, "red must be preserved");
                assert_eq!(c.g, color.g, "green must be preserved");
                assert_eq!(c.b, color.b, "blue must be preserved");
            }
        }
        assert!(saw_lit, "center row must contain at least one Lit dot (the band peak)");
    }

    #[test]
    fn ring_degenerate_1x1_no_panic() {
        let color = Rgba::new(1, 2, 3, 77);
        let buf = rasterize_shape(ShapeKind::Ring, 1, 1, color);
        assert_eq!(buf.cols(), 1);
        assert_eq!(buf.rows(), 1);
        assert_eq!(
            buf.get(0, 0),
            Dot::Transparent,
            "1x1 buffer's sole dot is the center (the hole) — must be Transparent, unlike Ellipse"
        );
    }

    #[test]
    fn ring_degenerate_zero_width_no_panic() {
        let buf = rasterize_shape(ShapeKind::Ring, 0, 5, Rgba::new(255, 255, 255, 255));
        assert_eq!(buf.cols(), 0, "zero width_dots must produce cols()==0");
        assert_eq!(buf.rows(), 5, "height_dots must still be honored");
    }
}
