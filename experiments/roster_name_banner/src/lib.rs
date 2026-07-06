//! Shared letterform data + dot-drawing helpers for the braille dot-font
//! prototypes (`braille_name`, `braille_name_bold`, `braille_name_italic`,
//! `braille_name_sgr_style`). Factored out so the style variants reuse the
//! same base glyph data and drawing/printing code instead of copy-pasting
//! `braille_name.rs` four times.

pub mod braille_font {
    use engine_core::color::Rgba;
    use engine_render::dots::{dots_to_grid, Dot, DotBuffer};
    use engine_render::draw_grid;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    /// Height (in dots) of every letterform in this font, regular or
    /// stylized — bold/italic only ever change width/shape, never height.
    pub const GLYPH_H: usize = 8;

    /// One letter's REGULAR dot pattern, 8 dots tall (7 shape rows + 1 blank
    /// spacer row). Width is PROPORTIONAL per letter (a real font's
    /// variable-width metric, not a fixed monospace cell) — e.g. `L`/`I` are
    /// narrow, most letters are 4, and `M`/`W` get a genuinely wide 7-dot box
    /// with real diagonal strokes (not just a wider blank-padded box — the
    /// first widening pass only widened the box and still read as a blob).
    /// Forcing every letter into the same fixed-width box (the original
    /// version of this prototype) is what made it read as cramped: narrow
    /// letters were padded with wasted blank columns instead of sitting
    /// close to their neighbors. '#' = lit, '·' = unlit.
    fn glyph_dots(c: char) -> [&'static str; GLYPH_H] {
        match c {
            'A' => ["·###·", "#···#", "#···#", "#####", "#···#", "#···#", "#···#", "······"],
            'B' => ["###·", "#··#", "#··#", "###·", "#··#", "#··#", "###·", "····"],
            'C' => ["·###", "#···", "#···", "#···", "#···", "#···", "·###", "····"],
            'D' => ["###·", "#··#", "#··#", "#··#", "#··#", "#··#", "###·", "····"],
            'E' => ["####", "#···", "#···", "###·", "#···", "#···", "####", "····"],
            'F' => ["####", "#···", "#···", "###·", "#···", "#···", "#···", "····"],
            'G' => ["·###", "#···", "#···", "#·##", "#··#", "#··#", "·###", "····"],
            'H' => ["#··#", "#··#", "#··#", "####", "#··#", "#··#", "#··#", "····"],
            'I' => ["###", "·#·", "·#·", "·#·", "·#·", "·#·", "###", "···"],
            'J' => ["··##", "···#", "···#", "···#", "#··#", "#··#", "·##·", "····"],
            'K' => ["#··#", "#·#·", "##··", "##··", "#·#·", "#··#", "#··#", "····"],
            'L' => ["#··", "#··", "#··", "#··", "#··", "#··", "###", "···"],
            // 7-dot-wide, genuine crossing diagonals (not just a wider box):
            // outer verticals full-height, inner diagonals meeting near the
            // top-third — the classic dot-matrix M.
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
            'N' => ["#··#", "##·#", "#·##", "#··#", "#··#", "#··#", "#··#", "····"],
            'O' => ["·##·", "#··#", "#··#", "#··#", "#··#", "#··#", "·##·", "····"],
            'P' => ["###·", "#··#", "#··#", "###·", "#···", "#···", "#···", "····"],
            // Circle is byte-for-byte identical to `O` (rows 0-6), padded 1
            // dot wider for tail clearance. Tail is a single row (row 7, the
            // shared inter-line spacing row) with 2 dots poking out below
            // and to the right of the circle's own footprint. This is the
            // version locked in with the bold font — later attempts to
            // shrink the circle further were tried and reverted; revisit
            // shrinking later rather than re-guessing it here.
            'Q' => ["·##··", "#··#·", "#··#·", "#··#·", "#··#·", "#··#·", "·##··", "···##"],
            'R' => ["###·", "#··#", "#··#", "###·", "#·#·", "#··#", "#··#", "····"],
            'S' => ["·###", "#···", "#···", "·##·", "···#", "···#", "###·", "····"],
            'T' => ["#####", "··#··", "··#··", "··#··", "··#··", "··#··", "··#··", "······"],
            'U' => ["#··#", "#··#", "#··#", "#··#", "#··#", "#··#", "·##·", "····"],
            'V' => ["#···#", "#···#", "#···#", "·#·#·", "·#·#·", "··#··", "··#··", "·····"],
            // Mirror of M (flipped top-to-bottom): outer verticals meeting
            // via inner diagonals near the BOTTOM-third instead of the top —
            // same "genuine diagonal, wide box" treatment as M, not a blob.
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
            // Widened to 7 dots (from the original 5-wide version, where both
            // diagonals converged early and then ran as a static vertical
            // bar for 3 rows — reading as an hourglass/blob, not an X). At
            // 7 wide the diagonals close to a single clean point at the
            // exact middle row, then diverge again, the same "genuine
            // diagonal across a wider box" treatment used for M/W.
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

    /// A per-character lit-dot matrix: `matrix[row][col]`, uniform width
    /// across all rows and across every char a given font style returns
    /// (space included) — `draw_banner` relies on this to lay out gaps.
    pub type GlyphMatrix = Vec<Vec<bool>>;

    fn to_matrix(rows: [&'static str; GLYPH_H]) -> GlyphMatrix {
        rows.iter()
            .map(|row| row.chars().map(|ch| ch == '#').collect())
            .collect()
    }

    /// Regular weight/slant — the base letterform, unmodified.
    pub fn regular_matrix(c: char) -> GlyphMatrix {
        to_matrix(glyph_dots(c))
    }

    /// Bold: smears every lit dot one column to its right (classic
    /// pixel-font bold technique — thickens vertical strokes and heavies up
    /// horizontal bars/corners), widening the glyph box by 1 dot column
    /// (4 -> 5) so the smear has room without wrapping.
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

    /// Per-row rightward shear used by `italic_matrix`/`bold_italic_matrix`:
    /// top rows (index 0) shift further right than bottom rows (index 7),
    /// the classic pixel-font italic technique. Max shift is 2 dots, so
    /// shearing widens whatever matrix it's applied to by 2 dot columns.
    const ITALIC_SHIFT: [usize; GLYPH_H] = [2, 2, 2, 1, 1, 1, 0, 0];

    /// Applies `ITALIC_SHIFT` to an arbitrary already-built `GlyphMatrix` —
    /// shared by `italic_matrix` (shears the regular base) and
    /// `bold_italic_matrix` (shears the already-bolded base), so the italic
    /// transform composes with bold instead of being reimplemented against
    /// `glyph_dots` a second time.
    fn shear(base: GlyphMatrix, shifts: &[usize; GLYPH_H]) -> GlyphMatrix {
        let w = base[0].len();
        let max_shift = shifts.iter().copied().max().unwrap_or(0);
        base.iter()
            .enumerate()
            .map(|(r, row)| {
                let shift = shifts[r];
                let mut out = vec![false; w + max_shift];
                for (col, &lit) in row.iter().enumerate() {
                    if lit {
                        out[col + shift] = true;
                    }
                }
                out
            })
            .collect()
    }

    /// Italic: shifts each dot-row of the REGULAR letterform horizontally by
    /// `ITALIC_SHIFT[row]`, producing a rightward lean toward the top.
    pub fn italic_matrix(c: char) -> GlyphMatrix {
        shear(to_matrix(glyph_dots(c)), &ITALIC_SHIFT)
    }

    /// Bold + italic combined: applies the bold stroke-thickening transform
    /// first, then shears the resulting (already-thickened) matrix — i.e.
    /// composes `bold_matrix` and the italic shear on the same letterform,
    /// rather than italicizing the regular weight and separately thickening
    /// it (order matters: bold-then-shear keeps the thickened strokes
    /// contiguous under the slant instead of shearing thin strokes and then
    /// fattening the already-diagonal result).
    pub fn bold_italic_matrix(c: char) -> GlyphMatrix {
        shear(bold_matrix(c), &ITALIC_SHIFT)
    }

    /// Draws `text` (uppercased) into `buf`, centered in `area`, using
    /// `matrix_of` to turn each character into its lit-dot matrix (regular,
    /// bold, or italic — same drawing/centering code for all three). Builds
    /// one `DotBuffer` sized to `area` in dot units (width*2 x height*4,
    /// same convention `draw_board_lines`/sprite dots use), lights each
    /// letter's dots directly, converts via `dots_to_grid`, and blits with
    /// `draw_grid` — identical technique to the real engine pipeline.
    /// `gap` is the dot-column gap between adjacent letters.
    pub fn draw_banner(
        buf: &mut Buffer,
        area: Rect,
        text: &str,
        color: Rgba,
        gap: usize,
        matrix_of: impl Fn(char) -> GlyphMatrix,
    ) {
        let dot_cols = area.width as usize * 2;
        let dot_rows = area.height as usize * 4;
        if dot_cols == 0 || dot_rows == 0 {
            return;
        }
        let mut dots = DotBuffer::new(dot_cols, dot_rows);

        let upper: Vec<char> = text.chars().map(|c| c.to_ascii_uppercase()).collect();
        let matrices: Vec<GlyphMatrix> = upper.iter().map(|&c| matrix_of(c)).collect();
        let widths: Vec<usize> = matrices
            .iter()
            .map(|m| m.first().map(|r| r.len()).unwrap_or(0))
            .collect();
        let total_w: usize =
            widths.iter().sum::<usize>() + gap.saturating_mul(widths.len().saturating_sub(1));
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
            cursor_x += m.first().map(|r| r.len()).unwrap_or(0) + gap;
        }

        let grid = dots_to_grid(&dots);
        draw_grid(buf, area, &grid);
    }

    /// Prints `buf`'s `w`x`h` region as a plain-text grid framed in `+-+`,
    /// under a `--- {title} ---` header — the shared stdout format every
    /// prototype binary in this crate uses.
    pub fn print_buffer(buf: &Buffer, w: u16, h: u16, title: &str) {
        println!("--- {title} ({w}x{h}) ---");
        println!("+{}+", "-".repeat(w as usize));
        for y in 0..h {
            let row: String = (0..w)
                .map(|x| buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" ").to_string())
                .collect();
            println!("|{row}|");
        }
        println!("+{}+", "-".repeat(w as usize));
        println!();
    }
}
