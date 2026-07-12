//! Shared close (X) button glyph for modal popups (prompt editor, battle
//! menu). A white "X" in the top-centre cell cradled by a red braille
//! semicircle arc underneath it, keyed to `ButtonState`. The caller owns the
//! `ButtonCore`, its rect, and hit-testing; this only draws.
//!
//! Layout in a 3×2-cell region (6×8 dots):
//! ```text
//!   [arc][ X ][arc]
//!   [arc][arc][arc]
//! ```
//! the arc is the bottom half of an ellipse — its upper ends occupy the
//! bottom dot-rows of the top side cells, so the "X" sits in the opening.

use engine_core::color::Rgba;
use engine_render::dots::{Dot, DotBuffer};
use engine_render::{label, ButtonState, DotRect, TextAlign};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

/// Arc color by state — red; dim idle, bright hover, dark pressed.
const ARC_IDLE: Rgba = Rgba::rgb(0xc0, 0x30, 0x30);
const ARC_HOVER: Rgba = Rgba::rgb(0xff, 0x55, 0x55);
const ARC_PRESSED: Rgba = Rgba::rgb(0x80, 0x20, 0x20);
/// "X" glyph color by state — white; dim idle, bright hover, gray pressed.
const X_IDLE: Rgba = Rgba::rgb(0xc8, 0xc8, 0xc8);
const X_HOVER: Rgba = Rgba::rgb(0xff, 0xff, 0xff);
const X_PRESSED: Rgba = Rgba::rgb(0x8c, 0x8c, 0x8c);

/// Row (in dots) below which the arc is drawn; the top rows are left clear for
/// the "X" text cell.
const ARC_TOP_DOT: f32 = 2.0;

/// Draws the close button into `region` (expected 3×2 cells): a red semicircle
/// arc in the lower dots with a white centered "X" in the top cell-row. Zero-
/// area `region` draws nothing and does not panic.
pub(crate) fn draw_close_button(buf: &mut Buffer, region: DotRect, state: ButtonState) {
    let cr = region.to_cell_rect();
    if cr.width == 0 || cr.height == 0 {
        return;
    }
    let (arc_color, x_color) = match state {
        ButtonState::Idle => (ARC_IDLE, X_IDLE),
        ButtonState::Hover => (ARC_HOVER, X_HOVER),
        ButtonState::Pressed => (ARC_PRESSED, X_PRESSED),
    };

    let w_dots = cr.width as usize * 2;
    let h_dots = cr.height as usize * 4;
    let arc = semicircle_dots(w_dots, h_dots, arc_color);
    crate::scenes::post_battle::columns::blit_dots(
        buf,
        DotRect { x: cr.x as i32 * 2, y: cr.y as i32 * 4, w: w_dots as i32, h: h_dots as i32 },
        &arc,
    );

    // "X" in the TOP cell-row only (centered horizontally), so it sits in the
    // arc's opening rather than the vertical middle of the whole region.
    label(
        buf,
        Rect::new(cr.x, cr.y, cr.width, 1),
        "X",
        TextAlign::Center,
        Style::default().fg(Color::Rgb(x_color.r, x_color.g, x_color.b)),
    );
}

/// The bottom half of an ellipse ring rasterized into a `w×h` [`DotBuffer`]:
/// lit where a dot at/below [`ARC_TOP_DOT`] lands within the ring band of an
/// ellipse spanning the box width and reaching its bottom. The top rows (where
/// the "X" sits) and the outside are left `Transparent`.
fn semicircle_dots(w: usize, h: usize, color: Rgba) -> DotBuffer {
    let mut buf = DotBuffer::new(w, h);
    if w == 0 || h == 0 {
        return buf;
    }
    let cx = (w as f32 - 1.0) / 2.0;
    let cy = ARC_TOP_DOT;
    let rx = w as f32 / 2.0;
    let ry = (h as f32) - cy; // reach the bottom edge
    for row in 0..h {
        if (row as f32) < cy {
            continue; // top rows stay clear for the "X"
        }
        for col in 0..w {
            let dx = (col as f32 - cx) / rx;
            let dy = (row as f32 - cy) / ry;
            let d = (dx * dx + dy * dy).sqrt();
            if d > 0.6 && d <= 1.0 {
                buf.set(col, row, Dot::Lit(color));
            }
        }
    }
    buf
}
