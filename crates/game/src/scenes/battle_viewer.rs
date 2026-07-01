use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::Rect;
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
