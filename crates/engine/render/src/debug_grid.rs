//! Debug cell-boundary gridline overlay (bucket b5).
//!
//! [`draw_debug_grid`] is a post-composite pass over an already-rendered
//! ratatui [`Buffer`]: every 12 braille dots it lights a reference line — the
//! cell's left-column dots for a vertical line, the cell's top-row dots for a
//! horizontal line — and recolors that cell to full black (if its underlying
//! color is bright) or full white (if dark), so the line is unambiguously
//! visible against any background. Cells not on a line are left completely
//! untouched.

use ratatui::buffer::Buffer;
use ratatui::style::Color;
use engine_core::color::Rgba;

use crate::dot_diff::decode_braille_cell;
use crate::dots::{luma, DOTS};

/// Reference lines are spaced every 12 braille dots (the sub-cell 2-wide ×
/// 4-tall dot grid), not every N terminal cells — dots are roughly square on
/// screen, so a dot-spaced grid reads as square, unlike a cell-spaced one. A
/// terminal cell is 2 dots wide by 4 dots tall, so 12 dots is
/// [`GRID_SPACING_COLS`] (6) whole cell-columns horizontally and
/// [`GRID_SPACING_ROWS`] (3) whole cell-rows vertically.
///
/// Spacing, in whole terminal cell-columns, between vertical reference lines
/// drawn by [`draw_debug_grid`]. A vertical line is drawn wherever `cell_col
/// % GRID_SPACING_COLS == 0` — everything in between is left untouched, so
/// the overlay stays a coarse alignment reference instead of marking every
/// single cell.
pub const GRID_SPACING_COLS: u16 = 6;

/// Spacing, in whole terminal cell-rows, between horizontal reference lines
/// drawn by [`draw_debug_grid`]. A horizontal line is drawn wherever
/// `cell_row % GRID_SPACING_ROWS == 0` — everything in between is left
/// untouched, so the overlay stays a coarse alignment reference instead of
/// marking every single cell.
pub const GRID_SPACING_ROWS: u16 = 3;

/// Bit mask of the cell's left-column dots (`dx == 0`), derived from
/// `dots.rs`'s `DOTS` table rather than hardcoded, so it can't silently drift
/// if `DOTS` ever changes. Lit for cells on a vertical reference line.
const fn compute_left_col_mask() -> u8 {
    let mut mask = 0u8;
    let mut k = 0;
    while k < DOTS.len() {
        let (dx, _dy, bit) = DOTS[k];
        if dx == 0 {
            mask |= 1 << bit;
        }
        k += 1;
    }
    mask
}

/// Bit mask of the cell's top-row dots (`dy == 0`), derived from `dots.rs`'s
/// `DOTS` table. Lit for cells on a horizontal reference line.
const fn compute_top_row_mask() -> u8 {
    let mut mask = 0u8;
    let mut k = 0;
    while k < DOTS.len() {
        let (_dx, dy, bit) = DOTS[k];
        if dy == 0 {
            mask |= 1 << bit;
        }
        k += 1;
    }
    mask
}

const LEFT_COL_MASK: u8 = compute_left_col_mask();
const TOP_ROW_MASK: u8 = compute_top_row_mask();

/// Bit mask of the boundary dots (cell left column + top row, `dx == 0 ||
/// dy == 0`) — the mask applied to a cell that sits on both a vertical and a
/// horizontal reference line.
const BOUNDARY_MASK: u8 = LEFT_COL_MASK | TOP_ROW_MASK;

/// Post-composite sparse, high-contrast cell-boundary gridline overlay.
///
/// A cell is only touched when it sits on a reference line: `cell_col %
/// GRID_SPACING_COLS == 0` lights that cell's left-column dots (`dx == 0`,
/// mask `0x47`); `cell_row % GRID_SPACING_ROWS == 0` lights its top-row dots
/// (`dy == 0`, mask `0x09`); a cell on both gets the full `0x4F`. Every other
/// cell is left completely untouched.
///
/// A touched cell is recolored to full black if its underlying/assumed color
/// is bright (luma >= 128), or full white if dark — not a partial blend — so
/// the line is unambiguously visible against any background. A blank cell
/// (empty/space symbol) is treated as underlying black and painted the same
/// way, so the grid stays visible over transparent background.
///
/// A non-braille cell that is NOT blank — i.e. it carries real text (a scene
/// label, menu item, or HUD glyph) — is left completely untouched even when
/// it sits on a reference line. Per CLAUDE.md's "Braille is universal except
/// text" rule, text must never be converted to a braille glyph;
/// `decode_braille_cell` alone can't tell a blank cell apart from a text cell
/// (both decode to `None`), so this function checks the raw symbol itself to
/// disambiguate before deciding whether to paint.
pub fn draw_debug_grid(buf: &mut Buffer) {
    let area = buf.area;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let vertical = x % GRID_SPACING_COLS == 0;
            let horizontal = y % GRID_SPACING_ROWS == 0;
            let line_mask = match (vertical, horizontal) {
                (true, true) => BOUNDARY_MASK,
                (true, false) => LEFT_COL_MASK,
                (false, true) => TOP_ROW_MASK,
                (false, false) => continue, // not on a reference line: leave untouched
            };

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
            let out = if l >= 128 {
                Rgba::rgb(0, 0, 0)
            } else {
                Rgba::rgb(255, 255, 255)
            };

            let new_mask = mask | line_mask;
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

    /// A blank on-line cell (no braille content, assumed underlying black)
    /// must be painted full white — not a partial blend — proving the new
    /// full-contrast (not `GRID_CONTRAST_BLEND`-style) behavior.
    #[test]
    fn draw_debug_grid_blank_on_line_cell_paints_full_white() {
        let mut buf = make_buf(1, 1); // (0,0) is always on both lines (0 % GRID_SPACING_COLS == 0, 0 % GRID_SPACING_ROWS == 0)
        draw_debug_grid(&mut buf);

        let (mask, color) = decode_braille_cell(&buf, 0, 0)
            .expect("on-line cell must be painted, not blank");
        assert_eq!(
            mask & 0x4F,
            0x4F,
            "cell at the intersection of a vertical and horizontal line must have full boundary dots lit"
        );
        assert_eq!(
            color,
            Rgba::rgb(255, 255, 255),
            "assumed-black background must paint full white, not a partial blend"
        );
    }

    /// A cell already fully lit bright white, sitting on a line, must stay
    /// with boundary dots lit and its fg painted full black — not a partial
    /// blend toward black.
    #[test]
    fn draw_debug_grid_bright_content_on_line_darkens_to_full_black() {
        let mut buf = make_buf(1, 1); // (0,0) is always on both lines
        seed_braille_cell_at(&mut buf, 0, 0, 0xFF, Rgba::rgb(255, 255, 255));

        draw_debug_grid(&mut buf);

        let (mask, color) = decode_braille_cell(&buf, 0, 0).expect("cell must still be lit");
        assert_eq!(mask & 0x4F, 0x4F, "boundary dots must remain lit");
        assert_eq!(
            color,
            Rgba::rgb(0, 0, 0),
            "bright white content must darken to full black, not a partial blend"
        );
    }

    /// Direction/strength guard: a dark (but non-black) underlying color on a
    /// line paints full white, while a bright (but non-white) underlying
    /// color on a line paints full black — proving the target is chosen
    /// per-cell from luma, and that it swings all the way, not partially.
    #[test]
    fn draw_debug_grid_on_line_target_follows_luma_at_full_strength() {
        let mut dark_buf = make_buf(1, 1); // (0,0) always on-line
        seed_braille_cell_at(&mut dark_buf, 0, 0, 0x01, Rgba::rgb(10, 20, 30)); // luma ≈ 18, dark
        draw_debug_grid(&mut dark_buf);
        let (_, dark_out) = decode_braille_cell(&dark_buf, 0, 0).unwrap();
        assert_eq!(dark_out, Rgba::rgb(255, 255, 255), "dark underlying cell must paint full white");

        let mut bright_buf = make_buf(1, 1); // (0,0) always on-line
        seed_braille_cell_at(&mut bright_buf, 0, 0, 0x01, Rgba::rgb(200, 210, 220)); // luma ≈ 208, bright
        draw_debug_grid(&mut bright_buf);
        let (_, bright_out) = decode_braille_cell(&bright_buf, 0, 0).unwrap();
        assert_eq!(bright_out, Rgba::rgb(0, 0, 0), "bright underlying cell must paint full black");
    }

    /// `BOUNDARY_MASK`, derived from `dots.rs::DOTS`, must equal `0x4F` (the
    /// left column + top row of a braille cell) — guards magic-number drift.
    /// `LEFT_COL_MASK`/`TOP_ROW_MASK` (its vertical-only/horizontal-only
    /// halves) must equal `0x47`/`0x09` respectively.
    #[test]
    fn boundary_and_line_masks_derived_from_dots_have_expected_values() {
        assert_eq!(super::LEFT_COL_MASK, 0x47, "left-column (dx==0) mask");
        assert_eq!(super::TOP_ROW_MASK, 0x09, "top-row (dy==0) mask");
        assert_eq!(super::BOUNDARY_MASK, 0x4F, "left column | top row");
    }

    /// Reference lines must be spaced every 12 braille dots, not every N
    /// cells: with a 2-dot-wide, 4-dot-tall cell, that's 6 cell-columns
    /// horizontally and 3 cell-rows vertically.
    #[test]
    fn grid_spacing_is_12_dots_ie_6_cols_by_3_rows() {
        assert_eq!(GRID_SPACING_COLS, 6, "12 dots / 2 dots-per-cell-width");
        assert_eq!(GRID_SPACING_ROWS, 3, "12 dots / 4 dots-per-cell-height");
    }

    /// A cell that is NOT on any reference line (neither its column is a
    /// multiple of `GRID_SPACING_COLS` nor its row a multiple of
    /// `GRID_SPACING_ROWS`) must be left completely untouched, whether it
    /// started blank or carrying braille content — this is the core of the
    /// "sparse reference lines, not every cell" fix.
    #[test]
    fn draw_debug_grid_off_line_cells_are_untouched() {
        let mut buf = make_buf(20, 20);
        // Seed some off-line content that must survive unchanged.
        seed_braille_cell_at(&mut buf, 5, 5, 0x2A, Rgba::rgb(123, 45, 67));

        draw_debug_grid(&mut buf);

        // Off-line blank cell (4 % GRID_SPACING_COLS == 4, 4 % GRID_SPACING_ROWS == 1):
        // must remain blank (never converted to braille).
        assert!(
            decode_braille_cell(&buf, 4, 4).is_none(),
            "off-line blank cell (4,4) must not be painted"
        );
        let cell = buf.cell((4, 4)).unwrap();
        assert!(cell.symbol().trim().is_empty(), "off-line blank cell (4,4) must stay blank");

        // Off-line cell that had braille content: must be byte-for-byte unchanged.
        let (mask, color) = decode_braille_cell(&buf, 5, 5).expect("seeded cell must still be braille");
        assert_eq!(mask, 0x2A, "off-line cell's mask must be untouched");
        assert_eq!(color, Rgba::rgb(123, 45, 67), "off-line cell's color must be untouched");
    }

    /// A cell on a vertical-only line (`x % GRID_SPACING_COLS == 0`, `y %
    /// GRID_SPACING_ROWS != 0`) must get only its left-column dots lit
    /// (`LEFT_COL_MASK`), not the full boundary mask — proving vertical and
    /// horizontal lines are marked independently, not always together.
    #[test]
    fn draw_debug_grid_vertical_only_line_lights_left_column_only() {
        let mut buf = make_buf(20, 20);
        draw_debug_grid(&mut buf);

        // x=6 is on the vertical line (6 % GRID_SPACING_COLS == 0); y=5 is
        // NOT on a horizontal line (5 % GRID_SPACING_ROWS == 2).
        let (mask, color) = decode_braille_cell(&buf, GRID_SPACING_COLS, 5)
            .expect("cell (6,5) is on a vertical line and must be painted");
        assert_eq!(mask, super::LEFT_COL_MASK, "vertical-only line must light exactly the left column");
        assert_eq!(color, Rgba::rgb(255, 255, 255), "assumed-black background paints full white");
    }

    /// A cell on a horizontal-only line (`y % GRID_SPACING_ROWS == 0`, `x %
    /// GRID_SPACING_COLS != 0`) must get only its top-row dots lit
    /// (`TOP_ROW_MASK`).
    #[test]
    fn draw_debug_grid_horizontal_only_line_lights_top_row_only() {
        let mut buf = make_buf(20, 20);
        draw_debug_grid(&mut buf);

        // y=3 is on the horizontal line (3 % GRID_SPACING_ROWS == 0); x=5 is
        // NOT on a vertical line (5 % GRID_SPACING_COLS == 5).
        let (mask, color) = decode_braille_cell(&buf, 5, GRID_SPACING_ROWS)
            .expect("cell (5,3) is on a horizontal line and must be painted");
        assert_eq!(mask, super::TOP_ROW_MASK, "horizontal-only line must light exactly the top row");
        assert_eq!(color, Rgba::rgb(255, 255, 255), "assumed-black background paints full white");
    }

    /// A cell at the intersection of a vertical AND horizontal line (`x %
    /// GRID_SPACING_COLS == 0 && y % GRID_SPACING_ROWS == 0`) must get the
    /// full boundary mask.
    #[test]
    fn draw_debug_grid_intersection_lights_full_boundary_mask() {
        let mut buf = make_buf(20, 20);
        draw_debug_grid(&mut buf);

        let (mask, _) = decode_braille_cell(&buf, GRID_SPACING_COLS, GRID_SPACING_ROWS)
            .expect("cell (6,3) is a line intersection and must be painted");
        assert_eq!(mask, super::BOUNDARY_MASK, "intersection must light the full boundary mask");
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
