//! Shared close (X) button glyph for modal popups (prompt editor, battle
//! menu). A red braille bottom-semicircle (a "bowl" — the lower half of a ring)
//! under a centered white "X", keyed to `ButtonState`. The caller owns the
//! `ButtonCore`, its rect, and hit-testing, and passes a ~square-dot `region`
//! (so the bowl reads round); this only draws.

use engine_core::color::Rgba;
use engine_render::dots::Dot;
use engine_render::{label, ui_primitives, ButtonState, DotRect, TextAlign};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Style};

/// Ring (outline) color by state — red; dim idle, bright hover, dark pressed.
const RING_IDLE: Rgba = Rgba::rgb(0xc0, 0x30, 0x30);
const RING_HOVER: Rgba = Rgba::rgb(0xff, 0x55, 0x55);
const RING_PRESSED: Rgba = Rgba::rgb(0x80, 0x20, 0x20);
/// "X" glyph color by state — white; dim idle, bright hover, gray pressed.
const X_IDLE: Rgba = Rgba::rgb(0xc8, 0xc8, 0xc8);
const X_HOVER: Rgba = Rgba::rgb(0xff, 0xff, 0xff);
const X_PRESSED: Rgba = Rgba::rgb(0x8c, 0x8c, 0x8c);

/// Draws the close button into `region`: a **round** braille ring (diameter =
/// the region's smaller dot dimension, so it stays a circle rather than an
/// ellipse for any region aspect) centered in `region`, with a centered "X"
/// text glyph, tinted red per `state`. Zero-area `region` draws nothing and
/// does not panic.
pub(crate) fn draw_close_button(buf: &mut Buffer, region: DotRect, state: ButtonState) {
    let cr = region.to_cell_rect();
    if cr.width == 0 || cr.height == 0 {
        return;
    }
    let (ring_color, x_color) = match state {
        ButtonState::Idle => (RING_IDLE, X_IDLE),
        ButtonState::Hover => (RING_HOVER, X_HOVER),
        ButtonState::Pressed => (RING_PRESSED, X_PRESSED),
    };

    // Circle diameter = the smaller dot dimension → a square (round) ring box,
    // centered in the region.
    let w_dots = cr.width as i32 * 2;
    let h_dots = cr.height as i32 * 4;
    let d = w_dots.min(h_dots);
    let mut dots = ui_primitives::ring(d as usize, d as usize, 1, ring_color, Dot::Transparent);
    // Remove the top half of the ring — leaving a bottom-half semicircle "bowl"
    // under the (centered) X.
    for row in 0..(d as usize) / 2 {
        for col in 0..d as usize {
            dots.set(col, row, Dot::Transparent);
        }
    }
    crate::scenes::post_battle::columns::blit_dots(
        buf,
        DotRect {
            x: cr.x as i32 * 2 + (w_dots - d) / 2,
            y: cr.y as i32 * 4 + (h_dots - d) / 2,
            w: d,
            h: d,
        },
        &dots,
    );

    label(
        buf,
        cr,
        "X",
        TextAlign::Center,
        Style::default().fg(Color::Rgb(x_color.r, x_color.g, x_color.b)),
    );
}
