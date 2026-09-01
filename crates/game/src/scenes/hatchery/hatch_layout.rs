//! Settled-placement layout: mirrors the roster detail screen's 2:1
//! left/right split for the reveal's resting state — the idling creature
//! and its name on the left, the stats-dock panel border on the right, both
//! carved above the stationary egg-dock strip and below the back button.

use std::time::Duration;

use ratatui::layout::Rect;

use engine_render::{Align, Basis, Direction, DotRect, DotRectTween, FlexChild, FlexStyle, Justify};

/// Height, in cells, of one wrapped line of the hatchling's name.
const NAME_LINE_H_CELLS: i32 = 1;
/// Clearance, in cells, between the back button's bottom edge and the top of
/// the settled content region.
const CONTENT_TOP_GAP_CELLS: i32 = 1;
/// Gap, in cells, between the creature column and the stats-dock border —
/// keeps the dock visibly separate from the creature, never touching.
const DOCK_RIGHT_MARGIN_CELLS: i32 = 1;

/// Dot-space region split for the settled placement: `name_zone` sits
/// directly above `creature` in the left column as flex siblings (a
/// wrapping name grows `name_zone` and shrinks `creature`, never
/// overlapping either); `dock_border` is the right column's stats-dock
/// panel border, fed to `detail_panel::interior_regions`.
#[derive(Clone, Copy, Debug)]
pub(super) struct SettledLayout {
    pub name_zone: DotRect,
    pub creature: DotRect,
    pub dock_border: DotRect,
}

/// Computes the settled-placement layout for a hatch reveal's resting
/// state: content sits below the back button and above `strip` (the
/// stationary egg dock); a 2:1 LEFT/RIGHT split carries the creature+name on
/// the left and the stats-dock border on the right; `name` is measured to
/// size `name_zone`'s height. Saturating throughout — a degenerate `area`
/// yields zero-size, non-negative rects, never a panic.
pub(super) fn settled_layout(area: Rect, strip: Rect, name: &str) -> SettledLayout {
    let back = super::Hatchery::back_dot_rect(area);

    let content_top = back.y.saturating_add(back.h).saturating_add(CONTENT_TOP_GAP_CELLS * 4);
    let content_bottom = strip.y as i32 * 4;
    let content_h = content_bottom.saturating_sub(content_top).max(0);

    let content = DotRect {
        x: area.x as i32 * 2,
        y: content_top,
        w: area.width as i32 * 2,
        h: content_h,
    };

    // LEFT/RIGHT Row split: the left column is `area.width * 2 / 3` cells
    // wide (integer-cell 2:1, not a flex-grow split — a flex-grow 2:1 is NOT
    // equivalent to this floored value); the right column is the sole grow
    // child, absorbing whatever remains after the left column and the
    // between-column gap, so it never overlaps the left column regardless
    // of `area`'s width.
    let left_w_cells = area.width * 2 / 3;
    let row = engine_render::flex(
        content,
        FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::Start,
            align_items: Align::Stretch,
            gap: DOCK_RIGHT_MARGIN_CELLS * 2,
        },
        &[
            FlexChild { basis: Basis::Fixed(left_w_cells as i32 * 2), grow: 0.0, shrink: 0.0 },
            FlexChild { basis: Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
        ],
    );
    let [left_col, dock_border] = row[..] else {
        unreachable!("flex() with 2 children returns exactly 2 rects")
    };

    // LEFT column Column flex: `name_zone` (a flex sibling above `creature`)
    // is sized by the name's wrapped line count at the left column's width,
    // clamped to a sane 1-3 line band; `creature` is the sole grow child, so
    // a taller `name_zone` shrinks `creature` and the two never overlap.
    let name_lines = engine_render::wrapped_line_count(name, left_w_cells as usize).clamp(1, 3);
    let name_h_dots = name_lines as i32 * NAME_LINE_H_CELLS * 4;
    let left_children = engine_render::flex(
        left_col,
        FlexStyle {
            direction: Direction::Column,
            justify_content: Justify::Start,
            align_items: Align::Stretch,
            gap: 0,
        },
        &[
            FlexChild { basis: Basis::Fixed(name_h_dots), grow: 0.0, shrink: 0.0 },
            FlexChild { basis: Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
        ],
    );
    let [name_zone, creature] = left_children[..] else {
        unreachable!("flex() with 2 children returns exactly 2 rects")
    };

    SettledLayout { name_zone, creature, dock_border }
}

/// Dot-space poses for the three elements the Slide phase animates:
/// `creature` and `name_zone` travel from their centered Beat poses to the
/// settled layout's rects; `dock_border` enters from fully off the right
/// edge of `area` and settles into the dock's border.
#[derive(Clone, Copy, Debug)]
pub(super) struct SlidePose {
    pub creature: DotRect,
    pub name_zone: DotRect,
    pub dock_border: DotRect,
}

/// Eased poses at slide progress `p` (`0.0..=1.0`), reusing the roster
/// carousel's exact `ease_in_out` curve via `DotRectTween`. At `p == 0.0`
/// the creature and name sit at their `creature_start`/`name_start` (the
/// centered Beat pose) and the dock sits fully off `area`'s right edge; at
/// `p == 1.0` all three equal `settled_layout(area, strip, name)`'s rects
/// exactly, so the Slide phase's final frame is byte-identical to the
/// settled placement by construction.
pub(super) fn slide_pose(
    area: Rect,
    strip: Rect,
    name: &str,
    creature_start: DotRect,
    name_start: DotRect,
    p: f32,
) -> SlidePose {
    let settled = settled_layout(area, strip, name);
    let dock_start =
        DotRect { x: (area.x as i32 + area.width as i32) * 2, ..settled.dock_border };

    const UNIT: Duration = Duration::from_secs(1);
    let at = UNIT.mul_f32(p.clamp(0.0, 1.0));
    let sample = |from: DotRect, to: DotRect| DotRectTween::new(from, to, UNIT).at(at);

    SlidePose {
        creature: sample(creature_start, settled.creature),
        name_zone: sample(name_start, settled.name_zone),
        dock_border: sample(dock_start, settled.dock_border),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_for(area: Rect) -> Rect {
        super::super::focus::focus_layout(area).1
    }

    /// The creature sits in the left column, the stats-dock border in the
    /// right, and the left column is roughly twice the right column's
    /// width — the same 2:1 split the roster detail screen uses.
    #[test]
    fn creature_left_dock_right_two_to_one() {
        let area = Rect::new(0, 0, 90, 30);
        let strip = strip_for(area);
        let s = settled_layout(area, strip, "Emberling");

        assert!(
            s.creature.x < s.dock_border.x,
            "creature {:?} must sit left of the dock {:?}",
            s.creature,
            s.dock_border
        );
        let left_w = s.dock_border.x - s.creature.x;
        let right_w = s.dock_border.w.max(1);
        let ratio = left_w as f32 / right_w as f32;
        assert!(
            (ratio - 2.0).abs() < 0.5,
            "left column width {left_w} should be roughly 2x the right column width {right_w} (ratio {ratio})"
        );
    }

    /// A name that wraps to more lines grows `name_zone` and shrinks
    /// `creature` by construction (flex siblings); neither layout overlaps
    /// the name zone with the creature zone.
    #[test]
    fn wrapped_name_shrinks_creature_zone() {
        let area = Rect::new(0, 0, 90, 30);
        let strip = strip_for(area);
        let short = settled_layout(area, strip, "Emberling");
        let long = settled_layout(
            area,
            strip,
            "A Very Long Hatchling Name That Wraps Across Two Or More Full Lines Of Text",
        );

        assert!(
            long.name_zone.h > short.name_zone.h,
            "a wrapping name's zone ({}) must be taller than a short name's zone ({})",
            long.name_zone.h,
            short.name_zone.h
        );
        assert!(
            long.creature.h < short.creature.h,
            "the creature zone must shrink when the name wraps: long {} short {}",
            long.creature.h,
            short.creature.h
        );
        assert!(
            short.name_zone.y + short.name_zone.h <= short.creature.y,
            "short-name layout must not overlap: {:?} / {:?}",
            short.name_zone,
            short.creature
        );
        assert!(
            long.name_zone.y + long.name_zone.h <= long.creature.y,
            "wrapped-name layout must not overlap: {:?} / {:?}",
            long.name_zone,
            long.creature
        );
    }

    /// Neither the creature zone nor the dock border ever extends down into
    /// `strip`'s row range — the settled content sits entirely above the
    /// stationary egg dock, never over it.
    #[test]
    fn content_stays_above_egg_dock_strip() {
        let area = Rect::new(0, 0, 90, 30);
        let strip = strip_for(area);
        let strip_top_dots = strip.y as i32 * 4;
        let s = settled_layout(area, strip, "Emberling");

        assert!(
            s.creature.y + s.creature.h <= strip_top_dots,
            "creature zone {:?} must sit above the strip (top at {strip_top_dots} dots)",
            s.creature
        );
        assert!(
            s.dock_border.y + s.dock_border.h <= strip_top_dots,
            "dock border {:?} must sit above the strip (top at {strip_top_dots} dots)",
            s.dock_border
        );
    }

    /// A tiny terminal degrades to non-negative, zero-or-more-sized rects
    /// rather than panicking.
    #[test]
    fn degenerate_small_area_no_panic() {
        let area = Rect::new(0, 0, 5, 5);
        let strip = strip_for(area);
        let s = settled_layout(area, strip, "X");
        assert!(s.name_zone.w >= 0 && s.name_zone.h >= 0);
        assert!(s.creature.w >= 0 && s.creature.h >= 0);
        assert!(s.dock_border.w >= 0 && s.dock_border.h >= 0);
    }

    /// At slide progress `p == 0.0` every element sits at its supplied
    /// centered Beat pose and the dock sits fully off `area`'s right edge;
    /// at `p == 1.0` every element equals `settled_layout`'s rects exactly,
    /// so the Slide phase's last frame is the settled placement by
    /// construction.
    #[test]
    fn slide_pose_endpoints() {
        let area = Rect::new(0, 0, 90, 30);
        let strip = strip_for(area);
        let name = "Emberling";
        let settled = settled_layout(area, strip, name);

        let creature_start = DotRect { x: 40, y: 20, w: 30, h: 40 };
        let name_start = DotRect { x: 35, y: 10, w: 40, h: 12 };

        let at0 = slide_pose(area, strip, name, creature_start, name_start, 0.0);
        assert_eq!(at0.creature, creature_start, "p=0 creature must equal the supplied Beat start pose");
        assert_eq!(at0.name_zone, name_start, "p=0 name zone must equal the supplied Beat start pose");
        let off_right_x = (area.x as i32 + area.width as i32) * 2;
        assert_eq!(at0.dock_border.x, off_right_x, "p=0 dock must sit fully off the right edge");
        assert!(
            at0.dock_border.x > settled.dock_border.x,
            "p=0 dock start {} must sit further right than its settled x {}",
            at0.dock_border.x,
            settled.dock_border.x
        );

        let at1 = slide_pose(area, strip, name, creature_start, name_start, 1.0);
        assert_eq!(at1.creature, settled.creature, "p=1 creature must equal the settled layout exactly");
        assert_eq!(at1.name_zone, settled.name_zone, "p=1 name zone must equal the settled layout exactly");
        assert_eq!(at1.dock_border, settled.dock_border, "p=1 dock must equal the settled layout exactly");
    }

    /// Sweeping `p` from 0 to 1, the creature's x and the dock's x are each
    /// monotonically non-increasing (the creature slides LEFT, the dock
    /// slides IN from the right) and both end up strictly left of where
    /// they started.
    #[test]
    fn slide_pose_creature_left_dock_in_monotonic() {
        let area = Rect::new(0, 0, 90, 30);
        let strip = strip_for(area);
        let name = "Emberling";
        let creature_start = DotRect { x: 60, y: 20, w: 30, h: 40 };
        let name_start = DotRect { x: 55, y: 10, w: 40, h: 12 };

        let ps = [0.0f32, 0.25, 0.5, 0.75, 1.0];
        let poses: Vec<SlidePose> =
            ps.iter().map(|&p| slide_pose(area, strip, name, creature_start, name_start, p)).collect();

        for w in poses.windows(2) {
            assert!(
                w[1].creature.x <= w[0].creature.x,
                "creature.x must be non-increasing across the slide: {:?}",
                poses.iter().map(|s| s.creature.x).collect::<Vec<_>>()
            );
            assert!(
                w[1].dock_border.x <= w[0].dock_border.x,
                "dock_border.x must be non-increasing across the slide: {:?}",
                poses.iter().map(|s| s.dock_border.x).collect::<Vec<_>>()
            );
        }

        let last = poses.last().unwrap();
        assert!(
            last.creature.x < creature_start.x,
            "the creature must end left of its start x: start {} end {}",
            creature_start.x,
            last.creature.x
        );
        assert!(
            last.dock_border.x < poses[0].dock_border.x,
            "the dock must end left of its off-screen start x: start {} end {}",
            poses[0].dock_border.x,
            last.dock_border.x
        );
    }
}
