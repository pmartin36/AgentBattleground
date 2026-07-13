//! Pill capsule rendering for the ability hover tooltip (spec 49): the tinted,
//! over/underlined rounded capsule drawn behind a category label, and its
//! fixed-width metric.
#![allow(dead_code)]

use engine_core::color::Rgba;
use engine_render::dots::Dot;
use engine_render::{label, ui_primitives, DotRect, TextAlign};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Style};

/// Corner radius (dots) for the pill capsule ends — 2 for visibly rounded
/// caps that close onto the over/underline (the fixed chamfer now lights the
/// diagonal connector dot, so radius 2 reads clean). Explicit deviation from
/// the chamfer-1 house default, per the pill design.
pub(super) const PILL_CORNER_RADIUS_DOTS: usize = 2;
/// Pill height — 3 cells: an overline row, the text row, and an underline
/// row. Braille border dots can't share a cell with the terminal-text label,
/// so the dot lines above/below the text live in the adjacent cells.
pub(super) const PILL_HEIGHT_CELLS: u16 = 3;
/// Gap between adjacent pills in the pill row.
pub(super) const INTER_PILL_GAP_CELLS: u16 = 1;
/// Pill capsule border ring thickness (dots).
pub(super) const PILL_BORDER_THICKNESS_DOTS: usize = 1;
/// Pill label text color — a legible fg over the tinted capsule (never
/// `Style::default()`'s unset `Color::Reset`; see `lib.rs::label`'s caveat).
pub(super) const PILL_TEXT_COLOR: Rgba = Rgba::rgb(0xff, 0xff, 0xff);
/// Horizontal padding (cells) each side of a pill label, inside the pill.
/// 2 cells (not 1) so at least one dot column of the tinted border/fill
/// clears `PILL_CORNER_RADIUS_DOTS`'s chamfer and survives outside the
/// centered text glyphs (the leftmost/rightmost pill cell is fully
/// chamfered on every row when the radius is 3 dots — see `pill_width_dots`).
pub(super) const PILL_H_PAD_CELLS: u16 = 2;
/// Minimum pill width (cells) so a shrunk pill never fully chamfers away.
pub(super) const MIN_PILL_WIDTH_CELLS: u16 = 4;

/// A rounded capsule ("pill") hugging its centered `text`: a 6-dot-tall
/// capsule centered in the 3-cell `rect` with solid tinted, rounded END CAPS
/// on the left and right, an OVERLINE (top row) and UNDERLINE (bottom row)
/// across the full width, and a `Transparent` centre so the (white) label
/// stays legible. Built by filling the capsule and clearing the central
/// interior, leaving the caps and top/bottom rows lit. `rect` is dot-precise;
/// the label is placed at the floored cell rect (plain terminal text is
/// cell-quantized). A zero-area `rect` draws nothing and does not panic.
pub(super) fn pill(buf: &mut Buffer, rect: DotRect, text: &str, color: Rgba) {
    let cr = rect.to_cell_rect();
    if cr.width == 0 || cr.height == 0 {
        return;
    }

    let w_dots = cr.width as usize * 2;
    let box_h: usize = 6;
    let mut dots = ui_primitives::rounded_rect(
        w_dots,
        box_h,
        PILL_BORDER_THICKNESS_DOTS,
        PILL_CORNER_RADIUS_DOTS,
        color,
        Dot::Lit(color),
    );
    // Clear the central interior (between the end caps, below the overline and
    // above the underline) so the caps stay solid, the top/bottom rows stay as
    // the over/underline, and the middle is transparent behind the label.
    let cap_w = (PILL_H_PAD_CELLS as usize * 2).min(w_dots / 2);
    let inner_top = PILL_BORDER_THICKNESS_DOTS;
    let inner_bot = box_h.saturating_sub(PILL_BORDER_THICKNESS_DOTS);
    for row in inner_top..inner_bot {
        for col in cap_w..w_dots.saturating_sub(cap_w) {
            dots.set(col, row, Dot::Transparent);
        }
    }
    // Clip the outermost dot column of each cap — its 2-dot flat vertical edge
    // reads angular; dropping it tapers the cap to the chamfered diagonal so it
    // reads rounder.
    if w_dots > 0 {
        for row in 0..box_h {
            dots.set(0, row, Dot::Transparent);
            dots.set(w_dots - 1, row, Dot::Transparent);
        }
    }
    crate::scenes::post_battle::columns::blit_dots(
        buf,
        DotRect { x: cr.x as i32 * 2, y: cr.y as i32 * 4 + 3, w: w_dots as i32, h: box_h as i32 },
        &dots,
    );

    label(
        buf,
        cr,
        text,
        TextAlign::Center,
        Style::default().fg(Color::Rgb(
            PILL_TEXT_COLOR.r,
            PILL_TEXT_COLOR.g,
            PILL_TEXT_COLOR.b,
        )),
    );
}

/// A pill's fixed main-axis width in dots: label chars + padding each side,
/// floored at `MIN_PILL_WIDTH_CELLS` so a shrunk pill never fully chamfers
/// away.
pub(super) fn pill_width_dots(label: &str) -> i32 {
    ((label.chars().count() as u16 + PILL_H_PAD_CELLS * 2).max(MIN_PILL_WIDTH_CELLS) as i32) * 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenes::test_util::lit_dot_color;
    use ratatui::layout::Rect;

    /// The capsule is a tinted OUTLINE: the overline (top edge, dot row 3) and
    /// underline (bottom edge, dot row 8) decode to `color`, and the interior
    /// (dot row 5) is transparent — not a filled chip. Empty `text` so no glyph
    /// overwrites the sampled dots. Pill is 3 cells tall (6-cell × 3-cell rect).
    #[test]
    fn pill_outline_over_underline_lit_interior_transparent() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        let rect = DotRect { x: 0, y: 0, w: 12, h: 12 };
        let color = Rgba::rgb(0x11, 0x22, 0x33);

        pill(&mut buf, rect, "", color);

        assert_eq!(lit_dot_color(&buf, 5, 3), Some(color), "overline (top edge) dot must be lit");
        assert_eq!(lit_dot_color(&buf, 5, 8), Some(color), "underline (bottom edge) dot must be lit");
        assert_eq!(lit_dot_color(&buf, 5, 5), None, "interior must be transparent (outline, not filled)");
    }

    /// The capsule's outermost corner dot is chamfered (transparent/unlit)
    /// while a mid-edge dot on the same overline row is lit — proving rounded,
    /// not square, caps.
    #[test]
    fn pill_chamfered_corner_is_unlit_while_mid_edge_is_lit() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        let rect = DotRect { x: 0, y: 0, w: 12, h: 12 };
        let color = Rgba::rgb(0x11, 0x22, 0x33);

        pill(&mut buf, rect, "", color);

        assert_eq!(
            lit_dot_color(&buf, 0, 3),
            None,
            "outermost corner dot must be chamfered (unlit)"
        );
        assert_eq!(
            lit_dot_color(&buf, 5, 3),
            Some(color),
            "mid-edge dot on the same overline row must be lit"
        );
    }

    /// `text` is drawn centered inside the capsule's cell rect.
    #[test]
    fn pill_label_centered() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        let rect = DotRect { x: 0, y: 0, w: 20, h: 4 };
        let color = Rgba::rgb(0x11, 0x22, 0x33);

        pill(&mut buf, rect, "Hi", color);

        assert_eq!(buf.cell((4, 0)).unwrap().symbol(), "H");
        assert_eq!(buf.cell((5, 0)).unwrap().symbol(), "i");
    }

    /// A zero-area `rect` draws nothing and does not panic.
    #[test]
    fn pill_zero_area_rect_draws_nothing_and_does_not_panic() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        let rect = DotRect { x: 0, y: 0, w: 0, h: 0 };
        let color = Rgba::rgb(0x11, 0x22, 0x33);

        pill(&mut buf, rect, "text", color);

        for x in 0..4 {
            assert_eq!(
                buf.cell((x, 0)).unwrap().symbol(),
                " ",
                "cell ({x},0) must remain blank"
            );
        }
    }
}
