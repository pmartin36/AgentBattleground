use std::time::Duration;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use render::camera::{SideView, WorldPos};
use render::composite::{composite_dots, DotPlacement};
use render::dots::{dots_to_grid, dots_to_grid_tinted, tint, Dot, DotBuffer};
use render::transform::{place, Transform, Vec2};
use render::tween::Tween;
use render::{draw_grid, AnimatedSprite};
use scene_core::color::Rgba;
use scene_core::Inspectable;
use scene_core::SceneKey;
use serde_json::Value as JsonValue;

use crate::scene::{EngineCtx, InputEvent, Scene, Transition};
use crate::scene_id::SceneId;

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

/// The hand-authored demo playback sequence (b3-t1): Team A piece index 0
/// advances into the board while Team B piece index 6 dies, their windows
/// partially overlapping and sharing a `turn`, per spec 05's "hand-authored/
/// hardcoded directly in the scene" decision. Every `start_time` is `> 0.0`
/// so the elapsed==0.0 baseline is unperturbed.
pub fn demo_events() -> Vec<Event> {
    vec![
        // Team A piece (index 0) advances into the board.
        Event {
            turn: 1,
            start_time: 1.0,
            duration: 1.2,
            kind: EventKind::Move {
                piece_index: 0,
                to: (3, 3),
            },
        },
        // Team B piece (index 6) dies; window [1.6,2.6) overlaps the move's [1.0,2.2).
        Event {
            turn: 1,
            start_time: 1.6,
            duration: 1.0,
            kind: EventKind::Die { piece_index: 6 },
        },
    ]
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
/// REFINED lifetime split — `place_piece`, b4-t4, borrows from this). Thin
/// delegate over `piece_shape_and_color`'s tinted `.1` — see that function
/// for the untinted `raw` shape buffer.
pub fn piece_dots(
    piece: &Piece,
    sprite: &AnimatedSprite,
    elapsed: Duration,
    geom: &BoardGeometry,
) -> DotBuffer {
    piece_shape_and_color(piece, sprite, elapsed, geom).1
}

/// Same rasterize-once step as `piece_dots`, but returns BOTH the untinted
/// `raw` shape buffer (`.0`, real sprite RGB) and the team-tinted color
/// buffer (`.1`, `tint(&raw, piece.color)`) — order is `(shape, color)` =
/// `(untinted, tinted)`. `raw` and `tinted` always share identical dims and
/// identical Lit/Transparent topology (guaranteed by `tint`'s construction).
/// b3-t2 feeds `.0`s into the `shape` composite and `.1`s into the `color`
/// composite for `dots_to_grid_tinted`.
pub fn piece_shape_and_color(
    piece: &Piece,
    sprite: &AnimatedSprite,
    elapsed: Duration,
    geom: &BoardGeometry,
) -> (DotBuffer, DotBuffer) {
    let raw = sprite.rasterize_at(
        piece_elapsed(elapsed, piece.index),
        &piece.transform,
        sprite_base_dot_rows(&geom.camera),
    );
    let tinted = tint(&raw, piece.color);
    (raw, tinted)
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
    /// Playback event sequence (b1-t2's `Event`/`EventKind`). Hidden from the
    /// inspector: playback-internal runtime state, not part of the
    /// `Piece`-level editable surface. Seeded from `demo_events()` (b3-t1);
    /// driven each frame by `update()`/`drive_events()`.
    #[inspect(hidden)]
    pub events: Vec<Event>,
    /// Transient, scene-internal bookkeeping documented on `Event` above (NOT
    /// part of the authored `Event` data): the piece's starting
    /// `Transform.translate`/`Transform.scale` `(x, y)` captured the frame an
    /// event's window opens, keyed by `piece_index`. Populated/consumed by
    /// `update()`'s per-event driving loop (b2-t2/b2-t3) and empty otherwise.
    #[inspect(hidden)]
    pub event_from_values: std::collections::HashMap<usize, (f32, f32)>,
    /// Indices (into `events`) of events whose window has fully elapsed and
    /// already been finalized (exact landing assigned, `event_from_values`
    /// entry cleared). Distinct from `event_from_values`'s presence/absence
    /// because that alone can't tell "never started" apart from "already
    /// finalized" once its entry is removed — this set is the single source
    /// of truth `update()`'s driving loop checks to guarantee an event is
    /// only ever finalized once, so it never re-fights a later external edit
    /// (e.g. an inspector edit) to the same piece's `transform`.
    #[inspect(hidden)]
    settled_events: std::collections::HashSet<usize>,
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
            // Hand-authored demo sequence (b3-t1); driven each frame by
            // `update()`/`drive_events()`.
            events: demo_events(),
            event_from_values: std::collections::HashMap::new(),
            settled_events: std::collections::HashSet::new(),
        }
    }
}

impl BattleViewer {
    /// Drives every event whose window has begun and is not yet settled,
    /// every frame — independent of any other event, per the spec's overlap
    /// rule ("the playback clock evaluates which events are active at the
    /// current elapsed time every frame and drives every affected piece
    /// simultaneously"). Handles both `Move` (b2-t2) and `Die` (b2-t3) via
    /// the same loop shape.
    fn drive_events(&mut self) {
        for event_index in 0..self.events.len() {
            let event = self.events[event_index];
            if self.elapsed < event.start_time || self.settled_events.contains(&event_index) {
                continue;
            }
            match event.kind {
                EventKind::Move { piece_index, to } => {
                    let Some(piece) = self.pieces.iter_mut().find(|p| p.index == piece_index)
                    else {
                        continue;
                    };

                    // Gameplay truth commits instantly the frame the window opens;
                    // capture the from-value for the cosmetic tween in the same
                    // instant (transient bookkeeping, not authored `Event` data).
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        self.event_from_values.entry(piece_index)
                    {
                        entry.insert((piece.transform.translate.x, piece.transform.translate.y));
                        piece.col = to.0;
                        piece.row = to.1;
                    }

                    let target = world_pos_for_cell(to.0, to.1);
                    let end_time = event.start_time + event.duration;
                    if self.elapsed >= end_time {
                        // Exact landing — no residual Tween float drift — and settle
                        // once so a later external edit is never re-fought.
                        piece.transform.translate = target;
                        self.event_from_values.remove(&piece_index);
                        self.settled_events.insert(event_index);
                    } else {
                        let (from_x, from_y) = self.event_from_values[&piece_index];
                        let since_start =
                            Duration::from_secs_f32((self.elapsed - event.start_time).max(0.0));
                        let dur = Duration::from_secs_f32(event.duration);
                        let x = Tween::new(from_x, target.x, dur).at(since_start);
                        let y = Tween::new(from_y, target.y, dur).at(since_start);
                        piece.transform.translate = WorldPos::new(x, y);
                    }
                }
                EventKind::Die { piece_index } => {
                    let Some(piece) = self.pieces.iter_mut().find(|p| p.index == piece_index)
                    else {
                        continue;
                    };

                    // First frame the window is open: capture starting scale
                    // (no col/row commit — Die does not move the piece).
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        self.event_from_values.entry(piece_index)
                    {
                        entry.insert((piece.transform.scale.x, piece.transform.scale.y));
                    }

                    let end_time = event.start_time + event.duration;
                    if self.elapsed >= end_time {
                        // Exact landing — no residual Tween float drift — and settle
                        // once so a later external edit (e.g. a revive) is never
                        // re-fought.
                        piece.transform.scale = Vec2::splat(0.0);
                        piece.alive = false;
                        self.event_from_values.remove(&piece_index);
                        self.settled_events.insert(event_index);
                    } else {
                        let (from_x, from_y) = self.event_from_values[&piece_index];
                        let since_start =
                            Duration::from_secs_f32((self.elapsed - event.start_time).max(0.0));
                        let dur = Duration::from_secs_f32(event.duration);
                        let x = Tween::new(from_x, 0.0, dur).at(since_start);
                        let y = Tween::new(from_y, 0.0, dur).at(since_start);
                        piece.transform.scale = Vec2::new(x, y);
                    }
                }
            }
        }
    }
}

impl Scene for BattleViewer {
    fn id(&self) -> SceneKey {
        SceneId::BattleViewer.into()
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}

    fn update(&mut self, _ctx: &mut EngineCtx, dt: Duration) -> Option<Transition> {
        self.elapsed += dt.as_secs_f32();
        self.drive_events();
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let geom = board_geometry(area);
        draw_board_lines(frame.buffer_mut(), &geom);

        let elapsed = Duration::from_secs_f32(self.elapsed);
        let alive: Vec<&Piece> = self.pieces.iter().filter(|p| p.alive).collect();
        // One rasterize per piece -> (raw untinted shape buffer, team-tinted color
        // buffer). Compositing the raw set and the tinted set at identical placements
        // yields two buffers with identical Lit/Transparent topology, differing only
        // in RGB, so the glyph mask stays invariant to tint (dots_to_grid_tinted).
        let pairs: Vec<(DotBuffer, DotBuffer)> = alive
            .iter()
            .map(|&p| piece_shape_and_color(p, &self.sprite, elapsed, &geom))
            .collect();
        let shape_placements: Vec<DotPlacement> = alive
            .iter()
            .zip(&pairs)
            .map(|(&p, pair)| place_piece(&pair.0, p, &geom))
            .collect();
        let color_placements: Vec<DotPlacement> = alive
            .iter()
            .zip(&pairs)
            .map(|(&p, pair)| place_piece(&pair.1, p, &geom))
            .collect();

        let w = (geom.board_rect.width * 2) as usize;
        let h = (geom.board_rect.height * 4) as usize;
        let shape = composite_dots(w, h, &shape_placements);
        let color = composite_dots(w, h, &color_placements);
        let grid = dots_to_grid_tinted(&shape, &color);
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

    /// b3-t1 DELIVERABLE: `piece_shape_and_color`'s untinted `.0` carries the
    /// raw source RGB (not multiplied by `piece.color`), and its tinted `.1`
    /// equals the hand-derived per-team multiply-blend (same expected values
    /// as `piece_dots_tints_each_team_distinctly_via_multiply_blend`).
    #[test]
    fn piece_shape_and_color_untinted_carries_raw_source_rgb() {
        let sprite = AnimatedSprite::new(vec![opaque_image(6, 12)], Duration::from_millis(100));
        let geom = test_geom();
        let piece_a = Piece::new(1, TEAM_A_ROW, Team::A, 0);

        let (raw, tinted) = piece_shape_and_color(&piece_a, &sprite, Duration::ZERO, &geom);

        // TEAM_A_COLOR = (255,232,176): 200*255/255=200, 200*232/255=181, 200*176/255=138
        let expected_tinted = Rgba::rgb(200, 181, 138);

        assert!(raw.cols() > 0 && raw.rows() > 0, "raw buffer must be non-empty");
        for row in 0..raw.rows() {
            for col in 0..raw.cols() {
                assert_eq!(
                    raw.get(col, row),
                    Dot::Lit(Rgba::rgb(200, 200, 200)),
                    "raw dot ({col},{row}) must carry the source RGB, not the team tint"
                );
                assert_eq!(
                    tinted.get(col, row),
                    Dot::Lit(expected_tinted),
                    "tinted dot ({col},{row}) must equal the multiply-blend of raw with piece.color"
                );
            }
        }
    }

    /// b3-t1 DELIVERABLE: `raw`/`tinted` share identical dims and identical
    /// Lit/Transparent topology at every dot (mask parity) — no dot may be
    /// Lit in one buffer and Transparent in the other.
    #[test]
    fn piece_shape_and_color_topology_parity() {
        let sprite = AnimatedSprite::new(vec![opaque_image(6, 12)], Duration::from_millis(100));
        let geom = test_geom();
        let piece_b = Piece::new(1, TEAM_B_ROW, Team::B, 0);

        let (raw, tinted) = piece_shape_and_color(&piece_b, &sprite, Duration::ZERO, &geom);

        assert_eq!(raw.cols(), tinted.cols(), "raw/tinted must share identical column count");
        assert_eq!(raw.rows(), tinted.rows(), "raw/tinted must share identical row count");

        for row in 0..raw.rows() {
            for col in 0..raw.cols() {
                let raw_transparent = matches!(raw.get(col, row), Dot::Transparent);
                let tinted_transparent = matches!(tinted.get(col, row), Dot::Transparent);
                assert_eq!(
                    raw_transparent, tinted_transparent,
                    "dot ({col},{row}) must be Transparent in both buffers or neither"
                );
            }
        }
    }

    /// b3-t1 DELIVERABLE (delegation pin): `piece_shape_and_color(...).1` must
    /// equal `piece_dots(...)` for the same inputs — the pair producer's
    /// tinted half is not a separate, divergent computation.
    #[test]
    fn piece_shape_and_color_tinted_matches_piece_dots() {
        let sprite = AnimatedSprite::new(vec![opaque_image(6, 12)], Duration::from_millis(100));
        let geom = test_geom();
        let piece = Piece::new(1, TEAM_A_ROW, Team::A, 0);

        let expected = piece_dots(&piece, &sprite, Duration::ZERO, &geom);
        let (_, tinted) = piece_shape_and_color(&piece, &sprite, Duration::ZERO, &geom);

        assert_eq!(tinted.cols(), expected.cols());
        assert_eq!(tinted.rows(), expected.rows());
        for row in 0..expected.rows() {
            for col in 0..expected.cols() {
                assert_eq!(
                    tinted.get(col, row),
                    expected.get(col, row),
                    "tinted dot ({col},{row}) must match piece_dots' output"
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
// Tests: BattleViewer.events / event_from_values wiring (b2-t1) — fields
// exist and default empty; update()/render() are not yet touched.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod event_playback_wiring_tests {
    use super::*;

    /// DELIVERABLE (b3-t1): `BattleViewer::default()` carries a hand-authored
    /// demo event sequence — at least one `Move` and one `Die` — per the
    /// spec's "hand-authored/hardcoded directly in the scene" decision.
    #[test]
    fn default_events_contains_a_move_and_a_die() {
        let scene = BattleViewer::default();
        let has_move = scene
            .events
            .iter()
            .any(|e| matches!(e.kind, EventKind::Move { .. }));
        let has_die = scene
            .events
            .iter()
            .any(|e| matches!(e.kind, EventKind::Die { .. }));
        assert!(
            has_move,
            "default().events must contain at least one Move event, got {:?}",
            scene.events
        );
        assert!(
            has_die,
            "default().events must contain at least one Die event, got {:?}",
            scene.events
        );
    }

    /// DELIVERABLE: no authored event's `start_time` may be `<= 0.0` — this
    /// protects every elapsed==0.0 baseline test from perturbation now that
    /// real demo content is wired in.
    #[test]
    fn default_events_all_start_after_zero() {
        let scene = BattleViewer::default();
        assert!(
            !scene.events.is_empty(),
            "default().events must be non-empty once the demo sequence is authored"
        );
        for (i, e) in scene.events.iter().enumerate() {
            assert!(
                e.start_time > 0.0,
                "event[{i}] start_time must be > 0.0 to preserve the elapsed==0.0 baseline, \
                 got {}",
                e.start_time
            );
        }
    }

    /// DELIVERABLE: the authored Move and Die windows partially overlap,
    /// exercising the shipped overlap-handling path (b2-t5) in real demo
    /// data, per this task's own "at least partially overlapping" instruction.
    #[test]
    fn default_events_move_and_die_windows_partially_overlap() {
        let scene = BattleViewer::default();
        let move_event = scene
            .events
            .iter()
            .find(|e| matches!(e.kind, EventKind::Move { .. }))
            .expect("a Move event must be present");
        let die_event = scene
            .events
            .iter()
            .find(|e| matches!(e.kind, EventKind::Die { .. }))
            .expect("a Die event must be present");

        let move_end = move_event.start_time + move_event.duration;
        let die_end = die_event.start_time + die_event.duration;
        let overlap_start = move_event.start_time.max(die_event.start_time);
        let overlap_end = move_end.min(die_end);
        assert!(
            overlap_start < overlap_end,
            "Move [{}, {}) and Die [{}, {}) windows must partially overlap",
            move_event.start_time,
            move_end,
            die_event.start_time,
            die_end
        );
    }

    /// DELIVERABLE: multiple events may legitimately share a `turn` while
    /// having different `start_time`s — the authored Move and Die share a
    /// `turn` tag, demonstrating `turn` does not replace the clock.
    #[test]
    fn default_events_move_and_die_share_a_turn() {
        let scene = BattleViewer::default();
        let move_turn = scene
            .events
            .iter()
            .find(|e| matches!(e.kind, EventKind::Move { .. }))
            .expect("a Move event must be present")
            .turn;
        let die_turn = scene
            .events
            .iter()
            .find(|e| matches!(e.kind, EventKind::Die { .. }))
            .expect("a Die event must be present")
            .turn;
        assert_eq!(
            move_turn, die_turn,
            "authored Move and Die events should share a turn tag while differing in \
             start_time"
        );
    }

    /// DELIVERABLE: the transient from-value bookkeeping cache (documented on
    /// `Event` above) starts empty — nothing has captured a starting
    /// translate/scale before any event has begun driving.
    #[test]
    fn default_event_from_values_is_empty() {
        let scene = BattleViewer::default();
        assert!(
            scene.event_from_values.is_empty(),
            "BattleViewer::default().event_from_values must start empty, got {:?}",
            scene.event_from_values
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: `Move` event driving in `update()` (b2-t2) — instant col/row commit,
// cosmetic transform.translate lerp via Tween/ease_in_out, exact landing,
// settle-once (does not re-fight an externally mutated transform.translate
// after the event has completed).
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod move_event_driving_tests {
    use super::*;
    use crate::scene::EngineCtx;

    /// Builds a fresh default scene with piece 0's only playback event: a
    /// `Move` from its seeded start cell to `to`, active on
    /// `[start_time, start_time + duration)`.
    fn scene_with_single_move(start_time: f32, duration: f32, to: (u16, u16)) -> BattleViewer {
        BattleViewer {
            events: vec![Event {
                turn: 1,
                start_time,
                duration,
                kind: EventKind::Move {
                    piece_index: 0,
                    to,
                },
            }],
            ..BattleViewer::default()
        }
    }

    /// DELIVERABLE (3): `col`/`row` commit to `to` in the SAME instant the
    /// event's window opens (`elapsed >= start_time`), not before.
    #[test]
    fn move_col_row_commits_instantly_at_window_open_not_before() {
        let mut ctx = EngineCtx;

        let mut before = scene_with_single_move(1.0, 1.0, (5, 0));
        before.update(&mut ctx, Duration::from_secs_f32(0.999));
        assert_eq!(
            (before.pieces[0].col, before.pieces[0].row),
            (1, 0),
            "col/row must NOT yet commit to `to` before start_time"
        );

        let mut at_open = scene_with_single_move(1.0, 1.0, (5, 0));
        at_open.update(&mut ctx, Duration::from_secs_f32(1.0));
        assert_eq!(
            (at_open.pieces[0].col, at_open.pieces[0].row),
            (5, 0),
            "col/row must commit to `to` the instant elapsed reaches start_time"
        );
    }

    /// DELIVERABLE (1): strictly between `start_time` and
    /// `start_time + duration`, `transform.translate` is mid-glide — neither
    /// the old cell's world position nor the new cell's world position.
    #[test]
    fn move_transform_translate_strictly_between_endpoints_mid_tween() {
        let mut ctx = EngineCtx;
        let mut scene = scene_with_single_move(1.0, 1.0, (5, 0));
        let from = world_pos_for_cell(1, 0);
        let to = world_pos_for_cell(5, 0);

        scene.update(&mut ctx, Duration::from_secs_f32(1.5));
        let mid = scene.pieces[0].transform.translate;

        assert_eq!(mid.y, from.y, "row is unchanged, y must not move");
        assert!(
            mid.x > from.x.min(to.x) && mid.x < from.x.max(to.x),
            "mid-tween translate.x ({}) must be strictly between the start ({}) and end ({}) x",
            mid.x,
            from.x,
            to.x
        );
    }

    /// DELIVERABLE (2): at/after `start_time + duration`, `transform.translate`
    /// lands EXACTLY on `world_pos_for_cell(to)` — no residual Tween float
    /// drift.
    #[test]
    fn move_transform_translate_lands_exactly_at_target_after_duration() {
        let mut ctx = EngineCtx;
        let mut scene = scene_with_single_move(1.0, 1.0, (5, 0));

        scene.update(&mut ctx, Duration::from_secs_f32(2.0));

        assert_eq!(
            scene.pieces[0].transform.translate,
            world_pos_for_cell(5, 0),
            "transform.translate must land exactly on the target cell's center once the \
             event's window has fully elapsed"
        );
    }

    /// DELIVERABLE (4) settle regression: once the `Move` event has fully
    /// completed, an externally-mutated `transform.translate` (e.g. an
    /// inspector edit) must NOT be re-derived/overwritten by a later
    /// `update()` call for the same already-settled event.
    #[test]
    fn move_settled_event_does_not_refight_externally_mutated_translate() {
        let mut ctx = EngineCtx;
        let mut scene = scene_with_single_move(1.0, 1.0, (5, 0));

        // Complete the event.
        scene.update(&mut ctx, Duration::from_secs_f32(2.0));
        assert_eq!(
            scene.pieces[0].transform.translate,
            world_pos_for_cell(5, 0),
            "test setup: event must have landed exactly before the external-edit step"
        );

        // Simulate an external (e.g. inspector) edit after settling.
        let external = WorldPos::new(9.25, 9.25);
        scene.pieces[0].transform.translate = external;

        // Further updates must not touch the already-settled event's piece.
        scene.update(&mut ctx, Duration::from_secs_f32(1.0));
        assert_eq!(
            scene.pieces[0].transform.translate, external,
            "an already-settled Move event must not overwrite a later external edit to \
             transform.translate"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: Die event driving in update() (b2-t3) — scale-to-zero lerp, `alive`
// flip, settle-once (does not re-fight an externally revived `alive` after
// the event has completed).
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod die_event_driving_tests {
    use super::*;
    use crate::scene::EngineCtx;

    /// Builds a fresh default scene with piece 0's only playback event: a
    /// `Die`, active on `[start_time, start_time + duration)`.
    fn scene_with_single_die(start_time: f32, duration: f32) -> BattleViewer {
        BattleViewer {
            events: vec![Event {
                turn: 1,
                start_time,
                duration,
                kind: EventKind::Die { piece_index: 0 },
            }],
            ..BattleViewer::default()
        }
    }

    /// DELIVERABLE (1): sampling `transform.scale`'s magnitude at several
    /// strictly-increasing elapsed times within the event's active window
    /// shows strictly decreasing magnitude (progressive shrink, not a jump to
    /// zero).
    #[test]
    fn die_scale_magnitude_strictly_decreases_within_window() {
        let mut ctx = EngineCtx;

        let mut prev_mag = f32::MAX;
        for t in [1.25_f32, 1.5, 1.75] {
            // Fresh scene per sample: the event is re-driven from t=0 each
            // time so each sample reflects only elapsed time `t`, not
            // accumulated per-frame drift.
            let mut probe = scene_with_single_die(1.0, 1.0);
            probe.update(&mut ctx, Duration::from_secs_f32(t));
            let s = probe.pieces[0].transform.scale;
            let mag = (s.x * s.x + s.y * s.y).sqrt();
            assert!(
                mag < prev_mag,
                "scale magnitude at t={t} ({mag}) must be strictly less than the previous \
                 sample ({prev_mag})"
            );
            prev_mag = mag;
        }
    }

    /// DELIVERABLE (2): `alive` is `true` up to just before
    /// `start_time + duration`, and exactly `false` (with `scale` snapped to
    /// zero) at/after it.
    #[test]
    fn die_alive_flips_false_exactly_at_completion() {
        let mut ctx = EngineCtx;

        let mut before = scene_with_single_die(1.0, 1.0);
        before.update(&mut ctx, Duration::from_secs_f32(1.999));
        assert!(
            before.pieces[0].alive,
            "alive must still be true strictly before start_time + duration"
        );

        let mut at_complete = scene_with_single_die(1.0, 1.0);
        at_complete.update(&mut ctx, Duration::from_secs_f32(2.0));
        assert!(
            !at_complete.pieces[0].alive,
            "alive must be false the instant elapsed reaches start_time + duration"
        );
        assert_eq!(
            at_complete.pieces[0].transform.scale,
            Vec2::splat(0.0),
            "transform.scale must land exactly on zero once the event's window has fully \
             elapsed"
        );
    }

    /// DELIVERABLE (3) settle regression: once the `Die` event has fully
    /// completed (`alive == false`), an externally-revived `alive` (the
    /// spec's named hypothetical revive mechanic) must NOT be re-flipped back
    /// to `false` by a later `update()` call for the same already-settled
    /// event.
    #[test]
    fn die_settled_event_does_not_refight_external_revive() {
        let mut ctx = EngineCtx;
        let mut scene = scene_with_single_die(1.0, 1.0);

        // Complete the event.
        scene.update(&mut ctx, Duration::from_secs_f32(2.0));
        assert!(
            !scene.pieces[0].alive,
            "test setup: event must have settled (alive == false) before the revive step"
        );

        // Simulate an external revive after settling.
        scene.pieces[0].alive = true;

        // Further updates must not touch the already-settled event's piece.
        scene.update(&mut ctx, Duration::from_secs_f32(1.0));
        assert!(
            scene.pieces[0].alive,
            "an already-settled Die event must not re-flip an externally revived `alive` \
             back to false"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: overlapping/simultaneous multi-piece events in one frame (b2-t5) —
// proves the spec's "Events may overlap in time... drives every affected
// piece simultaneously" bullet: a single update() landing two different
// events (on two different pieces) mid-flight drives BOTH independently, with
// no leakage between them.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod overlapping_events_tests {
    use super::*;
    use crate::scene::EngineCtx;

    /// Builds a scene with two simultaneous, independent events on different
    /// pieces: piece 0 (Team A) `Move`s to `(5, 0)`, piece 6 (Team B) `Die`s —
    /// both active on the same `[start_time, start_time + duration)` window.
    fn scene_with_overlapping_move_and_die(start_time: f32, duration: f32) -> BattleViewer {
        BattleViewer {
            events: vec![
                Event {
                    turn: 1,
                    start_time,
                    duration,
                    kind: EventKind::Move {
                        piece_index: 0,
                        to: (5, 0),
                    },
                },
                Event {
                    turn: 1,
                    start_time,
                    duration,
                    kind: EventKind::Die { piece_index: 6 },
                },
            ],
            ..BattleViewer::default()
        }
    }

    /// DELIVERABLE: a single `update()` landing both events' windows
    /// mid-flight drives both target pieces to correct, independent partial
    /// progress — neither stuck at its start state, neither jumped to its end
    /// state — and neither event leaks into the other piece.
    #[test]
    fn overlapping_move_and_die_on_different_pieces_both_progress_independently_mid_flight() {
        let mut ctx = EngineCtx;
        let mut scene = scene_with_overlapping_move_and_die(1.0, 1.0);

        let move_from = world_pos_for_cell(1, 0);
        let move_to = world_pos_for_cell(5, 0);
        let die_start_scale = scene.pieces[6].transform.scale;
        let die_start_translate = scene.pieces[6].transform.translate;
        let move_start_scale = scene.pieces[0].transform.scale;

        scene.update(&mut ctx, Duration::from_secs_f32(1.5));

        // (a) Move piece (0): col/row committed instantly, translate mid-glide.
        assert_eq!(
            (scene.pieces[0].col, scene.pieces[0].row),
            (5, 0),
            "Move piece's col/row must already be committed to `to` mid-flight"
        );
        let move_x = scene.pieces[0].transform.translate.x;
        assert!(
            move_x > move_from.x.min(move_to.x) && move_x < move_from.x.max(move_to.x),
            "Move piece's translate.x ({move_x}) must be strictly between start ({}) and end ({}) \
             x while the Die event is simultaneously active",
            move_from.x,
            move_to.x
        );

        // (b) Die piece (6): still alive, scale shrinking but not yet zero.
        assert!(
            scene.pieces[6].alive,
            "Die piece must still be alive mid-flight, while the Move event is simultaneously \
             active"
        );
        let die_scale = scene.pieces[6].transform.scale;
        let die_mag = (die_scale.x * die_scale.x + die_scale.y * die_scale.y).sqrt();
        let start_mag = (die_start_scale.x * die_start_scale.x
            + die_start_scale.y * die_start_scale.y)
            .sqrt();
        assert!(
            die_mag > 0.0 && die_mag < start_mag,
            "Die piece's scale magnitude ({die_mag}) must be strictly between 0 and its starting \
             magnitude ({start_mag}) mid-flight"
        );

        // (c) Cross-independence: neither event leaks into the other piece.
        assert_eq!(
            scene.pieces[0].transform.scale, move_start_scale,
            "the Move piece's scale must be untouched by the simultaneously-active Die event"
        );
        assert_eq!(
            (scene.pieces[6].col, scene.pieces[6].row),
            (1, TEAM_B_ROW),
            "the Die piece's col/row must be untouched by the simultaneously-active Move event"
        );
        assert_eq!(
            scene.pieces[6].transform.translate, die_start_translate,
            "the Die piece's translate must be untouched by the simultaneously-active Move event"
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
    use crate::scenes::test_util::render_to_buffer;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;
    use render::camera::Camera;

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
        assert_eq!(scene.id(), SceneKey::from(SceneId::BattleViewer));
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

    /// b3-t2 DELIVERABLE: the tint-shape-invariance bug. `render()`'s glyph
    /// mask (braille `symbol`) must be decided from the untinted sprite shape
    /// alone — changing ONLY a piece's `color` (team tint), with its
    /// transform/frame/placement held fixed, must never move the mask. Holds
    /// the SAME piece fixed (so `Team::scale_x`'s mirror never enters) and
    /// varies just `piece.color` between TEAM_A_COLOR and TEAM_B_COLOR — the
    /// spec's own "team-A/team-B" verification technique, isolated to a
    /// single piece so only the tint differs between the two renders.
    #[test]
    fn render_glyph_mask_invariant_to_tint() {
        let mut scene = BattleViewer::default();
        scene.pieces.truncate(1);
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area);

        let buf_a = render_to_buffer(&scene, 100, 50);

        scene.pieces[0].color = TEAM_B_COLOR;
        let buf_b = render_to_buffer(&scene, 100, 50);

        let grid_line_fg = Color::Rgb(GRID_LINE_COLOR.r, GRID_LINE_COLOR.g, GRID_LINE_COLOR.b);
        let mut color_diff_found = false;
        for y in geom.board_rect.y..geom.board_rect.bottom() {
            for x in geom.board_rect.x..geom.board_rect.right() {
                let cell_a = buf_a.cell((x, y)).unwrap();
                let cell_b = buf_b.cell((x, y)).unwrap();
                assert_eq!(
                    cell_a.symbol(),
                    cell_b.symbol(),
                    "cell ({x},{y}) glyph must be invariant to a tint-only piece.color change"
                );
                if cell_a.fg == grid_line_fg {
                    continue; // board grid-line glyph, not piece tint
                }
                if is_braille_glyph(cell_a.symbol()) && cell_a.fg != cell_b.fg {
                    color_diff_found = true;
                }
            }
        }
        assert!(
            color_diff_found,
            "expected at least one piece glyph cell's color to differ between the two tints"
        );
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

    /// Scans a `WINDOW`-cell box around `center` for any non-grid-line
    /// braille glyph — the same piece-glyph-presence probe used by
    /// `render_reflects_mutated_stored_piece_transform_translate`.
    fn has_piece_glyph_near(buf: &Buffer, geom: &BoardGeometry, center: (i32, i32)) -> bool {
        let grid_line_fg = Color::Rgb(GRID_LINE_COLOR.r, GRID_LINE_COLOR.g, GRID_LINE_COLOR.b);
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
    }

    /// b2-t4 DELIVERABLE: a piece with `alive == false` contributes NO glyph
    /// to the composited render, while a still-alive, spatially-disjoint
    /// sibling's glyphs remain present. `transform` is left intact on the
    /// dead piece (not driven through a real `Die` event) so the ONLY reason
    /// it can vanish is `render()`'s new `alive` filter, not a collapsed
    /// zero scale.
    #[test]
    fn render_excludes_dead_piece_keeps_alive_sibling() {
        let mut scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area);

        let target_center = terminal_center_cell(scene.pieces[0].transform.translate, &geom);
        let sibling_center = terminal_center_cell(scene.pieces[6].transform.translate, &geom);
        assert_eq!(scene.pieces[0].team, Team::A, "test setup: target must be Team A");
        assert_eq!(scene.pieces[6].team, Team::B, "test setup: sibling must be Team B");

        scene.pieces[0].alive = false;

        let buf = render_to_buffer(&scene, 100, 50);

        assert!(
            !has_piece_glyph_near(&buf, &geom, target_center),
            "no piece glyph should remain near a dead piece's center {target_center:?}"
        );
        assert!(
            has_piece_glyph_near(&buf, &geom, sibling_center),
            "a still-alive sibling's glyphs must remain present near {sibling_center:?}"
        );
    }

    /// b2-t4 DELIVERABLE (revive, no special-casing): flipping a previously
    /// excluded piece's `alive` back to `true` (transform untouched) makes
    /// its glyphs reappear on the very next `render()`, with no other code
    /// change — proving exclusion is a pure per-frame filter on `alive`, not
    /// a one-way/sticky removal.
    #[test]
    fn render_reincludes_piece_when_alive_flipped_back_true() {
        let mut scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area);

        let target_center = terminal_center_cell(scene.pieces[0].transform.translate, &geom);

        scene.pieces[0].alive = false;
        let buf_dead = render_to_buffer(&scene, 100, 50);
        assert!(
            !has_piece_glyph_near(&buf_dead, &geom, target_center),
            "test setup: piece must be excluded while alive == false"
        );

        scene.pieces[0].alive = true;
        let buf_revived = render_to_buffer(&scene, 100, 50);
        assert!(
            has_piece_glyph_near(&buf_revived, &geom, target_center),
            "piece glyph must reappear near {target_center:?} once alive is flipped back to true"
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
