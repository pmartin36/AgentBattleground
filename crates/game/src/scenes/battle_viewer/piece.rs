use super::*;

/// Which side a piece belongs to. Rendering differences (tint, mirror) are
/// added by b4-t3; this enum carries only identity.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Inspectable)]
pub enum Team {
    A,
    B,
}

/// Team A's active (fighting) row is one cell inward of its bench row; Team
/// B's active row mirrors it from the bottom. Bench rows sit one cell
/// INWARD of their own active row (toward the midline), not out at the
/// board's own edge — `BENCH_COL` (outside the drawn grid) is what already
/// carries bench "off the field"; the row itself just needs to read as
/// close to the action, not doubly set apart on both axes.
pub const TEAM_A_ROW: u16 = 1;
pub const TEAM_B_ROW: u16 = BOARD_ROWS - 2;
pub const TEAM_A_BENCH_ROW: u16 = 2;
pub const TEAM_B_BENCH_ROW: u16 = BOARD_ROWS - 3;

/// Symmetric empty column margin on each board edge framing the 3 centered
/// active columns: `(BOARD_COLS - 3) / 2`. For `BOARD_COLS = 7` this is 2,
/// leaving cols 0-1 and 5-6 empty.
const COL_MARGIN: u16 = (BOARD_COLS - 3) / 2;

/// The 3 centered active (fighting) columns each team's active pieces
/// occupy, ascending, with symmetric empty margins on both board edges
/// (18's centering approach, narrowed). For `BOARD_COLS = 7`: `[2, 3, 4]`.
pub const ACTIVE_COLS: [u16; 3] = [COL_MARGIN, COL_MARGIN + 1, COL_MARGIN + 2];

/// The single column the lone bench piece stands on — ONE PAST the drawn
/// grid's far edge (`BOARD_COLS`, world x center `BOARD_COLS + 0.5`), not a
/// column shared with any active piece. This is deliberate, not a margin
/// accident: bench sitting outside the grid on the depth axis is what makes
/// it read as "behind the field" under Sideline (whose depth axis is
/// column) and "off to the side" under Over-the-shoulder (whose spread axis
/// is column) — the SAME board position serves both readings, because each
/// camera maps the column axis to a different screen axis. `BOARD_COLS`
/// (not a column before `0`) was chosen because Sideline's camera sits on
/// the negative/low side (`SIDELINE_CAMERA_DEPTH`) — the far/high side is
/// farther from it, so bench renders smaller there, reading as "receded."
pub const BENCH_COL: u16 = BOARD_COLS;

/// One placed piece. `index` is a stable 0..8 ordinal (column-ascending
/// within a team, Team A before Team B) used later by b4-t3's phase-stagger.
/// `transform`/`color` are owned, seeded once at construction by `Piece::new`
/// (b2-t1) — no `Eq`/`Hash`, since `Transform` has `f32` fields.
#[derive(Clone, Copy, PartialEq, Debug, Inspectable)]
pub struct Piece {
    #[inspect(readonly)]
    pub col: u16,
    #[inspect(readonly)]
    pub row: u16,
    pub team: Team,
    pub index: usize,
    pub transform: Transform,
    pub color: Rgba,
    pub alive: bool,
}

impl Piece {
    /// Sole construction path. Seeds the owned `transform`/`color` fields
    /// once from the piece's `(col, row, team)`: `transform = { translate:
    /// world_pos_for_cell(col, row), rotation: 0.0, scale: (team.scale_x(),
    /// 1.0) }`, `color = team.tint_color()`. After construction these fields
    /// are the piece's own state — nothing re-derives them from `team` again.
    pub fn new(col: u16, row: u16, team: Team, index: usize) -> Self {
        let transform = Transform {
            translate: world_pos_for_cell(col, row),
            rotation: 0.0,
            scale: Vec2::new(team.scale_x(), 1.0),
        };
        let color = team.tint_color();
        Self {
            col,
            row,
            team,
            index,
            transform,
            color,
            alive: true,
        }
    }
}

/// The 3-active+1-bench-per-side layout (8 pieces total). Each team gets 3
/// active pieces on its active row (`TEAM_A_ROW`/`TEAM_B_ROW`) across the
/// centered `ACTIVE_COLS` (ascending), plus 1 bench piece on its bench row
/// (`TEAM_A_BENCH_ROW`/`TEAM_B_BENCH_ROW`) at `BENCH_COL`. Deterministic
/// order: Team A active (ascending col), Team A bench, Team B active
/// (ascending col), Team B bench — indices 0..8 contiguous.
pub fn pieces() -> Vec<Piece> {
    let mut out = Vec::with_capacity(8);
    let mut index = 0;
    for (team, active_row, bench_row) in [
        (Team::A, TEAM_A_ROW, TEAM_A_BENCH_ROW),
        (Team::B, TEAM_B_ROW, TEAM_B_BENCH_ROW),
    ] {
        for col in ACTIVE_COLS {
            out.push(Piece::new(col, active_row, team, index));
            index += 1;
        }
        out.push(Piece::new(BENCH_COL, bench_row, team, index));
        index += 1;
    }
    out
}

/// World position of a board cell's CENTER (not its corner) — matches spec 05's
/// "movement lerps world position between cell centers." General for any cell.
pub fn world_pos_for_cell(col: u16, row: u16) -> WorldPos {
    WorldPos::new(col as f32 + 0.5, row as f32 + 0.5)
}

/// Team A tint (pale gold).
pub const TEAM_A_COLOR: Rgba = Rgba::rgb(0xff, 0xe8, 0xb0);
/// Team B tint (pale mint).
pub const TEAM_B_COLOR: Rgba = Rgba::rgb(0xb0, 0xff, 0xe0);

impl Team {
    /// This team's tint color: A -> `TEAM_A_COLOR`, B -> `TEAM_B_COLOR`.
    pub fn tint_color(self) -> Rgba {
        match self {
            Team::A => TEAM_A_COLOR,
            Team::B => TEAM_B_COLOR,
        }
    }

    /// Horizontal mirror factor for `Transform.scale.x`: A -> 1.0, B -> -1.0.
    pub fn scale_x(self) -> f32 {
        match self {
            Team::A => 1.0,
            Team::B => -1.0,
        }
    }
}

#[cfg(test)]
mod piece_layout_tests {
    use super::*;
    use std::collections::HashSet;

    /// Final layout: 3 active + 1 bench per side (8 total). Each team's 3
    /// active pieces sit on its active row across the centered `ACTIVE_COLS`;
    /// each team's single bench piece sits on its bench row at `BENCH_COL`.
    /// No duplicate (team, col, row) entries.
    #[test]
    fn pieces_are_3_active_1_bench_per_side() {
        let ps = pieces();
        assert_eq!(ps.len(), 8, "expected exactly 8 pieces");

        let team_a: Vec<&Piece> = ps.iter().filter(|p| p.team == Team::A).collect();
        let team_b: Vec<&Piece> = ps.iter().filter(|p| p.team == Team::B).collect();
        assert_eq!(team_a.len(), 4, "expected 4 Team A pieces (3 active + 1 bench)");
        assert_eq!(team_b.len(), 4, "expected 4 Team B pieces (3 active + 1 bench)");

        let active_cols: HashSet<u16> = ACTIVE_COLS.iter().copied().collect();

        let a_active: Vec<&&Piece> = team_a.iter().filter(|p| p.row == TEAM_A_ROW).collect();
        let a_bench: Vec<&&Piece> = team_a
            .iter()
            .filter(|p| p.row == TEAM_A_BENCH_ROW)
            .collect();
        assert_eq!(a_active.len(), 3, "expected 3 Team A active pieces");
        assert_eq!(a_bench.len(), 1, "expected 1 Team A bench piece");
        let a_active_cols: HashSet<u16> = a_active.iter().map(|p| p.col).collect();
        assert_eq!(
            a_active_cols, active_cols,
            "Team A active columns must be ACTIVE_COLS"
        );
        assert_eq!(
            a_bench[0].col, BENCH_COL,
            "Team A bench piece must sit on BENCH_COL"
        );

        let b_active: Vec<&&Piece> = team_b.iter().filter(|p| p.row == TEAM_B_ROW).collect();
        let b_bench: Vec<&&Piece> = team_b
            .iter()
            .filter(|p| p.row == TEAM_B_BENCH_ROW)
            .collect();
        assert_eq!(b_active.len(), 3, "expected 3 Team B active pieces");
        assert_eq!(b_bench.len(), 1, "expected 1 Team B bench piece");
        let b_active_cols: HashSet<u16> = b_active.iter().map(|p| p.col).collect();
        assert_eq!(
            b_active_cols, active_cols,
            "Team B active columns must be ACTIVE_COLS"
        );
        assert_eq!(
            b_bench[0].col, BENCH_COL,
            "Team B bench piece must sit on BENCH_COL"
        );

        let unique: HashSet<(bool, u16, u16)> = ps
            .iter()
            .map(|p| (p.team == Team::A, p.col, p.row))
            .collect();
        assert_eq!(unique.len(), 8, "no duplicate (team, col, row) entries");
    }

    /// Piece indices form the stable contiguous set 0..8, with no gaps or
    /// duplicates — b4-t3's phase-stagger depends on this.
    #[test]
    fn piece_indices_are_stable_0_to_8() {
        let ps = pieces();
        let mut indices: Vec<usize> = ps.iter().map(|p| p.index).collect();
        indices.sort_unstable();
        assert_eq!(indices, (0..8).collect::<Vec<_>>());
    }

    /// Pins the exact index -> (team, col, row) assignment order: Team A's 3
    /// active (ascending col), then Team A's bench, then Team B's 3 active
    /// (ascending col), then Team B's bench. Downstream code (demo_events'
    /// `piece_index: 6` targeting Team B, scene-wiring's `scene.pieces[6]`)
    /// relies on this exact order — guard against silent reordering.
    #[test]
    fn piece_index_order_is_a_active_then_bench_then_b_active_then_bench() {
        let ps = pieces();
        let by_index = |i: usize| ps.iter().find(|p| p.index == i).expect("index must exist");

        for (i, &col) in ACTIVE_COLS.iter().enumerate() {
            let p = by_index(i);
            assert_eq!(p.team, Team::A, "index {i} must be Team A active");
            assert_eq!(p.col, col, "index {i} must be at ACTIVE_COLS[{i}]");
            assert_eq!(p.row, TEAM_A_ROW);
        }
        let a_bench = by_index(3);
        assert_eq!(a_bench.team, Team::A);
        assert_eq!(a_bench.col, BENCH_COL);
        assert_eq!(a_bench.row, TEAM_A_BENCH_ROW);

        for (i, &col) in ACTIVE_COLS.iter().enumerate() {
            let p = by_index(4 + i);
            assert_eq!(p.team, Team::B, "index {} must be Team B active", 4 + i);
            assert_eq!(p.col, col);
            assert_eq!(p.row, TEAM_B_ROW);
        }
        let b_bench = by_index(7);
        assert_eq!(b_bench.team, Team::B);
        assert_eq!(b_bench.col, BENCH_COL);
        assert_eq!(b_bench.row, TEAM_B_BENCH_ROW);
    }

    /// world_pos_for_cell must return the cell CENTER, not the corner — pins
    /// the convention the plan-validator flagged as at risk of regressing.
    #[test]
    fn world_pos_is_cell_center() {
        assert_eq!(world_pos_for_cell(0, 0), WorldPos::new(0.5, 0.5));
        assert_eq!(world_pos_for_cell(3, 7), WorldPos::new(3.5, 7.5));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: per-piece render pipeline (b4-t3)
// ─────────────────────────────────────────────────────────────────────────────

