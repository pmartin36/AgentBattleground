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

/// Main axis a `flex()` container distributes its children along.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Row,
    Column,
}

/// Main-axis distribution of children after basis+grow/shrink sizing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Justify {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// Cross-axis alignment of each child within the container's cross extent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Start,
    Center,
    End,
    Stretch,
}

/// A child's main-axis base size before grow/shrink is applied.
pub enum Basis {
    /// Exact dot size on the main axis.
    Fixed(i32),
    /// Content-driven: given the container's main-axis extent, returns this
    /// child's natural (main, cross) size in dots. Invoked once per `flex()`
    /// call, in child order, before grow/shrink distribution.
    Intrinsic(Box<dyn Fn(i32) -> (i32, i32)>),
}

/// A single child passed into `flex()`.
pub struct FlexChild {
    pub basis: Basis,
    /// > 0.0: claims a proportional share of leftover main-axis space
    /// > (CSS flex-grow). 0.0 (default): never grows.
    pub grow: f32,
    /// > 0.0: compresses proportionally, weighted by `shrink * basis`
    /// > (CSS flex-shrink), when basis+gaps exceed the container. 0.0
    /// > (default): never shrinks below its basis.
    pub shrink: f32,
}

/// Container-level layout style passed into `flex()`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlexStyle {
    pub direction: Direction,
    pub justify_content: Justify,
    pub align_items: Align,
    /// Dots, between adjacent children only — never before the first or
    /// after the last child.
    pub gap: i32,
}

/// Lay out `children` along `style.direction` within `container`, per the
/// 5-step solve order: (1) resolve each child's main-axis basis + natural
/// cross size (an `Intrinsic` closure is invoked exactly once, in child
/// order, with the container's main-axis extent); (2) sum bases + `gap *
/// (n-1)`; (3) distribute leftover main-axis space proportionally by `grow`
/// if the container has room, or distribute the deficit proportionally by
/// `shrink * basis` (clamped at zero) if over-committed; (4) position along
/// the main axis per `justify_content`; (5) align on the cross axis per
/// `align_items`. Pure function; never panics; every returned `DotRect` is
/// contained within `container` (same clip contract as `anchor`/`stack`).
/// Empty `children` returns `vec![]`.
pub fn flex(container: DotRect, style: FlexStyle, children: &[FlexChild]) -> Vec<DotRect> {
    if children.is_empty() {
        return vec![];
    }
    let n = children.len();
    let is_row = matches!(style.direction, Direction::Row);
    let main_extent = if is_row { container.w } else { container.h };
    let cross_extent = if is_row { container.h } else { container.w };

    // Step 1: resolve each child's main-axis basis + natural cross size.
    let mut main_basis = Vec::with_capacity(n);
    let mut natural_cross = Vec::with_capacity(n);
    for child in children {
        let (m, c) = match &child.basis {
            Basis::Fixed(v) => (*v, cross_extent),
            Basis::Intrinsic(f) => f(main_extent),
        };
        main_basis.push(m);
        natural_cross.push(c);
    }

    // Step 2: total basis + gaps (gap only between children).
    let gap_total = if n > 1 { style.gap.saturating_mul(n as i32 - 1) } else { 0 };
    let total_basis = main_basis
        .iter()
        .fold(0i32, |acc, &v| acc.saturating_add(v))
        .saturating_add(gap_total);

    // Step 3: grow / shrink.
    let free = main_extent.saturating_sub(total_basis);
    let grow_weights: Vec<f32> = children.iter().map(|c| c.grow.max(0.0)).collect();
    let grow_sum: f32 = grow_weights.iter().sum();
    let shrink_weights: Vec<f32> = children
        .iter()
        .zip(&main_basis)
        .map(|(c, &b)| c.shrink.max(0.0) * (b as f32))
        .collect();
    let shrink_sum: f32 = shrink_weights.iter().sum();

    let final_main: Vec<i32> = if free > 0 && grow_sum > 0.0 {
        let extra = distribute(free, &grow_weights);
        main_basis.iter().zip(extra).map(|(&b, e)| b.saturating_add(e)).collect()
    } else if free < 0 && shrink_sum > 0.0 {
        let removed = distribute(free.saturating_neg(), &shrink_weights);
        main_basis.iter().zip(removed).map(|(&b, r)| b.saturating_sub(r).max(0)).collect()
    } else {
        main_basis.clone()
    };

    // Step 4: justify — main-axis leading offset + inter-child extra spacing.
    let content = final_main
        .iter()
        .fold(0i32, |acc, &v| acc.saturating_add(v))
        .saturating_add(gap_total);
    let slack = main_extent.saturating_sub(content).max(0);
    let internal_gaps = n.saturating_sub(1);

    let (lead, gap_extra): (i32, Vec<i32>) = match style.justify_content {
        Justify::Start => (0, vec![0; internal_gaps]),
        Justify::End => (slack, vec![0; internal_gaps]),
        Justify::Center => (slack / 2, vec![0; internal_gaps]),
        Justify::SpaceBetween => {
            if internal_gaps == 0 {
                (0, vec![])
            } else {
                (0, distribute(slack, &vec![1.0f32; internal_gaps]))
            }
        }
        Justify::SpaceAround => {
            let slots = distribute(slack, &vec![1.0f32; 2 * n]);
            let lead = slots.first().copied().unwrap_or(0);
            let extra = (0..internal_gaps)
                .map(|i| slots[2 * i + 1].saturating_add(slots[2 * i + 2]))
                .collect();
            (lead, extra)
        }
        Justify::SpaceEvenly => {
            let slots = distribute(slack, &vec![1.0f32; n + 1]);
            let lead = slots.first().copied().unwrap_or(0);
            let extra = (0..internal_gaps).map(|i| slots[i + 1]).collect();
            (lead, extra)
        }
    };

    // Step 5: cross-axis size/offset, assemble, clip to `container`.
    let mut out = Vec::with_capacity(n);
    let mut cursor = lead;
    for i in 0..n {
        let main_off = cursor;
        let main_size = final_main[i].max(0);

        let (cross_size, cross_off) = if matches!(style.align_items, Align::Stretch) {
            (cross_extent.max(0), 0)
        } else {
            let size = natural_cross[i].max(0);
            let leftover = cross_extent.saturating_sub(size).max(0);
            let off = match style.align_items {
                Align::Start => 0,
                Align::Center => leftover / 2,
                Align::End => leftover,
                Align::Stretch => unreachable!("handled above"),
            };
            (size, off)
        };

        let rect = if is_row {
            DotRect {
                x: container.x.saturating_add(main_off),
                y: container.y.saturating_add(cross_off),
                w: main_size,
                h: cross_size,
            }
        } else {
            DotRect {
                x: container.x.saturating_add(cross_off),
                y: container.y.saturating_add(main_off),
                w: cross_size,
                h: main_size,
            }
        };
        out.push(clip_to_container(rect, container));

        cursor = cursor.saturating_add(final_main[i]);
        if i < internal_gaps {
            cursor = cursor.saturating_add(style.gap).saturating_add(gap_extra[i]);
        }
    }

    out
}

/// Largest-remainder proportional split of `total` (dots) across `weights`.
/// Returns a `Vec` the same length as `weights` that sums EXACTLY to `total`
/// (never over/under by rounding drift), every entry `>= 0`, proportional to
/// its weight, with leftover dots handed to the largest fractional
/// remainders (ties -> lower index). Returns all zeros if `total <= 0`,
/// `weights` is empty, or every weight is `<= 0`. Shared by grow, shrink, and
/// the three `Space*` justifications so every proportional split in `flex()`
/// goes through one deterministic implementation.
fn distribute(total: i32, weights: &[f32]) -> Vec<i32> {
    if weights.is_empty() || total <= 0 {
        return vec![0; weights.len()];
    }
    let sum: f32 = weights.iter().map(|w| w.max(0.0)).sum();
    if sum <= 0.0 {
        return vec![0; weights.len()];
    }

    let total_f = total as f32;
    let raw: Vec<f32> = weights.iter().map(|w| total_f * w.max(0.0) / sum).collect();
    let mut out: Vec<i32> = raw.iter().map(|r| r.floor() as i32).collect();
    let floor_sum: i32 = out.iter().fold(0i32, |acc, &v| acc.saturating_add(v));
    let mut remainder = total.saturating_sub(floor_sum).max(0);

    let mut fracs: Vec<(usize, f32)> =
        raw.iter().zip(&out).enumerate().map(|(i, (r, &f))| (i, r - f as f32)).collect();
    fracs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));

    let mut idx = 0;
    while remainder > 0 && idx < fracs.len() {
        out[fracs[idx].0] = out[fracs[idx].0].saturating_add(1);
        remainder -= 1;
        idx += 1;
    }

    out
}

/// Clip `r` to be fully contained within `container`, matching `Rect`'s
/// `.intersection` used by `anchor`/`stack` — but for dot-space `DotRect`,
/// which has no such method and may carry a negative-size `container`
/// (degrades to a zero-size clip rather than panicking).
fn clip_to_container(r: DotRect, container: DotRect) -> DotRect {
    let c_right = container.x.saturating_add(container.w.max(0));
    let c_bottom = container.y.saturating_add(container.h.max(0));
    let c_right = c_right.max(container.x);
    let c_bottom = c_bottom.max(container.y);

    let x = r.x.clamp(container.x, c_right);
    let y = r.y.clamp(container.y, c_bottom);
    let right = r.x.saturating_add(r.w.max(0)).min(c_right).max(x);
    let bottom = r.y.saturating_add(r.h.max(0)).min(c_bottom).max(y);

    DotRect { x, y, w: right - x, h: bottom - y }
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

    // ---- b2-t2: flex() solver ----

    fn fixed(main: i32, grow: f32, shrink: f32) -> FlexChild {
        FlexChild { basis: Basis::Fixed(main), grow, shrink }
    }

    #[test]
    fn fixed_only_row_accumulates_by_basis_and_gap() {
        let container = DotRect { x: 0, y: 0, w: 100, h: 40 };
        let style = FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::Start,
            align_items: Align::Start,
            gap: 2,
        };
        let children = [fixed(10, 0.0, 0.0), fixed(20, 0.0, 0.0), fixed(30, 0.0, 0.0)];
        let out = flex(container, style, &children);
        assert_eq!(out.len(), 3);
        // Cross axis: Fixed children fill the container's full cross extent
        // (mirrors `stack`'s "no stretch declared, full cross size" today).
        assert_eq!(out[0], DotRect { x: 0, y: 0, w: 10, h: 40 });
        assert_eq!(out[1], DotRect { x: 12, y: 0, w: 20, h: 40 });
        assert_eq!(out[2], DotRect { x: 34, y: 0, w: 30, h: 40 });
    }

    #[test]
    fn single_grow_child_absorbs_leftover_main_space() {
        let container = DotRect { x: 0, y: 0, w: 100, h: 10 };
        let style = FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::Start,
            align_items: Align::Start,
            gap: 0,
        };
        let children = [fixed(20, 0.0, 0.0), fixed(30, 1.0, 0.0), fixed(10, 0.0, 0.0)];
        let out = flex(container, style, &children);
        assert_eq!(out[0].w, 20, "non-grow sibling keeps its basis");
        assert_eq!(out[1].w, 70, "sole grow child absorbs all leftover (100 - 60)");
        assert_eq!(out[2].w, 10, "non-grow sibling keeps its basis");
        assert_eq!(out[0].x, 0);
        assert_eq!(out[1].x, 20);
        assert_eq!(out[2].x, 90);
    }

    #[test]
    fn over_committed_shrink_distributes_deficit_proportionally_no_negative() {
        // basis 40+40=80 > container 50; equal shrink*basis weights (40, 40)
        // split the 30-dot deficit evenly: 15 each -> final 25 + 25 = 50.
        let container = DotRect { x: 0, y: 0, w: 50, h: 5 };
        let style = FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::Start,
            align_items: Align::Start,
            gap: 0,
        };
        let children = [fixed(40, 0.0, 1.0), fixed(40, 0.0, 1.0)];
        let out = flex(container, style, &children);
        assert_eq!(out[0].w, 25);
        assert_eq!(out[1].w, 25);
        assert!(out[0].w >= 0 && out[1].w >= 0, "shrink must never go negative");
    }

    #[test]
    fn justify_variants_position_main_axis() {
        // Two fixed children (10, 20), gap 0, content = 30; container width
        // 102 makes slack (72) evenly divisible by every Space* denominator
        // (1 gap, 4 half-slots, 3 slots) so expected offsets are exact
        // integers, not remainder-distribution-dependent.
        let container = DotRect { x: 0, y: 0, w: 102, h: 5 };
        let children = || [fixed(10, 0.0, 0.0), fixed(20, 0.0, 0.0)];
        let style = |justify| FlexStyle {
            direction: Direction::Row,
            justify_content: justify,
            align_items: Align::Start,
            gap: 0,
        };

        let out = flex(container, style(Justify::Start), &children());
        assert_eq!((out[0].x, out[1].x), (0, 10), "Start");

        let out = flex(container, style(Justify::End), &children());
        assert_eq!((out[0].x, out[1].x), (72, 82), "End");

        let out = flex(container, style(Justify::Center), &children());
        assert_eq!((out[0].x, out[1].x), (36, 46), "Center");

        let out = flex(container, style(Justify::SpaceBetween), &children());
        assert_eq!((out[0].x, out[1].x), (0, 82), "SpaceBetween");

        let out = flex(container, style(Justify::SpaceAround), &children());
        assert_eq!((out[0].x, out[1].x), (18, 64), "SpaceAround");

        let out = flex(container, style(Justify::SpaceEvenly), &children());
        assert_eq!((out[0].x, out[1].x), (24, 58), "SpaceEvenly");
    }

    #[test]
    fn align_variants_size_cross_axis() {
        // A single Intrinsic child with natural cross size 6 inside a
        // height-40 Row container (leftover = 34) exercises each Align
        // variant's cross-axis size/offset.
        let container = DotRect { x: 0, y: 0, w: 100, h: 40 };
        let child = || FlexChild {
            basis: Basis::Intrinsic(Box::new(|_main| (10, 6))),
            grow: 0.0,
            shrink: 0.0,
        };
        let style = |align| FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::Start,
            align_items: align,
            gap: 0,
        };

        let out = flex(container, style(Align::Start), std::slice::from_ref(&child()));
        assert_eq!((out[0].y, out[0].h), (0, 6), "Start");

        let out = flex(container, style(Align::Center), std::slice::from_ref(&child()));
        assert_eq!((out[0].y, out[0].h), (17, 6), "Center: leftover 34 / 2");

        let out = flex(container, style(Align::End), std::slice::from_ref(&child()));
        assert_eq!((out[0].y, out[0].h), (34, 6), "End: full leftover");

        let out = flex(container, style(Align::Stretch), std::slice::from_ref(&child()));
        assert_eq!((out[0].y, out[0].h), (0, 40), "Stretch fills the full cross extent");
    }

    #[test]
    fn gap_applied_only_between_children_not_before_first_or_after_last() {
        let container = DotRect { x: 0, y: 0, w: 100, h: 1 };
        let style = FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::Start,
            align_items: Align::Start,
            gap: 5,
        };
        let children = [fixed(10, 0.0, 0.0), fixed(10, 0.0, 0.0), fixed(10, 0.0, 0.0)];
        let out = flex(container, style, &children);
        assert_eq!(out[0].x, 0);
        assert_eq!(out[1].x, 15, "gap applied between child 0 and 1");
        assert_eq!(out[2].x, 30, "gap applied between child 1 and 2");
        // 3*10 + 2*5 = 40, not 3*10 + 3*5 = 45 — no trailing gap after the
        // last child.
        assert_eq!(out[2].x + out[2].w, 40);
    }

    #[test]
    fn direction_row_vs_column_swaps_axis() {
        let style = |direction| FlexStyle {
            direction,
            justify_content: Justify::Start,
            align_items: Align::Start,
            gap: 0,
        };
        let children = || [fixed(10, 0.0, 0.0), fixed(20, 0.0, 0.0)];

        let row_container = DotRect { x: 0, y: 0, w: 100, h: 50 };
        let out = flex(row_container, style(Direction::Row), &children());
        assert_eq!(out[0], DotRect { x: 0, y: 0, w: 10, h: 50 });
        assert_eq!(out[1], DotRect { x: 10, y: 0, w: 20, h: 50 });

        let col_container = DotRect { x: 0, y: 0, w: 50, h: 100 };
        let out = flex(col_container, style(Direction::Column), &children());
        assert_eq!(out[0], DotRect { x: 0, y: 0, w: 50, h: 10 });
        assert_eq!(out[1], DotRect { x: 0, y: 10, w: 50, h: 20 });
    }

    #[test]
    fn intrinsic_basis_invoked_exactly_once_with_container_main_extent() {
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let calls_clone = calls.clone();
        let children = [FlexChild {
            basis: Basis::Intrinsic(Box::new(move |main_extent| {
                calls_clone.set(calls_clone.get() + 1);
                (main_extent / 2, 7)
            })),
            grow: 0.0,
            shrink: 0.0,
        }];
        let style = FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::Start,
            align_items: Align::Start,
            gap: 0,
        };
        let container = DotRect { x: 0, y: 0, w: 100, h: 20 };
        let out = flex(container, style, &children);
        assert_eq!(calls.get(), 1, "Intrinsic closure must be invoked exactly once");
        assert_eq!(out[0].w, 50, "Intrinsic basis honored: main_extent/2 = 100/2");
    }

    #[test]
    fn empty_children_returns_empty_vec() {
        let container = DotRect { x: 0, y: 0, w: 100, h: 40 };
        let style = FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::Start,
            align_items: Align::Start,
            gap: 0,
        };
        let out = flex(container, style, &[]);
        assert_eq!(out, Vec::<DotRect>::new());
    }

    #[test]
    fn zero_and_negative_size_container_does_not_panic() {
        let style = FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::Center,
            align_items: Align::Center,
            gap: 1,
        };
        let children = [fixed(10, 1.0, 0.0), fixed(5, 0.0, 0.0)];

        let zero = DotRect { x: 0, y: 0, w: 0, h: 0 };
        let out = flex(zero, style, &children);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|r| r.w >= 0 && r.h >= 0), "never negative on a zero-size container");

        let negative = DotRect { x: 3, y: 4, w: -5, h: -10 };
        let out = flex(negative, style, &children);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|r| r.w >= 0 && r.h >= 0), "never negative on a negative-size container");
    }

    // ---- b2-t3: Tier 1 primitive acceptance suite (synthetic pain-point scenarios) ----

    #[test]
    fn top_align_within_available_band() {
        // A child sitting flush at the near edge of a taller band, keeping its
        // natural cross size (Align::Start), with a grow sibling absorbing the
        // remaining main-axis space.
        let container = DotRect { x: 0, y: 0, w: 100, h: 40 };
        let style = FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::Start,
            align_items: Align::Start,
            gap: 0,
        };
        let children = [
            FlexChild { basis: Basis::Intrinsic(Box::new(|_main| (30, 8))), grow: 0.0, shrink: 0.0 },
            fixed(0, 1.0, 0.0),
        ];
        let out = flex(container, style, &children);
        assert_eq!(out[0], DotRect { x: 0, y: 0, w: 30, h: 8 });
        assert_eq!(out[1], DotRect { x: 30, y: 0, w: 70, h: 40 });
        assert_eq!(out[0].y, 0, "Align::Start keeps the child flush at the band's near edge");
        assert_eq!(out[0].h, 8, "natural cross size, not stretched (Align::Center would give y==16)");
        assert_eq!(out[1].w, 70, "grow sibling absorbs the 100-30 remainder");
    }

    #[test]
    fn cross_axis_centers_against_computed_subregion_not_container() {
        // Two-level nesting: an inner flex() lays out unevenly-sized children
        // (mirroring dot-cluster + label widths); the union of its result
        // becomes the container for an outer flex() that centers a third
        // element against that sub-region, not the full outer container.
        let inner_container = DotRect { x: 0, y: 0, w: 100, h: 20 };
        let inner_style = FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::Start,
            align_items: Align::Start,
            gap: 4,
        };
        let inner_children = [
            FlexChild { basis: Basis::Intrinsic(Box::new(|_main| (10, 6))), grow: 0.0, shrink: 0.0 },
            FlexChild { basis: Basis::Intrinsic(Box::new(|_main| (16, 8))), grow: 0.0, shrink: 0.0 },
            FlexChild { basis: Basis::Intrinsic(Box::new(|_main| (8, 4))), grow: 0.0, shrink: 0.0 },
        ];
        let inner_out = flex(inner_container, inner_style, &inner_children);
        assert_eq!(inner_out[0], DotRect { x: 0, y: 0, w: 10, h: 6 });
        assert_eq!(inner_out[1], DotRect { x: 14, y: 0, w: 16, h: 8 });
        assert_eq!(inner_out[2], DotRect { x: 34, y: 0, w: 8, h: 4 });

        // Test-local bounding-box union closure (not a `DotRect` method).
        let bbox = {
            let min_x = inner_out.iter().map(|r| r.x).min().unwrap();
            let min_y = inner_out.iter().map(|r| r.y).min().unwrap();
            let max_right = inner_out.iter().map(|r| r.x + r.w).max().unwrap();
            let max_bottom = inner_out.iter().map(|r| r.y + r.h).max().unwrap();
            DotRect { x: min_x, y: min_y, w: max_right - min_x, h: max_bottom - min_y }
        };
        assert_eq!(bbox, DotRect { x: 0, y: 0, w: 42, h: 8 });

        let outer_style = FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::Center,
            align_items: Align::Center,
            gap: 0,
        };
        let outer_children =
            [FlexChild { basis: Basis::Intrinsic(Box::new(|_main| (5, 4))), grow: 0.0, shrink: 0.0 }];
        let outer_out = flex(bbox, outer_style, &outer_children);
        assert_eq!(outer_out[0], DotRect { x: 18, y: 2, w: 5, h: 4 });
        assert_eq!(outer_out[0].y, 2, "cross-centered against bbox.h=8 (leftover 4/2)");
        assert_ne!(outer_out[0].y, 8, "must not center against the outer container's full h=20");
    }

    #[test]
    fn edge_anchored_resize_pins_far_edge() {
        // A grow child followed by a Fixed sibling pinned via Justify::End —
        // shrinking the fixed sibling must never move the far/right edge.
        let style = FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::End,
            align_items: Align::Start,
            gap: 0,
        };
        let container = DotRect { x: 0, y: 0, w: 100, h: 10 };

        let children_f20 = [fixed(0, 1.0, 0.0), fixed(20, 0.0, 0.0)];
        let out_f20 = flex(container, style, &children_f20);
        assert_eq!(out_f20[0], DotRect { x: 0, y: 0, w: 80, h: 10 });
        assert_eq!(out_f20[1], DotRect { x: 80, y: 0, w: 20, h: 10 });

        let children_f10 = [fixed(0, 1.0, 0.0), fixed(10, 0.0, 0.0)];
        let out_f10 = flex(container, style, &children_f10);
        assert_eq!(out_f10[0], DotRect { x: 0, y: 0, w: 90, h: 10 });
        assert_eq!(out_f10[1], DotRect { x: 90, y: 0, w: 10, h: 10 });

        assert_eq!(out_f20[1].x + out_f20[1].w, 100, "far edge pinned (F=20)");
        assert_eq!(out_f10[1].x + out_f10[1].w, 100, "far edge pinned (F=10)");
        assert_eq!(out_f10[1].x - out_f20[1].x, 10, "only the internal boundary moves (80->90)");
        assert_eq!(out_f10[0].w - out_f20[0].w, 10, "grow child absorbs the difference (80->90)");
    }

    #[test]
    fn gap_with_center_and_space_between_exact_spacing() {
        // 3 fixed(10) children, gap 4 -> content = 3*10 + 2*4 = 38.
        let style = |justify| FlexStyle {
            direction: Direction::Row,
            justify_content: justify,
            align_items: Align::Start,
            gap: 4,
        };
        let children = || [fixed(10, 0.0, 0.0), fixed(10, 0.0, 0.0), fixed(10, 0.0, 0.0)];

        // Justify::Center at two different widths: gap is constant, only the
        // lead offset shifts with the container width.
        let out_100 = flex(DotRect { x: 0, y: 0, w: 100, h: 1 }, style(Justify::Center), &children());
        assert_eq!((out_100[0].x, out_100[1].x, out_100[2].x), (31, 45, 59));

        let out_80 = flex(DotRect { x: 0, y: 0, w: 80, h: 1 }, style(Justify::Center), &children());
        assert_eq!((out_80[0].x, out_80[1].x, out_80[2].x), (21, 35, 49));

        for out in [&out_100, &out_80] {
            assert_eq!(out[1].x - (out[0].x + out[0].w), 4, "gap constant regardless of width");
            assert_eq!(out[2].x - (out[1].x + out[1].w), 4, "gap constant regardless of width");
        }
        assert_ne!(out_100[0].x, out_80[0].x, "only the lead offset shifts between widths");

        // Justify::SpaceBetween at w=100: slack 62 distributed over 2
        // internal gaps -> +31 each, last child flush to the far edge.
        let out_sb = flex(DotRect { x: 0, y: 0, w: 100, h: 1 }, style(Justify::SpaceBetween), &children());
        assert_eq!((out_sb[0].x, out_sb[1].x, out_sb[2].x), (0, 45, 90));
        assert_eq!(out_sb[1].x - (out_sb[0].x + out_sb[0].w), 35, "base gap 4 + distributed 31");
        assert_eq!(out_sb[2].x - (out_sb[1].x + out_sb[1].w), 35, "equal spacing between both pairs");
        assert_eq!(out_sb[2].x + out_sb[2].w, 100, "last child flush to the far edge");
    }

    #[test]
    fn solver_invariants_grow_shrink_proportional_and_intrinsic_once() {
        let style = FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::Start,
            align_items: Align::Start,
            gap: 0,
        };

        // 1. Over-committed + shrink>0 always fits within the container.
        let container = DotRect { x: 0, y: 0, w: 30, h: 1 };
        let children = [fixed(40, 0.0, 1.0), fixed(20, 0.0, 1.0)];
        let out = flex(container, style, &children);
        assert_eq!((out[0].w, out[1].w), (20, 10));
        let total: i32 = out.iter().map(|r| r.w).sum();
        assert!(total <= container.w, "compression keeps the summed extents within the container");

        // 2. Grow distribution is exactly proportional for >=3 distinct weights.
        let container = DotRect { x: 0, y: 0, w: 90, h: 1 };
        let children = [fixed(10, 1.0, 0.0), fixed(10, 2.0, 0.0), fixed(10, 3.0, 0.0)];
        let out = flex(container, style, &children);
        assert_eq!((out[0].w, out[1].w, out[2].w), (20, 30, 40), "extra shares 10,20,30 == 60*[1,2,3]/6");

        // 3. Shrink distribution is exactly proportional to shrink*basis
        // (same config as invariant 1: removed shares 20,10 == 30*[40,20]/60).
        let container = DotRect { x: 0, y: 0, w: 30, h: 1 };
        let children = [fixed(40, 0.0, 1.0), fixed(20, 0.0, 1.0)];
        let out = flex(container, style, &children);
        assert_eq!((out[0].w, out[1].w), (20, 10));

        // 4. Intrinsic invoked exactly once PER child (distinct from the
        // single-child case at flex.rs:585 — this covers 3 children).
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let make_child = || {
            let calls_clone = calls.clone();
            FlexChild {
                basis: Basis::Intrinsic(Box::new(move |_main| {
                    calls_clone.set(calls_clone.get() + 1);
                    (5, 5)
                })),
                grow: 0.0,
                shrink: 0.0,
            }
        };
        let children = [make_child(), make_child(), make_child()];
        let container = DotRect { x: 0, y: 0, w: 100, h: 10 };
        let _ = flex(container, style, &children);
        assert_eq!(calls.get(), 3, "Intrinsic invoked exactly once per child");
    }
}
