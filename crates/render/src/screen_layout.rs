//! Screen-space positioning: pure anchoring of an element `Rect` within a
//! container `Rect` (b1-t1). Stacking (b2) and Tween-backed animation (b3)
//! extend this same module.

use ratatui::layout::Rect;

/// A named position within the standard 3x3 anchor grid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Anchor {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

/// One axis's alignment: at the container's near edge, centered, or at the
/// container's far edge.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Align {
    Near,
    Center,
    Far,
}

impl Anchor {
    /// Decompose into (horizontal, vertical) axis alignment.
    fn axes(self) -> (Align, Align) {
        match self {
            Anchor::TopLeft => (Align::Near, Align::Near),
            Anchor::TopCenter => (Align::Center, Align::Near),
            Anchor::TopRight => (Align::Far, Align::Near),
            Anchor::CenterLeft => (Align::Near, Align::Center),
            Anchor::Center => (Align::Center, Align::Center),
            Anchor::CenterRight => (Align::Far, Align::Center),
            Anchor::BottomLeft => (Align::Near, Align::Far),
            Anchor::BottomCenter => (Align::Center, Align::Far),
            Anchor::BottomRight => (Align::Far, Align::Far),
        }
    }
}

/// Compute the offset of `origin` along one axis given `extent` (container
/// size on that axis) and `size` (element size on that axis), per `align`.
/// Uses saturating arithmetic — never underflows/panics.
fn axis_offset(origin: u16, extent: u16, size: u16, align: Align) -> u16 {
    let leftover = extent.saturating_sub(size);
    let delta = match align {
        Align::Near => 0,
        Align::Center => leftover / 2,
        Align::Far => leftover,
    };
    origin.saturating_add(delta)
}

/// Compute the `Rect` of `size` (w, h) positioned at `pos` within
/// `container`. Never panics; the result is always fully contained within
/// `container` (oversized `size` on an axis is clamped to the container's
/// bounds on that axis).
pub fn anchor(container: Rect, size: (u16, u16), pos: Anchor) -> Rect {
    let (w, h) = size;
    let (horiz, vert) = pos.axes();

    let x = axis_offset(container.x, container.width, w, horiz);
    let y = axis_offset(container.y, container.height, h, vert);

    Rect::new(x, y, w, h).intersection(container)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed container used by every exact-position case:
    /// x=10, y=20, width=100, height=40.
    fn container() -> Rect {
        Rect::new(10, 20, 100, 40)
    }

    const SIZE: (u16, u16) = (20, 8);

    /// One exact-`Rect` assertion per each of the 9 variants against a fixed
    /// container + fixed size, hand-computed per the per-axis rule:
    /// Left/Top -> container edge; Center -> edge + (extent - size)/2
    /// (truncating); Right/Bottom -> edge + (extent - size).
    #[test]
    fn anchor_exact_position_per_variant() {
        let c = container();
        let (w, h) = SIZE;

        let cases = [
            (Anchor::TopLeft, Rect::new(10, 20, w, h)),
            (Anchor::TopCenter, Rect::new(10 + (100 - w) / 2, 20, w, h)),
            (Anchor::TopRight, Rect::new(10 + (100 - w), 20, w, h)),
            (Anchor::CenterLeft, Rect::new(10, 20 + (40 - h) / 2, w, h)),
            (
                Anchor::Center,
                Rect::new(10 + (100 - w) / 2, 20 + (40 - h) / 2, w, h),
            ),
            (
                Anchor::CenterRight,
                Rect::new(10 + (100 - w), 20 + (40 - h) / 2, w, h),
            ),
            (Anchor::BottomLeft, Rect::new(10, 20 + (40 - h), w, h)),
            (
                Anchor::BottomCenter,
                Rect::new(10 + (100 - w) / 2, 20 + (40 - h), w, h),
            ),
            (Anchor::BottomRight, Rect::new(10 + (100 - w), 20 + (40 - h), w, h)),
        ];

        for (pos, expected) in cases {
            let got = anchor(c, SIZE, pos);
            assert_eq!(got, expected, "anchor mismatch for {pos:?}");
        }
    }

    /// `Center` truncating-divides the leftover space on an odd remainder
    /// (container width 101 - size width 20 = 81, an odd leftover) and stays
    /// fully contained within the container.
    #[test]
    fn anchor_center_truncates_odd_leftover_and_stays_contained() {
        let c = Rect::new(0, 0, 101, 41);
        let size = (20, 8);
        let got = anchor(c, size, Anchor::Center);

        // 81 / 2 = 40 (truncating), 33 / 2 = 16 (truncating)
        assert_eq!(got, Rect::new(40, 16, 20, 8));

        assert!(got.x >= c.x);
        assert!(got.y >= c.y);
        assert!(got.right() <= c.right());
        assert!(got.bottom() <= c.bottom());
    }

    /// `size` equal to the container's size returns a rect equal to the
    /// container itself, for every one of the 9 anchor variants.
    #[test]
    fn anchor_size_equals_container_returns_container_for_every_variant() {
        let c = Rect::new(5, 5, 50, 30);
        let size = (c.width, c.height);
        let variants = [
            Anchor::TopLeft,
            Anchor::TopCenter,
            Anchor::TopRight,
            Anchor::CenterLeft,
            Anchor::Center,
            Anchor::CenterRight,
            Anchor::BottomLeft,
            Anchor::BottomCenter,
            Anchor::BottomRight,
        ];
        for pos in variants {
            let got = anchor(c, size, pos);
            assert_eq!(got, c, "size==container must equal container for {pos:?}");
        }
    }

    /// `size` larger than `container` on the horizontal axis is clamped:
    /// offset stays at the container's left origin, no negative offset, no
    /// panic, and the result never escapes the container's bounds.
    #[test]
    fn anchor_oversized_width_clamps_within_container() {
        let c = Rect::new(10, 20, 100, 40);
        let size = (200, 8);
        let got = anchor(c, size, Anchor::Center);

        assert_eq!(got.x, c.x, "oversized width must clamp offset to container origin");
        assert!(got.right() <= c.right(), "result must not escape container right edge");
        assert!(got.x >= c.x);
    }

    /// `size` larger than `container` on the vertical axis is clamped:
    /// offset stays at the container's top origin, no negative offset, no
    /// panic, and the result never escapes the container's bounds.
    #[test]
    fn anchor_oversized_height_clamps_within_container() {
        let c = Rect::new(10, 20, 100, 40);
        let size = (20, 100);
        let got = anchor(c, size, Anchor::Center);

        assert_eq!(got.y, c.y, "oversized height must clamp offset to container origin");
        assert!(got.bottom() <= c.bottom(), "result must not escape container bottom edge");
        assert!(got.y >= c.y);
    }
}
