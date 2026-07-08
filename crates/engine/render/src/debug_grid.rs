//! Debug cell-boundary gridline overlay (bucket b5).
//!
//! [`draw_debug_grid`] is a post-composite pass over an already-rendered
//! ratatui [`Buffer`]: for every cell it lights the cell's left-column +
//! top-row dots (the on-screen cell-boundary grid) and recolors the whole
//! cell toward black (if its underlying color is bright) or toward white
//! (if dark) by [`GRID_CONTRAST_BLEND`], so the grid stays visible against
//! any background.

use ratatui::buffer::Buffer;
use ratatui::style::Color;
use engine_core::color::Rgba;

use crate::dot_diff::decode_braille_cell;
use crate::dots::{luma, DOTS};

/// Fraction blended toward black (bright cells) or white (dark cells) by
/// [`draw_debug_grid`].
pub const GRID_CONTRAST_BLEND: f32 = 0.25;

/// Bit mask of the boundary dots (cell left column + top row, `dx == 0 ||
/// dy == 0`), derived from `dots.rs`'s `DOTS` table rather than hardcoded, so
/// it can't silently drift if `DOTS` ever changes.
const fn compute_boundary_mask() -> u8 {
    let mut mask = 0u8;
    let mut k = 0;
    while k < DOTS.len() {
        let (dx, dy, bit) = DOTS[k];
        if dx == 0 || dy == 0 {
            mask |= 1 << bit;
        }
        k += 1;
    }
    mask
}

const BOUNDARY_MASK: u8 = compute_boundary_mask();

/// Post-composite adaptive-contrast cell-boundary gridline overlay.
///
/// For every braille cell in `buf`, lights the cell's left-column + top-row
/// dots (`dx == 0 || dy == 0`, mask `0x4F`) and recolors the cell toward
/// black if its underlying/assumed color is bright (luma >= 128), or toward
/// white if dark, by [`GRID_CONTRAST_BLEND`]. A blank cell (empty/space
/// symbol) is treated as underlying black and painted the same way, so the
/// grid stays visible over transparent background.
///
/// A non-braille cell that is NOT blank — i.e. it carries real text (a scene
/// label, menu item, or HUD glyph) — is left completely untouched. Per
/// CLAUDE.md's "Braille is universal except text" rule, text must never be
/// converted to a braille glyph; `decode_braille_cell` alone can't tell a
/// blank cell apart from a text cell (both decode to `None`), so this
/// function checks the raw symbol itself to disambiguate before deciding
/// whether to paint.
pub fn draw_debug_grid(buf: &mut Buffer) {
    let area = buf.area;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let is_blank = buf
                .cell((x, y))
                .map(|c| c.symbol().trim().is_empty())
                .unwrap_or(true);

            let (mask, color) = match decode_braille_cell(buf, x, y) {
                Some(mc) => mc,
                None if is_blank => (0, Rgba::rgb(0, 0, 0)),
                None => continue, // non-braille, non-blank: real text — preserve untouched
            };

            let l = luma(color.r, color.g, color.b);
            let target: f32 = if l >= 128 { 0.0 } else { 255.0 };
            let blend =
                |src: u8| (src as f32 + (target - src as f32) * GRID_CONTRAST_BLEND) as u8;
            let out = Rgba::rgb(blend(color.r), blend(color.g), blend(color.b));

            let new_mask = mask | BOUNDARY_MASK;
            let ch = char::from_u32(0x2800 + new_mask as u32).unwrap_or(' ');

            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char(ch);
                cell.set_fg(Color::Rgb(out.r, out.g, out.b));
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dot_diff::decode_braille_cell;
    use engine_core::color::Rgba;
    use ratatui::layout::Rect;
    use ratatui::style::Color;

    fn make_buf(w: u16, h: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, w, h))
    }

    fn seed_braille_cell_at(buf: &mut Buffer, x: u16, y: u16, mask: u8, color: Rgba) {
        let ch = char::from_u32(0x2800 + mask as u32).expect("valid braille codepoint");
        let cell = buf.cell_mut((x, y)).expect("cell must exist");
        cell.set_char(ch);
        cell.set_fg(Color::Rgb(color.r, color.g, color.b));
    }

    /// A fully blank buffer (no braille content anywhere) proves the pass
    /// paints over transparent background: every cell must end up with its
    /// boundary dots lit (mask `0x4F`) and fg lightened from assumed black
    /// toward white by exactly `GRID_CONTRAST_BLEND` (truncating):
    /// `0 + (255 - 0) * 0.25 = 63`.
    #[test]
    fn draw_debug_grid_blank_buffer_paints_and_lightens_every_cell() {
        let mut buf = make_buf(3, 2);
        draw_debug_grid(&mut buf);

        for y in 0..2u16 {
            for x in 0..3u16 {
                let (mask, color) = decode_braille_cell(&buf, x, y)
                    .unwrap_or_else(|| panic!("cell ({x},{y}) must be painted, not blank"));
                assert_eq!(
                    mask & 0x4F,
                    0x4F,
                    "cell ({x},{y}) boundary dots (left column + top row) must all be lit"
                );
                assert_eq!(
                    color,
                    Rgba::rgb(63, 63, 63),
                    "cell ({x},{y}) assumed-black background must lighten toward white by 25%"
                );
            }
        }
    }

    /// A cell already fully lit bright white must stay with boundary dots
    /// lit and its fg darkened toward black by 25% (truncating):
    /// `255 + (0 - 255) * 0.25 = 191`, still `< 255`.
    #[test]
    fn draw_debug_grid_bright_content_darkens_and_keeps_boundary_lit() {
        let mut buf = make_buf(1, 1);
        seed_braille_cell_at(&mut buf, 0, 0, 0xFF, Rgba::rgb(255, 255, 255));

        draw_debug_grid(&mut buf);

        let (mask, color) = decode_braille_cell(&buf, 0, 0).expect("cell must still be lit");
        assert_eq!(mask & 0x4F, 0x4F, "boundary dots must remain lit");
        assert_eq!(
            color,
            Rgba::rgb(191, 191, 191),
            "bright white content must darken toward black by 25%"
        );
    }

    /// Direction guard: a dark (but non-black) underlying color lightens
    /// (every channel increases), while a bright (but non-white) underlying
    /// color darkens (every channel decreases) — proving the blend direction
    /// is chosen per-cell from luma, not a fixed direction.
    #[test]
    fn draw_debug_grid_blend_direction_follows_luma() {
        let mut dark_buf = make_buf(1, 1);
        let dark_src = Rgba::rgb(10, 20, 30); // luma ≈ 18, dark
        seed_braille_cell_at(&mut dark_buf, 0, 0, 0x01, dark_src);
        draw_debug_grid(&mut dark_buf);
        let (_, dark_out) = decode_braille_cell(&dark_buf, 0, 0).unwrap();
        assert!(dark_out.r > dark_src.r, "dark cell red channel must lighten");
        assert!(dark_out.g > dark_src.g, "dark cell green channel must lighten");
        assert!(dark_out.b > dark_src.b, "dark cell blue channel must lighten");

        let mut bright_buf = make_buf(1, 1);
        let bright_src = Rgba::rgb(200, 210, 220); // luma ≈ 208, bright
        seed_braille_cell_at(&mut bright_buf, 0, 0, 0x01, bright_src);
        draw_debug_grid(&mut bright_buf);
        let (_, bright_out) = decode_braille_cell(&bright_buf, 0, 0).unwrap();
        assert!(bright_out.r < bright_src.r, "bright cell red channel must darken");
        assert!(bright_out.g < bright_src.g, "bright cell green channel must darken");
        assert!(bright_out.b < bright_src.b, "bright cell blue channel must darken");
    }

    /// `BOUNDARY_MASK`, derived from `dots.rs::DOTS`, must equal `0x4F` (the
    /// left column + top row of a braille cell) — guards magic-number drift.
    #[test]
    fn boundary_mask_derived_from_dots_equals_0x4f() {
        assert_eq!(super::BOUNDARY_MASK, 0x4F);
    }

    /// Regression: a non-braille cell carrying real text (a scene label,
    /// e.g. "R" from "Roster") must survive the overlay completely
    /// untouched — both its symbol AND its foreground color — never
    /// converted into a braille boundary glyph. `decode_braille_cell` alone
    /// can't distinguish a blank cell from a text cell (both return `None`);
    /// this pins the disambiguation that makes CLAUDE.md's "only text stays
    /// plain terminal characters" rule hold under the overlay too.
    #[test]
    fn draw_debug_grid_preserves_non_blank_text_cells_untouched() {
        let mut buf = make_buf(3, 1);
        {
            let cell = buf.cell_mut((1, 0)).expect("cell must exist");
            cell.set_char('R');
            cell.set_fg(Color::Rgb(200, 200, 200));
        }

        draw_debug_grid(&mut buf);

        let cell = buf.cell((1, 0)).expect("cell must still exist");
        assert_eq!(cell.symbol(), "R", "text glyph must not be converted to braille");
        assert_eq!(
            cell.fg,
            Color::Rgb(200, 200, 200),
            "text cell's foreground color must be untouched by the overlay"
        );

        // Neighboring blank cells must still be painted — the fix must not
        // have accidentally disabled painting altogether.
        let (mask, _) = decode_braille_cell(&buf, 0, 0).expect("blank neighbor must be painted");
        assert_eq!(mask & 0x4F, 0x4F, "blank neighbor's boundary dots must be lit");
    }
}
