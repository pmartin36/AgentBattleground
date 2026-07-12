//! Shared home button for the roster and post-battle screens: a filled gold
//! braille circle (via the engine `circle` primitive) with the amber-tinted
//! home icon centered on top, keyed to `ButtonState`. The caller owns the
//! `ButtonCore`, its rect, and hit-testing, and passes a ~square-dot `region`
//! (so the disc reads round); this only draws.

use engine_core::color::Rgba;
use engine_render::dots::{Dot, DotBuffer};
use engine_render::{ui_primitives, ButtonState, DotRect};
use ratatui::buffer::Buffer;

/// Disc color by state — gold; base idle, brighter hover, darker pressed.
/// Idle matches `Button`'s `PANEL_GOLD_TINT` so the swap keeps the same hue.
const GOLD_IDLE: Rgba = Rgba::rgb(0xc9, 0xa0, 0x3c);
const GOLD_HOVER: Rgba = Rgba::rgb(0xff, 0xcf, 0x5a);
const GOLD_PRESSED: Rgba = Rgba::rgb(0x8c, 0x6c, 0x28);
/// Icon multiply tint — amber, so a light icon reads as a clean dark-on-gold
/// silhouette (a white icon on gold smears at the luma threshold; this is why
/// `Button` tints `ICON_HOME` amber today).
const ICON_AMBER: Rgba = Rgba::rgb(0x8a, 0x4a, 0x00);

/// Draws the home button into `region`: a round filled gold disc (diameter =
/// the region's smaller dot dimension, so it stays a circle for any region
/// aspect) centered in `region`, with the amber-tinted `ICON_HOME` centered on
/// top, shaded per `state`. The `region` `DotRect` is blitted UNFLOORED so the
/// home rect's deliberate 1-dot-up sub-cell nudge survives. Zero-area `region`
/// draws nothing and does not panic.
pub(crate) fn draw_home_button(buf: &mut Buffer, region: DotRect, state: ButtonState) {
    let w_dots = region.w.max(0) as usize;
    let h_dots = region.h.max(0) as usize;
    if w_dots == 0 || h_dots == 0 {
        return;
    }
    let gold = match state {
        ButtonState::Idle => GOLD_IDLE,
        ButtonState::Hover => GOLD_HOVER,
        ButtonState::Pressed => GOLD_PRESSED,
    };

    // Round disc: diameter = the smaller dot dimension → a square (round) box.
    let d = w_dots.min(h_dots);
    let mut dots: DotBuffer = ui_primitives::circle(d, gold);

    // Amber-tinted home icon, aspect-fit to the region's cell rect (the same
    // fit `Button` uses) and centered on the disc; its lit dots overwrite the
    // gold, its transparent dots leave the gold showing.
    let cell_rect = region.to_cell_rect();
    let fitted = engine_render::asset_cache::convert(crate::assets::ICON_HOME, cell_rect);
    let (fc, fr) = (fitted.cols(), fitted.rows());
    if fc > 0 && fr > 0 {
        let icon_raw = engine_render::asset_cache::sprite_to_dots(
            crate::assets::ICON_HOME,
            fc as u32 * 2,
            fr as u32 * 4,
        );
        let icon = engine_render::dots::tint(&icon_raw, ICON_AMBER);
        let off_x = d.saturating_sub(icon.cols()) / 2;
        let off_y = d.saturating_sub(icon.rows()) / 2;
        for r in 0..icon.rows() {
            for c in 0..icon.cols() {
                if let Dot::Lit(color) = icon.get(c, r) {
                    let (tx, ty) = (off_x + c, off_y + r);
                    if tx < dots.cols() && ty < dots.rows() {
                        dots.set(tx, ty, Dot::Lit(color));
                    }
                }
            }
        }
    }

    // Center the disc in the region; blit unfloored (blit_dots pads by the
    // region's cell_remainder, preserving the sub-cell nudge — do NOT floor
    // the region to cells the way `draw_close_button` does).
    let disc_region = DotRect {
        x: region.x + (w_dots as i32 - d as i32) / 2,
        y: region.y + (h_dots as i32 - d as i32) / 2,
        w: d as i32,
        h: d as i32,
    };
    crate::scenes::post_battle::columns::blit_dots(buf, disc_region, &dots);
}
