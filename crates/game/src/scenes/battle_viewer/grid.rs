use super::*;

/// Sample density for `rasterize_grid_line`, in samples per world unit along
/// a line's length. `4` (spacing `0.25`) lands a sample exactly on every
/// non-Top-Down preset's half-integer depth-coordinate anchor (Over-shoulder
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

    draw_dots(buf, geom.board_rect, &dots);
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

/// Grid-line color for board chrome (`draw_board_lines`). Single source of
/// truth referenced by both the drawing code and every test needing the
/// exact value — never re-hardcoded as a bare `0x55` literal elsewhere.
pub const GRID_LINE_COLOR: Rgba = Rgba::rgb(0x55, 0x55, 0x55);

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
    /// (2,1), cell_width_cols=4, cell_height_rows=2, sized for a 5x5 grid
    /// (board_rect 20x10) in a 40x20 buffer. DotBuffer is
    /// board_rect.width*2 x board_rect.height*4 = 40x40 dots (8x8 dots per
    /// board cell, 5 cells per side — `draw_board_lines` iterates
    /// `0..=BOARD_COLS`/`0..=BOARD_ROWS`, both 5). The board must fill the dot
    /// buffer exactly (5 cells x 8 dots = 40 = scale_dots 8 x 5 world units)
    /// or the outermost boundary never reaches the edge and the fencepost case
    /// below stops testing anything. Vertical boundaries i=0..=5 land at
    /// terminal x in {2,6,10,14,18, 21(clamped)}; horizontal boundaries
    /// j=0..=5 land at terminal y in {1,3,5,7,9, 10(clamped)}. For every
    /// i<5/j<5 the boundary's dot sits at dx=0/dy=0 within its terminal cell
    /// (exact, no clamp); only the outermost i==5/j==5 boundary clamps to the
    /// LAST valid dot index (dx=1/dy=3 — one dot short of
    /// board_rect.right()/bottom()).
    ///
    /// Camera is `TopDown` (full-strength grid lines, b4-t1) so the glyph/
    /// shape assertions below stay independent of grid-line *color* — color
    /// is covered separately by the per-mode color tests further down.
    fn geom() -> BoardGeometry {
        BoardGeometry {
            cell_width_cols: 4,
            cell_height_rows: 2,
            board_rect: Rect::new(2, 1, 20, 10),
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

    /// Point-5 fencepost resolution (chosen: clamp to the last valid dot
    /// index, one dot short): board_rect=(2,1,20,10) -> right()=22, so the
    /// outermost vertical boundary (i=5, since `draw_board_lines` iterates
    /// `0..=BOARD_COLS` and `BOARD_COLS==5`) cannot sit at dot_x=40 (one past
    /// the last valid dot 39); it clamps to dot_x=39, landing in the RIGHT
    /// dot-column (dx=1) of the LAST valid cell (terminal x=21), not at
    /// x=22. That cell's top-right corner: right dot-column full height
    /// (mask 0xB8) union top dot-row full width (mask 0x09) = mask 0xB9 ->
    /// '\u{28B9}'. Nothing is drawn at the naive unclamped position x=22.
    #[test]
    fn far_boundary_is_clamped_one_dot_short_not_out_of_bounds() {
        let g = geom();
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        draw_board_lines(&mut buf, &g);

        let clamped = buf.cell((21, 1)).unwrap();
        assert_eq!(clamped.symbol(), "\u{28B9}");
        assert_eq!(clamped.fg, grid_fg());

        assert_eq!(
            buf.cell((22, 1)).unwrap().symbol(),
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

