//! Roster-style browse geometry: a 2:1 LEFT/RIGHT split carving the large
//! selected-egg slot on the left, the right detail panel, and the egg-dock
//! band along the bottom (the same band `tray::tray_band` owns). `egg` and
//! `panel` are dot-precise resting rects (unfloored — the hatch-out tween
//! threads them straight through); `dock` is a cell `Rect`.
#![allow(dead_code)]

use ratatui::layout::Rect;

use engine_render::{Align, Basis, Direction, DotRect, FlexChild, FlexStyle, Justify};

use super::tray;

/// Clearance, in cells, between the back button's bottom edge and the top of
/// the browse content region.
const CONTENT_TOP_GAP_CELLS: i32 = 1;
/// Gap, in cells, between the egg-slot column and the detail panel — keeps
/// the panel visibly separate from the egg slot, never touching.
const COL_GAP_CELLS: i32 = 1;

/// Roster-style browse geometry. See module docs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BrowseLayout {
    pub egg: DotRect,
    pub panel: DotRect,
    pub dock: Rect,
}

/// Computes the browse-surface layout for `area`: `dock` is
/// `tray::tray_band(area)`; `egg` and `panel` fill a 2:1 LEFT/RIGHT split of
/// the content band above the dock and below the back button, with `egg`
/// aspect-fit and centered in the left column. Saturating throughout — a
/// degenerate `area` yields zero-or-more-size, non-negative rects, never a
/// panic.
pub(crate) fn browse_layout(area: Rect) -> BrowseLayout {
    let dock = tray::tray_band(area);
    let back = super::Hatchery::back_dot_rect(area);

    let content_top = back.y.saturating_add(back.h).saturating_add(CONTENT_TOP_GAP_CELLS * 4);
    let content_bottom = dock.y as i32 * 4;
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
            gap: COL_GAP_CELLS * 2,
        },
        &[
            FlexChild { basis: Basis::Fixed(left_w_cells as i32 * 2), grow: 0.0, shrink: 0.0 },
            FlexChild { basis: Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
        ],
    );
    let [left_col, panel] = row[..] else {
        unreachable!("flex() with 2 children returns exactly 2 rects")
    };

    // Egg slot: aspect-fit `EGG_SLOT_W_DOTS`:`EGG_SLOT_H_DOTS` inside the
    // left column (inset by a small margin), centered, taking the smaller
    // of the height-bound and width-bound candidate.
    let inner = left_col.inset(COL_GAP_CELLS * 2, COL_GAP_CELLS * 2, 0, 0);
    let w_from_h = (inner.h * tray::EGG_SLOT_W_DOTS / tray::EGG_SLOT_H_DOTS.max(1)).max(0);
    let (egg_w, egg_h) = if w_from_h <= inner.w {
        (w_from_h, inner.h)
    } else {
        (inner.w, (inner.w * tray::EGG_SLOT_H_DOTS / tray::EGG_SLOT_W_DOTS.max(1)).max(0))
    };
    let egg = DotRect {
        x: inner.x + (inner.w - egg_w) / 2,
        y: inner.y + (inner.h - egg_h) / 2,
        w: egg_w,
        h: egg_h,
    };

    BrowseLayout { egg, panel, dock }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The left column (egg slot + margins) is roughly twice the width of
    /// the right column (the panel) — the same roster-style split
    /// `hatch_layout::settled_layout` uses for its creature/dock columns.
    #[test]
    fn browse_layout_columns_are_two_to_one() {
        let area = Rect::new(0, 0, 90, 30);
        let layout = browse_layout(area);

        let left_w = layout.panel.x - area.x as i32 * 2;
        let right_w = layout.panel.w.max(1);
        let ratio = left_w as f32 / right_w as f32;
        assert!(
            (ratio - 2.0).abs() < 0.5,
            "left column width {left_w} should be roughly 2x the right column width {right_w} (ratio {ratio})"
        );
    }

    /// The egg slot sits strictly left of, and disjoint from, the right
    /// panel.
    #[test]
    fn browse_layout_egg_left_of_and_disjoint_from_panel() {
        let area = Rect::new(0, 0, 90, 30);
        let layout = browse_layout(area);

        assert!(layout.egg.x < layout.panel.x, "egg {:?} must start left of panel {:?}", layout.egg, layout.panel);
        assert!(
            layout.egg.x + layout.egg.w <= layout.panel.x,
            "egg {:?} must not overlap panel {:?}",
            layout.egg,
            layout.panel
        );
    }

    /// Both the egg slot and the panel sit entirely above the dock band.
    #[test]
    fn browse_layout_egg_and_panel_above_dock() {
        let area = Rect::new(0, 0, 90, 30);
        let layout = browse_layout(area);
        let dock_top_dots = layout.dock.y as i32 * 4;

        assert!(
            layout.egg.y + layout.egg.h <= dock_top_dots,
            "egg {:?} must sit above the dock (top at {dock_top_dots} dots)",
            layout.egg
        );
        assert!(
            layout.panel.y + layout.panel.h <= dock_top_dots,
            "panel {:?} must sit above the dock (top at {dock_top_dots} dots)",
            layout.panel
        );
    }

    /// The dock band is exactly `tray::tray_band(area)` — reused, not
    /// recomputed.
    #[test]
    fn browse_layout_dock_is_tray_band() {
        let area = Rect::new(0, 0, 90, 30);
        let layout = browse_layout(area);
        assert_eq!(layout.dock, tray::tray_band(area));
    }

    /// The selected-egg rect exceeds a dock chip on both axes, so it
    /// visibly reads as "the large one" (mirrors the deleted
    /// `detail_layout` egg-size test, moved here for the roster-style
    /// layout's own egg slot).
    #[test]
    fn browse_layout_egg_exceeds_a_dock_chip() {
        for area in [Rect::new(0, 0, 90, 30), Rect::new(0, 0, 40, 20)] {
            let layout = browse_layout(area);
            assert!(
                layout.egg.w > tray::EGG_SLOT_W_DOTS && layout.egg.h > tray::EGG_SLOT_H_DOTS,
                "egg rect {:?} must exceed a dock chip ({}x{} dots) for area {area:?}",
                layout.egg,
                tray::EGG_SLOT_W_DOTS,
                tray::EGG_SLOT_H_DOTS
            );
        }
    }

    /// `egg` and `panel` are dot-precise `DotRect`s (never floored through
    /// an intermediate cell `Rect`) — the invariant the hatch-out tween
    /// depends on to animate at sub-cell precision.
    #[test]
    fn browse_layout_egg_and_panel_are_dot_precise() {
        let area = Rect::new(0, 0, 90, 30);
        let layout = browse_layout(area);
        let _egg: DotRect = layout.egg;
        let _panel: DotRect = layout.panel;
    }

    /// A tiny terminal degrades to non-negative, zero-or-more-sized rects
    /// rather than panicking.
    #[test]
    fn browse_layout_degenerate_small_area_no_panic() {
        let area = Rect::new(0, 0, 5, 5);
        let layout = browse_layout(area);
        assert!(layout.egg.w >= 0 && layout.egg.h >= 0);
        assert!(layout.panel.w >= 0 && layout.panel.h >= 0);
    }
}
