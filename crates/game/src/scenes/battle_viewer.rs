use std::time::Duration;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use render::camera::{SideView, WorldPos};
use render::composite::{composite_dots, DotPlacement};
use render::dots::{dots_to_grid, tint, DotBuffer};
use render::transform::{place, rasterize, Transform, Vec2};
use render::{draw_grid, AnimatedSprite};
use scene_core::color::Rgba;
use scene_core::scene_id::SceneId;
use serde_json::Value as JsonValue;

use crate::scene::{EngineCtx, InputEvent, Scene, Transition};

/// Single source of truth for the board's column count. Every downstream
/// consumer must reference this constant, never a bare literal `8`.
pub const BOARD_COLS: u16 = 8;
/// Single source of truth for the board's row count.
pub const BOARD_ROWS: u16 = 8;

/// Shared per-frame board/camera geometry, derived once from the render
/// `area` and consumed identically by board-line rendering and piece
/// placement. No downstream task may re-derive any of these fields.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct BoardGeometry {
    /// Terminal columns per board cell (== 2 * cell_height_rows).
    pub cell_width_cols: u16,
    /// Terminal rows per board cell.
    pub cell_height_rows: u16,
    /// The board's bounding rect, centered within the render `area`.
    pub board_rect: Rect,
    /// Camera derived from this geometry's dot scale.
    pub camera: SideView,
}

/// Derive the board geometry for a given render `area`. Total (never
/// panics) and deterministic: picks the largest integer `cell_height_rows`
/// such that the board fits `area`, clamped to a minimum of 1.
pub fn board_geometry(area: Rect) -> BoardGeometry {
    let cell_height_rows = (area.width / (2 * BOARD_COLS))
        .min(area.height / BOARD_ROWS)
        .max(1);
    let cell_width_cols = 2 * cell_height_rows;

    let w = cell_width_cols * BOARD_COLS;
    let bh = cell_height_rows * BOARD_ROWS;
    let bx = area.left() + area.width.saturating_sub(w) / 2;
    let by = area.top() + area.height.saturating_sub(bh) / 2;
    let board_rect = Rect::new(bx, by, w, bh);

    let camera = SideView::new((cell_height_rows * 4) as f32);

    BoardGeometry {
        cell_width_cols,
        cell_height_rows,
        board_rect,
        camera,
    }
}

/// Draws box-drawing border/grid lines for a `BOARD_COLS x BOARD_ROWS` grid
/// of `geom.cell_width_cols` x `geom.cell_height_rows`-sized cells, positioned
/// at `geom.board_rect`. Uses ONLY the fields of the `BoardGeometry` passed
/// in — no independent re-derivation of cell size/position. Cell interiors
/// are left untouched (lines only). Clips instead of panicking on an
/// undersized buffer.
pub fn draw_board_lines(buf: &mut Buffer, geom: &BoardGeometry) {
    let cw = geom.cell_width_cols;
    let chh = geom.cell_height_rows;
    let x0 = geom.board_rect.x;
    let y0 = geom.board_rect.y;

    let put = |buf: &mut Buffer, x: u16, y: u16, ch: char| {
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_char(ch);
            cell.set_fg(Color::Rgb(0x55, 0x55, 0x55));
        }
    };

    // Junctions: every grid-line intersection, glyph chosen by which edges
    // (if any) the intersection sits on.
    for j in 0..=BOARD_ROWS {
        let y = y0.saturating_add(j.saturating_mul(chh));
        let top = j == 0;
        let bottom = j == BOARD_ROWS;
        for i in 0..=BOARD_COLS {
            let x = x0.saturating_add(i.saturating_mul(cw));
            let left = i == 0;
            let right = i == BOARD_COLS;
            let ch = match (left, right, top, bottom) {
                (true, false, true, false) => '┌',
                (false, true, true, false) => '┐',
                (true, false, false, true) => '└',
                (false, true, false, true) => '┘',
                (true, false, false, false) => '├',
                (false, true, false, false) => '┤',
                (false, false, true, false) => '┬',
                (false, false, false, true) => '┴',
                _ => '┼',
            };
            put(buf, x, y, ch);
        }
    }

    // Horizontal runs: between consecutive vlines, along every hline row.
    for j in 0..=BOARD_ROWS {
        let y = y0.saturating_add(j.saturating_mul(chh));
        for i in 0..BOARD_COLS {
            let base_x = x0.saturating_add(i.saturating_mul(cw));
            for dx in 1..cw {
                put(buf, base_x.saturating_add(dx), y, '─');
            }
        }
    }

    // Vertical runs: between consecutive hlines, along every vline column.
    for i in 0..=BOARD_COLS {
        let x = x0.saturating_add(i.saturating_mul(cw));
        for j in 0..BOARD_ROWS {
            let base_y = y0.saturating_add(j.saturating_mul(chh));
            for dy in 1..chh {
                put(buf, x, base_y.saturating_add(dy), '│');
            }
        }
    }
}

/// Which side a piece belongs to. Rendering differences (tint, mirror) are
/// added by b4-t3; this enum carries only identity.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Team {
    A,
    B,
}

/// Team A occupies the top row; Team B occupies the bottom row.
pub const TEAM_A_ROW: u16 = 0;
pub const TEAM_B_ROW: u16 = BOARD_ROWS - 1;

/// One placed piece. `index` is a stable 0..12 ordinal (column-ascending
/// within a team, Team A before Team B) used later by b4-t3's phase-stagger.
/// `transform`/`color` are owned, seeded once at construction by `Piece::new`
/// (b2-t1) — no `Eq`/`Hash`, since `Transform` has `f32` fields.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Piece {
    pub col: u16,
    pub row: u16,
    pub team: Team,
    pub index: usize,
    pub transform: Transform,
    pub color: Rgba,
}

impl Piece {
    /// Sole construction path (b2-t1). Seeds `transform`/`color` once from
    /// the same math `piece_transform`/`Team::tint_color` compute on the fly
    /// today: `transform = { translate: world_pos_for_cell(col, row),
    /// rotation: 0.0, scale: (team.scale_x(), 1.0) }`, `color =
    /// team.tint_color()`.
    ///
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
        }
    }
}

/// The 12-piece 6v6 static layout: Team A on `TEAM_A_ROW`, Team B on
/// `TEAM_B_ROW`, both on columns `1..(BOARD_COLS - 1)` (cols 0 and the last
/// column left empty). Deterministic order: Team A cols asc, then Team B cols asc.
pub fn pieces() -> Vec<Piece> {
    let mut out = Vec::with_capacity(12);
    let mut index = 0;
    for (team, row) in [(Team::A, TEAM_A_ROW), (Team::B, TEAM_B_ROW)] {
        for col in 1..(BOARD_COLS - 1) {
            out.push(Piece::new(col, row, team, index));
            index += 1;
        }
    }
    out
}

/// World position of a board cell's CENTER (not its corner) — matches spec 05's
/// "movement lerps world position between cell centers." General for any cell.
pub fn world_pos_for_cell(col: u16, row: u16) -> WorldPos {
    WorldPos::new(col as f32 + 0.5, row as f32 + 0.5)
}

// ─────────────────────────────────────────────────────────────────────────────
// b4-t3: per-piece render pipeline — team tint, mirror, phase-staggered idle
// frame. Signatures/constants per research.md blueprint; bodies are stubs for
// the code-writer (test-writer only pins the observable contract).
// ─────────────────────────────────────────────────────────────────────────────

/// Per-index idle-animation desync offset (spec 05: pieces must not all
/// animate in lockstep).
pub const PIECE_STAGGER: std::time::Duration = std::time::Duration::from_millis(37);
/// `sprite_base_dot_rows` = `camera.scale_dots * SPRITE_DOT_RATIO`, rounded.
pub const SPRITE_DOT_RATIO: f32 = 1.2;
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

/// Per-index animation offset so the 12 idle loops don't play in lockstep:
/// `elapsed + PIECE_STAGGER * index`.
pub fn piece_elapsed(elapsed: Duration, index: usize) -> Duration {
    elapsed + PIECE_STAGGER * index as u32
}

/// Sprite height in dots, sized off the shared camera's per-world-unit dot
/// scale: `(camera.scale_dots * SPRITE_DOT_RATIO).round() as u32`.
pub fn sprite_base_dot_rows(camera: &SideView) -> u32 {
    (camera.scale_dots * SPRITE_DOT_RATIO).round() as u32
}

/// One `Transform` reused for both rasterize (scale/mirror) and place
/// (translate): `translate = world_pos_for_cell(piece.col, piece.row)`,
/// `rotation = 0.0`, `scale = Vec2::new(piece.team.scale_x(), 1.0)`.
pub fn piece_transform(piece: &Piece) -> Transform {
    Transform {
        translate: world_pos_for_cell(piece.col, piece.row),
        rotation: 0.0,
        scale: Vec2::new(piece.team.scale_x(), 1.0),
    }
}

/// Steps a-c of the per-piece pipeline: pick the staggered idle frame,
/// rasterize it (scale/mirror sized off `geom.camera`), then tint with the
/// piece's team color. Returns an owned `DotBuffer` (see research.md's
/// REFINED lifetime split — `place_piece`, b4-t4, borrows from this).
pub fn piece_dots(
    piece: &Piece,
    sprite: &AnimatedSprite,
    elapsed: Duration,
    geom: &BoardGeometry,
) -> DotBuffer {
    let frame = sprite.frame_at(piece_elapsed(elapsed, piece.index));
    let raw = rasterize(
        frame,
        &piece.transform,
        sprite_base_dot_rows(&geom.camera),
    );
    tint(&raw, piece.color)
}

/// Step d: thin reuse of `render::transform::place` through the shared
/// camera — places `dots` at the piece's cell CENTER world position.
pub fn place_piece<'a>(
    dots: &'a DotBuffer,
    piece: &Piece,
    geom: &BoardGeometry,
) -> DotPlacement<'a> {
    place(dots, world_pos_for_cell(piece.col, piece.row), &geom.camera)
}

/// Uniform per-frame playback speed for the bundled wizard idle GIF. The
/// GIF's own per-frame delays are intentionally ignored by `from_gif`; this
/// constant is the single source of truth for animation speed.
const WIZARD_FRAME_DUR: Duration = Duration::from_millis(100);

pub struct BattleViewer {
    elapsed: f32,
    sprite: AnimatedSprite,
    /// Owned piece state (b3-t1), seeded once from `pieces()` at construction
    /// — `render()` does not read this yet (that is b3-t2).
    pub pieces: Vec<Piece>,
}

impl Default for BattleViewer {
    fn default() -> Self {
        let sprite = AnimatedSprite::from_gif(
            include_bytes!("assets/wizard.gif"),
            WIZARD_FRAME_DUR,
        )
        .expect("bundled wizard.gif must decode");
        Self {
            elapsed: 0.0,
            sprite,
            pieces: pieces(),
        }
    }
}

impl Scene for BattleViewer {
    fn id(&self) -> SceneId {
        SceneId::BattleViewer
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}

    fn update(&mut self, _ctx: &mut EngineCtx, dt: Duration) -> Option<Transition> {
        self.elapsed += dt.as_secs_f32();
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let geom = board_geometry(area);
        draw_board_lines(frame.buffer_mut(), &geom);

        let elapsed = Duration::from_secs_f32(self.elapsed);
        let dotbufs: Vec<DotBuffer> = self
            .pieces
            .iter()
            .map(|p| piece_dots(p, &self.sprite, elapsed, &geom))
            .collect();
        let placements: Vec<DotPlacement> = self
            .pieces
            .iter()
            .zip(&dotbufs)
            .map(|(p, dots)| place_piece(dots, p, &geom))
            .collect();

        let composed = composite_dots(
            (geom.board_rect.width * 2) as usize,
            (geom.board_rect.height * 4) as usize,
            &placements,
        );
        let grid = dots_to_grid(&composed);
        draw_grid(frame.buffer_mut(), geom.board_rect, &grid);
    }

    fn handle_input(&mut self, _ev: InputEvent) -> Option<Transition> {
        None
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {}
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: board_geometry (b1-t1)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod board_geometry_tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use render::{draw_grid, Cell, Grid};
    use scene_core::color::Rgba;

    /// Exact-fit area: 128x64 fits exactly 8 cells of 16x8 dots each.
    #[test]
    fn exact_fit_area() {
        let g = board_geometry(Rect::new(0, 0, 128, 64));
        assert_eq!(g.cell_height_rows, 8);
        assert_eq!(g.cell_width_cols, 16);
        assert_eq!(g.board_rect, Rect::new(0, 0, 128, 64));
        assert_eq!(g.camera.scale_dots, 32.0);
    }

    /// Oversized area: geometry is centered within the larger area.
    #[test]
    fn oversized_area_is_centered() {
        let g = board_geometry(Rect::new(5, 5, 200, 100));
        assert_eq!(g.cell_height_rows, 12);
        assert_eq!(g.board_rect, Rect::new(9, 7, 192, 96));
    }

    /// Height-constrained area: height is the limiting dimension.
    #[test]
    fn height_constrained_area() {
        let g = board_geometry(Rect::new(0, 0, 300, 40));
        assert_eq!(g.cell_height_rows, 5);
        assert_eq!(g.board_rect, Rect::new(110, 0, 80, 40));
    }

    /// Width-constrained area: width is the limiting dimension.
    #[test]
    fn width_constrained_area() {
        let g = board_geometry(Rect::new(0, 0, 50, 300));
        assert_eq!(g.cell_height_rows, 3);
        assert_eq!(g.board_rect, Rect::new(1, 138, 48, 24));
    }

    /// Tiny area clamps to cell_height_rows == 1 and does not panic.
    #[test]
    fn tiny_area_clamps_without_panic() {
        let g = board_geometry(Rect::new(0, 0, 10, 5));
        assert_eq!(g.cell_height_rows, 1);
    }

    /// Invariant: cell_width_cols == 2 * cell_height_rows must hold for every
    /// area tested above.
    #[test]
    fn cell_width_is_always_double_cell_height() {
        let areas = [
            Rect::new(0, 0, 128, 64),
            Rect::new(5, 5, 200, 100),
            Rect::new(0, 0, 300, 40),
            Rect::new(0, 0, 50, 300),
            Rect::new(0, 0, 10, 5),
        ];
        for area in areas {
            let g = board_geometry(area);
            assert_eq!(
                g.cell_width_cols,
                2 * g.cell_height_rows,
                "cell_width_cols must be 2x cell_height_rows for area {area:?}"
            );
        }
    }

    /// The board-size constants are exactly 8x8 and must be referenced (not
    /// re-hardcoded) by every downstream consumer.
    #[test]
    fn board_size_constants_are_8x8() {
        assert_eq!(BOARD_COLS, 8);
        assert_eq!(BOARD_ROWS, 8);
    }

    /// Cross-check: board_geometry's centering derivation must land at the
    /// exact same (x,y) as render::draw_grid's own centering formula, when
    /// fed a Grid sized to match the geometry's board dimensions.
    #[test]
    fn board_rect_matches_draw_grid_centering() {
        let area = Rect::new(5, 5, 200, 100);
        let g = board_geometry(area);

        let cols = (g.cell_width_cols * BOARD_COLS) as usize;
        let rows = (g.cell_height_rows * BOARD_ROWS) as usize;
        let mut grid = Grid::new(cols, rows);
        grid.set(
            0,
            0,
            Cell::Glyph {
                ch: '⣿',
                color: Rgba::rgb(0xFF, 0xFF, 0xFF),
            },
        );

        let mut buf = Buffer::empty(Rect::new(0, 0, 205, 105));
        draw_grid(&mut buf, area, &grid);

        let landed = buf
            .cell((g.board_rect.x, g.board_rect.y))
            .expect("board_rect origin must be within buffer");
        assert_eq!(
            landed.symbol(),
            "⣿",
            "draw_grid's own centering must land the glyph at board_rect's (x,y)"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: draw_board_lines (b3-t1)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod draw_board_lines_tests {
    use super::*;
    use ratatui::buffer::Buffer;

    /// Hand-picked geometry used by every case below: board_rect origin
    /// (2,1), cell_width_cols=4, cell_height_rows=2, in a 40x20 buffer.
    /// vlines land at x in {2,6,10,14,18,22,26,30,34};
    /// hlines land at y in {1,3,5,7,9,11,13,15,17}.
    fn geom() -> BoardGeometry {
        BoardGeometry {
            cell_width_cols: 4,
            cell_height_rows: 2,
            board_rect: Rect::new(2, 1, 32, 16),
            camera: SideView::new(8.0),
        }
    }

    #[test]
    fn four_corners_are_box_corners() {
        let g = geom();
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        draw_board_lines(&mut buf, &g);

        assert_eq!(buf.cell((2, 1)).unwrap().symbol(), "┌");
        assert_eq!(buf.cell((34, 1)).unwrap().symbol(), "┐");
        assert_eq!(buf.cell((2, 17)).unwrap().symbol(), "└");
        assert_eq!(buf.cell((34, 17)).unwrap().symbol(), "┘");
    }

    #[test]
    fn interior_crossing_is_cross() {
        let g = geom();
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        draw_board_lines(&mut buf, &g);

        assert_eq!(buf.cell((6, 3)).unwrap().symbol(), "┼");
    }

    #[test]
    fn top_edge_tee_and_left_edge_tee() {
        let g = geom();
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        draw_board_lines(&mut buf, &g);

        assert_eq!(buf.cell((6, 1)).unwrap().symbol(), "┬");
        assert_eq!(buf.cell((2, 3)).unwrap().symbol(), "├");
    }

    #[test]
    fn straight_runs() {
        let g = geom();
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        draw_board_lines(&mut buf, &g);

        assert_eq!(buf.cell((3, 1)).unwrap().symbol(), "─");
        assert_eq!(buf.cell((2, 2)).unwrap().symbol(), "│");
    }

    /// A cell interior (neither on a vline nor an hline) must be left
    /// untouched — proves no fill, lines only.
    #[test]
    fn cell_interior_untouched() {
        let g = geom();
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        buf.cell_mut((4, 2)).unwrap().set_char('X');
        draw_board_lines(&mut buf, &g);

        assert_eq!(buf.cell((4, 2)).unwrap().symbol(), "X");
    }

    /// Drawing into a buffer smaller than board_rect must not panic — the
    /// out-of-bounds far border is silently clipped.
    #[test]
    fn no_panic_on_undersized_buffer() {
        let g = geom();
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 5));
        draw_board_lines(&mut buf, &g);
        // Top-left corner is still in-bounds and drawn.
        assert_eq!(buf.cell((2, 1)).unwrap().symbol(), "┌");
    }

    /// Two different BoardGeometry values (from two different areas) must
    /// produce lines at their own, different absolute positions — proves
    /// draw_board_lines consumes the geometry rather than a hardcoded size.
    #[test]
    fn two_geometries_scale() {
        let area_a = Rect::new(0, 0, 128, 64);
        let area_b = Rect::new(5, 5, 200, 100);
        let ga = board_geometry(area_a);
        let gb = board_geometry(area_b);
        assert_ne!(ga.board_rect, gb.board_rect);

        let mut buf_a = Buffer::empty(area_a);
        draw_board_lines(&mut buf_a, &ga);
        assert_eq!(
            buf_a.cell((ga.board_rect.x, ga.board_rect.y)).unwrap().symbol(),
            "┌"
        );
        assert_eq!(
            buf_a
                .cell((ga.board_rect.x + ga.cell_width_cols, ga.board_rect.y))
                .unwrap()
                .symbol(),
            "┬"
        );

        let mut buf_b = Buffer::empty(area_b);
        draw_board_lines(&mut buf_b, &gb);
        assert_eq!(
            buf_b.cell((gb.board_rect.x, gb.board_rect.y)).unwrap().symbol(),
            "┌"
        );
        assert_eq!(
            buf_b
                .cell((gb.board_rect.x + gb.cell_width_cols, gb.board_rect.y))
                .unwrap()
                .symbol(),
            "┬"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: pieces() + world_pos_for_cell (b4-t2)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod piece_layout_tests {
    use super::*;
    use std::collections::HashSet;

    /// 12 pieces total: 6 Team A on row 0, 6 Team B on row BOARD_ROWS-1, each
    /// team's columns exactly {1,2,3,4,5,6}, no duplicates.
    #[test]
    fn pieces_are_6v6_on_the_edge_rows() {
        let ps = pieces();
        assert_eq!(ps.len(), 12, "expected exactly 12 pieces");

        let team_a: Vec<&Piece> = ps.iter().filter(|p| p.team == Team::A).collect();
        let team_b: Vec<&Piece> = ps.iter().filter(|p| p.team == Team::B).collect();
        assert_eq!(team_a.len(), 6, "expected 6 Team A pieces");
        assert_eq!(team_b.len(), 6, "expected 6 Team B pieces");

        assert!(
            team_a.iter().all(|p| p.row == 0),
            "all Team A pieces must be on row 0"
        );
        assert!(
            team_b.iter().all(|p| p.row == BOARD_ROWS - 1),
            "all Team B pieces must be on row BOARD_ROWS - 1"
        );

        let expected_cols: HashSet<u16> = (1..=6).collect();
        let a_cols: HashSet<u16> = team_a.iter().map(|p| p.col).collect();
        let b_cols: HashSet<u16> = team_b.iter().map(|p| p.col).collect();
        assert_eq!(a_cols, expected_cols, "Team A columns must be {{1..6}}");
        assert_eq!(b_cols, expected_cols, "Team B columns must be {{1..6}}");

        let unique: HashSet<(bool, u16, u16)> = ps
            .iter()
            .map(|p| (p.team == Team::A, p.col, p.row))
            .collect();
        assert_eq!(unique.len(), 12, "no duplicate (team, col, row) entries");
    }

    /// Piece indices form the stable set 0..12, with no gaps or duplicates —
    /// b4-t3's phase-stagger depends on this.
    #[test]
    fn piece_indices_are_stable_0_to_11() {
        let ps = pieces();
        let mut indices: Vec<usize> = ps.iter().map(|p| p.index).collect();
        indices.sort_unstable();
        assert_eq!(indices, (0..12).collect::<Vec<_>>());
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

#[cfg(test)]
mod piece_render_tests {
    use super::*;
    use image::{DynamicImage, Rgba as PixelRgba, RgbaImage};
    use render::dots::Dot;

    /// A uniform fully-opaque RGBA image (source for the synthetic sprites
    /// below) — deterministic, unlike the real GIF asset.
    fn opaque_image(w: u32, h: u32) -> DynamicImage {
        let mut raw = RgbaImage::new(w, h);
        for p in raw.pixels_mut() {
            *p = PixelRgba([200, 200, 200, 255]);
        }
        DynamicImage::from(raw)
    }

    /// Hand-picked geometry (mirrors `draw_board_lines_tests::geom`).
    fn test_geom() -> BoardGeometry {
        BoardGeometry {
            cell_width_cols: 4,
            cell_height_rows: 2,
            board_rect: Rect::new(0, 0, 32, 16),
            camera: SideView::new(8.0),
        }
    }

    /// DELIVERABLE (1): with a synthetic single-frame fully-opaque uniform-gray
    /// (200,200,200) source image, `piece_dots` multiply-tints every dot to a
    /// hand-derived per-team color (`floor(200 * team_channel / 255)` per
    /// channel), and the two teams' resulting colors differ. (Opaque source =>
    /// every dot Lit even after mirror.)
    #[test]
    fn piece_dots_tints_each_team_distinctly_via_multiply_blend() {
        let sprite = AnimatedSprite::new(vec![opaque_image(6, 12)], Duration::from_millis(100));
        let geom = test_geom();

        let piece_a = Piece::new(1, TEAM_A_ROW, Team::A, 0);
        let piece_b = Piece::new(1, TEAM_B_ROW, Team::B, 0);

        let dots_a = piece_dots(&piece_a, &sprite, Duration::ZERO, &geom);
        let dots_b = piece_dots(&piece_b, &sprite, Duration::ZERO, &geom);

        // TEAM_A_COLOR = (255,232,176): 200*255/255=200, 200*232/255=181, 200*176/255=138
        let expected_a = Rgba::rgb(200, 181, 138);
        // TEAM_B_COLOR = (176,255,224): 200*176/255=138, 200*255/255=200, 200*224/255=175
        let expected_b = Rgba::rgb(138, 200, 175);

        assert!(dots_a.cols() > 0 && dots_a.rows() > 0, "Team A buffer must be non-empty");
        for row in 0..dots_a.rows() {
            for col in 0..dots_a.cols() {
                assert_eq!(
                    dots_a.get(col, row),
                    Dot::Lit(expected_a),
                    "Team A dot ({col},{row}) must be Lit(expected_a) for a uniform-gray opaque source"
                );
            }
        }

        assert!(dots_b.cols() > 0 && dots_b.rows() > 0, "Team B buffer must be non-empty");
        for row in 0..dots_b.rows() {
            for col in 0..dots_b.cols() {
                assert_eq!(
                    dots_b.get(col, row),
                    Dot::Lit(expected_b),
                    "Team B dot ({col},{row}) must be Lit(expected_b) for a uniform-gray opaque source"
                );
            }
        }

        assert_ne!(expected_a, expected_b, "the two teams' tinted colors must be distinct");
    }

    /// Direct-value pin (b1-t1): the two team tint constants must be the
    /// pale-gold / pale-mint defaults, not the old saturated blue/red.
    #[test]
    fn team_colors_are_pale_gold_and_pale_mint() {
        assert_eq!(TEAM_A_COLOR, Rgba::rgb(0xff, 0xe8, 0xb0), "TEAM_A_COLOR must be pale gold");
        assert_eq!(TEAM_B_COLOR, Rgba::rgb(0xb0, 0xff, 0xe0), "TEAM_B_COLOR must be pale mint");
    }

    /// Piece::new's stored, seeded `transform` mirrors Team B (`scale.x == -1.0`)
    /// and leaves Team A unmirrored (`== 1.0`).
    #[test]
    fn piece_transform_scale_x_mirrors_team_b_only() {
        let piece_a = Piece::new(1, TEAM_A_ROW, Team::A, 0);
        let piece_b = Piece::new(1, TEAM_B_ROW, Team::B, 3);

        assert_eq!(piece_a.transform.scale.x, 1.0, "Team A stored transform unmirrored");
        assert_eq!(piece_b.transform.scale.x, -1.0, "Team B stored transform mirrored");
    }

    /// b2-t1 DELIVERABLE: `Piece::new`'s seeded `transform` field must be
    /// bit-identical to what `piece_transform` computes on the fly for the
    /// same `(col, row, team)` — proves construction-time seeding didn't
    /// diverge from the existing per-frame math. Covers both teams so the
    /// mirror (`scale.x`) is pinned for Team B too.
    #[test]
    fn piece_new_seeds_transform_from_layout_math() {
        let piece_a = Piece::new(1, TEAM_A_ROW, Team::A, 0);
        assert_eq!(
            piece_a.transform,
            piece_transform(&piece_a),
            "Team A: Piece::new's seeded transform must match piece_transform's on-the-fly math"
        );

        let piece_b = Piece::new(1, TEAM_B_ROW, Team::B, 3);
        assert_eq!(
            piece_b.transform,
            piece_transform(&piece_b),
            "Team B: Piece::new's seeded transform must match piece_transform's on-the-fly math"
        );
    }

    /// b2-t1 DELIVERABLE: `Piece::new`'s seeded `color` field must be the
    /// piece's team default (the pastel `TEAM_A_COLOR`/`TEAM_B_COLOR`
    /// constants), not some other placeholder.
    #[test]
    fn piece_new_seeds_color_from_team_default() {
        let piece_a = Piece::new(1, TEAM_A_ROW, Team::A, 0);
        assert_eq!(piece_a.color, TEAM_A_COLOR, "Team A piece must seed color = TEAM_A_COLOR");

        let piece_b = Piece::new(1, TEAM_B_ROW, Team::B, 0);
        assert_eq!(piece_b.color, TEAM_B_COLOR, "Team B piece must seed color = TEAM_B_COLOR");
    }

    /// DELIVERABLE (3): using a synthetic multi-frame `AnimatedSprite`, two
    /// pieces with different `index` at the same `elapsed` must resolve to
    /// different frame indices for at least one tested `elapsed` — proves the
    /// per-piece stagger actually desyncs idle playback.
    #[test]
    fn piece_elapsed_desyncs_frame_selection_across_indices() {
        let sprite = AnimatedSprite::new(
            vec![opaque_image(1, 1), opaque_image(1, 1), opaque_image(1, 1)],
            Duration::from_millis(20),
        );
        let elapsed = Duration::from_millis(15);

        let idx0 = sprite.frame_index_at(piece_elapsed(elapsed, 0));
        let idx5 = sprite.frame_index_at(piece_elapsed(elapsed, 5));

        assert_ne!(
            idx0, idx5,
            "pieces with index 0 vs 5 at the same elapsed time must resolve to different frames"
        );
    }

    /// b3-t2 DELIVERABLE (unit-level reinforcement): `piece_dots` must tint
    /// using the piece's own stored `color` field, not `piece.team.tint_color()`
    /// re-derived fresh. Mutating `piece.color` to a sentinel black must show
    /// up in the tinted output.
    #[test]
    fn piece_dots_reads_piece_color_field_not_team_default() {
        let sprite = AnimatedSprite::new(vec![opaque_image(6, 12)], Duration::from_millis(100));
        let geom = test_geom();

        let mut piece = Piece::new(1, TEAM_A_ROW, Team::A, 0);
        piece.color = Rgba::rgb(0, 0, 0);

        let dots = piece_dots(&piece, &sprite, Duration::ZERO, &geom);

        assert!(dots.cols() > 0 && dots.rows() > 0, "buffer must be non-empty");
        for row in 0..dots.rows() {
            for col in 0..dots.cols() {
                assert_eq!(
                    dots.get(col, row),
                    Dot::Lit(Rgba::rgb(0, 0, 0)),
                    "piece_dots must tint using the mutated piece.color field (black), not piece.team.tint_color()"
                );
            }
        }
    }

    /// DELIVERABLE (4): `sprite_base_dot_rows` is a fixed, documented ratio of
    /// `camera.scale_dots`, pinned against the `SPRITE_DOT_RATIO` constant.
    #[test]
    fn sprite_base_dot_rows_matches_ratio_constant() {
        for scale in [8.0f32, 32.0f32, 5.0f32] {
            let camera = SideView::new(scale);
            let expected = (scale * SPRITE_DOT_RATIO).round() as u32;
            assert_eq!(
                sprite_base_dot_rows(&camera),
                expected,
                "sprite_base_dot_rows must equal (scale_dots * SPRITE_DOT_RATIO).round() for scale {scale}"
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: BattleViewer scene wiring (b4-t4) — replaces fill_and_label with the
// real board + 6v6 team-tinted idle-animating pieces.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod battle_viewer_scene_wiring_tests {
    use super::*;
    use crate::scene::{EngineCtx, Scene};
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;
    use ratatui::Terminal;

    fn render_to_buffer(scene: &BattleViewer, w: u16, h: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                scene.render(f, area);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// DELIVERABLE (1): a board-line corner glyph is present at the position
    /// `board_geometry(area)` independently predicts, and is not overwritten
    /// by a piece (no piece occupies board column 0).
    #[test]
    fn board_corner_glyph_present_at_predicted_position() {
        let scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area);

        let buf = render_to_buffer(&scene, 100, 50);
        let corner = buf
            .cell((geom.board_rect.x, geom.board_rect.y))
            .expect("board_rect origin must be within the rendered buffer");
        assert_eq!(
            corner.symbol(),
            "┌",
            "top-left board corner glyph must be present at board_geometry(area)'s predicted board_rect origin"
        );
    }

    /// A cell's symbol is a braille glyph (U+2800..=U+28FF) — i.e. sprite
    /// content, not a board-line character (┌─┼ etc.) or blank background.
    fn is_braille_glyph(sym: &str) -> bool {
        sym.chars().any(|c| ('\u{2800}'..='\u{28FF}').contains(&c))
    }

    /// DELIVERABLE (2)+(3): sprite glyph cells are present in both the top
    /// half (Team A) and bottom half (Team B) of the board, and the two
    /// halves' sets of glyph colors are disjoint — proving the two teams
    /// render with genuinely distinct (multiply-blend-tinted) palettes rather
    /// than the same untinted sprite in both places. Does not assert exact
    /// RGB values, since multiply-blend against the real (non-uniform) wizard
    /// sprite doesn't average to a single flat color the way a full
    /// color-replace would have.
    #[test]
    fn team_tinted_cells_present_and_banded_by_team() {
        use std::collections::HashSet;

        let scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area);
        let mid_y = geom.board_rect.y + geom.board_rect.height / 2;

        let buf = render_to_buffer(&scene, 100, 50);

        let mut top_colors: HashSet<(u8, u8, u8)> = HashSet::new();
        let mut bottom_colors: HashSet<(u8, u8, u8)> = HashSet::new();

        for y in geom.board_rect.y..geom.board_rect.bottom() {
            for x in geom.board_rect.x..geom.board_rect.right() {
                let cell = buf.cell((x, y)).unwrap();
                if !is_braille_glyph(cell.symbol()) {
                    continue;
                }
                if let Color::Rgb(r, g, b) = cell.fg {
                    if y < mid_y {
                        top_colors.insert((r, g, b));
                    } else {
                        bottom_colors.insert((r, g, b));
                    }
                }
            }
        }

        assert!(!top_colors.is_empty(), "expected some Team A glyph color in the top half");
        assert!(!bottom_colors.is_empty(), "expected some Team B glyph color in the bottom half");
        assert!(
            top_colors.is_disjoint(&bottom_colors),
            "top-half (Team A) and bottom-half (Team B) glyph colors must not overlap: \
             top={top_colors:?} bottom={bottom_colors:?}"
        );
    }

    /// DELIVERABLE (4): idle animation actually advances — after `update()`
    /// accumulates enough elapsed time to cross a frame boundary, at least
    /// one previously-lit board cell changes.
    #[test]
    fn idle_animation_advances_after_update() {
        let mut scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area);

        let buf_before = render_to_buffer(&scene, 100, 50);

        let mut ctx = EngineCtx;
        scene.update(&mut ctx, Duration::from_millis(150));

        let buf_after = render_to_buffer(&scene, 100, 50);

        let mut changed = false;
        'outer: for y in geom.board_rect.y..geom.board_rect.bottom() {
            for x in geom.board_rect.x..geom.board_rect.right() {
                let before = buf_before.cell((x, y)).unwrap();
                let after = buf_after.cell((x, y)).unwrap();
                if before.symbol() != after.symbol() || before.fg != after.fg {
                    changed = true;
                    break 'outer;
                }
            }
        }
        assert!(
            changed,
            "at least one board cell must change after update() advances past a frame boundary"
        );
    }

    /// DELIVERABLE (5): the pre-existing registry roundtrip test is
    /// unaffected by this task's changes (regression guard, duplicated here
    /// so a red run of this file alone still proves it).
    #[test]
    fn default_battle_viewer_reports_correct_scene_id() {
        let scene = BattleViewer::default();
        assert_eq!(scene.id(), SceneId::BattleViewer);
    }

    /// b3-t2 DELIVERABLE (the point of the whole feature): mutating a stored
    /// `scene.pieces[i].color` directly must change what the very next
    /// `render()` call draws — proving `render()` reads live stored state
    /// instead of silently re-deriving `piece.team.tint_color()` fresh every
    /// frame. Every piece's color is set to a pure-black sentinel; multiply-
    /// blend by black forces every lit sprite dot to black regardless of the
    /// (non-uniform) wizard source, so the expected output is exact.
    #[test]
    fn render_reflects_mutated_stored_piece_color() {
        let mut scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area);

        for p in &mut scene.pieces {
            p.color = Rgba::rgb(0, 0, 0);
        }

        let buf = render_to_buffer(&scene, 100, 50);

        let mut found_glyph = false;
        for y in geom.board_rect.y..geom.board_rect.bottom() {
            for x in geom.board_rect.x..geom.board_rect.right() {
                let cell = buf.cell((x, y)).unwrap();
                if !is_braille_glyph(cell.symbol()) {
                    continue;
                }
                found_glyph = true;
                assert_eq!(
                    cell.fg,
                    Color::Rgb(0, 0, 0),
                    "cell ({x},{y}) must reflect the mutated stored piece.color (black), not the team default"
                );
            }
        }
        assert!(found_glyph, "expected at least one sprite glyph cell in the board");
    }

    /// b3-t1 DELIVERABLE: `BattleViewer::default().pieces` is seeded from the
    /// same layout logic as the free `pieces()` function — a real, owned
    /// field, not a divergent copy or an empty placeholder.
    #[test]
    fn default_seeds_twelve_pieces_from_layout() {
        let scene = BattleViewer::default();
        assert_eq!(scene.pieces.len(), 12, "expected 12 seeded pieces");
        assert_eq!(
            scene.pieces,
            pieces(),
            "BattleViewer::default().pieces must match the free pieces() layout"
        );
    }

    /// b4-t1 DELIVERABLE (1): `BattleViewer.elapsed` is a plain `f32` seconds
    /// accumulator, seeded to `0.0` — not a `Duration`.
    #[test]
    fn elapsed_seeds_to_zero_as_f32() {
        let scene = BattleViewer::default();
        assert_eq!(scene.elapsed, 0.0_f32);
    }

    /// b4-t1 DELIVERABLE (2): `update()` accumulates `dt` into `elapsed` as
    /// whole seconds (`dt.as_secs_f32()`), not milliseconds or any other unit
    /// — 150ms of `dt` must land at ~0.15, not ~150.0.
    #[test]
    fn elapsed_accumulates_in_seconds_via_update() {
        let mut scene = BattleViewer::default();
        let mut ctx = EngineCtx;
        scene.update(&mut ctx, Duration::from_millis(150));
        assert!(
            (scene.elapsed - 0.15).abs() < 1e-4,
            "expected elapsed ~= 0.15 seconds after a 150ms update(), got {}",
            scene.elapsed
        );
    }
}
