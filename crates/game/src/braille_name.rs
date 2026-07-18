//! Braille bold dot-matrix name font. Ported from
//! `experiments/roster_name_banner/src/lib.rs`'s `braille_font` module,
//! bold weight only (per spec 35 Decisions v1 — italic/bold+italic are
//! validated-but-unused, out of scope). Draws an uppercased name string
//! into a `ratatui::buffer::Buffer` rect via the real `engine_render` dot
//! pipeline (`DotBuffer`/`Dot::Lit`/`dots_to_grid`/`draw_grid`), centered,
//! proportional-width, single flat color per call (see b2-t1's fade-toward-
//! background seam: callers pre-lerp `color` rather than this module
//! plumbing per-dot color).

use engine_core::color::Rgba;
use engine_render::dots::{Dot, DotBuffer};
use engine_render::draw_dots;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// Height (in dots) of every letterform in this font.
pub const GLYPH_H: usize = 8;

/// Dot-column gap between adjacent letters in `draw_name`.
pub const NAME_LETTER_GAP: usize = 1;

/// A per-character lit-dot matrix: `matrix[row][col]`, uniform width across
/// all rows for a given character.
pub type GlyphMatrix = Vec<Vec<bool>>;

/// One letter's REGULAR dot pattern, 8 dots tall (7 shape rows + 1 blank
/// spacer row). Width is PROPORTIONAL per letter (a real font's
/// variable-width metric, not a fixed monospace cell). '#' = lit, '·' =
/// unlit. Ported verbatim from `experiments/roster_name_banner/src/lib.rs`'s
/// `braille_font::glyph_dots`.
fn glyph_dots(c: char) -> [&'static str; GLYPH_H] {
    match c {
        'A' => ["·###·", "#···#", "#···#", "#####", "#···#", "#···#", "#···#", "······"],
        'B' => ["###·", "#··#", "#··#", "###·", "#··#", "#··#", "###·", "····"],
        'C' => ["·###", "#···", "#···", "#···", "#···", "#···", "·###", "····"],
        'D' => ["###·", "#··#", "#··#", "#··#", "#··#", "#··#", "###·", "····"],
        'E' => ["####", "#···", "#···", "###·", "#···", "#···", "####", "····"],
        'F' => ["####", "#···", "#···", "###·", "#···", "#···", "#···", "····"],
        'G' => ["·###·", "#···#", "#····", "#··##", "#···#", "#···#", "·###·", "·····"],
        'H' => ["#··#", "#··#", "#··#", "####", "#··#", "#··#", "#··#", "····"],
        'I' => ["###", "·#·", "·#·", "·#·", "·#·", "·#·", "###", "···"],
        'J' => ["··##", "···#", "···#", "···#", "#··#", "#··#", "·##·", "····"],
        'K' => ["#··#", "#·#·", "##··", "##··", "#·#·", "#··#", "#··#", "····"],
        'L' => ["#··", "#··", "#··", "#··", "#··", "#··", "###", "···"],
        'M' => [
            "#·····#",
            "##···##",
            "#·#·#·#",
            "#··#··#",
            "#·····#",
            "#·····#",
            "#·····#",
            "·······",
        ],
        'N' => ["#···#", "##··#", "#·#·#", "#··##", "#···#", "#···#", "#···#", "·····"],
        'O' => ["·##·", "#··#", "#··#", "#··#", "#··#", "#··#", "·##·", "····"],
        'P' => ["###·", "#··#", "#··#", "###·", "#···", "#···", "#···", "····"],
        'Q' => ["·##··", "#··#·", "#··#·", "#··#·", "#··#·", "#··#·", "·##··", "···##"],
        'R' => ["###·", "#··#", "#··#", "###·", "#·#·", "#··#", "#··#", "····"],
        'S' => ["·###", "#···", "#···", "·##·", "···#", "···#", "###·", "····"],
        'T' => ["#####", "··#··", "··#··", "··#··", "··#··", "··#··", "··#··", "······"],
        'U' => ["#··#", "#··#", "#··#", "#··#", "#··#", "#··#", "·##·", "····"],
        'V' => ["#···#", "#···#", "#···#", "·#·#·", "·#·#·", "··#··", "··#··", "·····"],
        'W' => [
            "#·····#",
            "#·····#",
            "#·····#",
            "#··#··#",
            "#·#·#·#",
            "##···##",
            "#·····#",
            "·······",
        ],
        'X' => [
            "#·····#",
            "·#···#·",
            "··#·#··",
            "···#···",
            "··#·#··",
            "·#···#·",
            "#·····#",
            "·······",
        ],
        'Y' => ["#···#", "·#·#·", "··#··", "··#··", "··#··", "··#··", "··#··", "·····"],
        'Z' => ["####", "···#", "··#·", "·#··", "#···", "#···", "####", "····"],
        _ => ["··", "··", "··", "··", "··", "··", "··", "··"], // space — narrow, no dots
    }
}

fn to_matrix(rows: [&'static str; GLYPH_H]) -> GlyphMatrix {
    rows.iter()
        .map(|row| row.chars().map(|ch| ch == '#').collect())
        .collect()
}

/// Bold-weight letterform for `c` (uppercased by callers) — smears every lit
/// dot one column right (classic pixel-font bold technique), widening the
/// glyph box by 1 dot column vs. the regular base.
pub fn bold_matrix(c: char) -> GlyphMatrix {
    let base = to_matrix(glyph_dots(c));
    let w = base[0].len();
    base.iter()
        .map(|row| {
            let mut out = vec![false; w + 1];
            for (col, &lit) in row.iter().enumerate() {
                if lit {
                    out[col] = true;
                    out[col + 1] = true;
                }
            }
            out
        })
        .collect()
}

/// Draws `name` (uppercased) into `buf`, centered in `area`, using the bold
/// braille dot font and the real engine dot pipeline (`DotBuffer` sized
/// `area.width*2 x area.height*4`, `dots_to_grid`, `draw_grid`). Every lit
/// dot is painted in the single flat `color`.
pub fn draw_name(buf: &mut Buffer, area: Rect, name: &str, color: Rgba) {
    let dot_cols = area.width as usize * 2;
    let dot_rows = area.height as usize * 4;
    if dot_cols == 0 || dot_rows == 0 {
        return;
    }
    let mut dots = DotBuffer::new(dot_cols, dot_rows);

    let upper: Vec<char> = name.chars().map(|c| c.to_ascii_uppercase()).collect();
    let matrices: Vec<GlyphMatrix> = upper.iter().map(|&c| bold_matrix(c)).collect();
    let widths: Vec<usize> = matrices
        .iter()
        .map(|m| m.first().map(|r| r.len()).unwrap_or(0))
        .collect();
    let total_w: usize = widths.iter().sum::<usize>()
        + NAME_LETTER_GAP.saturating_mul(widths.len().saturating_sub(1));
    let start_x = dot_cols.saturating_sub(total_w) / 2;
    let start_y = dot_rows.saturating_sub(GLYPH_H) / 2;

    let mut cursor_x = start_x;
    for m in &matrices {
        for (r, row) in m.iter().enumerate() {
            for (col, &lit) in row.iter().enumerate() {
                if lit {
                    dots.set(cursor_x + col, start_y + r, Dot::Lit(color));
                }
            }
        }
        cursor_x += m.first().map(|r| r.len()).unwrap_or(0) + NAME_LETTER_GAP;
    }

    draw_dots(buf, area, &dots);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `draw_name` must paint at least one non-space cell somewhere in the
    /// target rect for a real name — the base "it actually renders
    /// something" contract.
    #[test]
    fn draw_name_paints_non_space_cells() {
        let area = Rect::new(0, 0, 40, 2);
        let mut buf = Buffer::empty(area);
        draw_name(&mut buf, area, "EMBER WOLF", Rgba::rgb(0xff, 0xff, 0xff));

        let painted = (0..area.height)
            .any(|y| (0..area.width).any(|x| buf.cell((x, y)).unwrap().symbol() != " "));
        assert!(
            painted,
            "draw_name must paint at least one non-space cell for a non-empty name"
        );
    }

    /// `M` (a genuinely wide letterform, 7 shape-columns pre-bold) must
    /// occupy a strictly wider bold dot-matrix than `I` (a narrow 3-column
    /// letterform) — proving proportional width, not a fixed monospace box.
    #[test]
    fn bold_matrix_is_proportional_width_not_monospace() {
        let i_width = bold_matrix('I')[0].len();
        let m_width = bold_matrix('M')[0].len();
        assert!(
            m_width > i_width,
            "M ({m_width} dots wide) must be strictly wider than I ({i_width} dots wide) — proportional width, not monospace"
        );
    }

    /// A zero-size target area must not panic (prototype's `draw_banner`
    /// guards `dot_cols == 0 || dot_rows == 0` and returns early).
    #[test]
    fn draw_name_zero_size_area_does_not_panic() {
        let area = Rect::new(0, 0, 0, 0);
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        draw_name(&mut buf, area, "X", Rgba::rgb(0xff, 0xff, 0xff));
    }

    /// b1-t1: the ported prototype N/G bases are 5 dot-columns wide (widened
    /// from the old 4-wide bases), so their bold (smeared) forms must be
    /// strictly 6 dots wide — the widened-N / redrawn-G deliverable's width
    /// contract.
    #[test]
    fn bold_matrix_n_and_g_are_widened_to_six_dots() {
        let n_width = bold_matrix('N')[0].len();
        let g_width = bold_matrix('G')[0].len();
        assert_eq!(n_width, 6, "bold N must be 6 dots wide (widened 5-wide base), got {n_width}");
        assert_eq!(g_width, 6, "bold G must be 6 dots wide (widened 5-wide base), got {g_width}");
    }

    /// b1-t1: pins `bold_matrix('N')`/`('G')` to the exact prototype-locked
    /// bitmaps (experiments/title_logo/src/main.rs `glyph_dots` lines 90 &
    /// 92, smeared by this module's own `bold_matrix`) — guards against
    /// accidental future re-narrowing of N or the G spur re-welding to the
    /// stem.
    #[test]
    fn bold_matrix_n_and_g_match_pinned_prototype_bitmaps() {
        let expected_n: [&str; GLYPH_H] = [
            "##··##", "###·##", "######", "##·###", "##··##", "##··##", "##··##", "······",
        ];
        let expected_g: [&str; GLYPH_H] = [
            "·####·", "##··##", "##····", "##·###", "##··##", "##··##", "·####·", "······",
        ];

        let actual_n = bold_matrix('N');
        let actual_g = bold_matrix('G');

        for (row, expected_row) in expected_n.iter().enumerate() {
            let rendered: String =
                actual_n[row].iter().map(|&lit| if lit { '#' } else { '·' }).collect();
            assert_eq!(&rendered, expected_row, "bold N row {row} mismatch");
        }
        for (row, expected_row) in expected_g.iter().enumerate() {
            let rendered: String =
                actual_g[row].iter().map(|&lit| if lit { '#' } else { '·' }).collect();
            assert_eq!(&rendered, expected_row, "bold G row {row} mismatch");
        }
    }
}
