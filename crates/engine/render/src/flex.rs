//! Dot-native geometry (b1). `DotRect` is the foundational rect type flex
//! layout and its consumers compute in; `to_cell_rect`/`cell_remainder` are
//! the sole dot->cell boundary the rest of the engine's `ratatui::Rect`-based
//! APIs consume.

use ratatui::layout::Rect;

/// A rectangle in dot space (2 dots wide, 4 dots tall per terminal cell).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DotRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl DotRect {
    /// Floor to the containing whole-cell `Rect` (x/w ÷ 2, y/h ÷ 4).
    pub fn to_cell_rect(self) -> Rect {
        // NOTE: constructed via struct literal, not `Rect::new` — `Rect::new`
        // clamps `width`/`height` against `u16::MAX - x`/`u16::MAX - y`
        // (avoiding overflow on `right()`/`bottom()`), which would silently
        // zero `width` whenever `x` also saturates to `u16::MAX`. Each field
        // here must saturate independently at the u16 boundary.
        Rect {
            x: floor_to_u16(self.x, 2),
            y: floor_to_u16(self.y, 4),
            width: floor_to_u16(self.w, 2),
            height: floor_to_u16(self.h, 4),
        }
    }

    /// Sub-cell remainder of the origin after flooring — (dx, dy) in
    /// `0..2` / `0..4`.
    pub fn cell_remainder(self) -> (i32, i32) {
        (self.x.rem_euclid(2), self.y.rem_euclid(4))
    }

    /// Shrink `self` inward by the given dot amounts on each edge — the
    /// padding/margin equivalent for a container passed into `flex()`, and for a
    /// leaf `DotRect` needing a render-target inset independent of its
    /// layout-computed slot (Roster's `EDGE_MARGIN`, the sprite's 4-sided
    /// asymmetric inset, the details panel's extra left shift). An inset larger
    /// than the extent saturates the corresponding size to `0` rather than going
    /// negative or panicking — the same degradation `anchor`/`stack` use.
    pub fn inset(self, left: i32, right: i32, top: i32, bottom: i32) -> DotRect {
        DotRect {
            x: self.x.saturating_add(left),
            y: self.y.saturating_add(top),
            w: self.w.saturating_sub(left).saturating_sub(right).max(0),
            h: self.h.saturating_sub(top).saturating_sub(bottom).max(0),
        }
    }
}

/// Floor `v` by `stride` (a dot->cell axis divisor: 2 or 4), then
/// saturating-clamp into `0..=u16::MAX` before casting — never panics on
/// oversized or negative input, matching `screen_layout.rs`'s
/// floor+clamp-into-u16 convention.
fn floor_to_u16(v: i32, stride: i32) -> u16 {
    v.div_euclid(stride).clamp(0, u16::MAX as i32) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_cell_rect_floors_each_axis() {
        // Distinct values per field so an axis/stride swap is caught.
        let r = DotRect { x: 5, y: 9, w: 4, h: 8 };
        assert_eq!(r.to_cell_rect(), Rect { x: 2, y: 2, width: 2, height: 2 });
    }

    #[test]
    fn cell_remainder_in_range() {
        assert_eq!(DotRect { x: 5, y: 9, w: 0, h: 0 }.cell_remainder(), (1, 1));
        assert_eq!(DotRect { x: 2, y: 7, w: 0, h: 0 }.cell_remainder(), (0, 3));
        assert_eq!(DotRect { x: 3, y: 4, w: 0, h: 0 }.cell_remainder(), (1, 0));
    }

    #[test]
    fn round_trip_reconstructs_dotrect() {
        // Sweep origins, including non-cell-aligned ones, with cell-aligned
        // w/h (size has no separate remainder channel — full reconstruction
        // only holds when w is a multiple of 2 and h a multiple of 4).
        let cases = [
            DotRect { x: 5, y: 9, w: 4, h: 8 },
            DotRect { x: 0, y: 0, w: 2, h: 4 },
            DotRect { x: 1, y: 3, w: 6, h: 12 },
            DotRect { x: 7, y: 1, w: 10, h: 16 },
        ];
        for r in cases {
            let c = r.to_cell_rect();
            let (dx, dy) = r.cell_remainder();
            assert_eq!(c.x as i32 * 2 + dx, r.x, "x reconstruction for {r:?}");
            assert_eq!(c.y as i32 * 4 + dy, r.y, "y reconstruction for {r:?}");
            let reconstructed = DotRect {
                x: c.x as i32 * 2 + dx,
                y: c.y as i32 * 4 + dy,
                w: c.width as i32 * 2,
                h: c.height as i32 * 4,
            };
            assert_eq!(reconstructed, r, "full round-trip for {r:?}");
        }
    }

    #[test]
    fn to_cell_rect_oversized_and_negative_do_not_panic() {
        let huge = DotRect { x: i32::MAX, y: i32::MAX, w: i32::MAX, h: i32::MAX };
        let c = huge.to_cell_rect();
        assert_eq!(c.x, u16::MAX);
        assert_eq!(c.y, u16::MAX);
        assert_eq!(c.width, u16::MAX);
        assert_eq!(c.height, u16::MAX);

        let negative = DotRect { x: -5, y: -9, w: -4, h: -8 };
        let c = negative.to_cell_rect();
        assert_eq!(c.x, 0);
        assert_eq!(c.y, 0);
        assert_eq!(c.width, 0);
        assert_eq!(c.height, 0);
    }

    #[test]
    fn inset_symmetric() {
        let r = DotRect { x: 2, y: 4, w: 20, h: 40 };
        assert_eq!(r.inset(2, 2, 4, 4), DotRect { x: 4, y: 8, w: 16, h: 32 });
    }

    #[test]
    fn inset_asymmetric() {
        // Distinct value per edge so a left/right or top/bottom swap is caught.
        let r = DotRect { x: 0, y: 0, w: 20, h: 40 };
        assert_eq!(r.inset(1, 3, 0, 5), DotRect { x: 1, y: 0, w: 16, h: 35 });
    }

    #[test]
    fn inset_oversized_saturates_to_zero_no_panic() {
        let r = DotRect { x: 0, y: 0, w: 20, h: 40 };
        let out = r.inset(50, 50, 80, 80);
        assert_eq!(out.w, 0, "width must not go negative");
        assert_eq!(out.h, 0, "height must not go negative");
        assert_eq!(out.x, 50, "origin still advances by left");
        assert_eq!(out.y, 80, "origin still advances by top");

        // Boundary i32::MAX inset must not panic (overflow-safe saturating arithmetic).
        let huge = DotRect { x: 0, y: 0, w: 10, h: 10 };
        let out = huge.inset(i32::MAX, i32::MAX, i32::MAX, i32::MAX);
        assert_eq!(out.w, 0);
        assert_eq!(out.h, 0);
    }

    #[test]
    fn inset_zero_is_identity() {
        let r = DotRect { x: 3, y: 7, w: 12, h: 24 };
        assert_eq!(r.inset(0, 0, 0, 0), r);
    }
}
