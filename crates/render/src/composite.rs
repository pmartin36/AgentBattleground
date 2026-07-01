//! Depth-sorted compositor: blits multiple positioned `Grid`s into one.

use crate::grid::{Cell, Grid};

/// A source grid positioned at `(col, row)` in the destination, with a
/// depth-sort key.  Negative offsets are allowed (cells that fall outside the
/// destination are clipped).
#[derive(Clone, Copy, Debug)]
pub struct Placement<'a> {
    pub grid: &'a Grid,
    pub col: i32,
    pub row: i32,
    /// Back-to-front sort key (larger = nearer = drawn on top). This is an
    /// opaque scalar supplied by the caller's camera via `depth_key(position)`
    /// (see specs/13-rendering.md §"Depth & Draw Order"); the compositor never
    /// interprets it or assumes it equals the row. Callers pick what depth means
    /// for their camera — row (side view), row+col (isometric), etc.
    pub depth: i32,
}

/// Composite `placements` into a fresh `cols`×`rows` `Grid`, back-to-front by
/// `depth` ascending (far first; larger depth drawn last = on top).
///
/// - Opaque (`Glyph`) cells overwrite whatever a farther placement wrote.
/// - Transparent cells are skipped so farther cells show through gaps.
/// - Cells landing outside `[0, cols) × [0, rows)` are silently clipped.
/// - Equal-depth placements keep input order (stable sort).
pub fn composite(cols: usize, rows: usize, placements: &[Placement]) -> Grid {
    let mut out = Grid::new(cols, rows);
    let mut ordered: Vec<Placement> = placements.to_vec();
    ordered.sort_by_key(|p| p.depth); // stable ascending: far (low depth) first
    for p in &ordered {
        for r in 0..p.grid.rows() {
            for c in 0..p.grid.cols() {
                let cell = p.grid.get(c, r);
                if let Cell::Glyph { .. } = cell {
                    let dx = p.col + c as i32;
                    let dy = p.row + r as i32;
                    if dx >= 0 && (dx as usize) < cols && dy >= 0 && (dy as usize) < rows {
                        out.set(dx as usize, dy as usize, cell);
                    }
                }
            }
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use scene_core::color::Rgba;

    fn glyph(ch: char, r: u8, g: u8, b: u8) -> Cell {
        Cell::Glyph { ch, color: Rgba::rgb(r, g, b) }
    }

    // Helper: 1×1 grid with a single glyph.
    fn grid_1x1_glyph(ch: char, r: u8, g: u8, b: u8) -> Grid {
        let mut grid = Grid::new(1, 1);
        grid.set(0, 0, glyph(ch, r, g, b));
        grid
    }

    // ── occlusion ─────────────────────────────────────────────────────────────

    /// A near (higher depth) opaque cell over a far opaque cell at the same
    /// destination position must yield the near cell.
    ///
    /// Setup: 2×1 destination.
    ///   far  placement: depth=0, col=0, a single glyph 'F' at (0,0).
    ///   near placement: depth=1, col=0, a single glyph 'N' at (0,0).
    /// Expected: dest(0,0) = 'N' (near wins); dest(1,0) = Transparent.
    #[test]
    fn occlusion_near_opaque_wins_over_far_opaque() {
        let far_grid = grid_1x1_glyph('F', 255, 0, 0);
        let near_grid = grid_1x1_glyph('N', 0, 255, 0);

        let placements = [
            Placement { grid: &far_grid,  col: 0, row: 0, depth: 0 },
            Placement { grid: &near_grid, col: 0, row: 0, depth: 1 },
        ];
        let out = composite(2, 1, &placements);

        assert_eq!(
            out.get(0, 0),
            glyph('N', 0, 255, 0),
            "near (depth=1) opaque cell must overwrite far (depth=0) opaque cell"
        );
        assert_eq!(
            out.get(1, 0),
            Cell::Transparent,
            "position (1,0) untouched by both placements must be Transparent"
        );
    }

    // ── reveal ────────────────────────────────────────────────────────────────

    /// Where the near grid is Transparent at a position occupied by the far
    /// grid, the far grid's glyph must show through (not be overwritten).
    ///
    /// Setup: 2×1 destination.
    ///   far  placement: depth=0, col=0, glyph 'F' at (0,0) and (1,0).
    ///   near placement: depth=1, col=0, a 2×1 grid: (0,0)=glyph 'N', (1,0)=Transparent.
    /// Expected: dest(0,0)='N' (near opaque wins), dest(1,0)='F' (far shows through).
    #[test]
    fn reveal_far_cell_shows_through_near_transparent_gap() {
        let mut far_grid = Grid::new(2, 1);
        far_grid.set(0, 0, glyph('F', 255, 0, 0));
        far_grid.set(1, 0, glyph('F', 255, 0, 0));

        let mut near_grid = Grid::new(2, 1);
        near_grid.set(0, 0, glyph('N', 0, 255, 0));
        // (1,0) stays Transparent

        let placements = [
            Placement { grid: &far_grid,  col: 0, row: 0, depth: 0 },
            Placement { grid: &near_grid, col: 0, row: 0, depth: 1 },
        ];
        let out = composite(2, 1, &placements);

        assert_eq!(
            out.get(0, 0),
            glyph('N', 0, 255, 0),
            "near opaque cell at (0,0) must overwrite far"
        );
        assert_eq!(
            out.get(1, 0),
            glyph('F', 255, 0, 0),
            "near Transparent at (1,0) must allow far glyph to show through"
        );
    }

    // ── clip / no-panic ───────────────────────────────────────────────────────

    /// Placements with partially-out-of-bounds offsets must clip silently:
    /// only in-bounds cells are composited; the call must not panic.
    ///
    /// Setup: 2×2 destination.
    ///   placement A: col=-1, row=0, depth=0, a 2×1 grid with glyph 'A' at
    ///     source (0,0) and (1,0). Source (0,0) maps to dest (-1,0) — clipped.
    ///     Source (1,0) maps to dest (0,0) — in bounds. → only (0,0) written.
    ///   placement B: col=1, row=0, depth=1, a 2×1 grid with glyph 'B' at
    ///     source (0,0) and (1,0). (0,0)→dest(1,0) in bounds; (1,0)→dest(2,0) clipped.
    /// Expected: dest(0,0)='A', dest(1,0)='B'; no panic.
    #[test]
    fn clip_out_of_bounds_cells_no_panic() {
        let mut grid_a = Grid::new(2, 1);
        grid_a.set(0, 0, glyph('A', 200, 0, 0));
        grid_a.set(1, 0, glyph('A', 200, 0, 0));

        let mut grid_b = Grid::new(2, 1);
        grid_b.set(0, 0, glyph('B', 0, 200, 0));
        grid_b.set(1, 0, glyph('B', 0, 200, 0));

        let placements = [
            Placement { grid: &grid_a, col: -1, row: 0, depth: 0 },
            Placement { grid: &grid_b, col:  1, row: 0, depth: 1 },
        ];
        // Must not panic even though both placements are partially out of bounds.
        let out = composite(2, 2, &placements);

        assert_eq!(
            out.get(0, 0),
            glyph('A', 200, 0, 0),
            "grid_a's in-bounds cell at dest(0,0) must be composited"
        );
        assert_eq!(
            out.get(1, 0),
            glyph('B', 0, 200, 0),
            "grid_b's in-bounds cell at dest(1,0) must be composited"
        );
        // Row 1 untouched.
        assert_eq!(out.get(0, 1), Cell::Transparent);
        assert_eq!(out.get(1, 1), Cell::Transparent);
    }
}
