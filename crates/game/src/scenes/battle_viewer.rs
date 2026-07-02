use std::time::Duration;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use render::camera::{SideView, WorldPos};
use render::composite::{composite_dots, DotPlacement};
use render::dots::{dots_to_grid, tint, Dot, DotBuffer};
use render::transform::{place, rasterize, Transform, Vec2};
use render::{draw_grid, AnimatedSprite};
use scene_core::color::Rgba;
use scene_core::scene_id::SceneId;
use scene_core::Inspectable;
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

/// Draws thin braille grid lines for a `BOARD_COLS x BOARD_ROWS` grid of
/// `geom.cell_width_cols` x `geom.cell_height_rows`-sized cells, positioned at
/// `geom.board_rect`. Uses ONLY the fields of the `BoardGeometry` passed in —
/// no independent re-derivation of cell size/position. Builds a `DotBuffer`
/// sized `board_rect.width*2 x board_rect.height*4` (the same dot-sizing
/// convention the piece composite uses), lights one dot-column per vertical
/// boundary (dx=0 within its terminal cell) and one dot-row per horizontal
/// boundary (dy=0 within its terminal cell), converts via `dots_to_grid`, and
/// blits via `draw_grid` — junctions emerge purely as the bitwise union of
/// overlapping lit dots, no special-cased glyph table. Point-8 fencepost: the
/// outermost right/bottom boundary would land one dot past the last valid dot
/// index, so it is clamped to the last valid dot (one dot short) instead.
/// Cell interiors are left `Transparent` (lines only). Clips instead of
/// panicking on an undersized buffer.
pub fn draw_board_lines(buf: &mut Buffer, geom: &BoardGeometry) {
    let cw = geom.cell_width_cols;
    let chh = geom.cell_height_rows;
    let buf_cols = geom.board_rect.width as usize * 2;
    let buf_rows = geom.board_rect.height as usize * 4;
    if buf_cols == 0 || buf_rows == 0 {
        return;
    }

    let mut dots = DotBuffer::new(buf_cols, buf_rows);

    for i in 0..=BOARD_COLS {
        let dot_x = ((i * cw) as usize * 2).min(buf_cols - 1);
        for y in 0..buf_rows {
            dots.set(dot_x, y, Dot::Lit(GRID_LINE_COLOR));
        }
    }

    for j in 0..=BOARD_ROWS {
        let dot_y = ((j * chh) as usize * 4).min(buf_rows - 1);
        for x in 0..buf_cols {
            dots.set(x, dot_y, Dot::Lit(GRID_LINE_COLOR));
        }
    }

    let grid = dots_to_grid(&dots);
    draw_grid(buf, geom.board_rect, &grid);
}

/// Which side a piece belongs to. Rendering differences (tint, mirror) are
/// added by b4-t3; this enum carries only identity.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Inspectable)]
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

/// Grid-line color for board chrome (`draw_board_lines`). Single source of
/// truth referenced by both the drawing code and every test needing the
/// exact value — never re-hardcoded as a bare `0x55` literal elsewhere.
pub const GRID_LINE_COLOR: Rgba = Rgba::rgb(0x55, 0x55, 0x55);

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
/// camera — places `dots` at the piece's stored `transform.translate` world
/// position (seeded to the cell center by `Piece::new`, thereafter
/// live-editable).
pub fn place_piece<'a>(
    dots: &'a DotBuffer,
    piece: &Piece,
    geom: &BoardGeometry,
) -> DotPlacement<'a> {
    place(dots, piece.transform.translate, &geom.camera)
}

/// Uniform per-frame playback speed for the bundled wizard idle GIF. The
/// GIF's own per-frame delays are intentionally ignored by `from_gif`; this
/// constant is the single source of truth for animation speed.
const WIZARD_FRAME_DUR: Duration = Duration::from_millis(100);

// ─────────────────────────────────────────────────────────────────────────────
// Playback event data model (b1-t2). Data shape only — not yet wired into
// `BattleViewer`/`update()`/`render()` (that starts at b2-t1).
// ─────────────────────────────────────────────────────────────────────────────

/// A single playback event: a `Move` or `Die` acting on one piece, active
/// during `[start_time, start_time + duration)`. `turn` is a separate,
/// discrete grouping tag: multiple events may share the same `turn` while
/// having different `start_time`s — `turn` does not replace the clock, it
/// only labels which turn produced each event.
///
/// `EventKind::Move` carries only a destination (`to`), never a `from` — the
/// glide interpolates from wherever the piece's `Transform.translate` (or,
/// for `Die`, `Transform.scale`) actually is when the event's window opens,
/// via the existing `Tween`/`ease_in_out` utility. Remembering that starting
/// value for the duration of a multi-frame tween is transient, scene-internal
/// runtime bookkeeping (e.g. a small cache populated the frame an event's
/// window begins), not part of the authored `Event` data — the same way
/// `18-battle-viewer-baseline` keeps per-frame render state separate from the
/// data it derives from. This bookkeeping cache lives on `BattleViewer`
/// (added in b2-t1), not on `Event` itself.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Event {
    pub turn: u32,
    pub start_time: f32,
    pub duration: f32,
    pub kind: EventKind,
}

/// The kind of playback event. `piece_index` targets `Piece.index` — resolve
/// via `.iter()`/`.iter_mut().find(|p| p.index == piece_index)`, never
/// `pieces[piece_index]` — independent of `Piece.team`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EventKind {
    /// Moves the piece to `to`. Carries only a destination, no `from` field
    /// (MUST NOT gain one — the from-value is transient runtime bookkeeping,
    /// not authored data; see the doc comment on `Event` above).
    Move { piece_index: usize, to: (u16, u16) },
    /// Marks the piece dead.
    Die { piece_index: usize },
}

#[derive(Inspectable)]
pub struct BattleViewer {
    elapsed: f32,
    #[inspect(hidden)]
    sprite: AnimatedSprite,
    /// Owned piece state, seeded once from `pieces()` at construction.
    /// `render()` reads each piece's own `transform`/`color` fields directly
    /// — mutating an entry here changes what the next `render()` draws.
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

    fn inspect(&mut self) -> &mut dyn Inspectable {
        self
    }
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
    use ratatui::style::Color;

    /// Hand-picked geometry used by every case below: board_rect origin
    /// (2,1), cell_width_cols=4, cell_height_rows=2, in a 40x20 buffer.
    /// DotBuffer is board_rect.width*2 x board_rect.height*4 = 64x64 dots
    /// (8x8 dots per board cell). Vertical boundaries i=0..=8 land at
    /// terminal x in {2,6,10,14,18,22,26,30, 33(clamped)}; horizontal
    /// boundaries j=0..=8 land at terminal y in
    /// {1,3,5,7,9,11,13,15, 16(clamped)}. For every i<8/j<8 the boundary's
    /// dot sits at dx=0/dy=0 within its terminal cell (exact, no clamp); only
    /// the outermost i==8/j==8 boundary clamps to the LAST valid dot index
    /// (dx=1/dy=3 — one dot short of board_rect.right()/bottom()).
    fn geom() -> BoardGeometry {
        BoardGeometry {
            cell_width_cols: 4,
            cell_height_rows: 2,
            board_rect: Rect::new(2, 1, 32, 16),
            camera: SideView::new(8.0),
        }
    }

    /// GRID_LINE_COLOR converted to the ratatui fg representation `draw_grid`
    /// writes it as — never a duplicated `0x55` literal.
    fn grid_fg() -> Color {
        Color::Rgb(GRID_LINE_COLOR.r, GRID_LINE_COLOR.g, GRID_LINE_COLOR.b)
    }

    #[test]
    fn grid_line_color_is_the_hoisted_gray_constant() {
        assert_eq!(GRID_LINE_COLOR, Rgba::rgb(0x55, 0x55, 0x55));
    }

    /// Top-left corner: vertical boundary i=0 (left dot-column, full height,
    /// mask 0x47) union horizontal boundary j=0 (top dot-row, full width,
    /// mask 0x09) = mask 0x4F -> '\u{284F}' (⡏). No special-cased junction
    /// glyph logic — this is the emergent bitwise union.
    #[test]
    fn corner_glyph_is_union_of_vertical_and_horizontal_masks() {
        let g = geom();
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        draw_board_lines(&mut buf, &g);

        let cell = buf.cell((2, 1)).unwrap();
        assert_eq!(cell.symbol(), "\u{284F}");
        assert_eq!(cell.fg, grid_fg());
    }

    /// Interior crossing (i=1,j=1 boundary, both non-edge) collapses to the
    /// SAME union glyph as a corner — braille lines are 1-dot-thin L-joins,
    /// so every junction kind (corner/tee/cross) is indistinguishable.
    #[test]
    fn interior_crossing_collapses_to_same_union_glyph_as_corner() {
        let g = geom();
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        draw_board_lines(&mut buf, &g);

        let cell = buf.cell((6, 3)).unwrap();
        assert_eq!(cell.symbol(), "\u{284F}");
        assert_eq!(cell.fg, grid_fg());
    }

    /// A cell touched only by a vertical boundary (left dot-column lit all 4
    /// rows, no horizontal boundary through it) -> mask 0x47 -> '\u{2847}' (⡇).
    #[test]
    fn lone_vertical_run_cell_is_left_column_mask() {
        let g = geom();
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        draw_board_lines(&mut buf, &g);

        let cell = buf.cell((2, 2)).unwrap();
        assert_eq!(cell.symbol(), "\u{2847}");
        assert_eq!(cell.fg, grid_fg());
    }

    /// A cell touched only by a horizontal boundary (top dot-row lit both
    /// columns, no vertical boundary through it) -> mask 0x09 -> '\u{2809}' (⠉).
    #[test]
    fn lone_horizontal_run_cell_is_top_row_mask() {
        let g = geom();
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        draw_board_lines(&mut buf, &g);

        let cell = buf.cell((3, 1)).unwrap();
        assert_eq!(cell.symbol(), "\u{2809}");
        assert_eq!(cell.fg, grid_fg());
    }

    /// A true interior cell (no boundary anywhere in it) must stay
    /// `Cell::Transparent` — draw_grid leaves prior buffer content untouched,
    /// proving lines-only, no fill.
    #[test]
    fn cell_interior_untouched() {
        let g = geom();
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        buf.cell_mut((4, 2)).unwrap().set_char('X');
        draw_board_lines(&mut buf, &g);

        assert_eq!(buf.cell((4, 2)).unwrap().symbol(), "X");
    }

    /// Point-8 fencepost resolution (chosen: clamp to the last valid dot
    /// index, one dot short): board_rect=(2,1,32,16) -> right()=34, so the
    /// outermost vertical boundary (i=8) cannot sit at dot_x=64 (one past the
    /// last valid dot 63); it clamps to dot_x=63, landing in the RIGHT
    /// dot-column (dx=1) of the LAST valid cell (terminal x=33), not at x=34.
    /// That cell's top-right corner: right dot-column full height (mask 0xB8)
    /// union top dot-row full width (mask 0x09) = mask 0xB9 -> '\u{28B9}'.
    /// Nothing is drawn at the naive unclamped position x=34.
    #[test]
    fn far_boundary_is_clamped_one_dot_short_not_out_of_bounds() {
        let g = geom();
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        draw_board_lines(&mut buf, &g);

        let clamped = buf.cell((33, 1)).unwrap();
        assert_eq!(clamped.symbol(), "\u{28B9}");
        assert_eq!(clamped.fg, grid_fg());

        assert_eq!(
            buf.cell((34, 1)).unwrap().symbol(),
            " ",
            "nothing must be drawn one dot past board_rect's right edge"
        );
    }

    /// Drawing into a buffer smaller than board_rect must not panic — the
    /// out-of-bounds far border is silently clipped by draw_grid.
    #[test]
    fn no_panic_on_undersized_buffer() {
        let g = geom();
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 5));
        draw_board_lines(&mut buf, &g);
        // Top-left corner is still in-bounds and drawn.
        assert_eq!(buf.cell((2, 1)).unwrap().symbol(), "\u{284F}");
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
            "\u{284F}"
        );
        assert_eq!(
            buf_a
                .cell((ga.board_rect.x + ga.cell_width_cols, ga.board_rect.y))
                .unwrap()
                .symbol(),
            "\u{284F}"
        );

        let mut buf_b = Buffer::empty(area_b);
        draw_board_lines(&mut buf_b, &gb);
        assert_eq!(
            buf_b.cell((gb.board_rect.x, gb.board_rect.y)).unwrap().symbol(),
            "\u{284F}"
        );
        assert_eq!(
            buf_b
                .cell((gb.board_rect.x + gb.cell_width_cols, gb.board_rect.y))
                .unwrap()
                .symbol(),
            "\u{284F}"
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

    /// b2-t1 DELIVERABLE: `Piece::new`'s seeded `transform` field must match
    /// the hand-derived layout formula for the same `(col, row, team)`:
    /// `translate = world_pos_for_cell(col, row)`, `rotation = 0.0`, `scale =
    /// (team.scale_x(), 1.0)`. Covers both teams so the mirror (`scale.x`) is
    /// pinned for Team B too.
    #[test]
    fn piece_new_seeds_transform_from_layout_math() {
        let piece_a = Piece::new(1, TEAM_A_ROW, Team::A, 0);
        assert_eq!(
            piece_a.transform,
            Transform {
                translate: world_pos_for_cell(1, TEAM_A_ROW),
                rotation: 0.0,
                scale: Vec2::new(1.0, 1.0),
            },
            "Team A: Piece::new's seeded transform must match the hand-derived layout formula"
        );

        let piece_b = Piece::new(1, TEAM_B_ROW, Team::B, 3);
        assert_eq!(
            piece_b.transform,
            Transform {
                translate: world_pos_for_cell(1, TEAM_B_ROW),
                rotation: 0.0,
                scale: Vec2::new(-1.0, 1.0),
            },
            "Team B: Piece::new's seeded transform must match the hand-derived layout formula (mirrored)"
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

    /// b1-t1 DELIVERABLE: `Piece::new` seeds `alive = true` — a freshly
    /// constructed piece starts alive.
    #[test]
    fn piece_new_defaults_alive_true() {
        let piece = Piece::new(1, TEAM_A_ROW, Team::A, 0);
        assert!(piece.alive, "Piece::new must seed alive = true");
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
// Tests: playback event data model (b1-t2)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod event_data_model_tests {
    use super::*;

    /// SUGGESTED_TESTS: every field of a `Move` and a `Die` `Event` round-trips.
    #[test]
    fn event_move_and_die_fields_round_trip() {
        let mv = Event {
            turn: 3,
            start_time: 1.5,
            duration: 0.4,
            kind: EventKind::Move {
                piece_index: 2,
                to: (5, 6),
            },
        };
        assert_eq!(mv.turn, 3);
        assert_eq!(mv.start_time, 1.5);
        assert_eq!(mv.duration, 0.4);
        match mv.kind {
            EventKind::Move { piece_index, to } => {
                assert_eq!(piece_index, 2);
                assert_eq!(to, (5, 6));
            }
            _ => panic!("expected EventKind::Move"),
        }

        let die = Event {
            turn: 7,
            start_time: 2.0,
            duration: 0.8,
            kind: EventKind::Die { piece_index: 9 },
        };
        assert_eq!(die.turn, 7);
        assert_eq!(die.start_time, 2.0);
        assert_eq!(die.duration, 0.8);
        match die.kind {
            EventKind::Die { piece_index } => assert_eq!(piece_index, 9),
            _ => panic!("expected EventKind::Die"),
        }
    }

    /// `turn` is a separate grouping tag, independent of `start_time`: two
    /// events sharing the same `turn` but different `start_time`s must both
    /// be preserved independently (proves `turn` doesn't collapse/alias with
    /// `start_time`).
    #[test]
    fn turn_does_not_alias_start_time() {
        let e1 = Event {
            turn: 4,
            start_time: 0.1,
            duration: 0.2,
            kind: EventKind::Die { piece_index: 0 },
        };
        let e2 = Event {
            turn: 4,
            start_time: 0.9,
            duration: 0.2,
            kind: EventKind::Die { piece_index: 1 },
        };
        assert_eq!(e1.turn, e2.turn, "both events share the same turn");
        assert_ne!(
            e1.start_time, e2.start_time,
            "start_time is independent of turn and must not be aliased"
        );
    }

    /// Compile-time guard: `EventKind::Move` has EXACTLY `piece_index` and
    /// `to` — no `from` field. An exhaustive struct-pattern destructure (no
    /// `..`) fails to COMPILE the moment an extra field (e.g. `from`) is
    /// added to the variant, per the spec's explicit "MUST NOT gain a `from`
    /// field."
    #[test]
    fn move_variant_has_exactly_piece_index_and_to_no_from() {
        let kind = EventKind::Move {
            piece_index: 0,
            to: (0, 0),
        };
        let EventKind::Move { piece_index, to } = kind else {
            panic!("expected EventKind::Move");
        };
        assert_eq!(piece_index, 0);
        assert_eq!(to, (0, 0));
    }

    /// A doc comment documenting the transient from-value bookkeeping
    /// mechanism (populated the frame an event's window begins, scene-
    /// internal runtime state, not part of the authored `Event` data) must be
    /// present near the `Event`/`EventKind` declarations — grep-verifiable.
    #[test]
    fn doc_comment_documents_transient_from_value_bookkeeping() {
        let src = include_str!("battle_viewer.rs");
        let event_decl = src
            .find("pub struct Event")
            .expect("Event struct must exist in this file");
        let battle_viewer_decl = src
            .find("pub struct BattleViewer")
            .expect("BattleViewer struct must exist in this file");
        let section = &src[..battle_viewer_decl];
        assert!(
            event_decl < battle_viewer_decl,
            "Event must be declared before BattleViewer"
        );
        let lower = section.to_lowercase();
        assert!(
            lower.contains("transient") && lower.contains("bookkeeping"),
            "a doc comment on Event/EventKind must document the transient \
             from-value bookkeeping mechanism (expected the words \
             'transient' and 'bookkeeping' somewhere before BattleViewer)"
        );
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
    use render::camera::Camera;

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
        assert!(
            is_braille_glyph(corner.symbol()),
            "top-left board corner must be a braille grid-line glyph at board_geometry(area)'s \
             predicted board_rect origin, got {:?}",
            corner.symbol()
        );
        assert_eq!(
            corner.fg,
            Color::Rgb(GRID_LINE_COLOR.r, GRID_LINE_COLOR.g, GRID_LINE_COLOR.b),
            "top-left board corner must be colored GRID_LINE_COLOR"
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

        let grid_line_fg = Color::Rgb(GRID_LINE_COLOR.r, GRID_LINE_COLOR.g, GRID_LINE_COLOR.b);
        for y in geom.board_rect.y..geom.board_rect.bottom() {
            for x in geom.board_rect.x..geom.board_rect.right() {
                let cell = buf.cell((x, y)).unwrap();
                if !is_braille_glyph(cell.symbol()) {
                    continue;
                }
                if cell.fg == grid_line_fg {
                    continue; // board grid-line glyph, not piece tint
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
        let grid_line_fg = Color::Rgb(GRID_LINE_COLOR.r, GRID_LINE_COLOR.g, GRID_LINE_COLOR.b);

        let mut found_glyph = false;
        for y in geom.board_rect.y..geom.board_rect.bottom() {
            for x in geom.board_rect.x..geom.board_rect.right() {
                let cell = buf.cell((x, y)).unwrap();
                if !is_braille_glyph(cell.symbol()) {
                    continue;
                }
                if cell.fg == grid_line_fg {
                    continue; // board grid-line glyph, not a piece
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

    /// Projects a world position to the terminal cell its sprite is CENTERED
    /// on, using the same `geom.camera.project` + dot->cell conversion
    /// (`/2` cols, `/4` rows) that `place`/`dots_to_grid`/`draw_grid` use.
    fn terminal_center_cell(pos: WorldPos, geom: &BoardGeometry) -> (i32, i32) {
        let (dot_x, dot_y) = geom.camera.project(pos);
        (
            geom.board_rect.x as i32 + dot_x / 2,
            geom.board_rect.y as i32 + dot_y / 4,
        )
    }

    /// b3-t1 DELIVERABLE: `place_piece` must place a sprite at the piece's own
    /// stored, independently-editable `transform.translate`, not re-derive the
    /// position fresh from `col`/`row` every call. Mutating
    /// `pieces[0].transform.translate` to a distinct in-board world position
    /// must move the rendered sprite glyph on the very next `render()`: a
    /// glyph appears near the NEW projected cell, and none remains near the
    /// OLD col/row-derived cell.
    #[test]
    fn render_reflects_mutated_stored_piece_transform_translate() {
        let mut scene = BattleViewer::default();
        scene.pieces.truncate(1); // isolate to exactly one sprite on the board
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area);

        let old_translate = scene.pieces[0].transform.translate;
        let old_center = terminal_center_cell(old_translate, &geom);

        let new_translate = WorldPos::new(4.5, 4.5);
        assert_ne!(
            new_translate, old_translate,
            "test setup: new translate must differ from the seeded default"
        );
        scene.pieces[0].transform.translate = new_translate;
        let new_center = terminal_center_cell(new_translate, &geom);

        let buf = render_to_buffer(&scene, 100, 50);
        let grid_line_fg = Color::Rgb(GRID_LINE_COLOR.r, GRID_LINE_COLOR.g, GRID_LINE_COLOR.b);

        let has_piece_glyph_near = |center: (i32, i32)| -> bool {
            const WINDOW: i32 = 8;
            for dy in -WINDOW..=WINDOW {
                for dx in -WINDOW..=WINDOW {
                    let x = center.0 + dx;
                    let y = center.1 + dy;
                    if x < geom.board_rect.x as i32
                        || y < geom.board_rect.y as i32
                        || x >= geom.board_rect.right() as i32
                        || y >= geom.board_rect.bottom() as i32
                    {
                        continue;
                    }
                    let cell = buf.cell((x as u16, y as u16)).unwrap();
                    if is_braille_glyph(cell.symbol()) && cell.fg != grid_line_fg {
                        return true;
                    }
                }
            }
            false
        };

        assert!(
            has_piece_glyph_near(new_center),
            "expected a piece glyph near the NEW transform.translate-projected cell {new_center:?}"
        );
        assert!(
            !has_piece_glyph_near(old_center),
            "no piece glyph should remain near the OLD col/row-derived cell {old_center:?} \
             after transform.translate was mutated away from it"
        );
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

// ─────────────────────────────────────────────────────────────────────────────
// Tests: `#[derive(Inspectable)]` on Piece/Team/BattleViewer (b5-t1)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod inspectable_tests {
    use super::*;
    use scene_core::{FieldSchema, FieldTag, PatchError};

    fn field<'a>(schema: &'a FieldSchema, name: &str) -> &'a FieldSchema {
        schema
            .children
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("missing field `{name}` in schema {schema:?}"))
    }

    /// DELIVERABLE: `Piece::schema()` reports `col`/`row` as readonly `Int`,
    /// `team` as `Enum` (with Team's two variant names), `index` as a plain
    /// (non-readonly) `Int`, `transform` as a `Struct` with b3-t1's nested
    /// `translate`/`rotation`/`scale` leaves, and `color` as `Color`.
    #[test]
    fn piece_schema_reports_readonly_ints_enum_struct_and_color_fields() {
        let schema = Piece::schema();
        assert_eq!(schema.tag, FieldTag::Struct);

        let col = field(&schema, "col");
        assert_eq!(col.tag, FieldTag::Int);
        assert!(col.readonly, "col must be readonly");

        let row = field(&schema, "row");
        assert_eq!(row.tag, FieldTag::Int);
        assert!(row.readonly, "row must be readonly");

        let team = field(&schema, "team");
        assert_eq!(team.tag, FieldTag::Enum);
        assert_eq!(team.variants, vec!["A".to_string(), "B".to_string()]);

        let index = field(&schema, "index");
        assert_eq!(index.tag, FieldTag::Int);
        assert!(!index.readonly, "index must be editable, not readonly");

        let transform = field(&schema, "transform");
        assert_eq!(transform.tag, FieldTag::Struct);
        let transform_children: Vec<&str> =
            transform.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(transform_children, vec!["translate", "rotation", "scale"]);

        let color = field(&schema, "color");
        assert_eq!(color.tag, FieldTag::Color);
    }

    /// DELIVERABLE: `apply_patch` on a `Piece` value for `"team"` changes
    /// only `team` — `color`/`transform` are byte-unchanged.
    #[test]
    fn piece_apply_patch_on_team_changes_only_team() {
        let mut piece = Piece::new(1, TEAM_A_ROW, Team::A, 0);
        let before_color = piece.color;
        let before_transform = piece.transform;

        piece
            .apply_patch("team", serde_json::json!("B"))
            .expect("apply_patch on team must succeed");

        assert_eq!(piece.team, Team::B, "team must change");
        assert_eq!(piece.color, before_color, "color must be untouched by a team patch");
        assert_eq!(
            piece.transform, before_transform,
            "transform must be untouched by a team patch"
        );
    }

    /// DELIVERABLE: `apply_patch` for `"col"` returns `Err` (readonly
    /// rejection) and leaves the value unchanged.
    #[test]
    fn piece_apply_patch_on_readonly_col_is_err_and_unchanged() {
        let mut piece = Piece::new(1, TEAM_A_ROW, Team::A, 0);
        let before = piece;

        let result = piece.apply_patch("col", serde_json::json!(5));

        assert_eq!(result, Err(PatchError::Readonly));
        assert_eq!(
            piece, before,
            "piece must be byte-unchanged after a rejected readonly patch"
        );
    }

    /// DELIVERABLE: `BattleViewer::schema()` reports `elapsed` as an
    /// editable `Float` and `pieces` as a `List` of `Piece`-shaped elements;
    /// the `#[inspect(hidden)]` `sprite` field is absent entirely.
    #[test]
    fn battle_viewer_schema_reports_editable_elapsed_and_pieces_list_hides_sprite() {
        let schema = BattleViewer::schema();
        assert_eq!(schema.tag, FieldTag::Struct);

        let names: Vec<&str> = schema.children.iter().map(|c| c.name.as_str()).collect();
        assert!(
            !names.contains(&"sprite"),
            "hidden sprite field must be absent from schema: {names:?}"
        );

        let elapsed = field(&schema, "elapsed");
        assert_eq!(elapsed.tag, FieldTag::Float);
        assert!(!elapsed.readonly, "elapsed must be editable");

        let pieces = field(&schema, "pieces");
        assert_eq!(pieces.tag, FieldTag::List);
        assert_eq!(pieces.children.len(), 1, "List schema carries one element template");
        assert_eq!(
            pieces.children[0].tag,
            FieldTag::Struct,
            "pieces element template must be Piece-shaped (Struct)"
        );
    }

    /// DELIVERABLE: the fixed 12-piece layout (b4-t2) round-trips through
    /// `snapshot()` as a `pieces` array of exactly 12 Piece-shaped objects,
    /// and the hidden `sprite` field never appears in the snapshot either.
    #[test]
    fn battle_viewer_default_snapshot_has_twelve_piece_shaped_elements_and_hides_sprite() {
        let scene = BattleViewer::default();
        let snap = scene.snapshot();
        let obj = snap.as_object().expect("BattleViewer snapshot must be a JSON object");

        assert!(
            !obj.contains_key("sprite"),
            "hidden sprite field must be absent from snapshot"
        );

        let pieces = obj
            .get("pieces")
            .expect("pieces key must be present")
            .as_array()
            .expect("pieces snapshot must be an array");
        assert_eq!(pieces.len(), 12, "expected 12 seeded pieces in the snapshot");
        for p in pieces {
            let p = p.as_object().expect("each piece snapshot must be an object");
            for key in ["col", "row", "team", "index", "transform", "color"] {
                assert!(p.contains_key(key), "piece snapshot missing key `{key}`");
            }
        }
    }
}
