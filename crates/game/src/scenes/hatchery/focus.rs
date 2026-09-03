//! Focus-view layout: lays out the centered focus view and the relocated
//! tray strip. Pure functions; the scene owns `selected`/`mode`/
//! `pending_hatch` state and keys this layout off `selected`.

use ratatui::layout::Rect;

use engine_render::DotRect;

use super::tray::{EGG_SLOT_H_DOTS, EGG_SLOT_W_DOTS};

/// Height of the relocated tray strip while an egg is focused, in cells.
const STRIP_H_CELLS: u16 = 10;

/// Splits `area` for focus mode: the large centered egg rect (dots, in the
/// region above the strip) and the bottom tray strip (cells) the remaining
/// eggs lay out in via `tray::tray_slots(strip, n)`. The returned `DotRect`
/// must be disjoint from the strip and strictly larger than a tray slot.
pub(crate) fn focus_layout(area: Rect) -> (DotRect, Rect) {
    let strip = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(STRIP_H_CELLS),
        width: area.width,
        height: area.height.min(STRIP_H_CELLS),
    };

    let ax = area.x as i32 * 2;
    let ay = area.y as i32 * 4;
    let aw = area.width as i32 * 2;
    let region_bottom = strip.y as i32 * 4;
    let region_h = (region_bottom - ay).max(0);

    let w = (aw * 2 / 5).max(EGG_SLOT_W_DOTS + 8).min(aw);
    let h = (region_h * 2 / 3).max(EGG_SLOT_H_DOTS + 8).min(region_h);

    let focus = DotRect {
        x: ax + (aw - w) / 2,
        y: ay + (region_h - h) / 2,
        w,
        h,
    };

    (focus, strip)
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::tray::{EGG_SLOT_H_DOTS, EGG_SLOT_W_DOTS};

    /// The focus rect is strictly larger than a tray slot on both axes, so
    /// the focused egg is visibly bigger than its tray render.
    #[test]
    fn focus_layout_rect_is_larger_than_a_tray_slot() {
        let area = Rect::new(0, 0, 40, 20);
        let (focus_dr, _strip) = focus_layout(area);
        assert!(focus_dr.w > EGG_SLOT_W_DOTS, "focus rect width {} must exceed a tray slot's {EGG_SLOT_W_DOTS}", focus_dr.w);
        assert!(focus_dr.h > EGG_SLOT_H_DOTS, "focus rect height {} must exceed a tray slot's {EGG_SLOT_H_DOTS}", focus_dr.h);
    }

    /// The focus rect sits entirely above the relocated tray strip, so the
    /// large centered egg never overlaps the other eggs' hit-rects.
    #[test]
    fn focus_layout_rect_is_disjoint_from_the_strip() {
        let area = Rect::new(0, 0, 40, 20);
        let (focus_dr, strip) = focus_layout(area);
        let focus_cells = focus_dr.to_cell_rect();
        assert!(
            focus_cells.y + focus_cells.height <= strip.y,
            "focus rect {focus_cells:?} must not overlap the tray strip {strip:?}"
        );
    }
}
