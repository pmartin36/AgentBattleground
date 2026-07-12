//! Shared close (X) button glyph for modal popups (prompt editor, battle
//! menu). A rounded-rect OUTLINE that hugs a centered red "X" — the same
//! border-hug idiom as the roster Edit button, tinted red and keyed to
//! `ButtonState`. The caller owns the `ButtonCore`, its rect, and hit-testing;
//! this only draws.

use engine_core::color::Rgba;
use engine_render::dots::Dot;
use engine_render::{label, ui_primitives, ButtonState, DotRect, TextAlign};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Style};

/// Red X color by button state — dim idle, bright on hover, dark when pressed
/// (mirrors the luma ordering of `ButtonState::tint_color`).
const IDLE: Rgba = Rgba::rgb(0xc0, 0x30, 0x30);
const HOVER: Rgba = Rgba::rgb(0xff, 0x55, 0x55);
const PRESSED: Rgba = Rgba::rgb(0x80, 0x20, 0x20);

/// Border-hug box height in dots (an overline row, the text row, an underline
/// row — same 6-dot box the Edit button uses).
const BOX_H_DOTS: i32 = 6;

/// Draws the close button into `region` (expected ~3 cells tall so the border
/// rows hug the "X" without sharing its cell): a chamfer-1 rounded-rect
/// outline centered vertically in the region + a centered "X", tinted red per
/// `state`. Zero-area `region` draws nothing and does not panic.
pub(crate) fn draw_close_button(buf: &mut Buffer, region: DotRect, state: ButtonState) {
    let cr = region.to_cell_rect();
    if cr.width == 0 || cr.height == 0 {
        return;
    }
    let color = match state {
        ButtonState::Idle => IDLE,
        ButtonState::Hover => HOVER,
        ButtonState::Pressed => PRESSED,
    };

    // Outline that HUGS the "X": a 6-dot-tall rounded box centered in the
    // region, so its top/bottom edges land on the dot-rows adjacent to the
    // label cell rather than the region's far top/bottom rows.
    let w_dots = cr.width as i32 * 2;
    let y_off = (cr.height as i32 * 4 - BOX_H_DOTS) / 2;
    let dots = ui_primitives::rounded_rect(w_dots as usize, BOX_H_DOTS as usize, 1, 1, color, Dot::Transparent);
    crate::scenes::post_battle::columns::blit_dots(
        buf,
        DotRect { x: cr.x as i32 * 2, y: cr.y as i32 * 4 + y_off, w: w_dots, h: BOX_H_DOTS },
        &dots,
    );

    label(
        buf,
        cr,
        "X",
        TextAlign::Center,
        Style::default().fg(Color::Rgb(color.r, color.g, color.b)),
    );
}
