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

            // 45° chamfer off each corner: a dot within `corner_radius` of two
            // adjacent edges whose distance-sum clears the threshold is
            // dropped, so the square corner reads as rounded. Same formula as
            // Roster's private `draw_dot_box`, which this is meant to
            // eventually replace.
            let clipped = (d_left < corner_radius
                && d_top < corner_radius
                && d_left + d_top < corner_radius)
                || (d_right < corner_radius
                    && d_top < corner_radius
                    && d_right + d_top < corner_radius)
                || (d_left < corner_radius
                    && d_bottom < corner_radius
                    && d_left + d_bottom < corner_radius)
                || (d_right < corner_radius
                    && d_bottom < corner_radius
                    && d_right + d_bottom < corner_radius);
            if clipped {
                continue; // leave Transparent — the rounded corner reveals what's behind
            }

            let on_border = d_left < thickness
                || d_right < thickness
                || d_top < thickness
                || d_bottom < thickness;
            buf.set(col, row, if on_border { Dot::Lit(border) } else { fill });
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
}
