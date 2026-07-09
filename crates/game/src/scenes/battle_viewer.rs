use std::time::Duration;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use engine_render::camera::{Camera, DepthAxis, OrthographicCamera, PerspectiveCamera, WorldPos};
use engine_render::composite::{composite_scene, SpriteContent, SpriteDraw};
use engine_render::dots::{dots_to_grid, Dot, DotBuffer};
use engine_render::transform::{Transform, Vec2, VerticalAnchor};
use engine_render::tween::Tween;
use engine_render::{draw_grid, rasterize_shape, AnimatedSprite, ShapeKind};
use engine_core::color::Rgba;
use engine_core::Inspectable;
use engine_core::SceneKey;
use serde_json::Value as JsonValue;

use engine_core::scene::{EngineCtx, InputEvent, Scene, Transition};
use crate::creatures::{AnimationKind, Creature};
use crate::scene_id::SceneId;

/// Single source of truth for the board's column count. Every downstream
/// consumer must reference this constant, never a bare literal `8`.
pub const BOARD_COLS: u16 = 7;
/// Single source of truth for the board's row count.
pub const BOARD_ROWS: u16 = 7;

/// World-x the Sideline camera anchors on — the board's horizontal center
/// column, derived from `BOARD_COLS` (never a bare literal `3.5`).
const BOARD_CENTER_COL: f32 = BOARD_COLS as f32 / 2.0;

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
    pub camera: BattleCamera,
    /// Live tuning this geometry was built from (b4-t2: threaded to
    /// `BattleCamera::grid_line_color` at its sole caller, `draw_board_lines`,
    /// without churning `draw_board_lines`'s own signature).
    pub tuning: BattleViewerTuning,
    /// Screen-space dot offset applied on top of `camera`'s own projection
    /// so the fitted board bbox lands at the composite buffer's origin
    /// (b4-t1). Always `(0, 0)` for `TopDown` (its exact flat path is
    /// unchanged); fit-derived for `Sideline`/`OverShoulder`.
    pub screen_offset: (i32, i32),
}

/// Wraps a `BattleCamera` with a screen-space dot offset (b4-t1): every
/// `project` result is shifted by `offset`, `depth_key` passes through
/// unchanged (the offset never reorders draws). Grid lines and sprites both
/// go through this so the fit-to-viewport offset (`BoardGeometry::screen_offset`)
/// is inherited automatically rather than re-applied per call site.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FramedCamera {
    pub camera: BattleCamera,
    pub offset: (i32, i32),
}

impl Camera for FramedCamera {
    fn project(&self, pos: WorldPos) -> (i32, i32) {
        let (x, y) = self.camera.project(pos);
        (x + self.offset.0, y + self.offset.1)
    }

    fn depth_key(&self, pos: WorldPos) -> i32 {
        self.camera.depth_key(pos)
    }

    fn local_dots_per_world_unit(&self, pos: WorldPos) -> f32 {
        self.camera.local_dots_per_world_unit(pos)
    }
}

impl BoardGeometry {
    /// The camera to project through for on-screen output (grid lines AND
    /// sprites) — bakes in `screen_offset` so callers never need to apply it
    /// separately.
    pub fn framed_camera(&self) -> FramedCamera {
        FramedCamera {
            camera: self.camera,
            offset: self.screen_offset,
        }
    }
}

/// Derive the board geometry for a given render `area` under the given
/// camera `mode`. Total (never panics) and deterministic: picks the largest
/// integer `cell_height_rows` such that the board fits `area`, clamped to a
/// minimum of 1. `mode` selects the active camera variant; its dot scale is
/// always rebuilt from the area-derived scale (see `BattleCamera::with_scale_dots`),
/// not taken from whatever scale the caller's `mode` happened to carry in.
pub fn board_geometry(area: Rect, mode: BattleCamera, tuning: BattleViewerTuning) -> BoardGeometry {
    match mode {
        BattleCamera::TopDown(_) => {
            // `BENCH_COL == BOARD_COLS`: bench sits one column past the
            // drawn grid's own 7 columns (deliberately outside it — see
            // `BENCH_COL`'s doc comment), so the allocated board_rect needs
            // room for `BOARD_COLS + 1` columns' worth of width, not just
            // the 7 the grid itself draws — otherwise bench renders past
            // the buffer's own right edge and gets silently clipped.
            let total_cols = BOARD_COLS + 1;
            let cell_height_rows = (area.width / (2 * total_cols))
                .min(area.height / BOARD_ROWS)
                .max(1);
            let cell_width_cols = 2 * cell_height_rows;

            let w = cell_width_cols * total_cols;
            let bh = cell_height_rows * BOARD_ROWS;
            let bx = area.left() + area.width.saturating_sub(w) / 2;
            let by = area.top() + area.height.saturating_sub(bh) / 2;
            let board_rect = Rect::new(bx, by, w, bh);

            let camera = mode.with_scale_dots((cell_height_rows * 4) as f32);

            BoardGeometry {
                cell_width_cols,
                cell_height_rows,
                board_rect,
                camera,
                tuning,
                screen_offset: (0, 0),
            }
        }
        BattleCamera::Sideline(_) | BattleCamera::OverShoulder(_) => {
            fit_perspective_geometry(area, mode, tuning)
        }
    }
}

/// The board's 4 flat corners PLUS both teams' bench positions (`BENCH_COL`
/// sits one column past the drawn grid — see its doc comment), in a fixed
/// order — reused by both the reference and fitted projections in
/// `fit_perspective_geometry` (never a bare-literal corner list elsewhere).
/// Including bench here means the fit itself accounts for exactly how far
/// bench actually extends past the flat grid, on whichever screen axis that
/// camera maps it to — not a blanket margin guessing at it.
fn board_world_corners() -> [WorldPos; 6] {
    [
        WorldPos::new(0.0, 0.0),
        WorldPos::new(BOARD_COLS as f32, 0.0),
        WorldPos::new(0.0, BOARD_ROWS as f32),
        WorldPos::new(BOARD_COLS as f32, BOARD_ROWS as f32),
        world_pos_for_cell(BENCH_COL, TEAM_A_BENCH_ROW),
        world_pos_for_cell(BENCH_COL, TEAM_B_BENCH_ROW),
    ]
}

/// Axis-aligned dot bbox (`min_x, max_x, min_y, max_y`) of a set of projected
/// screen-dot points.
fn dot_bbox(pts: &[(i32, i32)]) -> (i32, i32, i32, i32) {
    let min_x = pts.iter().map(|p| p.0).min().unwrap();
    let max_x = pts.iter().map(|p| p.0).max().unwrap();
    let min_y = pts.iter().map(|p| p.1).min().unwrap();
    let max_y = pts.iter().map(|p| p.1).max().unwrap();
    (min_x, max_x, min_y, max_y)
}

/// Large reference scale for the fit solve: big enough that `project`'s
/// final `.round()` is negligible relative to the bbox span, so scaling by
/// a linear ratio (rather than iterating/binary-searching) lands accurately.
const FIT_REF_SCALE: f32 = 4096.0;

/// Fraction of the available VERTICAL dot extent the fit-solve corners' bbox
/// targets — width is NOT reduced (the board's wide/near edge should still
/// touch the screen's side edges, as it did before bench/heads needed
/// headroom). This is deliberately vertical-only: a standing creature's
/// head renders upward from its Bottom-anchored feet, past the flat corner
/// bbox `board_world_corners` covers, and needs room above the fitted
/// content — a concern that's about height, not width. (Bench's own extra
/// extent is instead handled precisely by including its actual world
/// position in `board_world_corners`, not by a margin guessing at it — an
/// earlier version of this constant shrank BOTH width and height uniformly,
/// which also pointlessly pulled the board's near edge in from the screen's
/// side edges, undoing the "fill available width" fix from earlier in this
/// same feature.)
const VERTICAL_HEADROOM_RATIO: f32 = 0.82;

/// Fit-to-viewport geometry for the perspective (`Sideline`/`OverShoulder`)
/// presets (b4-t1). `board_rect` spans the FULL render `area` (not just the
/// fitted board bbox — see `VERTICAL_HEADROOM_RATIO`) and `screen_offset`
/// shifts the fitted, camera-projected bbox to the center of that full
/// buffer. Solve is closed-form: projected dot coords are exactly
/// proportional to `scale_dots` (mod rounding), so projecting the corners at
/// `FIT_REF_SCALE` and comparing their bbox to the available dot area gives
/// the fill ratio directly — no iteration.
fn fit_perspective_geometry(area: Rect, mode: BattleCamera, tuning: BattleViewerTuning) -> BoardGeometry {
    let corners = board_world_corners();

    let cam_ref = mode.with_scale_dots(FIT_REF_SCALE);
    let rpts: Vec<(i32, i32)> = corners.iter().map(|&p| cam_ref.project(p)).collect();
    let (rmin_x, rmax_x, rmin_y, rmax_y) = dot_bbox(&rpts);
    let bbox_w = (rmax_x - rmin_x).max(1) as f32;
    let bbox_h = (rmax_y - rmin_y).max(1) as f32;

    let buf_w = (area.width as i32 * 2).max(1);
    let buf_h = (area.height as i32 * 4).max(1);
    let avail_w = buf_w as f32;
    let avail_h = (buf_h as f32 * VERTICAL_HEADROOM_RATIO).max(1.0);
    let f = (avail_w / bbox_w).min(avail_h / bbox_h);
    let scale = FIT_REF_SCALE * f;

    let camera = mode.with_scale_dots(scale);
    let fpts: Vec<(i32, i32)> = corners.iter().map(|&p| camera.project(p)).collect();
    let (fmin_x, fmax_x, fmin_y, fmax_y) = dot_bbox(&fpts);
    let span_x = (fmax_x - fmin_x).max(1);
    let span_y = (fmax_y - fmin_y).max(1);

    let board_rect = Rect::new(area.left(), area.top(), area.width.max(1), area.height.max(1));

    let off_x = -fmin_x + (buf_w - span_x) / 2;
    let off_y = -fmin_y + (buf_h - span_y) / 2;

    let cell_width_cols = (area.width / BOARD_COLS).max(1);
    let cell_height_rows = (area.height / BOARD_ROWS).max(1);

    BoardGeometry {
        cell_width_cols,
        cell_height_rows,
        board_rect,
        camera,
        tuning,
        screen_offset: (off_x, off_y),
    }
}

/// Sample density for `rasterize_grid_line`, in samples per world unit along
/// a line's length. `4` (spacing `0.25`) lands a sample exactly on every
/// non-Top-Down preset's half-integer `camera_depth` anchor (Over-shoulder
/// `6.5`, Sideline `3.5`), so perspective convergence near that anchor is
/// sampled precisely rather than approximated (b4-t1).
const GRID_LINE_SAMPLES_PER_UNIT: usize = 4;

/// Draws thin braille grid lines for a `BOARD_COLS x BOARD_ROWS` grid of
/// `geom.cell_width_cols` x `geom.cell_height_rows`-sized cells, positioned at
/// `geom.board_rect`. Uses ONLY the fields of the `BoardGeometry` passed in —
/// no independent re-derivation of cell size/position. Builds a `DotBuffer`
/// sized `board_rect.width*2 x board_rect.height*4` (the same dot-sizing
/// convention the piece composite uses). Each grid boundary line is defined
/// by its world-space endpoints, sampled at `GRID_LINE_SAMPLES_PER_UNIT`
/// samples per world unit, projected through `geom.camera.project()` — the
/// same projection pieces are placed with (`engine_render::transform::place`)
/// — and rasterized as connected dot segments (b4-t1: grid lines now track
/// the active camera instead of a fixed flat index). Converts via
/// `dots_to_grid` and blits via `draw_grid` — junctions emerge purely as the
/// bitwise union of overlapping lit dots, no special-cased glyph table.
/// Point-7 fencepost: a boundary that projects one dot past the last valid
/// dot index is clamped to the last valid dot (one dot short) instead.
/// Cell interiors are left `Transparent` (lines only). Clips instead of
/// panicking on an undersized buffer.
pub fn draw_board_lines(buf: &mut Buffer, geom: &BoardGeometry) {
    let buf_cols = geom.board_rect.width as usize * 2;
    let buf_rows = geom.board_rect.height as usize * 4;
    if buf_cols == 0 || buf_rows == 0 {
        return;
    }

    let mut dots = DotBuffer::new(buf_cols, buf_rows);
    let line_color = geom.camera.grid_line_color(&geom.tuning);
    let cam = geom.framed_camera();

    let vertical_samples = GRID_LINE_SAMPLES_PER_UNIT * BOARD_ROWS as usize + 1;
    for i in 0..=BOARD_COLS {
        let x = i as f32;
        rasterize_grid_line(
            &mut dots,
            &cam,
            WorldPos::new(x, 0.0),
            WorldPos::new(x, BOARD_ROWS as f32),
            vertical_samples,
            line_color,
        );
    }

    let horizontal_samples = GRID_LINE_SAMPLES_PER_UNIT * BOARD_COLS as usize + 1;
    for j in 0..=BOARD_ROWS {
        let y = j as f32;
        rasterize_grid_line(
            &mut dots,
            &cam,
            WorldPos::new(0.0, y),
            WorldPos::new(BOARD_COLS as f32, y),
            horizontal_samples,
            line_color,
        );
    }

    let grid = dots_to_grid(&dots);
    draw_grid(buf, geom.board_rect, &grid);
}

/// Samples the world-space segment `[start, end]` at `samples` evenly-spaced
/// points (endpoints inclusive), projects each through `camera` (the same
/// projection `engine_render::transform::place` uses for pieces), clamps
/// into `[0, dots.cols()-1] x [0, dots.rows()-1]` (point-7 fencepost — see
/// `draw_board_lines` doc), and plots a connected dot segment between each
/// pair of consecutive clamped points via `plot_dot_segment` (b4-t1).
/// `buf_cols`/`buf_rows` are read off `dots` itself (`DotBuffer::cols`/
/// `rows`) rather than taken as separate parameters — they are always
/// identical to `dots`' own size at every call site, so a redundant
/// parameter pair was dropped (keeps this under clippy's argument-count
/// lint without changing behavior).
fn rasterize_grid_line<C: Camera>(
    dots: &mut DotBuffer,
    camera: &C,
    start: WorldPos,
    end: WorldPos,
    samples: usize,
    color: Rgba,
) {
    let samples = samples.max(2);
    let max_x = dots.cols() as i32 - 1;
    let max_y = dots.rows() as i32 - 1;

    let mut prev: Option<(usize, usize)> = None;
    for s in 0..samples {
        let t = s as f32 / (samples - 1) as f32;
        let pos = WorldPos::new(
            start.x + (end.x - start.x) * t,
            start.y + (end.y - start.y) * t,
        );
        let (px, py) = camera.project(pos);
        let cx = px.clamp(0, max_x) as usize;
        let cy = py.clamp(0, max_y) as usize;

        match prev {
            Some((x0, y0)) => plot_dot_segment(dots, x0, y0, cx, cy, color),
            None => dots.set(cx, cy, Dot::Lit(color)),
        }
        prev = Some((cx, cy));
    }
}

/// Integer Bresenham: sets `Dot::Lit(color)` on every dot from `(x0,y0)` to
/// `(x1,y1)` inclusive. Callers pre-clamp both endpoints in-bounds, so every
/// interpolated dot stays in-bounds (b4-t1).
fn plot_dot_segment(dots: &mut DotBuffer, x0: usize, y0: usize, x1: usize, y1: usize, color: Rgba) {
    let (mut x, mut y) = (x0 as i32, y0 as i32);
    let (x1, y1) = (x1 as i32, y1 as i32);
    let dx = (x1 - x).abs();
    let sx: i32 = if x < x1 { 1 } else { -1 };
    let dy = -(y1 - y).abs();
    let sy: i32 = if y < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        dots.set(x as usize, y as usize, Dot::Lit(color));
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Wraps the three concrete `engine_render::camera::Camera` views usable by
/// the battle viewer. Exact passthrough: `project`/`depth_key` on any variant
/// must equal the wrapped camera's own output for the same `WorldPos` — no
/// recompute at this layer.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BattleCamera {
    Sideline(PerspectiveCamera),
    TopDown(OrthographicCamera),
    OverShoulder(PerspectiveCamera),
}

impl Camera for BattleCamera {
    fn project(&self, pos: WorldPos) -> (i32, i32) {
        match self {
            BattleCamera::Sideline(c) => c.project(pos),
            BattleCamera::TopDown(c) => c.project(pos),
            BattleCamera::OverShoulder(c) => c.project(pos),
        }
    }

    fn depth_key(&self, pos: WorldPos) -> i32 {
        match self {
            BattleCamera::Sideline(c) => c.depth_key(pos),
            BattleCamera::TopDown(c) => c.depth_key(pos),
            BattleCamera::OverShoulder(c) => c.depth_key(pos),
        }
    }

    /// Dots per world unit AT `pos` specifically (not a single near-reference
    /// value like `sprite_scale_dots`) — i.e. "how many dots wide is exactly
    /// one board cell, right where this piece actually stands." `TopDown` is
    /// constant everywhere (`c.scale_dots`); `Sideline`/`OverShoulder` shrink
    /// with distance from the camera, using the piece's own `forward_distance`
    /// rather than a shared reference point. This is what per-piece sprite
    /// and shadow sizing key off so they shrink smoothly with distance
    /// instead of using one flat per-frame size for every piece regardless
    /// of how far away it actually is.
    fn local_dots_per_world_unit(&self, pos: WorldPos) -> f32 {
        match self {
            BattleCamera::TopDown(c) => c.scale_dots,
            BattleCamera::Sideline(c) | BattleCamera::OverShoulder(c) => {
                c.dots_per_world_unit(c.forward_distance(pos))
            }
        }
    }
}

impl BattleCamera {
    /// Per-world-unit dot scale of the active variant, for sizing a sprite so
    /// it's roughly cell-sized at the camera's near reference depth. `TopDown`
    /// (orthographic) returns its `scale_dots` directly — that field already
    /// IS a per-world-unit rate for an orthographic camera. `Sideline`/
    /// `OverShoulder` do NOT — their `scale_dots` is a raw NDC-to-dots
    /// constant solved by viewport-fit, not a per-world-unit rate (using it
    /// directly overflowed sprites, since it's typically much larger than
    /// the actual dots-per-world-unit at any real position). The correct
    /// rate is `PerspectiveCamera::dots_per_world_unit`, evaluated at the
    /// SAME near-reference depth `depth_scale_factor` normalizes against, so
    /// a piece standing at that reference edge renders at exactly this size
    /// (matching `depth_scale_factor`'s own `1.0` there) and pieces farther
    /// away are scaled down from it by that same factor, not scaled down
    /// twice.
    pub fn sprite_scale_dots(&self) -> f32 {
        match self {
            BattleCamera::TopDown(c) => c.scale_dots,
            BattleCamera::Sideline(c) | BattleCamera::OverShoulder(c) => {
                c.dots_per_world_unit(near_reference_forward_distance(c))
            }
        }
    }

    /// Grid-line prominence for `draw_board_lines`: full-strength, opaque
    /// `GRID_LINE_COLOR` for `TopDown`; for `Sideline`/`OverShoulder` a
    /// translucent `Rgba::new(0xFF,0xFF,0xFF,tuning.grid_dim_alpha)` that
    /// blends via the real alpha-blit path (b1-t3) rather than a flat dark
    /// constant. Exhaustive match — no wildcard, so a future variant is
    /// forced to choose a prominence.
    pub fn grid_line_color(&self, tuning: &BattleViewerTuning) -> Rgba {
        match self {
            BattleCamera::TopDown(_) => GRID_LINE_COLOR,
            BattleCamera::Sideline(_) => Rgba::new(0xFF, 0xFF, 0xFF, tuning.grid_dim_alpha),
            BattleCamera::OverShoulder(_) => Rgba::new(0xFF, 0xFF, 0xFF, tuning.grid_dim_alpha),
        }
    }

    /// Elevation (degrees) of the active variant — exhaustive match (no
    /// wildcard), the permanent replacement for the removed `oblique()`
    /// accessor now that `Sideline`/`OverShoulder` no longer wrap the same
    /// concrete type as `TopDown`. `shadow_buffers`' squash factor reads
    /// through this.
    pub fn elevation_deg(&self) -> f32 {
        match self {
            BattleCamera::Sideline(c) => c.elevation_deg,
            BattleCamera::TopDown(_) => 90.0,
            BattleCamera::OverShoulder(c) => c.elevation_deg,
        }
    }

    /// Vertical anchor to use for every `SpriteDraw` (shadow and piece alike)
    /// under the active variant. `TopDown` centers sprites on their grid
    /// point (looking straight down, there is no "ground contact" edge);
    /// `Sideline`/`OverShoulder` anchor sprites by their bottom edge so a
    /// piece's feet land on its projected ground point instead of floating
    /// centered on it. Exhaustive match — no wildcard — so a future variant
    /// is forced to choose an anchor.
    pub fn vertical_anchor(&self) -> VerticalAnchor {
        match self {
            BattleCamera::TopDown(_) => VerticalAnchor::Center,
            BattleCamera::Sideline(_) => VerticalAnchor::Bottom,
            BattleCamera::OverShoulder(_) => VerticalAnchor::Bottom,
        }
    }

    /// Rebuild the active variant at `scale_dots`, preserving every other
    /// variant-specific world param. Scale is always area-derived (see
    /// `board_geometry`), so the incoming variant's own scale is
    /// intentionally replaced, not read. No longer takes a `tuning` param:
    /// the dead `grid_taper_*` threading this used to do is gone along with
    /// those fields.
    fn with_scale_dots(self, scale_dots: f32) -> Self {
        match self {
            BattleCamera::TopDown(_) => BattleCamera::TopDown(OrthographicCamera { scale_dots }),
            BattleCamera::Sideline(c) => BattleCamera::Sideline(PerspectiveCamera { scale_dots, ..c }),
            BattleCamera::OverShoulder(c) => BattleCamera::OverShoulder(PerspectiveCamera { scale_dots, ..c }),
        }
    }

    /// Sideline preset: looks down the column (world-x) axis, mild elevation,
    /// anchored on the board's horizontal center column. `camera_depth` sits
    /// strictly outside the occupied `[0, BOARD_COLS]` range (negative side)
    /// so every board cell projects with a comfortably positive
    /// `forward_distance` — no near-plane clamping.
    pub fn sideline_preset() -> Self {
        BattleCamera::Sideline(PerspectiveCamera {
            scale_dots: 0.0,
            depth_axis: DepthAxis::Col,
            elevation_deg: 10.0,
            camera_depth: SIDELINE_CAMERA_DEPTH,
            camera_height: 2.5,
            spread_center: BOARD_CENTER_COL,
            fov_deg: 55.0,
            // Camera sits on the LOW side (negative), looking toward
            // increasing column.
            facing_sign: 1.0,
        })
    }

    /// Over-the-shoulder preset: looks down the row (world-y, team-separation)
    /// axis from behind Team B's bench row. `camera_depth` sits strictly
    /// outside the occupied `[0, BOARD_ROWS]` range on the POSITIVE side
    /// (past `TEAM_B_BENCH_ROW`, not before row 0) so the shot is genuinely
    /// from behind Team B, and `camera_height` is tall enough to keep
    /// `forward_distance` positive all the way to the far edge (row 0) —
    /// see both constants' doc comments.
    pub fn over_shoulder_preset() -> Self {
        BattleCamera::OverShoulder(PerspectiveCamera {
            scale_dots: 0.0,
            depth_axis: DepthAxis::Row,
            elevation_deg: 30.0,
            camera_depth: OVER_SHOULDER_CAMERA_DEPTH,
            camera_height: OVER_SHOULDER_CAMERA_HEIGHT,
            spread_center: BOARD_CENTER_COL,
            fov_deg: 55.0,
            // Camera sits on the HIGH side (past the board's far edge),
            // looking toward decreasing row (toward Team A) — the opposite
            // of Sideline's facing, since it sits on the opposite side.
            facing_sign: -1.0,
        })
    }

    /// Top-down preset: straight-down plan view, true orthographic projection
    /// (spec 42 Decision 0) — no tilt, no taper, no depth-anchor.
    pub fn top_down_preset() -> Self {
        BattleCamera::TopDown(OrthographicCamera { scale_dots: 0.0 })
    }
}

#[cfg(test)]
mod battle_camera_tests {
    use super::*;

    /// Builds a standalone `PerspectiveCamera` (backs `Sideline`/`OverShoulder`
    /// after b3-t1) with arbitrary params — mirrors `oblique`'s role for the
    /// two migrated variants.
    fn perspective(depth_axis: DepthAxis, elevation_deg: f32, camera_depth: f32, scale: f32) -> PerspectiveCamera {
        PerspectiveCamera {
            depth_axis,
            elevation_deg,
            camera_depth,
            camera_height: 2.5,
            spread_center: 3.5,
            fov_deg: 55.0,
            scale_dots: scale,
            // Every call site here passes a negative camera_depth (low
            // side) — facing_sign is irrelevant to what these tests check
            // (enum-variant passthrough dispatch), so a fixed 1.0 matches
            // that convention without needing a fifth parameter.
            facing_sign: 1.0,
        }
    }

    /// Sideline variant must be an exact passthrough to the wrapped `PerspectiveCamera`.
    #[test]
    fn sideline_project_and_depth_key_match_wrapped_perspective() {
        let inner = perspective(DepthAxis::Col, 10.0, -4.0, 4.0);
        let cam = BattleCamera::Sideline(inner);
        let pos = WorldPos::new(1.5, 2.5);
        assert_eq!(cam.project(pos), inner.project(pos));
        assert_eq!(cam.depth_key(pos), inner.depth_key(pos));
    }

    /// TopDown variant must be an exact passthrough to the wrapped `OrthographicCamera`.
    #[test]
    fn topdown_project_and_depth_key_match_wrapped_orthographic() {
        let inner = OrthographicCamera { scale_dots: 4.0 };
        let cam = BattleCamera::TopDown(inner);
        let pos = WorldPos::new(1.5, 2.5);
        assert_eq!(cam.project(pos), inner.project(pos));
        assert_eq!(cam.depth_key(pos), inner.depth_key(pos));
    }

    /// OverShoulder variant must be an exact passthrough to the wrapped
    /// `PerspectiveCamera`, including its construction params (`camera_depth`) —
    /// checked at a position off the camera's depth so the perspective-divide
    /// terms are actually exercised (proves forwarding, not a fresh recompute).
    #[test]
    fn overshoulder_project_and_depth_key_match_wrapped_perspective() {
        let inner = perspective(DepthAxis::Row, 30.0, -2.0, 4.0);
        let cam = BattleCamera::OverShoulder(inner);
        let pos = WorldPos::new(1.5, 5.0); // off camera_depth (-2.0)
        assert_eq!(cam.project(pos), inner.project(pos));
        assert_eq!(cam.depth_key(pos), inner.depth_key(pos));
    }

    /// `BattleCamera::elevation_deg()` returns the wrapped camera's own
    /// `elevation_deg` for `Sideline`/`OverShoulder`, and the permanent
    /// literal `90.0` for `TopDown` (`OrthographicCamera` carries no
    /// `elevation_deg` field of its own) — `depth_scale_factor`/
    /// `shadow_buffers` read through this every frame.
    #[test]
    fn elevation_deg_returns_wrapped_camera_elevation_for_each_variant() {
        let side = perspective(DepthAxis::Col, 11.0, -4.0, 4.0);
        let top = OrthographicCamera { scale_dots: 4.0 };
        let over = perspective(DepthAxis::Row, 33.0, -3.0, 4.0);
        assert_eq!(BattleCamera::Sideline(side).elevation_deg(), 11.0);
        assert_eq!(BattleCamera::TopDown(top).elevation_deg(), 90.0);
        assert_eq!(BattleCamera::OverShoulder(over).elevation_deg(), 33.0);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: Sideline/OverShoulder -> PerspectiveCamera migration invariants
// (b3-t1 PUBLIC_SURFACE #2/#3, research.md). Property-only per the no-
// pinned-constant guardrail — never a single hardcoded "correct" preset
// constant. Framing (on-screen position) is explicitly NOT covered here;
// that is b4-t1's fit-to-viewport job (research.md's documented deferral).
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod camera_migration_tests {
    use super::*;

    /// Comfortably above the engine's private `NEAR_EPS=0.01` clamp floor —
    /// a local margin so this test never needs to reference the engine's
    /// private constant directly.
    const FORWARD_DISTANCE_MARGIN: f32 = 1.0;

    /// PUBLIC_SURFACE #2: `forward_distance` must be comfortably positive
    /// for every occupied board cell under `sideline_preset()`. Guaranteed
    /// once `camera_depth` sits strictly outside the occupied board range on
    /// the correct side — currently FAILS: `sideline_preset()`'s
    /// `camera_depth` is still the interim placeholder (see its doc note),
    /// clamping `forward_distance` to near the `NEAR_EPS` floor for cells
    /// near the (still mid-board) camera.
    #[test]
    fn sideline_forward_distance_positive_all_cells() {
        let BattleCamera::Sideline(cam) = BattleCamera::sideline_preset() else {
            unreachable!()
        };
        for col in 0..BOARD_COLS {
            for row in 0..BOARD_ROWS {
                let pos = world_pos_for_cell(col, row);
                let d = cam.forward_distance(pos);
                assert!(
                    d > FORWARD_DISTANCE_MARGIN,
                    "sideline_preset forward_distance at cell ({col},{row}) must exceed the \
                     margin ({FORWARD_DISTANCE_MARGIN}), got {d} (pos={pos:?})"
                );
            }
        }
    }

    /// PUBLIC_SURFACE #2, `over_shoulder_preset()` — see
    /// `sideline_forward_distance_positive_all_cells`'s doc note (same
    /// interim-placeholder reasoning applies).
    #[test]
    fn over_shoulder_forward_distance_positive_all_cells() {
        let BattleCamera::OverShoulder(cam) = BattleCamera::over_shoulder_preset() else {
            unreachable!()
        };
        for col in 0..BOARD_COLS {
            for row in 0..BOARD_ROWS {
                let pos = world_pos_for_cell(col, row);
                let d = cam.forward_distance(pos);
                assert!(
                    d > FORWARD_DISTANCE_MARGIN,
                    "over_shoulder_preset forward_distance at cell ({col},{row}) must exceed the \
                     margin ({FORWARD_DISTANCE_MARGIN}), got {d} (pos={pos:?})"
                );
            }
        }
    }

    /// PUBLIC_SURFACE #3: each non-Top-Down preset's `camera_depth` sits
    /// strictly outside its occupied board range on the correct (negative)
    /// side. Currently FAILS: both presets still carry the interim
    /// placeholder `camera_depth` (mid-board / edge-of-board — see
    /// `sideline_preset`/`over_shoulder_preset`'s doc notes), which is
    /// exactly the "camera in the middle of the pitch" bug this task's real
    /// tuning fixes.
    #[test]
    fn non_top_down_camera_depth_outside_board_range() {
        let BattleCamera::Sideline(side) = BattleCamera::sideline_preset() else {
            unreachable!()
        };
        assert!(
            side.camera_depth < 0.0 || side.camera_depth > BOARD_COLS as f32,
            "sideline_preset's camera_depth ({}) must sit strictly outside [0, {}] (the occupied \
             board range along its depth axis, Col)",
            side.camera_depth,
            BOARD_COLS
        );

        let BattleCamera::OverShoulder(over) = BattleCamera::over_shoulder_preset() else {
            unreachable!()
        };
        assert!(
            over.camera_depth < 0.0 || over.camera_depth > BOARD_ROWS as f32,
            "over_shoulder_preset's camera_depth ({}) must sit strictly outside [0, {}] (the \
             occupied board range along its depth axis, Row)",
            over.camera_depth,
            BOARD_ROWS
        );
    }

    /// The two candidate depth-axis coordinates at the board's extremes
    /// (the first/last row-or-column's cell center), returned as `(nearer,
    /// farther)` ordered by their OWN `forward_distance` at the camera's
    /// `spread_center` (not by raw distance from `camera_depth`) — a raw-
    /// distance heuristic is fooled by the `NEAR_EPS` clamp: a candidate
    /// technically "closer" to `camera_depth` by subtraction can actually be
    /// BEHIND the camera plane (negative raw forward, clamped near zero),
    /// which blows its projected spread up numerically without that being
    /// genuine convergence. Ordering by the real (clamped) `forward_distance`
    /// avoids that false positive.
    fn near_far_depths(cam: &PerspectiveCamera, depth_axis: DepthAxis, board_extent: u16) -> (f32, f32) {
        let low = 0.5;
        let high = board_extent as f32 - 0.5;
        let forward_at = |d: f32| {
            let pos = match depth_axis {
                DepthAxis::Col => WorldPos::new(d, cam.spread_center),
                DepthAxis::Row => WorldPos::new(cam.spread_center, d),
            };
            cam.forward_distance(pos)
        };
        if forward_at(low) <= forward_at(high) {
            (low, high)
        } else {
            (high, low)
        }
    }

    /// Projects `cam` at a fixed depth-axis coordinate `depth` across every
    /// spread-axis cell center and returns the range (`max - min`) of the
    /// resulting screen_x — a proxy for "how wide this depth slice reads on
    /// screen," used to prove/disprove real perspective convergence without
    /// needing fit-to-viewport framing (b4-t1, not yet landed).
    fn column_spread(cam: &BattleCamera, depth_axis: DepthAxis, depth: f32) -> i32 {
        let spread_extent = match depth_axis {
            DepthAxis::Col => BOARD_ROWS,
            DepthAxis::Row => BOARD_COLS,
        };
        let xs: Vec<i32> = (0..spread_extent)
            .map(|i| {
                let spread = i as f32 + 0.5;
                let pos = match depth_axis {
                    DepthAxis::Col => WorldPos::new(depth, spread),
                    DepthAxis::Row => WorldPos::new(spread, depth),
                };
                cam.project(pos).0
            })
            .collect();
        xs.iter().max().unwrap() - xs.iter().min().unwrap()
    }

    /// Interim convergence-direction evidence (mirrors the retired
    /// oblique-taper-camera-based test this replaces): a real perspective camera
    /// must converge — the depth-axis coordinate nearer the camera projects
    /// a WIDER spread of screen-x across the spread axis than the farther
    /// one. Computable without fit-to-viewport framing. Gated on both
    /// candidate depths clearing `FORWARD_DISTANCE_MARGIN` first — under the
    /// interim placeholder `camera_depth` this gate itself fails (one board
    /// extreme sits behind the still mid-board camera plane), which is the
    /// honest reason this can't yet be evaluated, rather than silently
    /// passing on a `NEAR_EPS`-clamp numerical artifact.
    #[test]
    fn sideline_near_depth_spread_exceeds_far_depth() {
        let cam = BattleCamera::sideline_preset().with_scale_dots(8.0);
        let BattleCamera::Sideline(inner) = cam else {
            unreachable!()
        };
        let (near_depth, far_depth) = near_far_depths(&inner, DepthAxis::Col, BOARD_COLS);
        let near_forward = inner.forward_distance(WorldPos::new(near_depth, inner.spread_center));
        let far_forward = inner.forward_distance(WorldPos::new(far_depth, inner.spread_center));
        assert!(
            near_forward > FORWARD_DISTANCE_MARGIN && far_forward > FORWARD_DISTANCE_MARGIN,
            "both candidate depths must be comfortably in front of the camera \
             (forward_distance > {FORWARD_DISTANCE_MARGIN}) before a convergence-direction \
             comparison is meaningful; got near_forward={near_forward} far_forward={far_forward} \
             (camera_depth={})",
            inner.camera_depth
        );

        let near_spread = column_spread(&cam, DepthAxis::Col, near_depth);
        let far_spread = column_spread(&cam, DepthAxis::Col, far_depth);
        assert!(
            near_spread > far_spread,
            "a depth-axis coordinate nearer the camera ({near_depth}) must project a WIDER \
             spread of screen-x than one farther away ({far_depth}): near_spread={near_spread} \
             far_spread={far_spread}"
        );
    }

    /// `over_shoulder_preset()` counterpart of
    /// `sideline_near_depth_spread_exceeds_far_depth` (depth axis Row).
    #[test]
    fn over_shoulder_near_depth_spread_exceeds_far_depth() {
        let cam = BattleCamera::over_shoulder_preset().with_scale_dots(8.0);
        let BattleCamera::OverShoulder(inner) = cam else {
            unreachable!()
        };
        let (near_depth, far_depth) = near_far_depths(&inner, DepthAxis::Row, BOARD_ROWS);
        let near_forward = inner.forward_distance(WorldPos::new(inner.spread_center, near_depth));
        let far_forward = inner.forward_distance(WorldPos::new(inner.spread_center, far_depth));
        assert!(
            near_forward > FORWARD_DISTANCE_MARGIN && far_forward > FORWARD_DISTANCE_MARGIN,
            "both candidate depths must be comfortably in front of the camera \
             (forward_distance > {FORWARD_DISTANCE_MARGIN}) before a convergence-direction \
             comparison is meaningful; got near_forward={near_forward} far_forward={far_forward} \
             (camera_depth={})",
            inner.camera_depth
        );

        let near_spread = column_spread(&cam, DepthAxis::Row, near_depth);
        let far_spread = column_spread(&cam, DepthAxis::Row, far_depth);
        assert!(
            near_spread > far_spread,
            "a depth-axis coordinate nearer the camera ({near_depth}) must project a WIDER \
             spread of screen-x than one farther away ({far_depth}): near_spread={near_spread} \
             far_spread={far_spread}"
        );
    }
}

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

/// World-y the over-shoulder camera sits behind: strictly outside the
/// occupied `[0, BOARD_ROWS]` range on the POSITIVE side (past the board's
/// own far edge, beyond Team B's own active/bench rows), so the shot is
/// genuinely from behind Team B looking at Team A — not the mirror image.
/// `PerspectiveCamera::facing_sign`
/// (`-1.0` for this preset) tells `forward_distance` which way the camera
/// looks, so `forward_distance` stays positive for ANY `camera_height` here
/// — no tall-camera workaround needed (an earlier version of this constant
/// paired a much taller `camera_height` with this depth specifically to
/// route around a since-fixed bug where the formula assumed a fixed facing
/// direction; that workaround produced a narrow, over-steep-looking shot
/// and is gone now that `facing_sign` exists). A comfortable margin past
/// `BOARD_ROWS` (not just barely past it) matters for a different reason:
/// `near_reference_forward_distance` (depth-scale/sprite-size baseline)
/// probes forward_distance AT the board boundary itself — too thin a margin
/// puts that boundary almost on top of the camera, blowing up the near/far
/// size ratio until the far piece rasterizes to a near-invisible handful of
/// dots (this shipped as a real bug at margin `0.5`).
const OVER_SHOULDER_CAMERA_DEPTH: f32 = 10.0;
/// World-units-above-ground the over-shoulder camera sits at — a modest,
/// human/creature shoulder-height scale (matching Sideline's `2.5`), not
/// constrained by keeping `forward_distance` positive (`facing_sign`
/// already guarantees that regardless of height).
const OVER_SHOULDER_CAMERA_HEIGHT: f32 = 3.0;

/// World-x/-y the sideline camera anchors its depth on, strictly outside the
/// occupied `[0, BOARD_COLS]` board range on the negative side, so every
/// board depth sits comfortably in front of the camera.
const SIDELINE_CAMERA_DEPTH: f32 = -4.0;

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
/// Kept well under `1.0` because the binding constraint is sprite WIDTH, not
/// height: idle GIFs are not square (widest real asset, frost_lizard, is
/// ~1.58:1 width:height), and width is derived downstream from
/// `base_dot_rows` by the raster aspect formula. `0.6` keeps both the height
/// and the width of every idle sprite inside its own square-in-dots cell.
pub const SPRITE_DOT_RATIO: f32 = 0.6;
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

/// Per-index animation offset so the 8 idle loops don't play in lockstep:
/// `elapsed + PIECE_STAGGER * index`.
pub fn piece_elapsed(elapsed: Duration, index: usize) -> Duration {
    elapsed + PIECE_STAGGER * index as u32
}

/// `forward_distance` at whichever board-edge (along `cam`'s depth axis) is
/// closer to the camera — the single reference point both `depth_scale_factor`
/// (ratio relative to it) and `sprite_scale_dots` (absolute dots-per-world-
/// unit AT it) key off, so a piece standing at that edge is always exactly
/// cell-sized before any depth falloff is applied.
fn near_reference_forward_distance(cam: &PerspectiveCamera) -> f32 {
    let extent = match cam.depth_axis {
        DepthAxis::Col => BOARD_COLS as f32,
        DepthAxis::Row => BOARD_ROWS as f32,
    };
    let depth_pos = |d: f32| match cam.depth_axis {
        DepthAxis::Col => WorldPos::new(d, 0.0),
        DepthAxis::Row => WorldPos::new(0.0, d),
    };
    cam.forward_distance(depth_pos(0.0))
        .min(cam.forward_distance(depth_pos(extent)))
}

/// Sprite height in dots, sized off the shared camera's per-world-unit dot
/// scale: `(camera.sprite_scale_dots() * SPRITE_DOT_RATIO).round() as u32`.
/// `TopDown`-only in practice now (see `sprite_base_dot_rows_width_fill` for
/// Sideline/OverShoulder) — kept pos-independent since `sprite_scale_dots()`
/// is constant everywhere for the orthographic Top-Down camera.
pub fn sprite_base_dot_rows(camera: &BattleCamera) -> u32 {
    (camera.sprite_scale_dots() * SPRITE_DOT_RATIO).round() as u32
}

/// Fraction of a cell's width, AT the piece's own position, that a creature's
/// rendered WIDTH targets under Sideline/OverShoulder — the binding
/// constraint there is width filling the base of the cell the piece stands
/// on (project owner's explicit ask), unlike Top-Down's height-ratio
/// approach (`SPRITE_DOT_RATIO`), which exists to keep sprites from
/// overflowing a fixed-size square cell from directly above.
const WIDTH_FILL_RATIO: f32 = 0.92;

/// Fraction of a cell's width, AT the piece's own position, that its contact
/// shadow's own outer diameter targets — deliberately smaller than
/// `WIDTH_FILL_RATIO` so the shadow reads as a mark under the creature's
/// feet, not another shape competing with it for the same footprint.
const SHADOW_WIDTH_RATIO: f32 = 0.55;

/// Per-piece sprite height in dots for Sideline/OverShoulder: sized so the
/// creature's rendered WIDTH is `WIDTH_FILL_RATIO` of a cell-width AT `pos`
/// (via `BattleCamera::local_dots_per_world_unit`, which already shrinks
/// with distance from the camera) — `aspect` (source image width/height)
/// converts that width target into the `base_dot_rows` the rasterizer
/// actually takes. Because this already varies with `pos`, no SEPARATE
/// depth-scale multiplier is applied on top (see `depth_scale_factor`) —
/// one distance term, not two stacked ones.
fn sprite_base_dot_rows_width_fill(camera: &BattleCamera, pos: WorldPos, aspect: f32) -> u32 {
    let target_width_dots = camera.local_dots_per_world_unit(pos) * WIDTH_FILL_RATIO;
    (target_width_dots / aspect.max(0.01)).round().max(1.0) as u32
}

/// Per-piece depth-scale multiplier (spec 41 Decision 4): always `1.0` now.
/// `TopDown` never had a `forward_distance` term to derive one from.
/// Sideline/OverShoulder used to apply a second, ratio-based shrink here on
/// top of `sprite_base_dot_rows_width_fill`'s own already-distance-dependent
/// sizing — that double-applied the same falloff (once as an absolute
/// per-position rate, once again as a relative ratio), over-shrinking far
/// pieces. Kept as a named seam (not deleted outright) since
/// `depth_scaled_transform` below still has a real job: preserving
/// `transform`'s own existing scale (team mirror, Die-tween shrink).
fn depth_scale_factor(_camera: &BattleCamera, _pos: WorldPos) -> f32 {
    1.0
}

/// Applies `depth_scale_factor` on top of `transform`'s OWN existing scale
/// (which may already carry a Die-event shrink tween) — multiplies, never
/// overwrites from `Team::scale_x()`/`1.0`. `translate`/`rotation` pass
/// through unchanged. The render loop's per-piece draw-construction calls
/// this same helper (not a second, duplicated inline formula).
fn depth_scaled_transform(transform: &Transform, camera: &BattleCamera) -> Transform {
    let factor = depth_scale_factor(camera, transform.translate);
    Transform {
        scale: Vec2::new(transform.scale.x * factor, transform.scale.y * factor),
        ..*transform
    }
}

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

/// Tunable constants for camera-dependent rendering (grid dimming, shadow
/// fade). Depth scaling (spec 41 Decision 4) is derived directly from the
/// active camera's `forward_distance`, not a tunable here.
#[derive(Clone, Copy, PartialEq, Debug, Inspectable)]
pub struct BattleViewerTuning {
    pub grid_dim_alpha: u8,
    pub shadow_fade_ms: u32,
}

impl Default for BattleViewerTuning {
    fn default() -> Self {
        Self {
            grid_dim_alpha: 0x46,
            shadow_fade_ms: 150,
        }
    }
}

#[derive(Inspectable)]
pub struct BattleViewer {
    elapsed: f32,
    /// The 8 bundled creatures (`crate::creatures::all()`), index-matched to
    /// `Piece.index` (b5-t1). Sourced by `piece_sprite` in `render()`'s draw
    /// loop instead of a single shared sprite.
    #[inspect(hidden)]
    creatures: Vec<Creature>,
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
    /// Active camera mode (b5-t1). Not part of the inspectable surface —
    /// `BattleCamera` does not implement `Inspectable` — and never persists
    /// across a scene re-entry; `enter()` resets it to
    /// `Self::default_camera_mode()` every time.
    #[inspect(hidden)]
    camera_mode: BattleCamera,
    /// Camera-dependent rendering tuning constants (b2-t1). Not yet consumed
    /// by any renderer — wired in by b3-t1/b4-t2/b6-t1/b7-t1.
    pub tuning: BattleViewerTuning,
}

impl Default for BattleViewer {
    fn default() -> Self {
        Self {
            elapsed: 0.0,
            creatures: crate::creatures::all(),
            pieces: pieces(),
            // Hand-authored demo sequence (b3-t1); driven each frame by
            // `update()`/`drive_events()`.
            events: demo_events(),
            event_from_values: std::collections::HashMap::new(),
            settled_events: std::collections::HashSet::new(),
            camera_mode: Self::default_camera_mode(),
            tuning: BattleViewerTuning::default(),
        }
    }
}

impl BattleViewer {
    /// Single source of truth for the starting camera, shared by `Default`
    /// and `Scene::enter()` so the two can never drift apart (b5-t1).
    fn default_camera_mode() -> BattleCamera {
        BattleCamera::sideline_preset()
    }

    /// Single source of truth for `index -> idle AnimatedSprite` (b5-t1).
    /// Used by `render()`'s per-piece draw loop instead of a single shared
    /// sprite.
    fn piece_sprite(&self, index: usize) -> Option<&AnimatedSprite> {
        self.creatures.get(index)?.animation(AnimationKind::Idle)
    }

    /// Shared drawable-piece iterator: alive pieces that have a sprite,
    /// paired with that sprite. `shadow_buffers` and `build_draws` both fold
    /// over this SAME iterator so their per-piece indices can never drift.
    fn drawable_pieces(&self) -> impl Iterator<Item = (&Piece, &AnimatedSprite)> {
        self.pieces
            .iter()
            .filter(|p| p.alive)
            .filter_map(|p| Some((p, self.piece_sprite(p.index)?)))
    }

    /// b7-t1: pure fade scalar in `[0,1]` for `piece_index`'s contact shadow,
    /// derived from `self.elapsed`/`self.events`/`self.tuning.shadow_fade_ms`
    /// (spec Decision 4). Standing still (no relevant event, or outside every
    /// event's window) => `1.0`. When multiple events target this piece, the
    /// MIN across them wins.
    fn shadow_alpha(&self, piece_index: usize) -> f32 {
        let fade = self.tuning.shadow_fade_ms as f32 / 1000.0;
        let mut alpha = 1.0_f32;
        for ev in &self.events {
            let target = match ev.kind {
                EventKind::Move { piece_index, .. } => piece_index,
                EventKind::Die { piece_index } => piece_index,
            };
            if target != piece_index {
                continue;
            }
            let t_start = ev.start_time;
            let t_end = ev.start_time + ev.duration;
            let a = if self.elapsed < t_start {
                1.0
            } else if fade > 0.0 && self.elapsed < t_start + fade {
                1.0 - (self.elapsed - t_start) / fade
            } else if self.elapsed < t_end {
                0.0
            } else if fade > 0.0 && self.elapsed < t_end + fade {
                (self.elapsed - t_end) / fade
            } else {
                1.0
            };
            alpha = alpha.min(a);
        }
        alpha
    }

    /// b7-t1/b1-t4: one team-colored `rasterize_shape(ShapeKind::Ring, ...)`
    /// buffer per drawable piece, same iteration order `build_draws` shares
    /// (`drawable_pieces`). Width is `SHADOW_WIDTH_RATIO` of a cell-width AT
    /// EACH PIECE'S OWN POSITION (`BattleCamera::local_dots_per_world_unit`)
    /// — sized per piece, not once per frame off a single flat cell metric,
    /// so a far piece's shadow is visibly smaller than a near piece's, the
    /// same way its sprite is. Height squashes by the camera's elevation
    /// sine (`k`) so a low, oblique camera flattens the annulus while
    /// Top-Down (`k=1`) keeps it round. The Ring's exact center is the
    /// alpha-0 hole; alpha peaks in a thin band near the outer edge.
    fn shadow_buffers(&self, geom: &BoardGeometry) -> Vec<DotBuffer> {
        let k = geom.camera.elevation_deg().to_radians().sin();
        self.drawable_pieces()
            .map(|(p, _)| {
                let rate = geom.camera.local_dots_per_world_unit(p.transform.translate);
                // `| 1` forces width (and, from it, height) odd so
                // `rasterize_shape`'s center (`(size-1)/2`) always lands on a
                // real dot — for `Ring` that center dot is the alpha-0 hole.
                let w = ((rate * SHADOW_WIDTH_RATIO).round().max(1.0) as usize) | 1;
                let h = ((((w as f32) * k).round().max(1.0)) as usize) | 1;
                let alpha = self.shadow_alpha(p.index);
                let col = Rgba::new(p.color.r, p.color.g, p.color.b, (255.0 * alpha).round() as u8);
                rasterize_shape(ShapeKind::Ring, w, h, col)
            })
            .collect()
    }

    /// b7-t1: per drawable piece, emits the shadow `SpriteDraw`
    /// (`Prerasterized`, `tint: None`) immediately followed by the piece's
    /// own `SpriteDraw` (`tint: None` — b7-t1 detint), sharing `translate`.
    /// `shadow_bufs` must come from `self.shadow_buffers(geom)` and align
    /// index-for-index with `drawable_pieces()`'s order.
    fn build_draws<'a>(
        &'a self,
        geom: &BoardGeometry,
        shadow_bufs: &'a [DotBuffer],
        elapsed: Duration,
    ) -> Vec<SpriteDraw<'a>> {
        let anchor = geom.camera.vertical_anchor();
        let is_top_down = matches!(geom.camera, BattleCamera::TopDown(_));
        // Top-Down's base size is position-independent (its `scale_dots` IS
        // a constant per-world-unit rate everywhere), so it's still fine to
        // compute once per frame; Sideline/OverShoulder need it per piece
        // (see the loop body) since it depends on that piece's own distance
        // from the camera AND its own sprite's aspect ratio.
        let top_down_base_dot_rows = sprite_base_dot_rows(&geom.camera);
        let mut draws = Vec::with_capacity(shadow_bufs.len() * 2);
        for (i, (p, sprite)) in self.drawable_pieces().enumerate() {
            let transform = depth_scaled_transform(&p.transform, &geom.camera);
            draws.push(SpriteDraw {
                content: SpriteContent::Prerasterized(&shadow_bufs[i]),
                translate: p.transform.translate,
                tint: None,
                // A ground-plane decal is centered ON the contact point
                // (spreads symmetrically around it), unlike a standing
                // creature (whose FEET are at the point, body extending
                // upward) — using the same per-camera Bottom anchor as the
                // creature here shifted the shadow's visible center away
                // from the piece's actual ground position under any camera
                // with a non-Center anchor.
                vertical_anchor: VerticalAnchor::Center,
            });
            let base_dot_rows = if is_top_down {
                top_down_base_dot_rows
            } else {
                let frame = sprite.frame_at(piece_elapsed(elapsed, p.index));
                let aspect = frame.width() as f32 / frame.height().max(1) as f32;
                sprite_base_dot_rows_width_fill(&geom.camera, p.transform.translate, aspect)
            };
            draws.push(SpriteDraw {
                content: SpriteContent::Animated {
                    sprite,
                    elapsed: piece_elapsed(elapsed, p.index),
                    transform,
                    base_dot_rows,
                },
                translate: p.transform.translate,
                tint: None,
                vertical_anchor: anchor,
            });
        }
        draws
    }

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

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {
        self.camera_mode = Self::default_camera_mode();
    }

    fn update(&mut self, _ctx: &mut EngineCtx, dt: Duration) -> Option<Transition> {
        self.elapsed += dt.as_secs_f32();
        self.drive_events();
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let geom = board_geometry(area, self.camera_mode, self.tuning);
        draw_board_lines(frame.buffer_mut(), &geom);

        let elapsed = Duration::from_secs_f32(self.elapsed);
        let shadow_bufs = self.shadow_buffers(&geom);
        let draws = self.build_draws(&geom, &shadow_bufs, elapsed);

        let w = (geom.board_rect.width * 2) as usize;
        let h = (geom.board_rect.height * 4) as usize;
        let cam = geom.framed_camera();
        let grid = composite_scene(w, h, &cam, &draws);
        draw_grid(frame.buffer_mut(), geom.board_rect, &grid);
    }

    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
        use crossterm::event::KeyCode;
        if let InputEvent::Key(key) = ev {
            // Direct selection: each digit always picks the same view,
            // never a next/prev cycle (spec 37). 1=Sideline 2=OverShoulder 3=TopDown.
            match key.code {
                KeyCode::Char('1') => self.camera_mode = BattleCamera::sideline_preset(),
                KeyCode::Char('2') => self.camera_mode = BattleCamera::over_shoulder_preset(),
                KeyCode::Char('3') => self.camera_mode = BattleCamera::top_down_preset(),
                _ => {}
            }
        }
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
    use engine_render::{draw_grid, Cell, Grid};
    use engine_core::color::Rgba;

    /// Default Sideline mode used by cases that don't care about camera
    /// mode — its inner scale is a throwaway (`board_geometry` always
    /// rebuilds scale from the area, see `BattleCamera::with_scale_dots`).
    fn sideline() -> BattleCamera {
        BattleCamera::sideline_preset()
    }

    /// Default tuning, used by every `board_geometry` call in this module.
    fn tuning() -> BattleViewerTuning {
        BattleViewerTuning::default()
    }

    /// Exact-fit area: 128x64 against an 8-column allocation (`BOARD_COLS`
    /// (7) drawn columns + 1 reserved for `BENCH_COL`, which sits one column
    /// past the drawn grid — see `BENCH_COL`'s doc comment; without this
    /// reservation bench renders past `board_rect`'s own edge and gets
    /// silently clipped, which is what "bench pieces sometimes go missing"
    /// turned out to be). TopDown mode (b4-t1): the flat integer-sizing path
    /// this pins is TopDown-only once perspective presets get a real bbox
    /// fit — re-pointed from Sideline.
    #[test]
    fn exact_fit_area() {
        let g = board_geometry(Rect::new(0, 0, 128, 64), BattleCamera::top_down_preset(), tuning());
        assert_eq!(g.cell_height_rows, 8);
        assert_eq!(g.cell_width_cols, 16);
        assert_eq!(g.board_rect, Rect::new(0, 4, 128, 56));
        assert_eq!(g.camera, BattleCamera::top_down_preset().with_scale_dots(32.0));
    }

    /// Oversized area: geometry is centered within the larger area. TopDown
    /// mode (b4-t1, re-pointed from Sideline — see `exact_fit_area`).
    #[test]
    fn oversized_area_is_centered() {
        let g = board_geometry(Rect::new(5, 5, 200, 100), BattleCamera::top_down_preset(), tuning());
        assert_eq!(g.cell_height_rows, 12);
        assert_eq!(g.board_rect, Rect::new(9, 13, 192, 84));
    }

    /// Height-constrained area: height is the limiting dimension. TopDown
    /// mode (b4-t1, re-pointed from Sideline — see `exact_fit_area`).
    #[test]
    fn height_constrained_area() {
        let g = board_geometry(Rect::new(0, 0, 300, 40), BattleCamera::top_down_preset(), tuning());
        assert_eq!(g.cell_height_rows, 5);
        assert_eq!(g.board_rect, Rect::new(110, 2, 80, 35));
    }

    /// Width-constrained area: width is the limiting dimension. TopDown mode
    /// (b4-t1, re-pointed from Sideline — see `exact_fit_area`).
    #[test]
    fn width_constrained_area() {
        let g = board_geometry(Rect::new(0, 0, 50, 300), BattleCamera::top_down_preset(), tuning());
        assert_eq!(g.cell_height_rows, 3);
        assert_eq!(g.board_rect, Rect::new(1, 139, 48, 21));
    }

    /// Tiny area clamps to cell_height_rows == 1 and does not panic.
    #[test]
    fn tiny_area_clamps_without_panic() {
        let g = board_geometry(Rect::new(0, 0, 10, 5), sideline(), tuning());
        assert_eq!(g.cell_height_rows, 1);
    }

    /// Invariant: cell_width_cols == 2 * cell_height_rows must hold for every
    /// area tested above. TopDown mode (b4-t1, re-pointed from Sideline —
    /// see `exact_fit_area`).
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
            let g = board_geometry(area, BattleCamera::top_down_preset(), tuning());
            assert_eq!(
                g.cell_width_cols,
                2 * g.cell_height_rows,
                "cell_width_cols must be 2x cell_height_rows for area {area:?}"
            );
        }
    }

    /// TopDown mode: yields the exact flat area-derived `board_rect`/cell
    /// fields (same formula `exact_fit_area` pins) AND `camera` is the
    /// TopDown variant rebuilt at the area-derived scale. (b4-t1: dropped
    /// the cross-mode equality against Sideline — perspective presets are now
    /// fit-sized, not flat, so that equality no longer holds; TopDown's own
    /// flat values are asserted directly instead.)
    #[test]
    fn top_down_mode_shares_geometry_but_has_topdown_camera() {
        let area = Rect::new(0, 0, 128, 64);
        let g_top = board_geometry(area, BattleCamera::top_down_preset(), tuning());

        assert_eq!(g_top.cell_height_rows, 8);
        assert_eq!(g_top.cell_width_cols, 16);
        assert_eq!(g_top.board_rect, Rect::new(0, 4, 128, 56));
        assert_eq!(
            g_top.camera,
            BattleCamera::top_down_preset().with_scale_dots(32.0)
        );
    }

    /// OverShoulder mode: the caller-supplied `camera_depth` is preserved
    /// (not reset/discarded) and scale is rebuilt to some positive
    /// area-derived value. (b4-t1: dropped the pinned `scale_dots == 36.0`
    /// expectation — perspective presets are now fit-to-viewport sized, so
    /// the exact scale is no longer the flat `cell_height_rows*4` value;
    /// only positivity is asserted, per the no-pinned-constant guardrail.)
    #[test]
    fn over_shoulder_mode_rebuilds_scale_and_preserves_camera_depth() {
        let area = Rect::new(0, 0, 128, 64);
        let mode = BattleCamera::OverShoulder(PerspectiveCamera {
            scale_dots: 0.0,
            depth_axis: DepthAxis::Row,
            elevation_deg: 30.0,
            camera_depth: 6.0,
            camera_height: 4.0,
            spread_center: BOARD_CENTER_COL,
            fov_deg: 55.0,
            facing_sign: -1.0,
        });
        let g = board_geometry(area, mode, tuning());

        let BattleCamera::OverShoulder(inner) = g.camera else {
            panic!("expected OverShoulder variant");
        };
        assert_eq!(inner.camera_depth, 6.0, "camera_depth must be preserved, not reset");
        assert!(
            inner.scale_dots > 0.0,
            "scale must be rebuilt to some positive area-derived value, got {}",
            inner.scale_dots
        );
    }

    /// PUBLIC_SURFACE #2 (b4-t1): for a perspective preset, the board's
    /// projected bounding box (via `geom.framed_camera()`) must be (a)
    /// CONTAINED within the composite buffer's dot rect — no negative/
    /// overflowing corner, the exact clip bug named in the spec's Purpose
    /// section — and (b) FILL the viewport along its limiting dimension.
    /// Threshold lowered from `0.9` to match `VIEWPORT_MARGIN_RATIO`
    /// (`0.72`, minus float slack): filling 100% of the buffer with the flat
    /// board corners' bbox left zero room for content that legitimately
    /// extends past those corners — a standing creature's head, or the
    /// bench piece (deliberately placed one column outside the drawn grid)
    /// — which is what "creature heads get clipped" turned out to be. Some
    /// fill-ratio is still required so the board doesn't shrink back to a
    /// tiny fraction of the screen, just not ~100%.
    fn assert_fitted_bbox_contained_and_fills(area: Rect, preset: BattleCamera) {
        let g = board_geometry(area, preset, tuning());
        let cam = g.framed_camera();
        let corners = [
            WorldPos::new(0.0, 0.0),
            WorldPos::new(BOARD_COLS as f32, 0.0),
            WorldPos::new(0.0, BOARD_ROWS as f32),
            WorldPos::new(BOARD_COLS as f32, BOARD_ROWS as f32),
        ];
        let pts: Vec<(i32, i32)> = corners.iter().map(|&p| cam.project(p)).collect();
        let min_x = pts.iter().map(|p| p.0).min().unwrap();
        let max_x = pts.iter().map(|p| p.0).max().unwrap();
        let min_y = pts.iter().map(|p| p.1).min().unwrap();
        let max_y = pts.iter().map(|p| p.1).max().unwrap();

        let avail_w = g.board_rect.width as i32 * 2;
        let avail_h = g.board_rect.height as i32 * 4;

        assert!(
            min_x >= 0 && max_x <= avail_w && min_y >= 0 && max_y <= avail_h,
            "projected board bbox [{min_x},{max_x}]x[{min_y},{max_y}] must be contained \
             in the composite buffer's dot rect [0,{avail_w}]x[0,{avail_h}]"
        );

        let span_x = (max_x - min_x) as f32;
        let span_y = (max_y - min_y) as f32;
        let fill = (span_x / avail_w as f32).max(span_y / avail_h as f32);
        assert!(
            fill >= 0.65,
            "fitted board bbox must fill at least ~65% of the viewport along its \
             limiting dimension, got {fill} (span=({span_x},{span_y}), avail=({avail_w},{avail_h}))"
        );
    }

    #[test]
    fn fitted_board_bbox_fills_viewport_sideline() {
        assert_fitted_bbox_contained_and_fills(Rect::new(0, 0, 80, 40), BattleCamera::sideline_preset());
    }

    #[test]
    fn fitted_board_bbox_fills_viewport_over_shoulder() {
        assert_fitted_bbox_contained_and_fills(
            Rect::new(0, 0, 80, 40),
            BattleCamera::over_shoulder_preset(),
        );
    }

    /// No mode ever panics, even against a tiny area.
    #[test]
    fn tiny_area_does_not_panic_for_any_mode() {
        let area = Rect::new(0, 0, 10, 5);
        let _ = board_geometry(area, sideline(), tuning());
        let _ = board_geometry(area, BattleCamera::top_down_preset(), tuning());
        let _ = board_geometry(area, BattleCamera::over_shoulder_preset(), tuning());
    }

    /// The board-size constants are exactly 7x7 and must be referenced (not
    /// re-hardcoded) by every downstream consumer.
    #[test]
    fn board_size_constants_are_7x7() {
        assert_eq!(BOARD_COLS, 7);
        assert_eq!(BOARD_ROWS, 7);
    }

    /// Row-layout constants (b2-t1): Team A bench(0)/active(1), Team B
    /// active(5)/bench(6), each active row exactly one cell inward of its
    /// team's bench row, and the layout is vertically symmetric.
    #[test]
    fn row_layout_constants_match_spec() {
        assert_eq!(TEAM_A_BENCH_ROW, 2);
        assert_eq!(TEAM_A_ROW, 1);
        assert_eq!(TEAM_B_ROW, BOARD_ROWS - 2);
        assert_eq!(TEAM_B_BENCH_ROW, BOARD_ROWS - 3);

        assert_eq!(
            TEAM_A_BENCH_ROW + TEAM_B_BENCH_ROW,
            BOARD_ROWS - 1,
            "bench rows must be vertically symmetric"
        );
        assert_eq!(
            TEAM_A_ROW + TEAM_B_ROW,
            BOARD_ROWS - 1,
            "active rows must be vertically symmetric"
        );
    }

    /// Column-layout constants (b2-t2): the 3 centered active columns have
    /// symmetric empty margins on both board edges, and the lone bench
    /// column sits at the center of that trio.
    #[test]
    fn column_layout_constants_are_centered() {
        assert_eq!(ACTIVE_COLS.len(), 3);
        assert_eq!(
            ACTIVE_COLS[1],
            ACTIVE_COLS[0] + 1,
            "active columns must be contiguous ascending"
        );
        assert_eq!(
            ACTIVE_COLS[2],
            ACTIVE_COLS[1] + 1,
            "active columns must be contiguous ascending"
        );

        let left_margin = ACTIVE_COLS[0];
        let right_margin = (BOARD_COLS - 1) - ACTIVE_COLS[2];
        assert_eq!(
            left_margin, right_margin,
            "empty margins on both board edges must be symmetric"
        );

        assert_eq!(
            BENCH_COL, BOARD_COLS,
            "bench column must sit one column past the drawn grid's far edge, \
             not among the active columns (project owner's explicit call: bench \
             reads as 'behind the field' under Sideline and 'off to the side' \
             under Over-the-shoulder only if it's on a column no active piece shares)"
        );
        assert!(
            !ACTIVE_COLS.contains(&BENCH_COL),
            "bench column must NOT be one of the 3 active columns"
        );
    }

    /// Cross-check: board_geometry's centering derivation must land at the
    /// exact same (x,y) as engine_render::draw_grid's own centering formula, when
    /// fed a Grid sized to match the geometry's board dimensions. TopDown
    /// mode (b4-t1, re-pointed from Sideline — see `exact_fit_area`).
    #[test]
    fn board_rect_matches_draw_grid_centering() {
        let area = Rect::new(5, 5, 200, 100);
        let g = board_geometry(area, BattleCamera::top_down_preset(), BattleViewerTuning::default());

        // `board_rect`'s own width now reserves `BOARD_COLS + 1` columns
        // (the extra one for `BENCH_COL`, which sits past the drawn grid —
        // see `BENCH_COL`'s doc comment), so the comparison grid here must
        // match that same width to cross-check against the right thing.
        let cols = (g.cell_width_cols * (BOARD_COLS + 1)) as usize;
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

    /// `BattleCamera::top_down_preset()` rebuilt at an arbitrary test `scale`
    /// (mirrors what `board_geometry`'s `with_scale_dots` does every frame).
    fn top_down_at(scale: f32) -> BattleCamera {
        BattleCamera::top_down_preset().with_scale_dots(scale)
    }

    /// `BattleCamera::sideline_preset()` rebuilt at an arbitrary test `scale`.
    fn sideline_at(scale: f32) -> BattleCamera {
        BattleCamera::sideline_preset().with_scale_dots(scale)
    }

    /// `BattleCamera::over_shoulder_preset()` rebuilt at an arbitrary test `scale`.
    fn over_shoulder_at(scale: f32) -> BattleCamera {
        BattleCamera::over_shoulder_preset().with_scale_dots(scale)
    }

    /// Hand-picked geometry used by every case below: board_rect origin
    /// (2,1), cell_width_cols=4, cell_height_rows=2, sized for a 7x7 grid
    /// (board_rect 28x14) in a 40x20 buffer. DotBuffer is
    /// board_rect.width*2 x board_rect.height*4 = 56x56 dots (8x8 dots per
    /// board cell, 7 cells per side — `draw_board_lines` iterates
    /// `0..=BOARD_COLS`/`0..=BOARD_ROWS`, both 7). Vertical boundaries
    /// i=0..=7 land at terminal x in {2,6,10,14,18,22,26, 29(clamped)};
    /// horizontal boundaries j=0..=7 land at terminal y in
    /// {1,3,5,7,9,11,13, 14(clamped)}. For every i<7/j<7 the boundary's dot
    /// sits at dx=0/dy=0 within its terminal cell (exact, no clamp); only the
    /// outermost i==7/j==7 boundary clamps to the LAST valid dot index
    /// (dx=1/dy=3 — one dot short of board_rect.right()/bottom()).
    ///
    /// Camera is `TopDown` (full-strength grid lines, b4-t1) so the glyph/
    /// shape assertions below stay independent of grid-line *color* — color
    /// is covered separately by the per-mode color tests further down.
    fn geom() -> BoardGeometry {
        BoardGeometry {
            cell_width_cols: 4,
            cell_height_rows: 2,
            board_rect: Rect::new(2, 1, 28, 14),
            camera: top_down_at(8.0),
            tuning: BattleViewerTuning::default(),
            screen_offset: (0, 0),
        }
    }

    /// Same geometry as `geom()` but with a caller-supplied camera, for
    /// per-mode grid-line-color tests (b4-t1).
    fn geom_with_camera(camera: BattleCamera) -> BoardGeometry {
        BoardGeometry { camera, ..geom() }
    }

    /// GRID_LINE_COLOR converted to the ratatui fg representation `draw_grid`
    /// writes it as — never a duplicated `0x55` literal.
    fn grid_fg() -> Color {
        Color::Rgb(GRID_LINE_COLOR.r, GRID_LINE_COLOR.g, GRID_LINE_COLOR.b)
    }

    /// DELIVERABLE (b1-t2): the real production blend (`Rgba::new(0xFF,0xFF,
    /// 0xFF,tuning.grid_dim_alpha).over(black)`, exactly what `draw_grid`
    /// composites over an empty board cell) must read as *meaningfully*
    /// dimmer than opaque `GRID_LINE_COLOR` — brightness sum <= ~60% of
    /// GRID_LINE_COLOR's. This is an invariant on the ratio, never a pinned
    /// "the new alpha is X" value (feature guardrail: no pinned-constant
    /// tests for visual-tuning values).
    #[test]
    fn dim_grid_reads_meaningfully_dimmer_than_opaque() {
        // Bound relaxed from an earlier 60% cap: that cap forced
        // `grid_dim_alpha` low enough to blend near-invisibly against the
        // scene's actual (non-black) background — legible-but-dimmer beats
        // dim-but-illegible (confirmed by actually rendering and looking,
        // not by this property alone). Still requires a clearly perceptible
        // reduction (>=10%), just not an aggressive one.
        let dim = Rgba::new(0xFF, 0xFF, 0xFF, BattleViewerTuning::default().grid_dim_alpha)
            .over(Rgba::rgb(0, 0, 0));
        let dim_sum = dim.r as u32 + dim.g as u32 + dim.b as u32;
        let opaque_sum = GRID_LINE_COLOR.r as u32 + GRID_LINE_COLOR.g as u32 + GRID_LINE_COLOR.b as u32;
        assert!(
            dim_sum * 100 <= opaque_sum * 90,
            "dim grid brightness sum {dim_sum} must be <= 90% of opaque brightness sum {opaque_sum}"
        );
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

    /// Point-7 fencepost resolution (chosen: clamp to the last valid dot
    /// index, one dot short): board_rect=(2,1,28,14) -> right()=30, so the
    /// outermost vertical boundary (i=7, since `draw_board_lines` iterates
    /// `0..=BOARD_COLS` and `BOARD_COLS==7`) cannot sit at dot_x=56 (one past
    /// the last valid dot 55); it clamps to dot_x=55, landing in the RIGHT
    /// dot-column (dx=1) of the LAST valid cell (terminal x=29), not at
    /// x=30. That cell's top-right corner: right dot-column full height
    /// (mask 0xB8) union top dot-row full width (mask 0x09) = mask 0xB9 ->
    /// '\u{28B9}'. Nothing is drawn at the naive unclamped position x=30.
    #[test]
    fn far_boundary_is_clamped_one_dot_short_not_out_of_bounds() {
        let g = geom();
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        draw_board_lines(&mut buf, &g);

        let clamped = buf.cell((29, 1)).unwrap();
        assert_eq!(clamped.symbol(), "\u{28B9}");
        assert_eq!(clamped.fg, grid_fg());

        assert_eq!(
            buf.cell((30, 1)).unwrap().symbol(),
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
    /// Uses `top_down_preset()` (not Sideline/OverShoulder) so corner
    /// positions stay the predictable flat-math positions regardless of
    /// camera-projection changes (b4-t1) — this test's intent is
    /// geometry-scaling, not camera-specific projection.
    #[test]
    fn two_geometries_scale() {
        let area_a = Rect::new(0, 0, 128, 64);
        let area_b = Rect::new(5, 5, 200, 100);
        let ga = board_geometry(area_a, BattleCamera::top_down_preset(), BattleViewerTuning::default());
        let gb = board_geometry(area_b, BattleCamera::top_down_preset(), BattleViewerTuning::default());
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

    /// `BattleCamera::grid_line_color` per-variant mapping (b4-t2): full
    /// strength, opaque `GRID_LINE_COLOR` for `TopDown`; a translucent
    /// `Rgba::new(0xFF,0xFF,0xFF,tuning.grid_dim_alpha)` — sourced from the
    /// live `tuning`, not a hardcoded constant — for `Sideline`/`OverShoulder`.
    #[test]
    fn battle_camera_grid_line_color_per_variant() {
        let tuning = BattleViewerTuning::default();
        let expected_dim = Rgba::new(0xFF, 0xFF, 0xFF, tuning.grid_dim_alpha);

        assert_eq!(top_down_at(8.0).grid_line_color(&tuning), GRID_LINE_COLOR);
        assert_eq!(sideline_at(8.0).grid_line_color(&tuning), expected_dim);
        assert_eq!(over_shoulder_at(8.0).grid_line_color(&tuning), expected_dim);
    }

    /// The dim color must be sourced from `tuning.grid_dim_alpha` live, not a
    /// hardcoded constant (b4-t2): a non-default alpha changes the returned
    /// dim color's alpha channel accordingly, for both dim variants.
    #[test]
    fn grid_line_color_dim_tracks_tuning_alpha() {
        let default_tuning = BattleViewerTuning::default();
        let custom_tuning = BattleViewerTuning {
            grid_dim_alpha: 0x10,
            ..default_tuning
        };
        assert_ne!(
            custom_tuning.grid_dim_alpha, default_tuning.grid_dim_alpha,
            "test setup: custom alpha must differ from the default"
        );

        assert_eq!(
            sideline_at(8.0).grid_line_color(&custom_tuning),
            Rgba::new(0xFF, 0xFF, 0xFF, custom_tuning.grid_dim_alpha),
            "Sideline dim color must track a non-default tuning.grid_dim_alpha"
        );
        assert_eq!(
            over_shoulder_at(8.0).grid_line_color(&custom_tuning),
            Rgba::new(0xFF, 0xFF, 0xFF, custom_tuning.grid_dim_alpha),
            "OverShoulder dim color must track a non-default tuning.grid_dim_alpha"
        );
    }

    /// Top-down mode must render grid lines at full `GRID_LINE_COLOR`
    /// strength (b4-t1).
    #[test]
    fn topdown_grid_lines_render_full_strength() {
        let g = geom_with_camera(top_down_at(8.0));
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        draw_board_lines(&mut buf, &g);

        let cell = buf.cell((2, 1)).unwrap();
        assert_eq!(cell.fg, grid_fg());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: pieces() + world_pos_for_cell (b4-t2)
// ─────────────────────────────────────────────────────────────────────────────

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

#[cfg(test)]
mod piece_render_tests {
    use super::*;
    use image::{DynamicImage, Rgba as PixelRgba, RgbaImage};

    /// A `BattleCamera` variant rebuilt at an arbitrary test `scale` — only
    /// `scale_dots` matters for the tests using this helper.
    fn at_scale(make: fn() -> BattleCamera, scale: f32) -> BattleCamera {
        make().with_scale_dots(scale)
    }

    /// A uniform fully-opaque RGBA image (source for the synthetic sprites
    /// below) — deterministic, unlike the real GIF asset.
    fn opaque_image(w: u32, h: u32) -> DynamicImage {
        let mut raw = RgbaImage::new(w, h);
        for p in raw.pixels_mut() {
            *p = PixelRgba([200, 200, 200, 255]);
        }
        DynamicImage::from(raw)
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

    /// DELIVERABLE (4): `sprite_base_dot_rows` is a fixed, documented ratio of
    /// the active `BattleCamera` variant's dot scale, pinned against the
    /// `SPRITE_DOT_RATIO` constant — for all 3 camera variants, not just
    /// Sideline (b3-t2: `sprite_base_dot_rows` now takes `&BattleCamera`).
    #[test]
    fn sprite_base_dot_rows_matches_ratio_constant() {
        // TopDown is orthographic — `sprite_scale_dots()` IS `scale_dots`
        // directly, so `sprite_base_dot_rows` is a fixed ratio of `scale`.
        for scale in [8.0f32, 32.0f32, 5.0f32] {
            let camera = at_scale(BattleCamera::top_down_preset, scale);
            let expected = (scale * SPRITE_DOT_RATIO).round() as u32;
            assert_eq!(
                sprite_base_dot_rows(&camera),
                expected,
                "sprite_base_dot_rows must equal (sprite_scale_dots() * SPRITE_DOT_RATIO).round() \
                 for scale {scale} and camera {camera:?}"
            );
        }
    }

    /// `sprite_base_dot_rows` for the perspective presets is a fixed ratio of
    /// `sprite_scale_dots()` too — but `sprite_scale_dots()` itself is now the
    /// camera's `dots_per_world_unit` at its near reference depth (NOT the
    /// raw `scale_dots` field, which is an NDC-to-dots constant, not a
    /// per-world-unit rate — using it directly is what overflowed sprite
    /// width under Over-the-shoulder).
    #[test]
    fn sprite_base_dot_rows_matches_ratio_constant_for_perspective_presets() {
        for camera in [
            at_scale(BattleCamera::sideline_preset, 8.0),
            at_scale(BattleCamera::over_shoulder_preset, 5.0),
        ] {
            let expected = (camera.sprite_scale_dots() * SPRITE_DOT_RATIO).round() as u32;
            assert_eq!(
                sprite_base_dot_rows(&camera),
                expected,
                "sprite_base_dot_rows must equal (sprite_scale_dots() * SPRITE_DOT_RATIO).round() \
                 for camera {camera:?}"
            );
        }
    }

    /// `BattleCamera::sprite_scale_dots` for `TopDown` must equal the active
    /// variant's own `scale_dots` (orthographic — that field already IS a
    /// per-world-unit rate).
    #[test]
    fn sprite_scale_dots_matches_active_variant_scale_for_top_down() {
        assert_eq!(at_scale(BattleCamera::top_down_preset, 32.0).sprite_scale_dots(), 32.0);
    }

    /// `BattleCamera::sprite_scale_dots` for the perspective presets must
    /// equal `dots_per_world_unit` at the camera's own near reference depth —
    /// NOT the raw `scale_dots` field (that was the sprite-overflow bug:
    /// `scale_dots` is an NDC-to-dots constant solved by viewport-fit, not a
    /// per-world-unit rate, and is typically much larger than the real rate
    /// at any actual position).
    #[test]
    fn sprite_scale_dots_matches_dots_per_world_unit_at_near_ref_for_perspective_presets() {
        let sideline = at_scale(BattleCamera::sideline_preset, 8.0);
        let BattleCamera::Sideline(inner) = sideline else { panic!("expected Sideline") };
        assert_eq!(
            sideline.sprite_scale_dots(),
            inner.dots_per_world_unit(near_reference_forward_distance(&inner)),
            "Sideline's sprite_scale_dots must equal dots_per_world_unit at its own near reference"
        );

        let over_shoulder = at_scale(BattleCamera::over_shoulder_preset, 5.0);
        let BattleCamera::OverShoulder(inner) = over_shoulder else { panic!("expected OverShoulder") };
        assert_eq!(
            over_shoulder.sprite_scale_dots(),
            inner.dots_per_world_unit(near_reference_forward_distance(&inner)),
            "OverShoulder's sprite_scale_dots must equal dots_per_world_unit at its own near reference"
        );

        // And it must NOT equal the raw scale_dots field — that's exactly
        // the units-mismatch bug this fixes.
        assert_ne!(
            sideline.sprite_scale_dots(),
            8.0,
            "sprite_scale_dots must not be the raw scale_dots field for a perspective preset"
        );
    }

    /// b1-t3 DELIVERABLE: a Top-Down piece sprite's rasterized dot footprint
    /// (height AND width) must be contained within its own cell's dot bounds.
    /// At today's `SPRITE_DOT_RATIO = 1.2` this overflows (height already
    /// exceeds `cell_height_rows*4`, and width overflows further for
    /// non-square idle GIFs, e.g. frost_lizard's ~1.58 aspect) — this is an
    /// invariant on containment, never a pinned "ratio is now X" value.
    #[test]
    fn top_down_sprite_footprint_fits_cell() {
        let area = Rect::new(0, 0, 80, 40);
        let geom = board_geometry(area, BattleCamera::top_down_preset(), BattleViewerTuning::default());
        let cell_h = geom.cell_height_rows as u32 * 4;
        let cell_w = geom.cell_width_cols as u32 * 2;
        let base = sprite_base_dot_rows(&geom.camera);

        assert!(
            base <= cell_h,
            "sprite_base_dot_rows ({base}) must fit within the cell's dot height ({cell_h}) \
             at SPRITE_DOT_RATIO={SPRITE_DOT_RATIO}"
        );

        for creature in crate::creatures::all() {
            let sprite = creature
                .animation(AnimationKind::Idle)
                .expect("every creature must have an Idle animation registered");
            let buf = sprite.rasterize_at(Duration::ZERO, &Transform::default(), base);
            assert!(
                buf.rows() as u32 <= cell_h && buf.cols() as u32 <= cell_w,
                "{}'s idle sprite footprint ({}x{} dots) must fit within the cell's dot bounds \
                 ({cell_w}x{cell_h}) at SPRITE_DOT_RATIO={SPRITE_DOT_RATIO} (base_dot_rows={base})",
                creature.name(),
                buf.cols(),
                buf.rows(),
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: depth-scale factor + per-piece scale wiring (b6-t1)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod depth_scale_tests {
    use super::*;
    use image::{DynamicImage, Rgba as PixelRgba, RgbaImage};

    /// Synthetic square sprite — only relative rasterized dimensions matter
    /// for this module's size-comparison assertions, not pixel content.
    fn solid_sprite(size: u32) -> AnimatedSprite {
        let mut raw = RgbaImage::new(size, size);
        for p in raw.pixels_mut() {
            *p = PixelRgba([200, 200, 200, 255]);
        }
        AnimatedSprite::new(vec![DynamicImage::from(raw)], Duration::from_millis(100))
    }

    /// Top-Down has no `PerspectiveCamera` backing it, so `depth_scale_factor`
    /// is exactly `1.0` for any `pos`, in-board or not.
    #[test]
    fn top_down_factor_always_one() {
        let mode = BattleCamera::top_down_preset();
        for pos in [
            WorldPos::new(0.5, 0.5),
            WorldPos::new(3.5, 3.5),
            WorldPos::new(6.5, 6.5),
            WorldPos::new(-10.0, 40.0),
        ] {
            assert_eq!(
                depth_scale_factor(&mode, pos),
                1.0,
                "Top-Down depth_scale_factor must be exactly 1.0 at pos {pos:?}"
            );
        }
    }

    /// Distance-based size now lives entirely in `sprite_base_dot_rows_width_fill`
    /// (spec 41 Decision 4's single distance term) — `depth_scale_factor` was
    /// deprecated to a constant `1.0` once that became position-dependent in
    /// its own right, to avoid double-applying the same falloff. Under
    /// Over-the-shoulder (depth axis = Row), the camera sits behind Team B
    /// (past Row `BOARD_ROWS`, the high side), so the near edge is Row
    /// `BOARD_ROWS`, not Row 0.0 — width-fill sizing strictly decreases
    /// moving away from it.
    #[test]
    fn over_shoulder_width_fill_decreases_with_distance() {
        // The bare preset's `scale_dots` is a `0.0` placeholder (only
        // `board_geometry`'s fit-to-viewport ever sets a real value) — with
        // it at 0.0, `local_dots_per_world_unit` is 0.0 everywhere and every
        // width-fill result clamps to the same `1` floor, hiding the
        // distance falloff this test exists to check. Rebuild at an
        // arbitrary nonzero scale first, the same way `piece_render_tests`'s
        // `at_scale` helper does.
        let mode = BattleCamera::over_shoulder_preset().with_scale_dots(40.0);
        let spread = 3.5;
        let aspect = 1.0;

        let near = sprite_base_dot_rows_width_fill(&mode, WorldPos::new(spread, BOARD_ROWS as f32 - 0.5), aspect);
        let mid = sprite_base_dot_rows_width_fill(&mode, WorldPos::new(spread, 3.5), aspect);
        let far = sprite_base_dot_rows_width_fill(&mode, WorldPos::new(spread, 0.0), aspect);

        assert!(near > mid, "near ({near}) must exceed mid ({mid})");
        assert!(mid > far, "mid ({mid}) must exceed far ({far})");
        for v in [near, mid, far] {
            assert!(v > 0, "width-fill base_dot_rows must stay positive: got {v}");
        }
    }

    /// Sideline analog of `over_shoulder_width_fill_decreases_with_distance`
    /// (depth axis = Col) — the "farther = strictly smaller" property must
    /// hold per non-Top-Down preset, not just one.
    #[test]
    fn sideline_width_fill_decreases_with_distance() {
        let mode = BattleCamera::sideline_preset().with_scale_dots(40.0);
        let spread = 3.5;
        let aspect = 1.0;

        let near = sprite_base_dot_rows_width_fill(&mode, WorldPos::new(0.5, spread), aspect);
        let mid = sprite_base_dot_rows_width_fill(&mode, WorldPos::new(3.5, spread), aspect);
        let far = sprite_base_dot_rows_width_fill(&mode, WorldPos::new(6.5, spread), aspect);

        assert!(near > mid, "near ({near}) must exceed mid ({mid})");
        assert!(mid > far, "mid ({mid}) must exceed far ({far})");
        for v in [near, mid, far] {
            assert!(v > 0, "width-fill base_dot_rows must stay positive: got {v}");
        }
    }

    /// A farther piece's rasterized sprite must be smaller than a nearer
    /// piece's under Over-the-shoulder — the visible depth cue this task
    /// adds. Only `translate`'s depth coordinate (world-y, since
    /// Over-the-shoulder's depth axis is Row) differs between the two;
    /// both are in-board positions. `base_dot_rows` now comes from
    /// `sprite_base_dot_rows_width_fill` (per-piece), not a single
    /// `depth_scaled_transform`-applied ratio.
    #[test]
    fn width_fill_sizing_shrinks_farther_piece_under_over_shoulder() {
        let mode = BattleCamera::over_shoulder_preset().with_scale_dots(40.0);
        let sprite = solid_sprite(20); // square synthetic sprite: aspect 1.0

        // Over-the-shoulder's camera sits behind Team B (past Row
        // BOARD_ROWS, the high side) — so Row 6.5 is NEAR the camera and
        // Row 0.5 is FAR from it, the opposite of the low-side presets.
        let near_pos = WorldPos::new(3.5, 6.5);
        let far_pos = WorldPos::new(3.5, 0.5);
        let near_transform = Transform { translate: near_pos, ..Transform::default() };
        let far_transform = Transform { translate: far_pos, ..Transform::default() };

        let near_rows = sprite_base_dot_rows_width_fill(&mode, near_pos, 1.0);
        let far_rows = sprite_base_dot_rows_width_fill(&mode, far_pos, 1.0);

        let near_buf = sprite.rasterize_at(Duration::ZERO, &near_transform, near_rows);
        let far_buf = sprite.rasterize_at(Duration::ZERO, &far_transform, far_rows);

        assert!(
            far_buf.cols() < near_buf.cols() && far_buf.rows() < near_buf.rows(),
            "a farther piece's rasterized sprite must be smaller: near={}x{}, far={}x{}",
            near_buf.cols(),
            near_buf.rows(),
            far_buf.cols(),
            far_buf.rows()
        );
    }

    /// Under Top-Down, no piece is ever depth-scaled smaller than another —
    /// two pieces at very different depths rasterize to identical sizes.
    #[test]
    fn depth_scaled_transform_equal_size_under_top_down() {
        let mode = BattleCamera::top_down_preset();
        let sprite = solid_sprite(20);

        let near_transform = Transform { translate: WorldPos::new(3.5, 3.5), ..Transform::default() };
        let far_transform = Transform { translate: WorldPos::new(3.5, 30.0), ..Transform::default() };

        let near_scaled = depth_scaled_transform(&near_transform, &mode);
        let far_scaled = depth_scaled_transform(&far_transform, &mode);

        let near_buf = sprite.rasterize_at(Duration::ZERO, &near_scaled, 20);
        let far_buf = sprite.rasterize_at(Duration::ZERO, &far_scaled, 20);

        assert_eq!(
            (near_buf.cols(), near_buf.rows()),
            (far_buf.cols(), far_buf.rows()),
            "Top-Down must not apply any depth-based size difference"
        );
    }

    /// Load-bearing regression (research.md refinement): `depth_scaled_transform`
    /// must MULTIPLY the depth factor into `transform`'s OWN existing scale
    /// (which may already carry a partial Die-shrink tween written by
    /// `drive_events`), never overwrite it from `Team::scale_x()`/`1.0`. A
    /// transform with an already-halved scale (simulating a mid-Die tween)
    /// must come out at exactly half of the untweened result — not silently
    /// popping back to full size.
    #[test]
    fn depth_scaled_transform_preserves_existing_tweened_scale() {
        let mode = BattleCamera::over_shoulder_preset();
        let pos = WorldPos::new(3.5, 6.5);

        let full_scale_transform = Transform {
            translate: pos,
            scale: Vec2::new(1.0, 1.0),
            ..Transform::default()
        };
        let half_scale_transform = Transform {
            translate: pos,
            scale: Vec2::new(0.5, 0.5),
            ..Transform::default()
        };

        let full_scaled = depth_scaled_transform(&full_scale_transform, &mode);
        let half_scaled = depth_scaled_transform(&half_scale_transform, &mode);

        assert_eq!(
            half_scaled.scale.x,
            full_scaled.scale.x * 0.5,
            "a half-tweened scale.x must remain exactly half the untweened result, not be overwritten"
        );
        assert_eq!(
            half_scaled.scale.y,
            full_scaled.scale.y * 0.5,
            "a half-tweened scale.y must remain exactly half the untweened result, not be overwritten"
        );
        assert_eq!(half_scaled.translate, pos, "translate must pass through unchanged");
        assert_eq!(half_scaled.rotation, 0.0, "rotation must pass through unchanged (billboarding invariant)");
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

    /// b4-t1 regression guard: every `demo_events()` `piece_index` must
    /// resolve to a real `Piece` under `pieces()`'s current numbering — a
    /// future roster resize must not silently strand a demo event pointing
    /// at a piece that no longer exists. Uses the module's `.find`/`.any(|p|
    /// p.index == ..)` resolution idiom (never positional `pieces[i]`
    /// indexing, per the module doc's stable-index convention).
    #[test]
    fn default_events_piece_indices_resolve_to_real_pieces() {
        let ps = pieces();
        let scene = BattleViewer::default();
        for e in &scene.events {
            let pi = match e.kind {
                EventKind::Move { piece_index, .. } => piece_index,
                EventKind::Die { piece_index } => piece_index,
            };
            assert!(
                ps.iter().any(|p| p.index == pi),
                "event {:?} references piece_index {pi}, which does not resolve to any \
                 Piece in pieces() (valid indices: {:?})",
                e,
                ps.iter().map(|p| p.index).collect::<Vec<_>>()
            );
        }
    }

    /// b4-t1: pins the demo's intended semantics — the authored Move targets
    /// a Team A piece, the authored Die targets a Team B piece — derived from
    /// `pieces()`, not bare literals.
    #[test]
    fn default_events_move_targets_team_a_and_die_targets_team_b() {
        let ps = pieces();
        let scene = BattleViewer::default();
        let move_index = scene
            .events
            .iter()
            .find_map(|e| match e.kind {
                EventKind::Move { piece_index, .. } => Some(piece_index),
                _ => None,
            })
            .expect("a Move event must be present");
        let die_index = scene
            .events
            .iter()
            .find_map(|e| match e.kind {
                EventKind::Die { piece_index } => Some(piece_index),
                _ => None,
            })
            .expect("a Die event must be present");

        let move_piece = ps
            .iter()
            .find(|p| p.index == move_index)
            .expect("Move's piece_index must resolve to a real piece");
        let die_piece = ps
            .iter()
            .find(|p| p.index == die_index)
            .expect("Die's piece_index must resolve to a real piece");

        assert_eq!(
            move_piece.team,
            Team::A,
            "authored Move should target a Team A piece, got {:?}",
            move_piece.team
        );
        assert_eq!(
            die_piece.team,
            Team::B,
            "authored Die should target a Team B piece, got {:?}",
            die_piece.team
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
    use engine_core::scene::EngineCtx;

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
            (ACTIVE_COLS[0], TEAM_A_ROW),
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
        let mut scene = scene_with_single_move(1.0, 1.0, (5, TEAM_A_ROW));
        let from = world_pos_for_cell(ACTIVE_COLS[0], TEAM_A_ROW);
        let to = world_pos_for_cell(5, TEAM_A_ROW);

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
    use engine_core::scene::EngineCtx;

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
    use engine_core::scene::EngineCtx;

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

        let move_from = world_pos_for_cell(ACTIVE_COLS[0], TEAM_A_ROW);
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
            (ACTIVE_COLS[2], TEAM_B_ROW),
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
// real board + 4v4 (3 active + 1 bench per side, 8 pieces total) team-tinted
// idle-animating pieces.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod battle_viewer_scene_wiring_tests {
    use super::*;
    use engine_core::scene::{EngineCtx, Scene};
    use crate::scenes::test_util::render_to_buffer;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;
    use engine_render::camera::Camera;

    /// Whatever `camera.grid_line_color()` + the real alpha-blit currently
    /// produce, blended the same way `draw_grid` blends it over the
    /// `Color::Reset` fallback. Used purely to identify/skip grid-line cells
    /// in the piece-tint assertions below, so those assertions stay correct
    /// regardless of `grid_line_color`'s implementation state (b4-t2).
    fn actual_grid_line_fg(camera: &BattleCamera, tuning: &BattleViewerTuning) -> Color {
        let blended = camera.grid_line_color(tuning).over(Rgba::rgb(0, 0, 0));
        Color::Rgb(blended.r, blended.g, blended.b)
    }

    /// A cell's symbol is a braille glyph (U+2800..=U+28FF) — i.e. sprite
    /// content, not a board-line character (┌─┼ etc.) or blank background.
    fn is_braille_glyph(sym: &str) -> bool {
        sym.chars().any(|c| ('\u{2800}'..='\u{28FF}').contains(&c))
    }

    /// DELIVERABLE (4): idle animation actually advances — after `update()`
    /// accumulates enough elapsed time to cross a frame boundary, at least
    /// one previously-lit board cell changes.
    ///
    /// b3-t1: pinned to `top_down_preset()` (not the scene's Sideline
    /// default) — Sideline/OverShoulder's on-screen framing is not fixed
    /// until b4-t1 lands, so a camera-agnostic assertion like this one must
    /// run against the one preset whose framing is stable (Top-Down, guarded
    /// byte-for-byte by the b0-t1 golden fixture).
    #[test]
    fn idle_animation_advances_after_update() {
        let mut scene = BattleViewer {
            camera_mode: BattleCamera::top_down_preset(),
            ..BattleViewer::default()
        };
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area, BattleCamera::top_down_preset(), BattleViewerTuning::default());

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

    // PRUNED (battle-viewer-perspective-camera-rework/b3-t1): the former
    // `render_glyph_mask_invariant_to_tint` test (was pinned to
    // `sideline_preset()`). Same "on-screen Sideline POSITION" class as the
    // grid-line-corner and board-corner-glyph tests already pruned above:
    // the scene's single test piece projects off the composite buffer under
    // `sideline_preset()` until b4-t1 adds the fit-to-viewport centering
    // offset (explicitly out of this task's scope). Re-pointing to
    // `top_down_preset()` was tried and does NOT fix it either — Top-Down
    // trips a separate, pre-existing, camera-independent defect in
    // `cell_from_dots_tinted`'s adaptive-luma mask threshold
    // (`crates/engine/render/src/dots.rs`: a shadow's team-colored dots and
    // the piece's own differently-colored sprite dots sharing a terminal
    // cell can flip a mask bit when team color changes; reproduced verbatim
    // on pre-b3-t1 `main`). No in-scope camera choice satisfies this test
    // under either preset, so it is deferred rather than chased: re-establish
    // once b4-t1 lands Sideline framing AND the dots.rs mask bug (b1-t4/
    // b7-t1 territory) is fixed independently.

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

    /// b3-t1 DELIVERABLE: the rendered sprite must be placed at the piece's
    /// own stored, independently-editable `transform.translate`, not re-derive
    /// the position fresh from `col`/`row` every call. Mutating
    /// `pieces[0].transform.translate` to a distinct in-board world position
    /// must move the rendered sprite glyph on the very next `render()`: a
    /// glyph appears near the NEW projected cell, and none remains near the
    /// OLD col/row-derived cell.
    ///
    /// Pinned to `top_down_preset()` (not the scene's Sideline default) —
    /// see `idle_animation_advances_after_update`'s doc note: Sideline's
    /// on-screen framing isn't fixed until b4-t1, so this camera-agnostic
    /// "reads live stored state" contract is only reliably checkable under
    /// Top-Down's stable framing.
    #[test]
    fn render_reflects_mutated_stored_piece_transform_translate() {
        let mut scene = BattleViewer {
            camera_mode: BattleCamera::top_down_preset(),
            ..BattleViewer::default()
        };
        scene.pieces.truncate(1); // isolate to exactly one sprite on the board
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area, BattleCamera::top_down_preset(), BattleViewerTuning::default());

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
        let grid_line_fg = actual_grid_line_fg(&geom.camera, &BattleViewerTuning::default());

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
        // Default scene camera is Sideline, which renders grid lines dimmed (b4-t1).
        let grid_line_fg = actual_grid_line_fg(&geom.camera, &BattleViewerTuning::default());
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

    /// Whole-board-rect equality check: `true` iff every cell's symbol AND fg
    /// color match between the two buffers. Used to compare a piece's
    /// `alive == false` render against a ground-truth render where that piece
    /// is entirely absent from `pieces` (not just flagged dead) — a strictly
    /// stronger, geometry-agnostic proof of "contributes NO glyph" than a
    /// spatial probe window, which (per b5-t1's real bundled creature art)
    /// can false-positive when a neighboring piece's own, unrelated sprite
    /// footprint happens to reach into the probed box.
    fn boards_pixel_identical(a: &Buffer, b: &Buffer, geom: &BoardGeometry) -> bool {
        for y in geom.board_rect.y..geom.board_rect.bottom() {
            for x in geom.board_rect.x..geom.board_rect.right() {
                let ca = a.cell((x, y)).unwrap();
                let cb = b.cell((x, y)).unwrap();
                if ca.symbol() != cb.symbol() || ca.fg != cb.fg {
                    return false;
                }
            }
        }
        true
    }

    /// b2-t4 DELIVERABLE: a piece with `alive == false` contributes NO glyph
    /// to the composited render — its render is pixel-identical to a scene
    /// where that piece is removed from `pieces` entirely — while a still-
    /// alive sibling's glyphs remain present. `transform` is left intact on
    /// the dead piece (not driven through a real `Die` event) so the ONLY
    /// reason it can vanish is `render()`'s `alive` filter, not a collapsed
    /// zero scale.
    ///
    /// Compares against "piece removed from the list" rather than probing a
    /// fixed-size window around the dead piece's own center: with real
    /// bundled creature art (b5-t1), a neighboring piece's sprite can be wide
    /// enough that its footprint reaches into a window centered on an
    /// adjacent bench piece, producing a false "glyph still present" positive
    /// unrelated to the `alive` filter under test. Diffing the FULL board
    /// against a ground-truth render with the piece entirely absent isolates
    /// exactly and only that piece's own contribution, regardless of
    /// neighboring sprites' size.
    /// Pinned to `top_down_preset()` (not the scene's Sideline default) —
    /// see `idle_animation_advances_after_update`'s doc note.
    #[test]
    fn render_excludes_dead_piece_keeps_alive_sibling() {
        let mut scene = BattleViewer {
            camera_mode: BattleCamera::top_down_preset(),
            ..BattleViewer::default()
        };
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area, BattleCamera::top_down_preset(), BattleViewerTuning::default());

        assert_eq!(scene.pieces[3].team, Team::A, "test setup: target must be Team A");
        assert_eq!(scene.pieces[7].team, Team::B, "test setup: sibling must be Team B");
        let sibling_center = terminal_center_cell(scene.pieces[7].transform.translate, &geom);

        let mut scene_removed = BattleViewer {
            camera_mode: BattleCamera::top_down_preset(),
            ..BattleViewer::default()
        };
        scene_removed.pieces.remove(3);
        let buf_removed = render_to_buffer(&scene_removed, 100, 50);

        scene.pieces[3].alive = false;
        let buf_dead = render_to_buffer(&scene, 100, 50);

        assert!(
            boards_pixel_identical(&buf_dead, &buf_removed, &geom),
            "a dead piece (alive == false) must render pixel-identical to that piece being \
             entirely absent from `pieces` — no residual glyph anywhere on the board"
        );
        assert!(
            has_piece_glyph_near(&buf_dead, &geom, sibling_center),
            "a still-alive sibling's glyphs must remain present near {sibling_center:?}"
        );
    }

    /// b2-t4 DELIVERABLE (revive, no special-casing): flipping a previously
    /// excluded piece's `alive` back to `true` (transform untouched) makes
    /// its glyphs reappear on the very next `render()`, with no other code
    /// change — proving exclusion is a pure per-frame filter on `alive`, not
    /// a one-way/sticky removal.
    ///
    /// Setup precondition ("dead piece contributes nothing") and the revive
    /// assertion both use the pixel-identical-to-piece-removed comparison —
    /// see `render_excludes_dead_piece_keeps_alive_sibling` for why a
    /// spatial probe window is unreliable against real bundled creature art
    /// (a neighboring piece's wider sprite can bleed into the window).
    /// Pinned to `top_down_preset()` (not the scene's Sideline default) —
    /// see `idle_animation_advances_after_update`'s doc note.
    #[test]
    fn render_reincludes_piece_when_alive_flipped_back_true() {
        let mut scene = BattleViewer {
            camera_mode: BattleCamera::top_down_preset(),
            ..BattleViewer::default()
        };
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area, BattleCamera::top_down_preset(), BattleViewerTuning::default());

        let mut scene_removed = BattleViewer {
            camera_mode: BattleCamera::top_down_preset(),
            ..BattleViewer::default()
        };
        scene_removed.pieces.remove(3);
        let buf_removed = render_to_buffer(&scene_removed, 100, 50);

        scene.pieces[3].alive = false;
        let buf_dead = render_to_buffer(&scene, 100, 50);
        assert!(
            boards_pixel_identical(&buf_dead, &buf_removed, &geom),
            "test setup: piece must render pixel-identical to piece-removed while alive == false"
        );

        scene.pieces[3].alive = true;
        let buf_revived = render_to_buffer(&scene, 100, 50);
        assert!(
            !boards_pixel_identical(&buf_revived, &buf_removed, &geom),
            "piece glyph must reappear (render must diverge from the piece-removed ground truth) \
             once alive is flipped back to true"
        );
    }

    /// b3-t1 DELIVERABLE: `BattleViewer::default().pieces` is seeded from the
    /// same layout logic as the free `pieces()` function — a real, owned
    /// field, not a divergent copy or an empty placeholder. Count is the
    /// 8-piece (3 active + 1 bench per side) layout.
    #[test]
    fn default_seeds_eight_pieces_from_layout() {
        let scene = BattleViewer::default();
        assert_eq!(scene.pieces.len(), 8, "expected 8 seeded pieces");
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
// Tests: contact shadow draw + sprite-tint removal (b7-t1)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod contact_shadow_tests {
    use super::*;

    /// PUBLIC_SURFACE case 1 (unit on `shadow_alpha`): a standing-still
    /// piece — `elapsed == 0.0`, before `demo_events`' `start_time`s — has no
    /// active event, so `shadow_alpha` must be exactly `1.0`.
    #[test]
    fn shadow_alpha_full_when_standing_still() {
        let scene = BattleViewer::default();
        assert_eq!(
            scene.shadow_alpha(0),
            1.0,
            "a piece with no active event (elapsed before every event's start_time) must be full shadow alpha"
        );
    }

    /// PUBLIC_SURFACE case 2: sampled partway through a `Move` longer than
    /// the fade window (`demo_events`' piece-0 Move: [1.0, 2.2), fade =
    /// `shadow_fade_ms/1000 = 0.15`), `shadow_alpha` must be exactly `0.0`.
    #[test]
    fn shadow_alpha_zero_mid_move_past_fade() {
        let scene = BattleViewer { elapsed: 1.5, ..BattleViewer::default() }; // 1.0+0.15=1.15 <= 1.5 < 2.2
        assert_eq!(
            scene.shadow_alpha(0),
            0.0,
            "sampled well inside the move's window, past the fade-out, alpha must be exactly 0.0"
        );
    }

    /// PUBLIC_SURFACE case 3: immediately after the move's window closes
    /// (`t_end = 2.2`, fade window `[2.2, 2.35)`), `shadow_alpha` must be
    /// partway back up from 0 toward 1, matching the fade-in formula exactly.
    #[test]
    fn shadow_alpha_fades_back_in_after_move_window() {
        let scene = BattleViewer { elapsed: 2.3, ..BattleViewer::default() }; // (2.3 - 2.2) / 0.15 = 0.6666...
        let alpha = scene.shadow_alpha(0);
        assert!(
            alpha > 0.0 && alpha < 1.0,
            "expected a partial fade-in value in (0,1), got {alpha}"
        );
        let expected = (2.3_f32 - 2.2_f32) / 0.15_f32;
        assert!(
            (alpha - expected).abs() < 1e-3,
            "expected shadow_alpha ~= {expected}, got {alpha}"
        );
    }

    /// Helper (b1-t4): scan `buf` for its highest-alpha `Lit` dot, returning
    /// `(col, row, color)`. Used to locate `ShapeKind::Ring`'s off-center
    /// peak band without pinning `RING_PEAK` or any exact alpha value.
    fn max_alpha_dot(buf: &DotBuffer) -> (usize, usize, Rgba) {
        let mut best: Option<(usize, usize, Rgba)> = None;
        for row in 0..buf.rows() {
            for col in 0..buf.cols() {
                if let Dot::Lit(c) = buf.get(col, row) {
                    if best.is_none_or(|(_, _, b)| c.a > b.a) {
                        best = Some((col, row, c));
                    }
                }
            }
        }
        best.expect("expected at least one Lit dot in the shadow buffer")
    }

    /// PUBLIC_SURFACE case 1 (shadow_buffers, all presets — b1-t4): the
    /// shadow buffer is a team-colored `ShapeKind::Ring` — the exact
    /// geometric-center dot is the transparent hole (never `Lit`), and the
    /// buffer's max-alpha dot sits off-center in the mid band. Replaces
    /// `shadow_center_dot_full_alpha_when_standing_still`'s Ellipse-shaped
    /// contract (full alpha at dead center), which no longer holds once the
    /// shadow's primitive swaps from `Ellipse` to `Ring`.
    #[test]
    fn shadow_is_ring_signature_all_presets() {
        let scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);
        for camera in [
            BattleCamera::top_down_preset(),
            BattleCamera::sideline_preset(),
            BattleCamera::over_shoulder_preset(),
        ] {
            let geom = board_geometry(area, camera, BattleViewerTuning::default());
            let bufs = scene.shadow_buffers(&geom);
            assert!(!bufs.is_empty(), "expected at least one shadow buffer for a drawable piece");
            let buf = &bufs[0];
            let (cx, cy) = (buf.cols() / 2, buf.rows() / 2);
            assert_eq!(
                buf.get(cx, cy),
                Dot::Transparent,
                "Ring's exact geometric center must be the transparent hole, not Lit"
            );
            let (peak_col, peak_row, peak) = max_alpha_dot(buf);
            assert!(peak.a > 0, "expected the shadow's peak dot to be Lit with alpha > 0");
            assert_ne!(
                (peak_col, peak_row),
                (cx, cy),
                "the max-alpha dot must be off-center (the mid band), not the hole"
            );
        }
    }

    /// Replaces `render_reflects_mutated_stored_piece_color` (b3-t2), rewritten
    /// for `ShapeKind::Ring` (b1-t4): after this task detints the piece
    /// sprite, team color lives ONLY on the shadow — mutating a stored
    /// `piece.color` must change what `shadow_buffers` bakes into the Ring's
    /// off-center peak dot RGB (the center is now the transparent hole and
    /// can carry no RGB), proving the shadow reads live stored state instead
    /// of a fixed team default.
    #[test]
    fn shadow_buffers_reflect_mutated_stored_piece_color() {
        let mut scene = BattleViewer::default();
        for p in &mut scene.pieces {
            p.color = Rgba::rgb(11, 22, 33);
        }
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area, BattleCamera::sideline_preset(), BattleViewerTuning::default());

        let bufs = scene.shadow_buffers(&geom);
        assert!(!bufs.is_empty(), "expected at least one shadow buffer for a drawable piece");
        let buf = &bufs[0];
        let (cx, cy) = (buf.cols() / 2, buf.rows() / 2);
        assert_eq!(
            buf.get(cx, cy),
            Dot::Transparent,
            "Ring's exact geometric center must be the transparent hole and can carry no RGB"
        );
        let (_, _, peak) = max_alpha_dot(buf);
        assert_eq!(
            (peak.r, peak.g, peak.b),
            (11, 22, 33),
            "shadow's off-center peak dot RGB must reflect the mutated stored piece.color, not a team default"
        );
    }

    /// PUBLIC_SURFACE case 4: the piece's own `SpriteDraw.tint` must be
    /// `None` — team color no longer multiply-tints the piece sprite
    /// (b7-t1 detint; carried only by the shadow).
    #[test]
    fn piece_own_sprite_tint_is_none() {
        let scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area, BattleCamera::sideline_preset(), BattleViewerTuning::default());
        let shadow_bufs = scene.shadow_buffers(&geom);
        let draws = scene.build_draws(&geom, &shadow_bufs, Duration::ZERO);

        assert!(draws.len() >= 2, "expected at least one shadow+piece pair");
        for pair in draws.chunks(2) {
            let piece = &pair[1];
            assert!(
                matches!(piece.content, SpriteContent::Animated { .. }),
                "the second entry of each pair must be the piece's own Animated draw"
            );
            assert_eq!(piece.tint, None, "the piece's own SpriteDraw.tint must be None");
        }
    }

    /// PUBLIC_SURFACE case 5: each shadow entry in `build_draws`' output is
    /// `Prerasterized`, carries `tint: None`, shares `translate` with the
    /// following piece entry, and sits immediately before it (order =
    /// shadow, piece, shadow, piece, ...).
    #[test]
    fn shadow_precedes_own_piece_and_shares_translate() {
        let scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area, BattleCamera::sideline_preset(), BattleViewerTuning::default());
        let shadow_bufs = scene.shadow_buffers(&geom);
        let draws = scene.build_draws(&geom, &shadow_bufs, Duration::ZERO);

        assert_eq!(
            draws.len(),
            shadow_bufs.len() * 2,
            "expected exactly one shadow draw + one piece draw per drawable piece"
        );
        assert!(draws.len() >= 2, "expected at least one drawable piece in the default scene");

        for pair in draws.chunks(2) {
            let (shadow, piece) = (&pair[0], &pair[1]);
            assert!(
                matches!(shadow.content, SpriteContent::Prerasterized(_)),
                "shadow entry must be SpriteContent::Prerasterized"
            );
            assert_eq!(shadow.tint, None, "shadow entry's tint must be None (color is baked into the shadow shape)");
            assert_eq!(
                shadow.translate, piece.translate,
                "shadow must share the very next entry's (its own piece's) translate"
            );
        }
    }

    /// PUBLIC_SURFACE case 6: shadow vertical squash differs by camera
    /// elevation. Top-Down (`elevation_deg = 90.0`, `k = sin(90°) = 1.0`)
    /// shadows are round (`rows() == cols()`); Sideline
    /// (`elevation_deg = 10.0`, `k = sin(10°) ≈ 0.17`) shadows are
    /// flattened (`rows()` much smaller than Top-Down's for the same width).
    #[test]
    fn shadow_squash_round_topdown_flat_sideline() {
        let scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);

        let geom_top = board_geometry(area, BattleCamera::top_down_preset(), BattleViewerTuning::default());
        let geom_side = board_geometry(area, BattleCamera::sideline_preset(), BattleViewerTuning::default());
        // Shadow width is now per-piece-position (`local_dots_per_world_unit`),
        // not a single camera-independent `cell_width_cols`-derived constant
        // — so TopDown's and Sideline's `cell_width_cols` are no longer
        // expected to match for the same area (TopDown also reserves an
        // extra column for `BENCH_COL`, Sideline doesn't need to). The
        // property this test actually cares about — round vs. flattened
        // shape — doesn't depend on that equality.

        let top_bufs = scene.shadow_buffers(&geom_top);
        let side_bufs = scene.shadow_buffers(&geom_side);
        assert!(!top_bufs.is_empty() && !side_bufs.is_empty(), "expected at least one shadow buffer each");

        let (topdown, sideline) = (&top_bufs[0], &side_bufs[0]);
        assert_eq!(
            topdown.cols(), topdown.rows(),
            "Top-Down (k=1) shadow must be round: rows() == cols()"
        );
        assert!(
            sideline.rows() < topdown.rows(),
            "Sideline (k~=0.17) shadow must be flattened relative to Top-Down: sideline.rows()={} topdown.rows()={}",
            sideline.rows(), topdown.rows()
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: billboarding invariant regression (b8-t1) — every piece's own
// Animated transform is never camera-rotated and never sheared non-uniformly,
// across all 3 camera presets, standing still and with active Move/Die events.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod billboarding_invariant_tests {
    use super::*;
    use engine_core::scene::{EngineCtx, Scene};

    const EPS: f32 = 1e-5;

    fn all_presets() -> [BattleCamera; 3] {
        [
            BattleCamera::sideline_preset(),
            BattleCamera::over_shoulder_preset(),
            BattleCamera::top_down_preset(),
        ]
    }

    /// Asserts the billboarding invariant on every draw pair produced by
    /// `build_draws` for one scene/preset combination: the piece's own
    /// `Animated` transform carries `rotation == 0.0` and
    /// `|scale.x| == |scale.y|` (within EPS); the shadow entry is always
    /// `Prerasterized` (carries no `Transform` at all, so its squash can
    /// never be a sprite shear/rotation). Returns whether any `Animated`
    /// entry had a negative `scale.x` (team-mirror sign), so callers can
    /// sanity-check the abs-comparison is non-vacuous.
    fn assert_billboarding_invariant(scene: &BattleViewer, geom: &BoardGeometry, elapsed: Duration) -> bool {
        let shadow_bufs = scene.shadow_buffers(geom);
        let draws = scene.build_draws(geom, &shadow_bufs, elapsed);
        assert!(draws.len() >= 2, "expected at least one shadow+piece pair");

        let mut saw_negative_scale_x = false;
        for pair in draws.chunks(2) {
            let (shadow, piece) = (&pair[0], &pair[1]);
            assert!(
                matches!(shadow.content, SpriteContent::Prerasterized(_)),
                "shadow entry must be SpriteContent::Prerasterized (never carries a Transform)"
            );
            match &piece.content {
                SpriteContent::Animated { transform, .. } => {
                    assert_eq!(
                        transform.rotation, 0.0,
                        "piece's own sprite must never be camera-rotated; got rotation={}",
                        transform.rotation
                    );
                    let (sx, sy) = (transform.scale.x.abs(), transform.scale.y.abs());
                    assert!(
                        (sx - sy).abs() < EPS,
                        "piece's own sprite scale must be uniform in magnitude: |scale.x|={} |scale.y|={}",
                        sx, sy
                    );
                    if transform.scale.x < 0.0 {
                        saw_negative_scale_x = true;
                    }
                }
                _ => panic!("expected the piece's own draw to be SpriteContent::Animated"),
            }
        }
        saw_negative_scale_x
    }

    /// PUBLIC_SURFACE: fresh default scene, standing still (`elapsed == 0`),
    /// across all 3 camera presets — no rotation, uniform scale magnitude.
    #[test]
    fn billboard_no_rotation_uniform_scale_all_presets_standing_still() {
        let scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);

        let mut saw_negative_scale_x = false;
        for camera in all_presets() {
            let geom = board_geometry(area, camera, BattleViewerTuning::default());
            saw_negative_scale_x |= assert_billboarding_invariant(&scene, &geom, Duration::ZERO);
        }
        assert!(
            saw_negative_scale_x,
            "expected at least one Team-B piece with scale.x < 0.0 across presets, \
             otherwise the abs-magnitude comparison is vacuous"
        );
    }

    /// PUBLIC_SURFACE: the invariant must keep holding while events are
    /// actively driving pieces — `demo_events()`'s `Move` (piece 0, window
    /// `[1.0, 2.2)`) and `Die` (piece 6, window `[1.6, 2.6)`) are both active
    /// at `elapsed = 2.0`, so this exercises both a mid-glide translate tween
    /// and a mid-shrink scale tween, across all 3 camera presets.
    #[test]
    fn billboard_invariant_holds_through_active_move_and_die() {
        let mut ctx = EngineCtx;
        let mut scene = BattleViewer::default();
        scene.update(&mut ctx, Duration::from_secs_f32(2.0));

        let area = Rect::new(0, 0, 100, 50);
        for camera in all_presets() {
            let geom = board_geometry(area, camera, BattleViewerTuning::default());
            assert_billboarding_invariant(&scene, &geom, Duration::from_secs_f32(2.0));
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: per-preset vertical anchor / feet-in-the-box placement (b5-t4)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod vertical_anchor_choice_tests {
    use super::*;

    /// PUBLIC_SURFACE (research.md b5-t4): every `SpriteDraw` `build_draws`
    /// emits (both the shadow and the piece of every pair) must carry
    /// `VerticalAnchor::Bottom` for Sideline/OverShoulder (feet-in-the-box)
    /// and `VerticalAnchor::Center` for TopDown (no verticality to anchor).
    /// Exercises the full call path (not a standalone accessor), so a
    /// mis-wired or reverted call site in `build_draws` is caught directly.
    #[test]
    fn build_draws_anchor_is_bottom_for_oblique_center_for_top_down() {
        let scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);

        let cases: [(BattleCamera, VerticalAnchor); 3] = [
            (BattleCamera::sideline_preset(), VerticalAnchor::Bottom),
            (BattleCamera::over_shoulder_preset(), VerticalAnchor::Bottom),
            (BattleCamera::top_down_preset(), VerticalAnchor::Center),
        ];

        for (camera, expected_sprite_anchor) in cases {
            let geom = board_geometry(area, camera, BattleViewerTuning::default());
            let shadow_bufs = scene.shadow_buffers(&geom);
            let draws = scene.build_draws(&geom, &shadow_bufs, Duration::ZERO);
            assert!(!draws.is_empty(), "expected at least one draw for preset {camera:?}");
            // build_draws emits (shadow, sprite) pairs per piece: the shadow
            // is ALWAYS Center (a ground decal spreads symmetrically around
            // its contact point, regardless of camera), only the creature
            // sprite's anchor varies per camera.
            for (i, draw) in draws.iter().enumerate() {
                let expected = if i % 2 == 0 { VerticalAnchor::Center } else { expected_sprite_anchor };
                assert_eq!(
                    draw.vertical_anchor, expected,
                    "preset {camera:?}, draw {i} ({}): vertical_anchor must be {expected:?}",
                    if i % 2 == 0 { "shadow" } else { "sprite" }
                );
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: feet-anchored placement lands sprites at the ground point (b5-t4)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod feet_anchored_placement_tests {
    use super::*;
    use crate::scenes::test_util::{lit_dot_color, render_to_buffer};

    /// Rounding tolerance for the folded feet-vs-rendered comparison
    /// (research.md b5-t4): covers `place`'s Bottom-anchor integer dot
    /// rounding — the shadow's own outer alpha-0 padding is folded out below,
    /// so it never needs to be absorbed by this margin. Widened from `2` to
    /// `5` for spec 41's corrected Over-the-shoulder placement (Decision 1's
    /// bug fix): that preset's much taller/farther camera (needed to keep
    /// `forward_distance` positive from the correct side, behind Team B)
    /// produces a wider near/far depth-scale spread than before, so integer
    /// rounding on the far team's already-small sprite accumulates a few
    /// more dots than the old, gentler camera did.
    const FEET_TOLERANCE_DOTS: i32 = 5;

    /// True for any rendered dot color that is NOT a shade of the board's
    /// grid-line gray. `GRID_LINE_COLOR` (and its translucent Sideline/
    /// OverShoulder blends, always blended toward black) keeps R≈G≈B; both
    /// team colors (`TEAM_A_COLOR` pale gold, `TEAM_B_COLOR` pale mint) are
    /// chromatic at every alpha. Used to keep the feet-probe's bottom-up scan
    /// from landing on a board grid-line dot instead of the target piece's
    /// team-colored shadow: `draw_board_lines` draws a line through every
    /// integer world column/row, and a piece's world position (`col + 0.5`)
    /// is always within half a unit of one — under `DepthAxis::Col`
    /// (Sideline) that line lands at nearly the piece's own screen depth;
    /// under `DepthAxis::Row` (OverShoulder) it runs the full screen column.
    /// Either way it can land in the exact scanned cell, and only a
    /// color-based filter (not a tighter cell window) reliably excludes it.
    pub(super) fn is_chromatic(color: Rgba) -> bool {
        let (max, min) = (
            color.r.max(color.g).max(color.b),
            color.r.min(color.g).min(color.b),
        );
        max.saturating_sub(min) > 12
    }

    /// For a non-Top-Down preset, one Team A piece from the demo roster
    /// (rendered alone — see the isolation comment below for why): the
    /// BOTTOMMOST chromatic (team/creature-colored, never board-grid-gray)
    /// rendered dot in its exact projected dot column must land within
    /// `FEET_TOLERANCE_DOTS` of the camera-projected ground point, once the
    /// shadow buffer's own bottom transparent padding (the Ring's alpha-0
    /// outer edge) is folded out. Projects through `geom.framed_camera()`
    /// (not bare `geom.camera`) since `render()` composites through the
    /// offset-baked camera (fit-to-viewport, b4-t1) — see research.md's
    /// correction #1. Verified via decoded dots (`lit_dot_color`, checked at
    /// the exact dot column rather than a cell-floored one — see
    /// `lit_dot_color`'s docs for why a coarser scan can find an unrelated
    /// element instead of the shadow — and restricted to chromatic dots via
    /// `is_chromatic` so a board grid-line dot is never mistaken for the
    /// shadow), never a raw `Rect`/`DotRect` field comparison, per this
    /// project's alignment-verification convention.
    fn assert_piece_feet_land_at_ground_point(camera: BattleCamera, col: u16, row: u16) {
        let mut scene = BattleViewer::default();
        // Isolate to just the target piece: the demo roster's other 7 pieces
        // (and their own shadows) are close enough in screen-space under a
        // perspective camera's spread/depth divide that a neighbor's WIDE
        // shadow can bleed into this piece's scanned column even from a
        // different board column/row — not just an exact-column collision.
        // Keeping only the piece under test removes every such cross-piece
        // ambiguity; the board grid chrome (excluded via `is_chromatic`) is
        // the only other rendered element left to guard against.
        scene.pieces.retain(|p| p.col == col && p.row == row);
        // `render()` (battle_viewer.rs's `Scene` impl) builds its geometry
        // from `self.camera_mode`, NOT from a parameter — so the scene's own
        // camera must actually be switched to `camera` or `render_to_buffer`
        // below renders under whatever `BattleViewer::default()` starts on
        // (Sideline) regardless of which preset this assertion is computing
        // `expected` for.
        scene.camera_mode = camera;
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area, camera, BattleViewerTuning::default());

        let piece = scene
            .pieces
            .first()
            .expect("demo pieces() must include a Team A piece at (col, row)");

        // Fold against the CREATURE's own rasterized buffer, not the
        // shadow's: the shadow is now ALWAYS Center-anchored (a ground decal
        // spreads symmetrically around its contact point, regardless of
        // camera — see `build_draws`), so it deliberately hangs below the
        // ground point under every camera. Only the creature sprite itself
        // carries the per-camera anchor this test is actually checking.
        let sprite = scene
            .piece_sprite(piece.index)
            .expect("Team A piece at (col, row) must have a sprite");
        let anchor = geom.camera.vertical_anchor();
        let transform = depth_scaled_transform(&piece.transform, &geom.camera);
        let base_dot_rows = sprite_base_dot_rows(&geom.camera);
        let creature_buf = sprite.rasterize_at(Duration::ZERO, &transform, base_dot_rows);

        let cam = geom.framed_camera();
        let (fx, fy) = cam.project(piece.transform.translate);
        let feet_dot_col = geom.board_rect.left() as i32 * 2 + fx;
        let feet_dot_row = geom.board_rect.top() as i32 * 4 + fy;

        // Fold out the creature buffer's own bottom-lit padding: the buffer-
        // local lowest LIT row (0-indexed from the top), and its distance
        // from the buffer's own last row.
        let lowest_local = (0..creature_buf.rows())
            .rev()
            .find(|&r| (0..creature_buf.cols()).any(|c| !matches!(creature_buf.get(c, r), Dot::Transparent)))
            .expect("creature buffer must have at least one lit dot");
        let bottom_padding = (creature_buf.rows() - 1 - lowest_local) as i32;

        // `place`'s Bottom anchor sets the buffer's own lowest row at grid-y
        // `fy - 1` (dot_y = py - rows, so the last row is dot_y + rows - 1 ==
        // py - 1); `place`'s Center anchor centers it on `fy` instead, so the
        // sprite's own bottom sits roughly `rows/2` below `fy`. Either way,
        // the folded padding removes the rest of the deterministic offset,
        // leaving only rounding for FEET_TOLERANCE_DOTS to absorb.
        let expected = match anchor {
            VerticalAnchor::Bottom => feet_dot_row - 1 - bottom_padding,
            VerticalAnchor::Center => {
                feet_dot_row + (creature_buf.rows() as i32) / 2 - 1 - bottom_padding
            }
        };

        let buf = render_to_buffer(&scene, area.width, area.height);
        // Find the actual BOTTOMMOST chromatic (team/creature-colored, never
        // board-grid-gray) dot in the exact projected dot COLUMN (not floored
        // to a terminal cell). The scan is capped at `expected + tolerance`
        // (never below it): the creature's own sprite, per its anchor, never
        // renders below that line — the only thing that legitimately would
        // is the shadow's below-ground overhang (it's Center-anchored and
        // therefore spreads past the ground point on purpose), which this
        // test is not checking and must not let bleed into "the creature's
        // own bottom." The isolated single-piece scene + exact dot column +
        // chromatic filter together rule out every OTHER non-anchor source
        // of a lower dot (a neighboring piece, a board grid line).
        // `ShapeKind::Ring`'s alpha profile is a genuine annulus (spec 41
        // Decision 5) — its exact geometric center (and a dot or two either
        // side of it) is deliberately hollow (`Dot::Transparent`), not lit.
        // Scanning only the single exact `feet_dot_col` can land inside that
        // hole and find nothing even though the ring is correctly rendered
        // and correctly anchored a couple of columns either side. A small
        // horizontal window is safe here (unlike a general-purpose neighbor
        // search): the scene above was already isolated to this one piece,
        // so there is no other piece/shadow left to produce a false
        // positive — only the ring's own hollow center to route around.
        const RING_HOLE_TOLERANCE_DOTS: i32 = 3;
        let scan_floor = expected + FEET_TOLERANCE_DOTS;
        let bottommost_chromatic = (feet_dot_col - RING_HOLE_TOLERANCE_DOTS
            ..=feet_dot_col + RING_HOLE_TOLERANCE_DOTS)
            .find_map(|probe_col| {
                (0..=scan_floor.min(area.height as i32 * 4 - 1))
                    .rev()
                    .find(|&dot_row| lit_dot_color(&buf, probe_col, dot_row).is_some_and(is_chromatic))
            });

        assert_eq!(
            bottommost_chromatic.map(|observed| (observed - expected).abs() <= FEET_TOLERANCE_DOTS),
            Some(true),
            "preset {camera:?}: bottommost chromatic (team/creature-colored) dot near dot column \
             {feet_dot_col} (±{RING_HOLE_TOLERANCE_DOTS}) is {bottommost_chromatic:?}, must land \
             within {FEET_TOLERANCE_DOTS} dots of the projected ground point (row {expected}) — \
             the creature must stand in its cell, not float centered on it"
        );
    }

    /// Team A's bench piece, under Sideline.
    #[test]
    fn sideline_bench_piece_feet_land_at_ground_point() {
        assert_piece_feet_land_at_ground_point(BattleCamera::sideline_preset(), BENCH_COL, TEAM_A_BENCH_ROW);
    }

    /// Team A's bench piece, under OverShoulder.
    #[test]
    fn over_shoulder_bench_piece_feet_land_at_ground_point() {
        assert_piece_feet_land_at_ground_point(BattleCamera::over_shoulder_preset(), BENCH_COL, TEAM_A_BENCH_ROW);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: bench-piece visibility regression, all three presets (b7-t1)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod bench_visibility_tests {
    use super::*;
    use crate::scenes::test_util::render_to_buffer;
    use super::feet_anchored_placement_tests::is_chromatic;

    /// Renders the demo `pieces()` layout under `camera` both with and
    /// without the piece at `(BENCH_COL, bench_row)`, then asserts the
    /// differential (`diff_dots`, control vs. full) contains at least one
    /// chromatic (team-colored, never board-grid-gray) lit dot that only
    /// exists in the full render. `board_geometry` depends only on
    /// `area`+`camera`+`tuning` (never `pieces`), so grid lines/board chrome
    /// are byte-identical in both renders and can never produce a false
    /// positive here — the only possible dot-level differences are the
    /// removed bench piece's own sprite+shadow dots (and anything they
    /// occluded).
    fn assert_bench_piece_visible(camera: BattleCamera, bench_row: u16) {
        let full = BattleViewer {
            camera_mode: camera,
            ..BattleViewer::default()
        };

        let mut control = BattleViewer {
            camera_mode: camera,
            ..BattleViewer::default()
        };
        control.pieces.retain(|p| !(p.col == BENCH_COL && p.row == bench_row));

        assert_eq!(
            full.pieces.len(),
            control.pieces.len() + 1,
            "no bench piece matched (BENCH_COL, {bench_row}) — layout drifted, test is not \
             exercising anything"
        );

        let buf_full = render_to_buffer(&full, 100, 50);
        let buf_ctrl = render_to_buffer(&control, 100, 50);

        let diff = engine_render::diff_dots(&buf_ctrl, &buf_full);
        let visible = diff
            .mismatches
            .iter()
            .any(|m| m.actual_lit && m.actual_color.is_some_and(is_chromatic));

        assert!(
            visible,
            "preset {camera:?}: bench piece at row {bench_row} produced no distinguishable \
             chromatic dot — it is invisible/off-screen (this is exactly spec 39's 'bench not \
             visible' bug)"
        );
    }

    #[test]
    fn bench_visible_sideline() {
        assert_bench_piece_visible(BattleCamera::sideline_preset(), TEAM_A_BENCH_ROW);
        assert_bench_piece_visible(BattleCamera::sideline_preset(), TEAM_B_BENCH_ROW);
    }

    #[test]
    fn bench_visible_over_shoulder() {
        assert_bench_piece_visible(BattleCamera::over_shoulder_preset(), TEAM_A_BENCH_ROW);
        assert_bench_piece_visible(BattleCamera::over_shoulder_preset(), TEAM_B_BENCH_ROW);
    }

    #[test]
    fn bench_visible_top_down() {
        assert_bench_piece_visible(BattleCamera::top_down_preset(), TEAM_A_BENCH_ROW);
        assert_bench_piece_visible(BattleCamera::top_down_preset(), TEAM_B_BENCH_ROW);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: camera_mode default + enter()-reset contract (b5-t1)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod camera_mode_tests {
    use super::*;
    use engine_core::scene::{EngineCtx, Scene};

    fn default_camera() -> BattleCamera {
        BattleViewer::default_camera_mode()
    }

    /// `BattleViewer::default()` must start on the pinned default camera
    /// (Sideline), matching today's pre-b5-t1 hardcoded render behavior.
    #[test]
    fn default_battle_viewer_starts_on_default_camera() {
        let scene = BattleViewer::default();
        assert_eq!(
            scene.camera_mode,
            default_camera(),
            "BattleViewer::default() must initialize camera_mode to the pinned default"
        );
    }

    /// DELIVERABLE: `enter()` resets `camera_mode` to the default even if it
    /// was previously left on a different variant (TopDown) — a prior
    /// session's/scene-visit's camera choice must never leak into the next
    /// entry.
    #[test]
    fn enter_resets_camera_mode_from_top_down_to_default() {
        let mut scene = BattleViewer {
            camera_mode: BattleCamera::top_down_preset(),
            ..Default::default()
        };

        let mut ctx = EngineCtx;
        scene.enter(&mut ctx, None);

        assert_eq!(
            scene.camera_mode,
            default_camera(),
            "enter() must reset camera_mode to the default even after it was set to TopDown"
        );
    }

    /// Same reset contract, starting from OverShoulder instead of TopDown —
    /// guards against a fix that only special-cases one non-default variant.
    #[test]
    fn enter_resets_camera_mode_from_over_shoulder_to_default() {
        let mut scene = BattleViewer {
            camera_mode: BattleCamera::over_shoulder_preset(),
            ..Default::default()
        };

        let mut ctx = EngineCtx;
        scene.enter(&mut ctx, None);

        assert_eq!(
            scene.camera_mode,
            default_camera(),
            "enter() must reset camera_mode to the default even after it was set to OverShoulder"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: handle_input maps keys 1/2/3 to direct camera selection (b5-t2)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod handle_input_camera_tests {
    use super::*;
    use crate::registry::GameCatalog;
    use crate::scenes::test_util::key_event;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use engine_core::scene::manager::SceneManager;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Direct, non-cycling selection: from every starting camera_mode,
    /// pressing '1'/'2'/'3' always jumps to the same fixed variant — never a
    /// step relative to the current mode.
    #[test]
    fn digit_keys_select_camera_mode_directly_from_every_start() {
        let starts: [BattleCamera; 3] = [
            BattleCamera::sideline_preset(),
            BattleCamera::over_shoulder_preset(),
            BattleCamera::top_down_preset(),
        ];

        for start in starts {
            for (digit, expect_variant) in [('1', "Sideline"), ('2', "OverShoulder"), ('3', "TopDown")] {
                let mut scene = BattleViewer {
                    camera_mode: start,
                    ..Default::default()
                };

                let transition = scene.handle_input(key_event(KeyCode::Char(digit)));

                assert!(
                    transition.is_none(),
                    "handle_input('{digit}') must never request a scene transition"
                );

                let matched = match expect_variant {
                    "Sideline" => matches!(scene.camera_mode, BattleCamera::Sideline(_)),
                    "OverShoulder" => matches!(scene.camera_mode, BattleCamera::OverShoulder(_)),
                    "TopDown" => matches!(scene.camera_mode, BattleCamera::TopDown(_)),
                    _ => unreachable!(),
                };
                assert!(
                    matched,
                    "pressing '{digit}' from {:?} must select {expect_variant} directly, got {:?}",
                    start, scene.camera_mode
                );
            }
        }
    }

    /// A non-digit key (and an out-of-range digit) must leave camera_mode
    /// untouched and never trigger a transition.
    #[test]
    fn non_digit_key_leaves_camera_mode_unchanged() {
        for code in [KeyCode::Char('x'), KeyCode::Char('4')] {
            let mut scene = BattleViewer {
                camera_mode: BattleCamera::top_down_preset(),
                ..Default::default()
            };

            let transition = scene.handle_input(key_event(code));

            assert!(transition.is_none(), "unmapped key must not request a transition");
            assert!(
                matches!(scene.camera_mode, BattleCamera::TopDown(_)),
                "unmapped key {:?} must leave camera_mode unchanged, got {:?}",
                code,
                scene.camera_mode
            );
        }
    }

    /// BEHAVIORAL (b6-t1 checkpoint): the keypress must actually reach
    /// `BattleViewer::handle_input` through the real `app.rs`/
    /// `SceneManager::route_key` path (b1-t2's seam) for all 3 camera modes,
    /// AND must stay a purely local camera change — never a global scene
    /// transition (no pending transition queued, scene identity unchanged).
    #[test]
    fn route_key_switches_battle_viewer_camera_end_to_end() {
        let mut mgr = SceneManager::with_scene(Box::new(BattleViewer::default()), Box::new(GameCatalog));

        let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();
        terminal.draw(|f| mgr.render(f)).unwrap();
        let buf_sideline = terminal.backend().buffer().clone();

        let mut render_after = |digit: char| {
            let quit = mgr.route_key(KeyEvent::new(KeyCode::Char(digit), KeyModifiers::NONE));
            assert!(!quit, "digit key '{digit}' must not be treated as a quit key");
            assert!(
                mgr.process_pending().is_none(),
                "digit key '{digit}' must never queue a scene transition — camera switching \
                 is local to BattleViewer, not a global scene change"
            );
            assert_eq!(
                mgr.active_id(),
                SceneId::BattleViewer.into(),
                "digit key '{digit}' must leave the active scene as BattleViewer"
            );
            terminal.draw(|f| mgr.render(f)).unwrap();
            terminal.backend().buffer().clone()
        };

        let buf_over_shoulder = render_after('2');
        let buf_top_down = render_after('3');

        assert_ne!(
            buf_sideline, buf_over_shoulder,
            "routing '2' through SceneManager::route_key must reach BattleViewer::handle_input \
             and change the rendered output (Sideline -> OverShoulder)"
        );
        assert_ne!(
            buf_sideline, buf_top_down,
            "routing '3' through SceneManager::route_key must reach BattleViewer::handle_input \
             and change the rendered output (Sideline -> TopDown)"
        );
        assert_ne!(
            buf_over_shoulder, buf_top_down,
            "OverShoulder and TopDown frames must be visibly distinct from each other"
        );

        // Back to Sideline via '1' — confirms direct (non-cycling) re-selection also
        // stays local and reproduces the original default frame.
        let buf_back_to_sideline = render_after('1');
        assert_eq!(
            buf_sideline, buf_back_to_sideline,
            "routing '1' must jump directly back to Sideline, reproducing the default frame"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: BattleViewer sources 8 distinct bundled creatures (b5-t1)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod creature_sourcing_tests {
    use super::*;

    /// DELIVERABLE (primary): `BattleViewer::default().creatures` is exactly
    /// `crate::creatures::all()`, in order — proves each `Piece.index` maps
    /// 1:1 to a distinct bundled creature, not one shared sprite.
    #[test]
    fn default_creatures_match_creatures_all_in_order() {
        let scene = BattleViewer::default();
        let expected: Vec<String> = crate::creatures::all()
            .iter()
            .map(|c| c.name().to_string())
            .collect();
        let actual: Vec<String> = scene.creatures.iter().map(|c| c.name().to_string()).collect();

        assert_eq!(
            actual, expected,
            "BattleViewer::default().creatures must equal crate::creatures::all(), in order"
        );
    }

    /// DELIVERABLE: `piece_sprite(p.index)` resolves to `Some` (the piece's
    /// own idle animation) for every piece the scene starts with — the
    /// per-piece draw loop must have a distinct sprite source for each of
    /// the 8 seeded pieces, not a shared fallback.
    #[test]
    fn piece_sprite_is_some_for_every_seeded_piece_index() {
        let scene = BattleViewer::default();

        for p in &scene.pieces {
            assert!(
                scene.piece_sprite(p.index).is_some(),
                "piece_sprite({}) must be Some for every seeded piece index",
                p.index
            );
        }
    }

    /// DELIVERABLE (render-seam distinctness): two different piece indices
    /// resolve to two different creatures' idle sprites (by name lookup
    /// through `creatures::all()`), not the same shared sprite instance.
    #[test]
    fn piece_sprite_differs_by_index_across_distinct_creatures() {
        let scene = BattleViewer::default();

        let all = crate::creatures::all();
        assert_ne!(
            all[0].name(),
            all[1].name(),
            "sanity: creatures::all()[0] and [1] must be distinct creatures"
        );

        let sprite0 = scene
            .piece_sprite(0)
            .expect("piece_sprite(0) must be Some");
        let sprite1 = scene
            .piece_sprite(1)
            .expect("piece_sprite(1) must be Some");

        assert_ne!(
            sprite0.frame_count(),
            0,
            "sanity: index-0 sprite must have at least one frame"
        );
        assert_ne!(
            sprite1.frame_count(),
            0,
            "sanity: index-1 sprite must have at least one frame"
        );

        // The two pieces' underlying creatures must be genuinely distinct
        // (per `scene.creatures[0].name() != scene.creatures[1].name()`),
        // proving `piece_sprite` sources per-index art rather than one
        // shared sprite repeated across every piece.
        assert_ne!(
            scene.creatures[0].name(),
            scene.creatures[1].name(),
            "piece_sprite(0) and piece_sprite(1) must source from distinct creatures"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: `#[derive(Inspectable)]` on Piece/Team/BattleViewer (b5-t1)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod inspectable_tests {
    use super::*;
    use engine_core::{FieldSchema, FieldTag, PatchError};

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

    /// DELIVERABLE (b2-t1): `BattleViewerTuning::default()` returns exactly
    /// the spec Decision 1 values. `grid_dim_alpha` is excluded (b1-t2): it
    /// is a visual-tuning value chosen by rendering, guarded instead by
    /// `dim_grid_reads_meaningfully_dimmer_than_opaque`'s invariant, never a
    /// pinned constant here. `grid_taper_*` (b3-t1) and `depth_scale_*`
    /// (b6-t1) no longer exist — depth scaling is derived from the camera's
    /// own `forward_distance`, not a tunable.
    #[test]
    fn battle_viewer_tuning_default_matches_spec_values() {
        let tuning = BattleViewerTuning::default();
        assert_eq!(tuning.shadow_fade_ms, 150);
    }

    /// DELIVERABLE (b2-t1): `BattleViewer::schema()` has a `tuning` child
    /// that is a `Struct` with the 2 remaining tuning leaves (b3-t1:
    /// `grid_taper_*` removed; b6-t1: `depth_scale_*` removed), none readonly.
    #[test]
    fn battle_viewer_schema_exposes_editable_tuning_struct() {
        let schema = BattleViewer::schema();
        let tuning = field(&schema, "tuning");
        assert_eq!(tuning.tag, FieldTag::Struct);

        for name in ["grid_dim_alpha", "shadow_fade_ms"] {
            let leaf = field(tuning, name);
            assert!(!leaf.readonly, "tuning.{name} must be editable, not readonly");
        }
    }

    /// DELIVERABLE (b2-t1): `apply_patch` on a `BattleViewer` value for
    /// `"tuning.shadow_fade_ms"` edits only that leaf — `grid_dim_alpha`
    /// (another tuning leaf) is unchanged. Repointed from the now-removed
    /// `tuning.depth_scale_min` (b6-t1).
    #[test]
    fn battle_viewer_apply_patch_on_tuning_leaf_edits_only_that_field() {
        let mut scene = BattleViewer::default();

        scene
            .apply_patch("tuning.shadow_fade_ms", serde_json::json!(999))
            .expect("apply_patch on tuning.shadow_fade_ms must succeed");

        let snap = scene.snapshot();
        let tuning = snap
            .as_object()
            .expect("BattleViewer snapshot must be a JSON object")
            .get("tuning")
            .expect("tuning key must be present in snapshot")
            .as_object()
            .expect("tuning must be a JSON object");

        assert_eq!(
            tuning.get("shadow_fade_ms").and_then(|v| v.as_u64()),
            Some(999),
            "shadow_fade_ms must reflect the patch"
        );
        assert_eq!(
            tuning.get("grid_dim_alpha").and_then(|v| v.as_u64()),
            Some(u64::from(BattleViewerTuning::default().grid_dim_alpha)),
            "grid_dim_alpha must be untouched by a shadow_fade_ms patch"
        );
    }

    /// DELIVERABLE: `BattleViewer::schema()` reports `elapsed` as an
    /// editable `Float` and `pieces` as a `List` of `Piece`-shaped elements;
    /// the `#[inspect(hidden)]` `creatures` field (b5-t1's per-piece
    /// creature catalog, replacing the old shared `sprite`) is absent
    /// entirely.
    #[test]
    fn battle_viewer_schema_reports_editable_elapsed_and_pieces_list_hides_creatures() {
        let schema = BattleViewer::schema();
        assert_eq!(schema.tag, FieldTag::Struct);

        let names: Vec<&str> = schema.children.iter().map(|c| c.name.as_str()).collect();
        assert!(
            !names.contains(&"creatures"),
            "hidden creatures field must be absent from schema: {names:?}"
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

    /// DELIVERABLE: the 8-piece layout round-trips through `snapshot()` as a
    /// `pieces` array of exactly 8 Piece-shaped objects, and the hidden
    /// `creatures` field (b5-t1) never appears in the snapshot either.
    #[test]
    fn battle_viewer_default_snapshot_has_eight_piece_shaped_elements_and_hides_creatures() {
        let scene = BattleViewer::default();
        let snap = scene.snapshot();
        let obj = snap.as_object().expect("BattleViewer snapshot must be a JSON object");

        assert!(
            !obj.contains_key("creatures"),
            "hidden creatures field must be absent from snapshot"
        );

        let pieces = obj
            .get("pieces")
            .expect("pieces key must be present")
            .as_array()
            .expect("pieces snapshot must be an array");
        assert_eq!(pieces.len(), 8, "expected 8 seeded pieces in the snapshot");
        for p in pieces {
            let p = p.as_object().expect("each piece snapshot must be an object");
            for key in ["col", "row", "team", "index", "transform", "color"] {
                assert!(p.contains_key(key), "piece snapshot missing key `{key}`");
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: Top-Down golden fixture lock (b0-t1)
//
// Locks Top-Down's rendered output for the camera rework: Top-Down's
// PROJECTION (`board_geometry`/`place`/`BattleCamera`'s formula) is frozen for
// the whole feature. Top-Down's rendered PIXELS are legitimately re-baselined
// at b1's two intentional global visual-tuning changes (b1-t3 sprite-dot
// ratio, b1-t4 shadow shape) — both repaint Top-Down without touching its
// projection. From b2 onward this fixture is locked byte-for-byte: every task
// that touches shared code must re-assert byte-identity against it for
// Top-Down, and any divergence is a regression. Mirrors `main_hub.rs`'s
// `golden_fixture_tests` precedent one-for-one.
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod golden_fixture_tests {
    use super::*;
    use crate::scenes::test_util::{
        buffer_to_art, load_battle_viewer_fixture, render_to_buffer, serialize_braille_buffer,
    };
    use engine_render::diff_dots;

    /// Top-Down, demo `pieces()` layout, `elapsed = 0.0`, rendered 80x40.
    /// Must dot-for-dot match the committed baseline fixture (re-baselined at
    /// b1-t3 sprite-ratio and b1-t4 shadow-shape; locked byte-for-byte from b2
    /// onward). Run with `UPDATE_BATTLE_VIEWER_FIXTURES=1` to (re)generate the
    /// `.fixture` + `.preview.txt` from the current render — only do this at
    /// one of the two designated b1 re-baseline points, and only after a
    /// manual visual pass over the preview (see
    /// `crates/game/tests/fixtures/battle_viewer/README.md`).
    #[test]
    fn top_down_golden_matches_baseline() {
        let generate = std::env::var("UPDATE_BATTLE_VIEWER_FIXTURES").is_ok();

        let scene = BattleViewer {
            camera_mode: BattleCamera::top_down_preset(),
            ..BattleViewer::default()
        };
        let actual = render_to_buffer(&scene, 80, 40);

        if generate {
            let fixture_path = format!(
                "{}/tests/fixtures/battle_viewer/top_down_golden.fixture",
                env!("CARGO_MANIFEST_DIR")
            );
            let preview_path = format!(
                "{}/tests/fixtures/battle_viewer/top_down_golden.preview.txt",
                env!("CARGO_MANIFEST_DIR")
            );
            std::fs::write(&fixture_path, serialize_braille_buffer(&actual))
                .unwrap_or_else(|e| panic!("failed to write {fixture_path}: {e}"));
            std::fs::write(&preview_path, buffer_to_art(&actual))
                .unwrap_or_else(|e| panic!("failed to write {preview_path}: {e}"));
            return;
        }

        let serialized = serialize_braille_buffer(&actual);
        assert!(
            serialized.lines().count() > 1,
            "Top-Down render must produce at least one lit braille cell \
             (got an empty/all-blank render — a vacuous fixture would never \
             catch a real regression)"
        );

        let fixture = load_battle_viewer_fixture("top_down_golden");
        let diff = diff_dots(&fixture, &actual);
        assert!(
            diff.is_match(),
            "Top-Down's current render diverges from the committed golden \
             fixture ({} dot mismatch(es) of {} compared); Top-Down's \
             projection is frozen for this feature and no task from b2 \
             onward may change its rendered dots — regenerate with \
             UPDATE_BATTLE_VIEWER_FIXTURES=1 only at a designated b1 \
             re-baseline point (b1-t3 sprite ratio, b1-t4 shadow shape), \
             and only after re-running the manual visual pass",
            diff.mismatches.len(),
            diff.dots_compared
        );
    }
}
