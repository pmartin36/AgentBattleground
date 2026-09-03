//! Master-detail layout for the browse surface: splits the scene area into
//! the large selected-egg rect (anchored roughly one third down from the
//! top), a full-width mad-lib body region below it, and the tray band along
//! the bottom (the same band `tray::tray_band` already owns). Pure layout —
//! mirrors `focus.rs`'s shape, but for the browse (not hatch-overlay)
//! surface.

use ratatui::layout::Rect;

use engine_render::DotRect;

use super::tray;

/// Splits `area` into `(egg, body, tray)`:
/// - `egg`: the large selected-egg dot rect, strictly larger than a tray
///   slot on both axes, top-anchored near one third of `area`'s height down
///   from the top, sitting entirely above `tray`.
/// - `body`: the full-width mad-lib region between `egg` and `tray`
///   (filled by the edit/detail render, not this function).
/// - `tray`: `tray::tray_band(area)`.
pub(crate) fn detail_layout(area: Rect) -> (DotRect, Rect, Rect) {
    let tray_rect = tray::tray_band(area);

    let ax = area.x as i32 * 2;
    let ay = area.y as i32 * 4;
    let aw = area.width as i32 * 2;
    let ah = area.height as i32 * 4;
    let tray_top = tray_rect.y as i32 * 4;

    let egg_top = ay + ah / 3;
    let avail = (tray_top - egg_top).max(0);
    let egg_h = (avail * 3 / 5).max(tray::EGG_SLOT_H_DOTS + 8);
    let egg_w = (egg_h * tray::EGG_SLOT_W_DOTS / tray::EGG_SLOT_H_DOTS).min(aw);

    let egg = DotRect { x: ax + (aw - egg_w) / 2, y: egg_top, w: egg_w, h: egg_h };

    let body_top_cell = ((egg_top + egg_h) / 4).max(area.y as i32) as u16;
    let body = Rect {
        x: area.x,
        y: body_top_cell,
        width: area.width,
        height: tray_rect.y.saturating_sub(body_top_cell),
    };

    (egg, body, tray_rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The selected-egg rect must be strictly larger than a tray slot on
    /// both axes, so it visibly reads as "the large one".
    #[test]
    fn detail_layout_egg_rect_is_larger_than_a_tray_slot_on_both_axes() {
        let area = Rect::new(0, 0, 40, 20);
        let (egg, _body, _tray) = detail_layout(area);
        assert!(
            egg.w > tray::EGG_SLOT_W_DOTS && egg.h > tray::EGG_SLOT_H_DOTS,
            "egg rect {egg:?} must exceed a tray slot ({}x{} dots)",
            tray::EGG_SLOT_W_DOTS,
            tray::EGG_SLOT_H_DOTS
        );
    }

    /// The selected-egg rect's top sits near one third of the scene height
    /// down from the top, not flush against any edge.
    #[test]
    fn detail_layout_egg_top_anchored_near_one_third_of_scene_height() {
        let area = Rect::new(0, 0, 40, 21);
        let (egg, _body, _tray) = detail_layout(area);
        let expected = area.y as i32 * 4 + (area.height as i32 * 4) / 3;
        let tolerance = 8;
        assert!(
            (egg.y - expected).abs() <= tolerance,
            "egg top {} must be within {tolerance} dots of one-third down ({expected})",
            egg.y
        );
    }

    /// The selected-egg rect sits entirely above the tray band — the two
    /// never overlap.
    #[test]
    fn detail_layout_egg_rect_is_disjoint_from_and_above_the_tray() {
        let area = Rect::new(0, 0, 40, 20);
        let (egg, _body, tray_rect) = detail_layout(area);
        let egg_cells = egg.to_cell_rect();
        assert!(
            egg_cells.y + egg_cells.height <= tray_rect.y,
            "egg rect {egg_cells:?} must sit above the tray {tray_rect:?}"
        );
    }

    /// The body region spans the full scene width and has non-zero height,
    /// so it can actually hold wrapped mad-lib prose.
    #[test]
    fn detail_layout_body_region_is_full_width_and_non_degenerate() {
        let area = Rect::new(0, 0, 40, 20);
        let (_egg, body, _tray) = detail_layout(area);
        assert!(
            body.width == area.width && body.height > 0,
            "body {body:?} must be full-width ({}) and non-degenerate",
            area.width
        );
    }
}
