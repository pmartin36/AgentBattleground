//! Depth-sorted compositor: blits multiple positioned `DotBuffer`s into one.

use crate::dots::{Dot, DotBuffer};

/// A dot-buffer positioned at `(dot_x, dot_y)` in the destination, with a
/// depth-sort key. Negative offsets are allowed (dots that fall outside the
/// destination are clipped).
#[derive(Clone, Copy, Debug)]
pub struct DotPlacement<'a> {
    pub dots: &'a DotBuffer,
    pub dot_x: i32,
    pub dot_y: i32,
    /// Back-to-front sort key (larger = nearer = drawn on top). Ascending =
    /// far first; equal depths keep input order (stable sort).
    pub depth: i32,
}

/// Composite dot buffers into a fresh `dot_cols`×`dot_rows` `DotBuffer`,
/// back-to-front by `depth` ascending (far first; larger depth drawn last =
/// on top).
///
/// - `Lit` dots overwrite whatever a farther placement wrote.
/// - `Transparent` dots are skipped so farther dots show through gaps.
/// - Dots landing outside `[0, dot_cols) × [0, dot_rows)` are silently clipped.
/// - Equal-depth placements keep input order (stable sort).
pub fn composite_dots(
    dot_cols: usize,
    dot_rows: usize,
    placements: &[DotPlacement],
) -> DotBuffer {
    let mut out = DotBuffer::new(dot_cols, dot_rows);
    let mut ordered: Vec<DotPlacement> = placements.to_vec();
    ordered.sort_by_key(|p| p.depth); // stable ascending: far (low depth) first
    for p in &ordered {
        for r in 0..p.dots.rows() {
            for c in 0..p.dots.cols() {
                let dot = p.dots.get(c, r);
                if let Dot::Lit(_) = dot {
                    let dx = p.dot_x + c as i32;
                    let dy = p.dot_y + r as i32;
                    if dx >= 0 && dy >= 0 {
                        out.set(dx as usize, dy as usize, dot);
                    }
                }
            }
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dots::{Dot, DotBuffer};
    use engine_core::color::Rgba;

    // ── composite_dots tests ──────────────────────────────────────────────────

    /// A near (higher depth) `Lit` dot over a far `Lit` dot at the same
    /// destination position must yield the near dot.
    ///
    /// Setup: 1×1 output.
    ///   far  placement: depth=0, dot_x=0, dot_y=0, Lit(color_a).
    ///   near placement: depth=1, dot_x=0, dot_y=0, Lit(color_b).
    /// Expected: out dot (0,0) == Lit(color_b).
    #[test]
    fn composite_dots_near_lit_over_far_wins() {
        let color_a = Rgba::rgb(255, 0, 0);
        let color_b = Rgba::rgb(0, 255, 0);
        let mut far_buf = DotBuffer::new(1, 1);
        far_buf.set(0, 0, Dot::Lit(color_a));
        let mut near_buf = DotBuffer::new(1, 1);
        near_buf.set(0, 0, Dot::Lit(color_b));

        let placements = [
            DotPlacement { dots: &far_buf,  dot_x: 0, dot_y: 0, depth: 0 },
            DotPlacement { dots: &near_buf, dot_x: 0, dot_y: 0, depth: 1 },
        ];
        let out = composite_dots(1, 1, &placements);

        assert_eq!(
            out.get(0, 0),
            Dot::Lit(color_b),
            "near (depth=1) Lit dot must overwrite far (depth=0) Lit dot"
        );
    }

    /// Where the near buffer is `Transparent` at a position the far buffer
    /// has `Lit`, the far dot must show through (not be overwritten).
    ///
    /// Setup: 2×1 output.
    ///   far  placement: depth=0, both dots Lit(color_a).
    ///   near placement: depth=1, dot (0,0)=Lit(color_b), dot (1,0)=Transparent.
    /// Expected: (0,0)==Lit(color_b), (1,0)==Lit(color_a).
    #[test]
    fn composite_dots_transparent_reveals_far() {
        let color_a = Rgba::rgb(255, 0, 0);
        let color_b = Rgba::rgb(0, 255, 0);
        let mut far_buf = DotBuffer::new(2, 1);
        far_buf.set(0, 0, Dot::Lit(color_a));
        far_buf.set(1, 0, Dot::Lit(color_a));
        let mut near_buf = DotBuffer::new(2, 1);
        near_buf.set(0, 0, Dot::Lit(color_b));
        // (1, 0) stays Dot::Transparent

        let placements = [
            DotPlacement { dots: &far_buf,  dot_x: 0, dot_y: 0, depth: 0 },
            DotPlacement { dots: &near_buf, dot_x: 0, dot_y: 0, depth: 1 },
        ];
        let out = composite_dots(2, 1, &placements);

        assert_eq!(
            out.get(0, 0),
            Dot::Lit(color_b),
            "near Lit at (0,0) must overwrite far"
        );
        assert_eq!(
            out.get(1, 0),
            Dot::Lit(color_a),
            "near Transparent at (1,0) must allow far Lit to show through"
        );
    }

    /// Swapping depths must flip the winner — depth, not input order, decides.
    ///
    /// Setup: 1×1 output.
    ///   placement A: depth=1 (near), Lit(color_a).
    ///   placement B: depth=0 (far),  Lit(color_b).
    /// Expected: out (0,0) == Lit(color_a) (A is nearer).
    #[test]
    fn composite_dots_swap_depth_flips_winner() {
        let color_a = Rgba::rgb(255, 0, 0);
        let color_b = Rgba::rgb(0, 0, 255);
        let mut buf_a = DotBuffer::new(1, 1);
        buf_a.set(0, 0, Dot::Lit(color_a));
        let mut buf_b = DotBuffer::new(1, 1);
        buf_b.set(0, 0, Dot::Lit(color_b));

        // A is near (depth=1), B is far (depth=0); A wins.
        let placements = [
            DotPlacement { dots: &buf_a, dot_x: 0, dot_y: 0, depth: 1 },
            DotPlacement { dots: &buf_b, dot_x: 0, dot_y: 0, depth: 0 },
        ];
        let out = composite_dots(1, 1, &placements);

        assert_eq!(
            out.get(0, 0),
            Dot::Lit(color_a),
            "color_a (depth=1, near) must win over color_b (depth=0, far)"
        );
    }

    /// A 1-dot `Lit` source buffer placed at `dot_x=1` in a 2-wide output
    /// must land at dot (1,0) — the sub-cell offset point.
    /// Dot (0,0) must remain `Transparent`.
    #[test]
    fn composite_dots_subcell_offset_places_one_dot_over() {
        let color = Rgba::rgb(100, 150, 200);
        let mut src = DotBuffer::new(1, 1);
        src.set(0, 0, Dot::Lit(color));

        let placements = [
            DotPlacement { dots: &src, dot_x: 1, dot_y: 0, depth: 0 },
        ];
        let out = composite_dots(2, 1, &placements);

        assert_eq!(
            out.get(0, 0),
            Dot::Transparent,
            "dot (0,0) must be Transparent — source placed at dot_x=1"
        );
        assert_eq!(
            out.get(1, 0),
            Dot::Lit(color),
            "dot (1,0) must be Lit — the source dot lands here via +1 offset"
        );
    }

    /// Out-of-bounds placements (negative x, negative y, beyond the far edge)
    /// must not panic; only in-bounds dots are written; everything else stays
    /// `Transparent`.
    #[test]
    fn composite_dots_out_of_bounds_no_panic() {
        let color = Rgba::rgb(10, 20, 30);
        let mut src = DotBuffer::new(1, 1);
        src.set(0, 0, Dot::Lit(color));

        let placements = [
            // Negative x → dot lands at x=-1, clipped.
            DotPlacement { dots: &src, dot_x: -1, dot_y: 0, depth: 0 },
            // Beyond dot_cols (2) → lands at x=3, clipped.
            DotPlacement { dots: &src, dot_x: 3, dot_y: 0, depth: 0 },
            // Negative y → dot lands at y=-1, clipped.
            DotPlacement { dots: &src, dot_x: 0, dot_y: -1, depth: 0 },
        ];
        // Must not panic.
        let out = composite_dots(2, 2, &placements);

        // All placements are out of bounds; entire output must be Transparent.
        for row in 0..2 {
            for col in 0..2 {
                assert_eq!(
                    out.get(col, row),
                    Dot::Transparent,
                    "dot ({col},{row}) must be Transparent — all placements were OOB"
                );
            }
        }
    }
}
