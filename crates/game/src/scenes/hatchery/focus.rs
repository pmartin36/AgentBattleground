//! Tap-to-focus interaction: classifies a completed tap on an egg by its
//! current state, lays out the centered focus view and the relocated tray
//! strip, and formats/draws the incubating countdown. Pure functions; the
//! scene applies their result and owns `focused`/`pending_define`/
//! `pending_hatch` state.

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use engine_render::{DotRect, TextAlign};

use crate::player_data::EggState;

use super::tray::{EGG_SLOT_H_DOTS, EGG_SLOT_W_DOTS};

/// Height of the relocated tray strip while an egg is focused, in cells.
const STRIP_H_CELLS: u16 = 10;

/// How a completed tap on an egg resolves. `Focus` toggles/swaps the
/// centered incubating view; `Define`/`Hatch` are the extension points a
/// mad-lib definition flow and a hatch sequence attach their own action to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TapRoute {
    Focus,
    Define,
    Hatch,
}

/// Classifies a tap by the tapped egg's state. Exhaustive match with no
/// wildcard: a future `EggState` variant must fail to compile here rather
/// than silently falling through to one of these three routes.
pub(crate) fn route_tap(state: &EggState) -> TapRoute {
    match state {
        EggState::Undefined => TapRoute::Define,
        EggState::Incubating { .. } => TapRoute::Focus,
        EggState::Ready => TapRoute::Hatch,
    }
}

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

/// Formats a remaining incubation `Duration` as `HH:MM:SS`.
pub(crate) fn format_remaining(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

/// Draws the `HH:MM:SS` readout centered in a 1-cell row just below the
/// focused egg. Text, exempt from the braille dot pipeline.
pub(crate) fn draw_countdown(buf: &mut Buffer, focus: DotRect, remaining: Duration) {
    let cr = focus.to_cell_rect();
    let row = Rect {
        x: cr.x,
        y: (cr.y + cr.height).min(buf.area.height.saturating_sub(1)),
        width: cr.width,
        height: 1,
    };
    engine_render::label(
        buf,
        row,
        &format_remaining(remaining),
        TextAlign::Center,
        Style::default().fg(Color::Rgb(0xff, 0xff, 0xff)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    use super::super::tray::{EGG_SLOT_H_DOTS, EGG_SLOT_W_DOTS};

    /// A tap on an `Undefined` egg routes to `Define`.
    #[test]
    fn route_tap_undefined_routes_to_define() {
        assert_eq!(route_tap(&EggState::Undefined), TapRoute::Define);
    }

    /// A tap on an `Incubating` egg routes to `Focus`.
    #[test]
    fn route_tap_incubating_routes_to_focus() {
        let state = EggState::Incubating { started_at: SystemTime::now() };
        assert_eq!(route_tap(&state), TapRoute::Focus);
    }

    /// A tap on a `Ready` egg routes to `Hatch`.
    #[test]
    fn route_tap_ready_routes_to_hatch() {
        assert_eq!(route_tap(&EggState::Ready), TapRoute::Hatch);
    }

    /// `23:59:59` formats exactly, with no rounding or truncation.
    #[test]
    fn format_remaining_formats_hours_minutes_seconds() {
        let d = Duration::from_secs(23 * 3600 + 59 * 60 + 59);
        assert_eq!(format_remaining(d), "23:59:59");
    }

    /// A zero duration formats as all zeros, not an empty or negative string.
    #[test]
    fn format_remaining_zero_is_all_zeros() {
        assert_eq!(format_remaining(Duration::ZERO), "00:00:00");
    }

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
