use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use engine_core::color::Rgba;

/// One braille cell: transparent (background shows through) or a lit glyph.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Cell {
    #[default]
    Transparent,
    Glyph { ch: char, color: Rgba },
}

/// A cols×rows grid of braille cells, row-major.
/// `cells[row * cols + col]` addresses cell (col, row).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Grid {
    cols: usize,
    rows: usize,
    cells: Vec<Cell>,
}

impl Grid {
    /// All-transparent grid of the given size.
    pub fn new(cols: usize, rows: usize) -> Self {
        Grid {
            cols,
            rows,
            cells: vec![Cell::Transparent; cols * rows],
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Returns the cell at (col, row). Panics if out of bounds.
    pub fn get(&self, col: usize, row: usize) -> Cell {
        assert!(col < self.cols, "col {col} out of bounds (cols={})", self.cols);
        assert!(row < self.rows, "row {row} out of bounds (rows={})", self.rows);
        self.cells[row * self.cols + col]
    }

    /// Sets the cell at (col, row). Panics if out of bounds.
    ///
    /// Deliberately asymmetric with `dots::DotBuffer::set` (which silently
    /// clips OOB writes): `Grid::set`'s only writer, `dots_to_grid`, maps dot
    /// blocks to cells 1:1 with matching dimensions, so an OOB write here is
    /// always a real bug, never an expected clip — unlike `DotBuffer`, which
    /// is written through arbitrary-offset placements (`composite_dots`)
    /// where partial off-buffer overflow is the common, expected case.
    pub fn set(&mut self, col: usize, row: usize, cell: Cell) {
        assert!(col < self.cols, "col {col} out of bounds (cols={})", self.cols);
        assert!(row < self.rows, "row {row} out of bounds (rows={})", self.rows);
        self.cells[row * self.cols + col] = cell;
    }
}

/// Blit `grid` into `buf`, centered within `area`.
///
/// - Transparent cells leave the buffer cell's prior content unchanged.
/// - Glyph cells write their `ch` as symbol. Fully opaque (`color.a == 0xFF`)
///   glyphs overwrite fg with `Color::Rgb(r,g,b)` byte-identically; translucent
///   glyphs (`color.a < 0xFF`) alpha-composite `color` over the destination
///   cell's current fg (`Color::Rgb(r,g,b)` read as opaque, any other variant
///   incl. `Color::Reset` treated as opaque black) via `Rgba::over`.
/// - Cells that fall outside `area ∩ buf.area` are silently clipped.
pub fn draw_grid(buf: &mut Buffer, area: Rect, grid: &Grid) {
    let base_x = area.left() + area.width.saturating_sub(grid.cols() as u16) / 2;
    let base_y = area.top() + area.height.saturating_sub(grid.rows() as u16) / 2;
    let clip = area.intersection(buf.area);

    for row in 0..grid.rows() {
        for col in 0..grid.cols() {
            if let Cell::Glyph { ch, color } = grid.get(col, row) {
                let x = base_x.saturating_add(col as u16);
                let y = base_y.saturating_add(row as u16);
                if clip.left() <= x && x < clip.right() && clip.top() <= y && y < clip.bottom() {
                    if let Some(cell) = buf.cell_mut((x, y)) {
                        cell.set_char(ch);
                        if color.a == 0xFF {
                            cell.set_fg(Color::Rgb(color.r, color.g, color.b));
                        } else {
                            let dest = match cell.fg {
                                Color::Rgb(r, g, b) => Rgba::new(r, g, b, 0xFF),
                                _ => Rgba::new(0, 0, 0, 0xFF),
                            };
                            let blended = color.over(dest);
                            cell.set_fg(Color::Rgb(blended.r, blended.g, blended.b));
                        }
                    }
                }
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
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Color;
    use engine_core::color::Rgba;

    fn make_buf(w: u16, h: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, w, h))
    }

    // ── Grid data-structure invariants ────────────────────────────────────────

    /// Grid::new produces a grid with the correct dimensions and every cell
    /// starts as Transparent.
    #[test]
    fn grid_new_all_transparent_with_correct_dims() {
        let g = Grid::new(3, 2);
        assert_eq!(g.cols(), 3, "cols must match constructor arg");
        assert_eq!(g.rows(), 2, "rows must match constructor arg");
        for row in 0..2 {
            for col in 0..3 {
                assert_eq!(
                    g.get(col, row),
                    Cell::Transparent,
                    "cell ({col},{row}) must start Transparent"
                );
            }
        }
    }

    /// set then get round-trips a Glyph cell; all other cells stay Transparent.
    #[test]
    fn grid_get_set_round_trip() {
        let mut g = Grid::new(3, 2);
        let glyph = Cell::Glyph {
            ch: '⣿',
            color: Rgba::rgb(0xAB, 0xCD, 0xEF),
        };
        g.set(1, 0, glyph);
        assert_eq!(g.get(1, 0), glyph, "set then get must return the same Cell");
        // Neighbours left untouched.
        assert_eq!(g.get(0, 0), Cell::Transparent);
        assert_eq!(g.get(2, 0), Cell::Transparent);
        assert_eq!(g.get(0, 1), Cell::Transparent);
    }

    // ── draw_grid correctness ─────────────────────────────────────────────────

    /// A 2×1 grid (one Glyph, one Transparent) in a 6×5 buffer:
    ///   centering: base_x = (6-2)/2 = 2, base_y = (5-1)/2 = 2
    /// The Glyph lands at (2,2) with the correct symbol and fg.
    /// The Transparent cell's slot (3,2) keeps the sentinel pre-seeded there.
    #[test]
    fn draw_grid_blits_glyph_and_skips_transparent() {
        const SENTINEL: &str = "X";
        let color = Rgba::rgb(0xFF, 0x80, 0x00);
        let mut grid = Grid::new(2, 1);
        grid.set(0, 0, Cell::Glyph { ch: '⣿', color });
        // (1,0) stays Transparent.

        let mut buf = make_buf(6, 5);
        // Pre-seed the slot where the Transparent cell would land.
        if let Some(c) = buf.cell_mut((3u16, 2u16)) {
            c.set_char('X');
        }

        let area = Rect::new(0, 0, 6, 5);
        draw_grid(&mut buf, area, &grid);

        // Glyph cell — must be written at (2,2).
        let glyph_cell = buf.cell((2u16, 2u16)).expect("cell (2,2) must exist");
        assert_eq!(
            glyph_cell.symbol(),
            "⣿",
            "glyph cell must carry the braille character"
        );
        assert_eq!(
            glyph_cell.fg,
            Color::Rgb(0xFF, 0x80, 0x00),
            "glyph cell fg must match color"
        );

        // Transparent cell — buffer slot must be unchanged (sentinel remains).
        let transp_cell = buf.cell((3u16, 2u16)).expect("cell (3,2) must exist");
        assert_eq!(
            transp_cell.symbol(),
            SENTINEL,
            "Transparent cell must leave prior buffer content untouched"
        );
    }

    /// A 1×1 grid in a 5×5 area: the single Glyph must land exactly at (2,2)
    /// (center: base_x = (5-1)/2 = 2, base_y = (5-1)/2 = 2).
    #[test]
    fn draw_grid_centering_single_cell_exact_position() {
        let color = Rgba::rgb(0x10, 0x20, 0x30);
        let mut grid = Grid::new(1, 1);
        grid.set(0, 0, Cell::Glyph { ch: '⠿', color });

        let mut buf = make_buf(5, 5);
        draw_grid(&mut buf, Rect::new(0, 0, 5, 5), &grid);

        let cell = buf.cell((2u16, 2u16)).expect("cell (2,2) must exist");
        assert_eq!(cell.symbol(), "⠿", "single glyph must land at center (2,2)");
        assert_eq!(cell.fg, Color::Rgb(0x10, 0x20, 0x30));

        // Ensure no other cell was written.
        for y in 0..5u16 {
            for x in 0..5u16 {
                if x == 2 && y == 2 {
                    continue;
                }
                let other = buf.cell((x, y)).unwrap();
                assert_ne!(
                    other.symbol(),
                    "⠿",
                    "only center cell (2,2) should be written; found glyph at ({x},{y})"
                );
            }
        }
    }

    // ── draw_grid alpha-blend (b1-t3) ─────────────────────────────────────────

    /// A translucent Glyph (a=0x80) blitted over a cell whose fg is already
    /// seeded black must alpha-composite via `Rgba::over`, not overwrite:
    /// white(a=0x80) over black(a=0xFF) -> mid grey (lerp(0,255,128/255)=128).
    #[test]
    fn draw_grid_translucent_blends_over_seeded_fg() {
        let color = Rgba::new(0xFF, 0xFF, 0xFF, 0x80);
        let mut grid = Grid::new(1, 1);
        grid.set(0, 0, Cell::Glyph { ch: '⣿', color });

        let mut buf = make_buf(1, 1);
        if let Some(c) = buf.cell_mut((0u16, 0u16)) {
            c.set_fg(Color::Rgb(0x00, 0x00, 0x00));
        }

        draw_grid(&mut buf, Rect::new(0, 0, 1, 1), &grid);

        let cell = buf.cell((0u16, 0u16)).expect("cell (0,0) must exist");
        assert_eq!(cell.symbol(), "⣿", "glyph char is still written");
        assert_eq!(
            cell.fg,
            Color::Rgb(0x80, 0x80, 0x80),
            "translucent color must blend over the seeded dest fg, not overwrite it"
        );
    }

    /// A translucent Glyph blitted over a fresh (Color::Reset) cell must
    /// blend as if over opaque black — the fallback the battle-viewer's
    /// grid lines actually hit (drawn into a fresh buffer).
    #[test]
    fn draw_grid_translucent_blends_over_reset_fallback_as_black() {
        let color = Rgba::new(0xFF, 0xFF, 0xFF, 0x60);
        let mut grid = Grid::new(1, 1);
        grid.set(0, 0, Cell::Glyph { ch: '⣿', color });

        let mut buf = make_buf(1, 1);
        // Fresh buffer cell fg defaults to Color::Reset — not pre-seeded.

        draw_grid(&mut buf, Rect::new(0, 0, 1, 1), &grid);

        let cell = buf.cell((0u16, 0u16)).expect("cell (0,0) must exist");
        assert_eq!(
            cell.fg,
            Color::Rgb(0x60, 0x60, 0x60),
            "Color::Reset dest must be treated as opaque black for the blend"
        );
    }

    /// An opaque Glyph (a=0xFF) must still be a byte-identical overwrite
    /// regardless of what fg already occupies the destination cell.
    #[test]
    fn draw_grid_opaque_still_overwrites_regardless_of_dest() {
        let color = Rgba::rgb(0xAA, 0xBB, 0xCC);
        let mut grid = Grid::new(1, 1);
        grid.set(0, 0, Cell::Glyph { ch: '⣿', color });

        let mut buf = make_buf(1, 1);
        if let Some(c) = buf.cell_mut((0u16, 0u16)) {
            c.set_fg(Color::Rgb(0x11, 0x22, 0x33));
        }

        draw_grid(&mut buf, Rect::new(0, 0, 1, 1), &grid);

        let cell = buf.cell((0u16, 0u16)).expect("cell (0,0) must exist");
        assert_eq!(
            cell.fg,
            Color::Rgb(0xAA, 0xBB, 0xCC),
            "a==0xFF must overwrite the dest fg byte-identically"
        );
    }

    /// A grid larger than the buffer must not panic; out-of-bounds cells are
    /// silently clipped. Mirrors fill_oversized_area_does_not_panic.
    #[test]
    fn draw_grid_oversized_does_not_panic() {
        let color = Rgba::rgb(0xFF, 0x00, 0x00);
        let mut grid = Grid::new(100, 100);
        grid.set(0, 0, Cell::Glyph { ch: '⣿', color });

        let mut buf = make_buf(3, 3);
        // Must not panic; any in-bounds cells may or may not be written —
        // the only requirement is no panic.
        draw_grid(&mut buf, Rect::new(0, 0, 3, 3), &grid);
    }

    // ── Grid::set OOB contract (regression guard) ─────────────────────────────

    /// `Grid::set` on an out-of-bounds `(col, row)` must panic — not silently
    /// clip. This is intentionally asymmetric with `dots::DotBuffer::set` (see
    /// that type's equivalent test): `Grid::set`'s only writer, `dots_to_grid`,
    /// never legitimately goes out of bounds, so a panic here is a real-bug
    /// safety net, not a load-bearing behavior anyone should "fix" to match
    /// `DotBuffer`'s silent clip — see the doc comment on `Grid::set` for the
    /// full reasoning.
    #[test]
    #[should_panic]
    fn grid_set_out_of_bounds_panics_dots_to_grid_never_needs_a_silent_clip() {
        let mut g = Grid::new(2, 2);
        g.set(5, 0, Cell::Glyph { ch: '⣿', color: Rgba::rgb(1, 2, 3) });
    }
}
