use super::*;

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
    match mode.fit {
        FitMode::ExactFit => {
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
        FitMode::ViewportFit => fit_perspective_geometry(area, mode, tuning),
        FitMode::Manual => {
            let board_rect = Rect::new(area.left(), area.top(), area.width.max(1), area.height.max(1));
            let cell_width_cols = (area.width / BOARD_COLS).max(1);
            let cell_height_rows = (area.height / BOARD_ROWS).max(1);

            // NOT `(0, 0)`: free-roam deliberately has no auto-fit-to-content
            // (the whole point is the user controls framing directly — spec
            // 42 Decision 5), but a raw `project()` result of `(0, 0)`
            // (looking at the camera's own aim point) still needs to land
            // SOMEWHERE — an unshifted origin puts it at the buffer's
            // top-left corner, not the middle of the screen, so entering
            // free-roam rendered as a tiny wedge jammed in one corner
            // (shipped bug). Centering the origin doesn't reintroduce any
            // scale-fitting — `scale_dots` is still whatever the camera
            // itself carries, untouched.
            let screen_offset = (
                (board_rect.width as i32 * 2) / 2,
                (board_rect.height as i32 * 4) / 2,
            );

            BoardGeometry {
                cell_width_cols,
                cell_height_rows,
                board_rect,
                camera: mode,
                tuning,
                screen_offset,
            }
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

    /// OverShoulder mode: the caller-supplied depth coordinate (`y`) is
    /// preserved (not reset/discarded) and scale is rebuilt to some positive
    /// area-derived value. (b4-t1: dropped the pinned `scale_dots == 36.0`
    /// expectation — perspective presets are now fit-to-viewport sized, so
    /// the exact scale is no longer the flat `cell_height_rows*4` value;
    /// only positivity is asserted, per the no-pinned-constant guardrail.
    /// b2-t1: `camera_depth`/`facing_sign` re-expressed as `y`/`yaw_deg`.)
    #[test]
    fn over_shoulder_mode_rebuilds_scale_and_preserves_camera_depth() {
        let area = Rect::new(0, 0, 128, 64);
        let mode = BattleCamera {
            camera: AnyCamera::Perspective(PerspectiveCamera {
                x: BOARD_CENTER_COL,
                y: 6.0,
                height: 4.0,
                yaw_deg: 180.0,
                pitch_deg: 30.0,
                fov_deg: 55.0,
                scale_dots: 0.0,
            }),
            fit: FitMode::ViewportFit,
        };
        let g = board_geometry(area, mode, tuning());

        let AnyCamera::Perspective(inner) = g.camera.camera else {
            panic!("expected Perspective variant");
        };
        assert_eq!(inner.y, 6.0, "the depth coordinate (y) must be preserved, not reset");
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

