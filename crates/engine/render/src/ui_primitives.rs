//! Crisp 2D UI-chrome primitives rasterized into a [`DotBuffer`] — the menu/
//! panel counterpart to `shapes.rs`'s soft, radial *effect* shapes (shadows).
//! Everything here has hard edges — a fixed-thickness border, optional
//! chamfered corners — and a caller-chosen interior fill, suited to
//! screen-space UI rather than in-scene effects.

use crate::dots::{Dot, DotBuffer};
use engine_core::color::Rgba;

/// Rasterize a rounded-rect panel into a fresh `width_dots × height_dots`
/// [`DotBuffer`]: a `thickness`-dot border ring in `border`, its corners
/// chamfered by `corner_radius` (a 45° diagonal cut — the braille aesthetic,
/// not a true arc), wrapped around an interior filled with `fill`.
///
/// `fill` is a whole [`Dot`], which is what lets one primitive cover every
/// panel style:
/// - `Dot::Transparent` — a hollow border frame (whatever is behind shows
///   through the middle).
/// - `Dot::Occlude` — a border around a blank that *covers* what is behind it
///   (a screen-space overlay with an empty centre).
/// - `Dot::Lit(color)` — a solid-colour filled panel.
///
/// Chamfered corner dots are left `Transparent`, so the border and the fill
/// round off together.
pub fn rounded_rect(
    width_dots: usize,
    height_dots: usize,
    thickness: usize,
    corner_radius: usize,
    border: Rgba,
    fill: Dot,
) -> DotBuffer {
    let mut buf = DotBuffer::new(width_dots, height_dots);
    if width_dots == 0 || height_dots == 0 {
        return buf;
    }

    for row in 0..height_dots {
        for col in 0..width_dots {
            let d_left = col;
            let d_right = width_dots - 1 - col;
            let d_top = row;
            let d_bottom = height_dots - 1 - row;

            // 45° chamfer off each corner. Within `corner_radius` of two
            // adjacent edges the square corner is cut on the anti-diagonal:
            //   - dots inside the diagonal (edge-distance `sum < radius`) are
            //     dropped (left Transparent), so the corner reads as rounded;
            //   - dots ON the diagonal band (`radius <= sum < radius+thickness`)
            //     stay lit as border, so the two straight edges remain JOINED
            //     across the cut. Without this band the corner loses its
            //     connecting dot and the border reads as broken — e.g. at
            //     thickness 1 / radius 2 the dot at (1,1) was left as interior
            //     fill, leaving a diagonal gap between the edges.
            let mut cut = false;
            let mut chamfer_border = false;
            for (a, b) in [
                (d_left, d_top),
                (d_right, d_top),
                (d_left, d_bottom),
                (d_right, d_bottom),
            ] {
                if a < corner_radius && b < corner_radius {
                    let sum = a + b;
                    if sum < corner_radius {
                        cut = true;
                    } else if sum < corner_radius + thickness {
                        chamfer_border = true;
                    }
                }
            }
            if cut {
                continue; // leave Transparent — the rounded corner reveals what's behind
            }

            let on_edge = d_left < thickness
                || d_right < thickness
                || d_top < thickness
                || d_bottom < thickness;
            buf.set(col, row, if chamfer_border || on_edge { Dot::Lit(border) } else { fill });
        }
    }

    buf
}

/// A square-cornered rectangle panel — [`rounded_rect`] with no corner
/// chamfer. Same `border`/`fill` semantics.
pub fn rect(
    width_dots: usize,
    height_dots: usize,
    thickness: usize,
    border: Rgba,
    fill: Dot,
) -> DotBuffer {
    rounded_rect(width_dots, height_dots, thickness, 0, border, fill)
}

/// Rasterize a `thickness`-dot-thick ellipse RING inscribed in a
/// `width_dots × height_dots` box into a fresh [`DotBuffer`]. A dot is lit
/// `border` when it lies inside the outer ellipse (which touches the box's
/// mid-edges) but outside the inner ellipse `thickness` dots smaller; interior
/// dots take `fill`; dots outside the outer ellipse — the corners — are left
/// `Transparent`. A SQUARE dot box yields a round circle; a non-square box an
/// ellipse fitting the box. Because braille dots are ~square, a round on-screen
/// circle needs a roughly square DOT region (equal width/height in dots), not
/// equal cells. Same `border`/`fill` semantics as [`rounded_rect`].
pub fn ring(
    width_dots: usize,
    height_dots: usize,
    thickness: usize,
    border: Rgba,
    fill: Dot,
) -> DotBuffer {
    let mut buf = DotBuffer::new(width_dots, height_dots);
    if width_dots == 0 || height_dots == 0 {
        return buf;
    }
    let cx = (width_dots as f32 - 1.0) / 2.0;
    let cy = (height_dots as f32 - 1.0) / 2.0;
    let ax = width_dots as f32 / 2.0;
    let ay = height_dots as f32 / 2.0;
    let t = thickness as f32;
    // Inner semi-axes; when the ring is as thick as the box it collapses to a
    // filled disc (inner ellipse vanishes) rather than going negative.
    let ix = ax - t;
    let iy = ay - t;
    for row in 0..height_dots {
        for col in 0..width_dots {
            let dx = col as f32 - cx;
            let dy = row as f32 - cy;
            let outer = (dx / ax).powi(2) + (dy / ay).powi(2);
            if outer > 1.0 {
                continue; // outside the ellipse — a corner; leave Transparent
            }
            let inside_inner =
                ix > 0.0 && iy > 0.0 && (dx / ix).powi(2) + (dy / iy).powi(2) <= 1.0;
            buf.set(col, row, if inside_inner { fill } else { Dot::Lit(border) });
        }
    }
    buf
}

/// Rasterize a solid filled disc of `diameter_dots` into a fresh square
/// [`DotBuffer`]: every dot whose distance from the centre is within the radius
/// is `Lit(color)`; corners outside the circle stay `Transparent`. Takes a
/// single diameter (not w×h), so a `circle` is ALWAYS round — an ellipse is
/// `ring` with a non-square box. Because braille dots are ~square, this reads
/// round on screen.
pub fn circle(diameter_dots: usize, color: Rgba) -> DotBuffer {
    // thickness == diameter collapses `ring`'s inner ellipse, so every in-circle
    // dot falls to `Lit(border)`: a gap-free solid disc that does not depend on
    // the fill arg. (`ring`'s `outer <= 1` test on a square box is exactly
    // `distance <= radius`.)
    ring(diameter_dots, diameter_dots, diameter_dots, color, Dot::Lit(color))
}

/// A `thickness`-dot-thick round outline (hollow interior) of `diameter_dots` —
/// `circle` with only its rim lit. Always round (single diameter).
pub fn circle_outline(diameter_dots: usize, thickness: usize, color: Rgba) -> DotBuffer {
    ring(diameter_dots, diameter_dots, thickness, color, Dot::Transparent)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const BORDER: Rgba = Rgba::rgb(10, 20, 30);
    const FILL: Rgba = Rgba::rgb(200, 100, 50);

    /// Output buffer dims must equal the requested dot dimensions.
    #[test]
    fn rounded_rect_has_requested_dimensions() {
        let buf = rounded_rect(8, 6, 1, 2, BORDER, Dot::Occlude);
        assert_eq!(buf.cols(), 8, "cols must equal width_dots");
        assert_eq!(buf.rows(), 6, "rows must equal height_dots");
    }

    /// A square (radius 0) rect: every perimeter dot is `Lit(border)`, every
    /// interior dot is exactly the `fill` — checked over the whole buffer.
    #[test]
    fn rect_border_is_lit_interior_is_fill() {
        let (w, h) = (6usize, 5usize);
        let buf = rect(w, h, 1, BORDER, Dot::Occlude);
        for row in 0..h {
            for col in 0..w {
                let on_border = col == 0 || col == w - 1 || row == 0 || row == h - 1;
                let expected = if on_border { Dot::Lit(BORDER) } else { Dot::Occlude };
                assert_eq!(buf.get(col, row), expected, "dot ({col},{row})");
            }
        }
    }

    /// `fill = Transparent` → a hollow frame: lit border, revealing interior.
    #[test]
    fn transparent_fill_leaves_hollow_interior() {
        let buf = rect(5, 5, 1, BORDER, Dot::Transparent);
        assert_eq!(buf.get(2, 2), Dot::Transparent, "interior must reveal (Transparent)");
        assert_eq!(buf.get(0, 2), Dot::Lit(BORDER), "border must still be lit");
    }

    /// `fill = Occlude` → a covering blank interior (the menu-overlay case).
    #[test]
    fn occlude_fill_covers_interior() {
        let buf = rect(5, 5, 1, BORDER, Dot::Occlude);
        assert_eq!(buf.get(2, 2), Dot::Occlude, "interior must be an occluder");
    }

    /// `fill = Lit(color)` → a solid colored panel; the fill color is distinct
    /// from the border color, so this proves the two are wired separately.
    #[test]
    fn lit_fill_produces_solid_panel_distinct_from_border() {
        let buf = rect(5, 5, 1, BORDER, Dot::Lit(FILL));
        assert_eq!(buf.get(2, 2), Dot::Lit(FILL), "interior must carry the fill color");
        assert_eq!(buf.get(0, 0), Dot::Lit(BORDER), "corner carries the border color (radius 0)");
    }

    /// `thickness = 2` widens the border ring to two dots on each edge; the
    /// third dot in is interior.
    #[test]
    fn thickness_two_makes_a_two_dot_border() {
        let buf = rect(8, 8, 2, BORDER, Dot::Occlude);
        assert_eq!(buf.get(1, 4), Dot::Lit(BORDER), "second column is still border at thickness 2");
        assert_eq!(buf.get(2, 4), Dot::Occlude, "third column is interior");
    }

    /// `corner_radius = 2` chamfers the three outermost dots at each corner
    /// (leaving them Transparent) while mid-edge border dots stay lit.
    #[test]
    fn corner_radius_chamfers_the_corner_dots() {
        let buf = rounded_rect(8, 8, 1, 2, BORDER, Dot::Occlude);
        assert_eq!(buf.get(0, 0), Dot::Transparent, "outermost corner dot must be chamfered off");
        assert_eq!(buf.get(1, 0), Dot::Transparent, "adjacent corner dot must be chamfered off");
        assert_eq!(buf.get(0, 1), Dot::Transparent, "adjacent corner dot must be chamfered off");
        assert_eq!(buf.get(4, 0), Dot::Lit(BORDER), "mid top-edge dot must stay lit");
    }

    /// Regression: at thickness 1 / radius 2 the corner's connecting diagonal
    /// dot (1,1) must be lit so the two straight edges stay JOINED across the
    /// chamfer. Previously it was left as interior fill, breaking the border.
    #[test]
    fn chamfer_lights_the_corner_connecting_dot() {
        let buf = rounded_rect(8, 8, 1, 2, BORDER, Dot::Occlude);
        assert_eq!(buf.get(1, 1), Dot::Lit(BORDER), "diagonal connector dot must be lit");
        assert_eq!(buf.get(0, 0), Dot::Transparent, "outer corner dot still chamfered");
        assert_eq!(buf.get(1, 0), Dot::Transparent, "adjacent corner dot still chamfered");
        assert_eq!(buf.get(0, 1), Dot::Transparent, "adjacent corner dot still chamfered");
    }

    /// `rect(..)` must equal `rounded_rect(.., corner_radius = 0, ..)`
    /// bit-for-bit — it's defined as exactly that.
    #[test]
    fn rect_equals_rounded_rect_with_zero_radius() {
        let a = rect(7, 5, 1, BORDER, Dot::Occlude);
        let b = rounded_rect(7, 5, 1, 0, BORDER, Dot::Occlude);
        assert_eq!(a, b, "rect must equal rounded_rect with corner_radius 0");
    }

    /// A zero-width (or -height) request returns an empty buffer of that size
    /// rather than panicking.
    #[test]
    fn zero_dimension_returns_empty_without_panic() {
        let buf = rounded_rect(0, 5, 1, 2, BORDER, Dot::Occlude);
        assert_eq!(buf.cols(), 0);
        assert_eq!(buf.rows(), 5);
    }

    /// A ring on a square box lights every mid-edge, fills the centre, and
    /// leaves the corners (outside the ellipse) unlit.
    #[test]
    fn ring_lights_mid_edges_fills_centre_clears_corners() {
        let buf = ring(8, 8, 1, BORDER, Dot::Occlude);
        assert_eq!(buf.get(0, 3), Dot::Lit(BORDER), "left mid-edge lit");
        assert_eq!(buf.get(7, 4), Dot::Lit(BORDER), "right mid-edge lit");
        assert_eq!(buf.get(3, 0), Dot::Lit(BORDER), "top mid-edge lit");
        assert_eq!(buf.get(4, 7), Dot::Lit(BORDER), "bottom mid-edge lit");
        assert_eq!(buf.get(3, 3), Dot::Occlude, "centre is fill");
        assert_eq!(buf.get(4, 4), Dot::Occlude, "centre is fill");
        assert_eq!(buf.get(0, 0), Dot::Transparent, "corner outside the ellipse is unlit");
        assert_eq!(buf.get(7, 7), Dot::Transparent, "corner outside the ellipse is unlit");
    }

    /// On a square box the ring is symmetric under horizontal and vertical
    /// reflection — i.e. it reads as a round circle, not a lopsided blob.
    #[test]
    fn ring_is_symmetric_on_a_square_box() {
        let buf = ring(8, 8, 1, BORDER, Dot::Transparent);
        for row in 0..8 {
            for col in 0..8 {
                assert_eq!(buf.get(col, row), buf.get(7 - col, row), "h-symmetry ({col},{row})");
                assert_eq!(buf.get(col, row), buf.get(col, 7 - row), "v-symmetry ({col},{row})");
            }
        }
    }

    /// Zero-dimension ring returns an empty buffer rather than panicking.
    #[test]
    fn ring_zero_dimension_returns_empty_without_panic() {
        let buf = ring(0, 6, 1, BORDER, Dot::Occlude);
        assert_eq!(buf.cols(), 0);
        assert_eq!(buf.rows(), 6);
    }

    /// `circle` is a gap-free solid disc: EVERY dot within the radius is
    /// `Lit(color)` (no `Transparent` interior gaps), and the corners outside
    /// the circle are `Transparent`.
    #[test]
    fn circle_is_a_gap_free_solid_disc() {
        let d = 8usize;
        let buf = circle(d, BORDER);
        assert_eq!(buf.cols(), d);
        assert_eq!(buf.rows(), d);
        assert_eq!(buf.get(0, 0), Dot::Transparent, "corner outside the circle");
        assert_eq!(buf.get(7, 7), Dot::Transparent, "corner outside the circle");
        // Mirror `ring`'s own `outer <= 1` test: every in-circle dot must be lit
        // (no gaps), every out-of-circle dot Transparent.
        let c = (d as f32 - 1.0) / 2.0;
        let a = d as f32 / 2.0;
        for row in 0..d {
            for col in 0..d {
                let inside = ((col as f32 - c) / a).powi(2) + ((row as f32 - c) / a).powi(2) <= 1.0;
                let expected = if inside { Dot::Lit(BORDER) } else { Dot::Transparent };
                assert_eq!(buf.get(col, row), expected, "circle dot ({col},{row})");
            }
        }
    }

    /// `circle` is symmetric under horizontal and vertical reflection — round.
    #[test]
    fn circle_is_symmetric() {
        let buf = circle(8, BORDER);
        for row in 0..8 {
            for col in 0..8 {
                assert_eq!(buf.get(col, row), buf.get(7 - col, row), "h-symmetry ({col},{row})");
                assert_eq!(buf.get(col, row), buf.get(col, 7 - row), "v-symmetry ({col},{row})");
            }
        }
    }

    /// `circle_outline` lights only the rim; the interior is hollow.
    #[test]
    fn circle_outline_rim_lit_interior_hollow() {
        let buf = circle_outline(8, 1, BORDER);
        assert_eq!(buf.get(0, 3), Dot::Lit(BORDER), "left rim lit");
        assert_eq!(buf.get(3, 0), Dot::Lit(BORDER), "top rim lit");
        assert_eq!(buf.get(3, 3), Dot::Transparent, "interior hollow");
        assert_eq!(buf.get(4, 4), Dot::Transparent, "interior hollow");
        assert_eq!(buf.get(0, 0), Dot::Transparent, "corner outside the circle");
    }
}
