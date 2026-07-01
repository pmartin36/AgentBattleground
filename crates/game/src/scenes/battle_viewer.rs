use std::time::Duration;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use render::camera::SideView;
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

#[derive(Default)]
pub struct BattleViewer;

impl BattleViewer {
    pub const COLOR: Rgba = Rgba::rgb(0xc8, 0x1e, 0x1e);
}

impl Scene for BattleViewer {
    fn id(&self) -> SceneId {
        SceneId::BattleViewer
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}

    fn update(&mut self, _ctx: &mut EngineCtx, _dt: Duration) -> Option<Transition> {
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        super::fill_and_label(frame, area, Self::COLOR, self.id().display_name());
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
