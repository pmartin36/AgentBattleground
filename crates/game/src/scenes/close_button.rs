//! Shared close (X) button glyph for modal popups (prompt editor, battle
//! menu). A braille ring around a centered white "X", keyed to `ButtonState`.
//! The caller owns the `ButtonCore`, its rect, and hit-testing, and passes a
//! ~square-dot `region` (so the ring reads round); this only draws.

use engine_core::color::Rgba;
use engine_render::dots::Dot;
use engine_render::{label, ui_primitives, ButtonState, DotRect, TextAlign};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Style};

/// White X color by button state — dim idle, bright on hover, gray when pressed
/// (the standard `ButtonState::tint_color` luma ordering).
const IDLE: Rgba = Rgba::rgb(0xc8, 0xc8, 0xc8);
const HOVER: Rgba = Rgba::rgb(0xff, 0xff, 0xff);
const PRESSED: Rgba = Rgba::rgb(0x8c, 0x8c, 0x8c);

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
    let color = match state {
        ButtonState::Idle => IDLE,
        ButtonState::Hover => HOVER,
        ButtonState::Pressed => PRESSED,
    };

    // Circle diameter = the smaller dot dimension → a square (round) ring box,
    // centered in the region.
    let w_dots = cr.width as i32 * 2;
    let h_dots = cr.height as i32 * 4;
    let d = w_dots.min(h_dots);
    let dots = ui_primitives::ring(d as usize, d as usize, 1, color, Dot::Transparent);
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
        Style::default().fg(Color::Rgb(color.r, color.g, color.b)),
    );
}
