//! Screen-space positioning: pure anchoring of an element `Rect` within a
//! container `Rect` (b1-t1). Stacking (b2) and Tween-backed animation (b3)
//! extend this same module.

use std::time::Duration;

use ratatui::layout::Rect;

use crate::tween::Tween;

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

/// Axis along which `stack` sequences elements.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackAxis {
    Vertical,
    Horizontal,
}

/// Lay out each element in `sizes` in order along `axis`, starting at
/// `container`'s origin edge on that axis and advancing by each element's
/// main-axis extent plus `gap`. Cross-axis position is `container`'s start
/// edge; cross-axis size is the element's own declared size (no stretch).
/// Empty `sizes` returns an empty `Vec`. Never panics.
pub fn stack(container: Rect, sizes: &[(u16, u16)], gap: u16, axis: StackAxis) -> Vec<Rect> {
    let mut out = Vec::with_capacity(sizes.len());

    match axis {
        StackAxis::Vertical => {
            let mut cursor = container.y;
            for &(w, h) in sizes {
                out.push(Rect::new(container.x, cursor, w, h));
                cursor = cursor.saturating_add(h).saturating_add(gap);
            }
        }
        StackAxis::Horizontal => {
            let mut cursor = container.x;
            for &(w, h) in sizes {
                out.push(Rect::new(cursor, container.y, w, h));
                cursor = cursor.saturating_add(w).saturating_add(gap);
            }
        }
    }

    out
}

/// Animates an element's `Rect` from `from` to `to` over `dur`, delegating
/// all interpolation/easing to `crate::tween::Tween` (one channel per Rect
/// field).
pub struct RectTween {
    x: Tween,
    y: Tween,
    width: Tween,
    height: Tween,
}

impl RectTween {
    pub fn new(from: Rect, to: Rect, dur: Duration) -> Self {
        RectTween {
            x: Tween::new(from.x as f32, to.x as f32, dur),
            y: Tween::new(from.y as f32, to.y as f32, dur),
            width: Tween::new(from.width as f32, to.width as f32, dur),
            height: Tween::new(from.height as f32, to.height as f32, dur),
        }
    }

    /// Sample the animated `Rect` at `elapsed`. Each field is the eased
    /// value from its `Tween`, rounded to nearest and clamped into `u16`
    /// range.
    pub fn at(&self, elapsed: Duration) -> Rect {
        Rect::new(
            to_u16(self.x.at(elapsed)),
            to_u16(self.y.at(elapsed)),
            to_u16(self.width.at(elapsed)),
            to_u16(self.height.at(elapsed)),
        )
    }
}

/// Round to nearest and clamp into `u16` (floor at 0 defensively; the f32
/// here never goes negative given u16 inputs and t in [0,1]).
fn to_u16(v: f32) -> u16 {
    v.round().clamp(0.0, u16::MAX as f32) as u16
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

    // --------------------------------------------------------------- stack

    /// Vertical stack of 3 elements with distinct heights (differing from
    /// `container.width`, to catch a stretch-to-fill bug) and a nonzero gap:
    /// exact y-offsets accumulate by each element's own height + gap, x stays
    /// at `container.x`, and width is each element's own w (not
    /// `container.width`).
    #[test]
    fn stack_vertical_three_elements_exact_offsets_and_own_cross_size() {
        let c = container(); // x=10, y=20, width=100, height=40
        let sizes = [(15u16, 5u16), (30, 8), (10, 12)];
        let gap = 3u16;

        let got = stack(c, &sizes, gap, StackAxis::Vertical);

        let expected = vec![
            Rect::new(10, 20, 15, 5),
            Rect::new(10, 20 + 5 + 3, 30, 8),
            Rect::new(10, 20 + 5 + 3 + 8 + 3, 10, 12),
        ];
        assert_eq!(got, expected);
    }

    /// Horizontal stack is the exact x/width <-> y/height mirror of the
    /// vertical case: x-offsets accumulate by own width + gap, y stays at
    /// `container.y`, height is each element's own h.
    #[test]
    fn stack_horizontal_three_elements_exact_offsets_and_own_cross_size() {
        let c = container(); // x=10, y=20, width=100, height=40
        let sizes = [(5u16, 15u16), (8, 30), (12, 10)];
        let gap = 3u16;

        let got = stack(c, &sizes, gap, StackAxis::Horizontal);

        let expected = vec![
            Rect::new(10, 20, 5, 15),
            Rect::new(10 + 5 + 3, 20, 8, 30),
            Rect::new(10 + 5 + 3 + 8 + 3, 20, 12, 10),
        ];
        assert_eq!(got, expected);
    }

    /// An empty `sizes` slice returns an empty `Vec` without panicking, for
    /// either axis.
    #[test]
    fn stack_empty_sizes_returns_empty_vec() {
        let c = container();
        assert!(stack(c, &[], 4, StackAxis::Vertical).is_empty());
        assert!(stack(c, &[], 4, StackAxis::Horizontal).is_empty());
    }

    /// A single element returns a length-1 `Vec` positioned at the
    /// container's origin with the element's own size — the gap must not
    /// affect a single-element stack.
    #[test]
    fn stack_single_element_at_container_origin() {
        let c = container();
        let sizes = [(20u16, 8u16)];

        let got = stack(c, &sizes, 5, StackAxis::Vertical);
        assert_eq!(got, vec![Rect::new(c.x, c.y, 20, 8)]);
    }

    // ----------------------------------------------------------- RectTween

    /// Round-half-away-from-zero then clamp to `[0, u16::MAX]` — the
    /// conversion `RectTween::at` must apply per field, matched here so
    /// comparisons against a raw `Tween` don't spuriously differ by 1 on
    /// half-integer midpoints.
    fn to_u16(v: f32) -> u16 {
        v.round().clamp(0.0, u16::MAX as f32) as u16
    }

    /// `at(Duration::ZERO) == from` and `at(dur) == to`, with a nonzero
    /// `dur` and distinct values on all four fields so a swapped-field bug
    /// is caught.
    #[test]
    fn rect_tween_hits_endpoints() {
        let from = Rect::new(0, 5, 10, 20);
        let to = Rect::new(50, 40, 30, 60);
        let dur = Duration::from_secs(2);

        let tw = RectTween::new(from, to, dur);

        assert_eq!(tw.at(Duration::ZERO), from, "at(0) must equal from");
        assert_eq!(tw.at(dur), to, "at(dur) must equal to");
    }

    /// `at(dur/2)` for each of x/y/width/height must equal delegating to
    /// `tween::Tween::at` directly on that field (proving delegation, not
    /// reimplementation of lerp/easing).
    #[test]
    fn rect_tween_midpoint_delegates_to_tween_per_field() {
        let from = Rect::new(0, 5, 10, 20);
        let to = Rect::new(50, 40, 30, 60);
        let dur = Duration::from_secs(2);
        let half = dur / 2;

        let tw = RectTween::new(from, to, dur);
        let got = tw.at(half);

        let expected_x = to_u16(Tween::new(from.x as f32, to.x as f32, dur).at(half));
        let expected_y = to_u16(Tween::new(from.y as f32, to.y as f32, dur).at(half));
        let expected_w = to_u16(Tween::new(from.width as f32, to.width as f32, dur).at(half));
        let expected_h = to_u16(Tween::new(from.height as f32, to.height as f32, dur).at(half));

        assert_eq!(got.x, expected_x, "x must delegate to Tween::at");
        assert_eq!(got.y, expected_y, "y must delegate to Tween::at");
        assert_eq!(got.width, expected_w, "width must delegate to Tween::at");
        assert_eq!(got.height, expected_h, "height must delegate to Tween::at");
    }

    /// Composition with `anchor()`: sliding in from off-screen-left (x=0) to
    /// an anchor()-computed resting `Rect` inside a container with `x > 0`
    /// produces x that is non-decreasing across elapsed and strictly greater
    /// at the far end than the near end, reaching `to.x` at `dur`.
    #[test]
    fn rect_tween_slide_in_from_off_screen_left_is_monotonic_and_reaches_target() {
        let container = Rect::new(20, 0, 100, 10);
        let size = (20, 10);
        let to = anchor(container, size, Anchor::BottomCenter);
        let from = Rect::new(0, to.y, to.width, to.height); // off-screen-left

        let dur = Duration::from_secs(4);
        let tw = RectTween::new(from, to, dur);

        let samples: Vec<u16> = [
            Duration::ZERO,
            dur / 4,
            dur / 2,
            dur * 3 / 4,
            dur,
        ]
        .into_iter()
        .map(|elapsed| tw.at(elapsed).x)
        .collect();

        for pair in samples.windows(2) {
            assert!(
                pair[1] >= pair[0],
                "x must be non-decreasing across elapsed: {samples:?}"
            );
        }
        assert!(
            samples[4] > samples[0],
            "x must strictly increase from near to far end: {samples:?}"
        );
        assert_eq!(*samples.last().unwrap(), to.x, "must reach target x at dur");
    }
}
