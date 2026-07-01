//! Golden tests for the compositor — Tier-3 correctness gate.
//!
//! Cases (a)–(e) are hand-derived from the back-to-front, cell-level,
//! depth-ascending rule documented in specs/13-rendering.md §"Depth & Draw Order"
//! and implemented in `render::composite`.
//!
//! Oracle rule (independent of implementation internals):
//!
//! 1. Sort placements ascending by depth (lower depth = farther = drawn first).
//! 2. For each cell in each placement (sorted order), if the source cell is
//!    `Glyph` and the destination is in bounds, overwrite the destination.
//! 3. Transparent source cells are skipped (prior destination value survives).
//! 4. Out-of-bounds cells are silently discarded (no panic).
//! 5. Equal-depth placements keep input order (stable sort).


use render::{composite, Cell, Grid, Placement};
use scene_core::color::Rgba;

// ── helpers ───────────────────────────────────────────────────────────────────

fn glyph(ch: char, r: u8, gr: u8, b: u8) -> Cell {
    Cell::Glyph { ch, color: Rgba::rgb(r, gr, b) }
}

/// Build a 1×1 grid with a single glyph cell.
fn grid_1x1(ch: char, r: u8, gr: u8, b: u8) -> Grid {
    let mut grid = Grid::new(1, 1);
    grid.set(0, 0, glyph(ch, r, gr, b));
    grid
}

// ── (a) occlusion ─────────────────────────────────────────────────────────────

/// A near (higher-depth) opaque cell over a far opaque cell at the same
/// destination position must yield the near cell.
///
/// Setup: 2×1 destination.
///
/// - far  (depth=0): 1×1 grid, `'F'` red,   placed at (0,0).
/// - near (depth=1): 1×1 grid, `'N'` green, placed at (0,0).
///
/// Hand-derived oracle (back-to-front):
///
/// 1. Draw far  → dest(0,0) = `'F'`
/// 2. Draw near → dest(0,0) = `'N'`  ← near opaque overwrites far
///
/// Result: dest(0,0) = `'N'`;  dest(1,0) = `Transparent` (untouched).
#[test]
fn golden_occlusion_near_opaque_over_far() {
    let far_grid  = grid_1x1('F', 255, 0, 0);
    let near_grid = grid_1x1('N', 0, 255, 0);

    let placements = [
        Placement { grid: &far_grid,  col: 0, row: 0, depth: 0 },
        Placement { grid: &near_grid, col: 0, row: 0, depth: 1 },
    ];
    let out = composite(2, 1, &placements);

    assert_eq!(out.get(0, 0), glyph('N', 0, 255, 0), "near (depth=1) opaque cell wins over far (depth=0)");
    assert_eq!(out.get(1, 0), Cell::Transparent, "untouched cell stays Transparent");
}

// ── (b) reveal ────────────────────────────────────────────────────────────────

/// Where the near grid is Transparent at a position the far grid occupies,
/// the far grid's glyph must show through.
///
/// Setup: 2×1 destination.
///
/// - far  (depth=0): 2×1 grid, `'F'` red   at (0,0) and (1,0).
/// - near (depth=1): 2×1 grid, `'N'` green at (0,0); (1,0) = Transparent.
///
/// Hand-derived oracle:
///
/// - After far blit:  dest(0,0)=`'F'`, dest(1,0)=`'F'`
/// - After near blit: dest(0,0)=`'N'` (overwritten), dest(1,0)=`'F'` (near Transparent → skip)
///
/// Result: dest(0,0) = `'N'`;  dest(1,0) = `'F'`.
#[test]
fn golden_reveal_far_through_near_transparent() {
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

    assert_eq!(out.get(0, 0), glyph('N', 0, 255, 0), "near opaque at (0,0) wins over far");
    assert_eq!(out.get(1, 0), glyph('F', 255, 0, 0), "far shows through near Transparent at (1,0)");
}

// ── (c) depth-swap flips winner ───────────────────────────────────────────────

/// Swapping the depth values of case (a)'s placements must flip which glyph
/// wins. Input array order is identical to (a); only `depth` fields are swapped.
/// This isolates depth ordering from stable-sort input-order tie-breaking.
///
/// Setup: 2×1 destination.
///
/// - grid_a (glyph `'F'` red)   at (0,0), depth=1  ← was depth=0 in (a)
/// - grid_b (glyph `'N'` green) at (0,0), depth=0  ← was depth=1 in (a)
///
/// Hand-derived oracle (ascending depth sort):
///
/// - Sort:   grid_b (depth=0) drawn first, grid_a (depth=1) drawn last.
/// - Draw b: dest(0,0) = `'N'`
/// - Draw a: dest(0,0) = `'F'`  ← grid_a drawn last → wins
///
/// Result: dest(0,0) = `'F'` (opposite of case (a) where `'N'` won).
#[test]
fn golden_depth_swap_flips_winner() {
    let grid_a = grid_1x1('F', 255, 0, 0); // same glyph as "far" in (a)
    let grid_b = grid_1x1('N', 0, 255, 0); // same glyph as "near" in (a)

    // Same array order as (a); depths are swapped.
    let placements = [
        Placement { grid: &grid_a, col: 0, row: 0, depth: 1 }, // was depth=0
        Placement { grid: &grid_b, col: 0, row: 0, depth: 0 }, // was depth=1
    ];
    let out = composite(2, 1, &placements);

    assert_eq!(out.get(0, 0), glyph('F', 255, 0, 0), "depth=1 grid_a drawn last wins; opposite of case (a)");
}

// ── (d) clipping ─────────────────────────────────────────────────────────────

/// Placements partially outside the destination must be clipped silently;
/// in-bounds cells still composite correctly; no panic.
///
/// Setup: 2×2 destination.
///
/// - placement A: 2×1 grid `'A'` blue,  col=-1, row=0, depth=0
///   - src(0,0)→dest(-1,0) clipped; src(1,0)→dest(0,0) kept.
/// - placement B: 2×1 grid `'B'` teal,  col=1,  row=0, depth=1
///   - src(0,0)→dest(1,0) kept;    src(1,0)→dest(2,0) clipped.
///
/// Hand-derived oracle:
///
/// - dest(0,0) = `'A'` (in-bounds from A)
/// - dest(1,0) = `'B'` (in-bounds from B)
/// - dest(0,1) = dest(1,1) = Transparent (row 1 untouched)
/// - No panic.
#[test]
fn golden_clip_partial_out_of_bounds_no_panic() {
    let mut grid_a = Grid::new(2, 1);
    grid_a.set(0, 0, glyph('A', 0, 0, 200));
    grid_a.set(1, 0, glyph('A', 0, 0, 200));

    let mut grid_b = Grid::new(2, 1);
    grid_b.set(0, 0, glyph('B', 0, 180, 180));
    grid_b.set(1, 0, glyph('B', 0, 180, 180));

    let placements = [
        Placement { grid: &grid_a, col: -1, row: 0, depth: 0 },
        Placement { grid: &grid_b, col:  1, row: 0, depth: 1 },
    ];
    // Must not panic.
    let out = composite(2, 2, &placements);

    assert_eq!(out.get(0, 0), glyph('A', 0, 0, 200), "A's in-bounds cell at dest(0,0)");
    assert_eq!(out.get(1, 0), glyph('B', 0, 180, 180), "B's in-bounds cell at dest(1,0)");
    assert_eq!(out.get(0, 1), Cell::Transparent, "row 1 untouched");
    assert_eq!(out.get(1, 1), Cell::Transparent, "row 1 untouched");
}

// ── (e) three-way stack: nearest opaque wins per cell ────────────────────────

/// Three overlapping placements covering the same 1×3 column.
/// Each cell in the column independently resolves to the nearest opaque glyph.
///
/// Setup: 1×3 destination (cols=1, rows=3); all placements at col=0, row=0.
///
/// - far  (depth=0): row0=`'A'` opaque, row1=`'A'` opaque, row2=`'A'` opaque
/// - mid  (depth=1): row0=Transparent,  row1=`'M'` opaque, row2=`'M'` opaque
/// - near (depth=2): row0=Transparent,  row1=Transparent,  row2=`'N'` opaque
///
/// Hand-derived oracle (back-to-front blit):
///
/// - After far:  (0,0)=`'A'`, (0,1)=`'A'`, (0,2)=`'A'`
/// - After mid:  (0,0)=`'A'` (skip), (0,1)=`'M'`, (0,2)=`'M'`
/// - After near: (0,0)=`'A'` (skip), (0,1)=`'M'` (skip), (0,2)=`'N'`
///
/// Result: (0,0)=`'A'`, (0,1)=`'M'`, (0,2)=`'N'` — each position holds the
/// nearest opaque cell covering it.
#[test]
fn golden_three_way_stack_nearest_opaque_per_cell() {
    let mut far_grid = Grid::new(1, 3);
    far_grid.set(0, 0, glyph('A', 200, 100, 50));
    far_grid.set(0, 1, glyph('A', 200, 100, 50));
    far_grid.set(0, 2, glyph('A', 200, 100, 50));

    let mut mid_grid = Grid::new(1, 3);
    // (0,0) stays Transparent
    mid_grid.set(0, 1, glyph('M', 50, 200, 100));
    mid_grid.set(0, 2, glyph('M', 50, 200, 100));

    let mut near_grid = Grid::new(1, 3);
    // (0,0) and (0,1) stay Transparent
    near_grid.set(0, 2, glyph('N', 100, 50, 200));

    let placements = [
        Placement { grid: &far_grid,  col: 0, row: 0, depth: 0 },
        Placement { grid: &mid_grid,  col: 0, row: 0, depth: 1 },
        Placement { grid: &near_grid, col: 0, row: 0, depth: 2 },
    ];
    let out = composite(1, 3, &placements);

    assert_eq!(out.get(0, 0), glyph('A', 200, 100, 50), "only far covers row 0");
    assert_eq!(out.get(0, 1), glyph('M', 50, 200, 100), "mid nearest opaque at row 1");
    assert_eq!(out.get(0, 2), glyph('N', 100, 50, 200), "near nearest opaque at row 2");
}
