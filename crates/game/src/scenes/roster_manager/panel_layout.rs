// `panel_interior_regions`, `PanelRegions`, and the region constants below
// are consumed by the detail-panel render pass (widget rendering) and input
// (ability hit-testing) in `mod.rs`.

use super::*;

impl RosterManager {
    /// b1-t3: 1-cell blank top margin above the stamina row, measured from
    /// the panel interior's top edge (`details_panel_rects` border inset 1
    /// cell).
    pub(super) const PANEL_TOP_MARGIN_CELLS: i32 = 1;
    /// Stamina band height — 2 cells: the "Stamina" label + bar on the top
    /// cell-row, and the braille header underline on the row beneath (like the
    /// Abilities/Instructions header bands). `render_stamina_row` constrains the
    /// label+bar to the top 4-dot row so `bars::draw_bar` still gets an
    /// exact-height target.
    pub(super) const STAMINA_ROW_H_CELLS: i32 = 2;
    /// b1-t3: gap between the stamina row and the abilities header band.
    pub(super) const STAMINA_ABILITIES_GAP_CELLS: i32 = 2;
    /// b1-t3: header band height — row 0 is the header text, row 1 is the
    /// blank row reserved for the b2-t1 braille underline. Shared by both
    /// the abilities and instructions headers.
    pub(super) const HEADER_BAND_H_CELLS: i32 = 2;
    /// b1-t3: height of the 2x2 ability grid band (two 1-cell rows, no
    /// inter-row gap).
    pub(super) const ABILITY_GRID_H_CELLS: i32 = 2;
    /// b1-t3: pinned inter-column gap between the ability grid's two
    /// columns.
    pub(super) const ABILITY_GRID_COL_GAP_CELLS: i32 = 1;
    /// b1-t3: inter-row gap within an ability grid column (none — the two
    /// rows sit flush).
    pub(super) const ABILITY_GRID_ROW_GAP_CELLS: i32 = 0;
    /// b1-t3: gap between the ability grid band and the instructions header
    /// band.
    pub(super) const ABILITIES_INSTRUCTIONS_GAP_CELLS: i32 = 2;
    /// Width (cells) of the Edit-button SLOT in the instructions header row.
    /// The button itself is this minus `EDIT_BUTTON_RIGHT_MARGIN_CELLS` = 8
    /// cells (an even width, so "Edit" centers symmetrically between the borders)
    /// with a 1-cell gap to the panel's right edge.
    pub(super) const EDIT_BUTTON_W_CELLS: i32 = 9;
    /// Gap (cells) between the Edit button's right border and the panel's right
    /// edge, so the button isn't crammed against the frame.
    pub(super) const EDIT_BUTTON_RIGHT_MARGIN_CELLS: i32 = 1;
    /// Edit-button height (cells). The button is a rounded-rect OUTLINE around
    /// its "Edit" label; braille border dots and the terminal label can't
    /// share a cell, so it needs a top-border row, a text row, and a
    /// bottom-border row = 3 cells. The instructions header band is only 2
    /// cells, so the button extends 1 cell UP into the blank
    /// `ABILITIES_INSTRUCTIONS` gap above it (never DOWN into preview text).
    pub(super) const EDIT_BUTTON_H_CELLS: i32 = 3;

    /// b1-t3: carves the details-panel INTERIOR (inside the existing 1-cell
    /// border inset already applied by `details_panel_rects`) into named
    /// regions, composed entirely from `engine_render::flex` over
    /// `DotRect` — no hand-computed cell offsets. See research.md b1-t3 for
    /// the full column-then-subflex breakdown (top margin, stamina row,
    /// gap, abilities header+grid band, gap, instructions header+edit-
    /// button band, grow preview).
    ///
    pub(super) fn panel_interior_regions(area: Rect) -> PanelRegions {
        let [ex_dots, ab_dots] = Self::right_col_dots(area);
        let border = engine_render::DotRect { h: ex_dots.h + ab_dots.h, ..ex_dots };
        let interior = border.inset(2, 2, 4, 4);

        let column = engine_render::flex(
            interior,
            engine_render::FlexStyle {
                direction: engine_render::Direction::Column,
                justify_content: engine_render::Justify::Start,
                align_items: engine_render::Align::Stretch,
                gap: 0,
            },
            &[
                // top margin
                engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(Self::PANEL_TOP_MARGIN_CELLS * 4),
                    grow: 0.0,
                    shrink: 0.0,
                },
                // stamina row
                engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(Self::STAMINA_ROW_H_CELLS * 4),
                    grow: 0.0,
                    shrink: 0.0,
                },
                // gap
                engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(Self::STAMINA_ABILITIES_GAP_CELLS * 4),
                    grow: 0.0,
                    shrink: 0.0,
                },
                // abilities header band (text row + reserved underline row)
                engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(Self::HEADER_BAND_H_CELLS * 4),
                    grow: 0.0,
                    shrink: 0.0,
                },
                // ability grid band
                engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(Self::ABILITY_GRID_H_CELLS * 4),
                    grow: 0.0,
                    shrink: 0.0,
                },
                // gap
                engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(Self::ABILITIES_INSTRUCTIONS_GAP_CELLS * 4),
                    grow: 0.0,
                    shrink: 0.0,
                },
                // instructions header band (text row + reserved underline row)
                engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(Self::HEADER_BAND_H_CELLS * 4),
                    grow: 0.0,
                    shrink: 0.0,
                },
                // preview — sole grow child, absorbs remaining space
                engine_render::FlexChild { basis: engine_render::Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
            ],
        );
        let [_top_margin, stamina, _gap1, abilities_header_band, grid_band, _gap2, instr_header_band, preview] =
            column[..]
        else {
            unreachable!("flex() with 8 children returns exactly 8 rects")
        };

        // `.min(4)` (not a bare `4`) so a collapsed band (insufficient
        // vertical room — e.g. a short terminal) yields a genuinely
        // zero-height header instead of one whose height claims more than
        // the band actually has, which would make `draw_header_underline`
        // (anchored at `header.y + header.h`) draw past the band's real
        // bottom edge — this shipped as a real bug: at a short area the
        // "Instructions" header (see `instructions_header` below) painted
        // its underline 1 dot into the `dot_row` band beneath the panel.
        let abilities_header =
            engine_render::DotRect { h: abilities_header_band.h.min(4), ..abilities_header_band };

        // Ability grid: 2 columns x 2 rows, reading order [TL, TR, BL, BR].
        // Names are centered within each column (no left indent).
        let grid_cols = engine_render::flex(
            grid_band,
            engine_render::FlexStyle {
                direction: engine_render::Direction::Row,
                justify_content: engine_render::Justify::Start,
                align_items: engine_render::Align::Stretch,
                gap: Self::ABILITY_GRID_COL_GAP_CELLS * 2,
            },
            &[
                engine_render::FlexChild { basis: engine_render::Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
                engine_render::FlexChild { basis: engine_render::Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
            ],
        );
        let [left_col, right_col] = grid_cols[..] else {
            unreachable!("flex() with 2 children returns exactly 2 rects")
        };

        let grid_rows_style = engine_render::FlexStyle {
            direction: engine_render::Direction::Column,
            justify_content: engine_render::Justify::Start,
            align_items: engine_render::Align::Stretch,
            gap: Self::ABILITY_GRID_ROW_GAP_CELLS * 4,
        };
        let row_children = [
            engine_render::FlexChild { basis: engine_render::Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
            engine_render::FlexChild { basis: engine_render::Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
        ];
        let left_rows = engine_render::flex(left_col, grid_rows_style, &row_children);
        let right_rows = engine_render::flex(right_col, grid_rows_style, &row_children);
        let [tl, bl] = left_rows[..] else { unreachable!("flex() with 2 children returns exactly 2 rects") };
        let [tr, br] = right_rows[..] else { unreachable!("flex() with 2 children returns exactly 2 rects") };
        let ability_cells = [tl, tr, bl, br];

        // Instructions header band: left header slot | right edit-button slot.
        let instr_split = engine_render::flex(
            instr_header_band,
            engine_render::FlexStyle {
                direction: engine_render::Direction::Row,
                justify_content: engine_render::Justify::Start,
                align_items: engine_render::Align::Stretch,
                gap: 0,
            },
            &[
                engine_render::FlexChild { basis: engine_render::Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
                engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(Self::EDIT_BUTTON_W_CELLS * 2),
                    grow: 0.0,
                    shrink: 0.0,
                },
            ],
        );
        let [instr_header_left, edit_button_band] = instr_split[..] else {
            unreachable!("flex() with 2 children returns exactly 2 rects")
        };
        // `.min(4)` — see `abilities_header`'s comment above; same collapsed-
        // band hazard applies here (this is the field whose forced height
        // actually shipped the bleed-into-`dot_row` bug).
        let instructions_header =
            engine_render::DotRect { h: instr_header_left.h.min(4), ..instr_header_left };

        // The Edit button is a 3-cell-tall rounded-rect outline (see
        // `EDIT_BUTTON_H_CELLS`): keep its BOTTOM edge aligned to the header
        // band's bottom and grow UPWARD one cell into the blank
        // `ABILITIES_INSTRUCTIONS` gap above, so it never paints DOWN into the
        // preview text below. It draws its own border+label (`render_edit_button`),
        // not through `engine_render::Button`, so there is no extra-row padding
        // hazard to clamp against.
        let edit_button = engine_render::DotRect {
            y: edit_button_band.y + edit_button_band.h - Self::EDIT_BUTTON_H_CELLS * 4,
            h: Self::EDIT_BUTTON_H_CELLS * 4,
            // Shrink the width off the RIGHT so a 1-cell gap sits between the
            // button and the panel's right edge (the slot's right edge).
            w: edit_button_band.w - Self::EDIT_BUTTON_RIGHT_MARGIN_CELLS * 2,
            ..edit_button_band
        };

        PanelRegions {
            stamina,
            abilities_header,
            ability_cells,
            instructions_header,
            edit_button,
            preview,
        }
    }
}

/// b1-t3: named interior regions of the details panel, all in DOT space
/// (unfloored `DotRect` — CLAUDE.md rule 5; every dot-drawn consumer floors
/// at its own draw site, never here). Reading order for `ability_cells`:
/// `[0]`=top-left `[1]`=top-right `[2]`=bottom-left `[3]`=bottom-right.
#[derive(Clone, Copy, Debug)]
pub(super) struct PanelRegions {
    /// 1-cell-tall row for `bars::draw_labeled_bar` (b2-t2).
    pub stamina: engine_render::DotRect,
    /// 1-cell text rect; a blank cell row is reserved directly beneath for
    /// the b2-t1 braille underline.
    pub abilities_header: engine_render::DotRect,
    pub ability_cells: [engine_render::DotRect; 4],
    /// 1-cell text rect (left slot); underline row reserved beneath.
    pub instructions_header: engine_render::DotRect,
    /// Right slot of the instructions header row, `HEADER_BAND_H_CELLS`
    /// tall.
    pub edit_button: engine_render::DotRect,
    /// Fills all remaining vertical space below the instructions header
    /// band.
    pub preview: engine_render::DotRect,
}

/// b1-t3: pure-layout-math tests over `PanelRegions` in DOT space — per
/// CLAUDE.md, comparing `DotRect` fields directly is valid here (this is
/// layout math, not a rendered-braille alignment claim, which would require
/// decoding dots instead).
#[cfg(test)]
mod panel_layout_tests {
    use super::*;

    /// Recomputes the same interior `details_panel_rects`/`right_col_dots`
    /// produce, for tests that need the interior's own edges (top margin,
    /// right edge) rather than just inter-region relationships.
    fn interior(area: Rect) -> engine_render::DotRect {
        let [ex_dots, ab_dots] = RosterManager::right_col_dots(area);
        let border = engine_render::DotRect { h: ex_dots.h + ab_dots.h, ..ex_dots };
        border.inset(2, 2, 4, 4)
    }

    #[test]
    fn regions_ordered_top_to_bottom() {
        let area = Rect::new(0, 0, 80, 30);
        let r = RosterManager::panel_interior_regions(area);

        assert!(r.stamina.y < r.abilities_header.y, "stamina.y ({}) must be above abilities_header.y ({})", r.stamina.y, r.abilities_header.y);
        for c in r.ability_cells {
            assert!(
                r.abilities_header.y < c.y,
                "ability cell y={} must be below abilities_header.y={}",
                c.y, r.abilities_header.y
            );
        }
        let max_cell_y = r.ability_cells.iter().map(|c| c.y).max().unwrap();
        assert!(
            max_cell_y < r.instructions_header.y,
            "lowest ability cell y ({max_cell_y}) must be above instructions_header.y ({})",
            r.instructions_header.y
        );
        assert!(
            r.instructions_header.y < r.preview.y,
            "instructions_header.y ({}) must be above preview.y ({})",
            r.instructions_header.y, r.preview.y
        );
    }

    #[test]
    fn two_by_two_grid_four_nonoverlapping_cells() {
        let area = Rect::new(0, 0, 80, 30);
        let r = RosterManager::panel_interior_regions(area);
        let [tl, tr, bl, br] = r.ability_cells;

        assert_eq!(tl.x, bl.x, "left column shares x: tl={tl:?} bl={bl:?}");
        assert_eq!(tr.x, br.x, "right column shares x: tr={tr:?} br={br:?}");
        assert_eq!(tl.y, tr.y, "top row shares y: tl={tl:?} tr={tr:?}");
        assert_eq!(bl.y, br.y, "bottom row shares y: bl={bl:?} br={br:?}");
        assert!(tl.x < tr.x, "left column (x={}) must be left of right column (x={})", tl.x, tr.x);
        assert!(
            tr.x >= tl.x + tl.w + RosterManager::ABILITY_GRID_COL_GAP_CELLS * 2,
            "inter-column gap missing: tr.x={} tl.x={} tl.w={} expected_gap_dots={}",
            tr.x, tl.x, tl.w, RosterManager::ABILITY_GRID_COL_GAP_CELLS * 2
        );
        assert!(tl.y < bl.y, "top row (y={}) must be above bottom row (y={})", tl.y, bl.y);
    }

    #[test]
    fn both_gaps_present_and_top_margin() {
        let area = Rect::new(0, 0, 80, 30);
        let interior = interior(area);
        let r = RosterManager::panel_interior_regions(area);

        assert_eq!(
            r.stamina.y - interior.y,
            RosterManager::PANEL_TOP_MARGIN_CELLS * 4,
            "top margin: stamina.y={} interior.y={}", r.stamina.y, interior.y
        );
        assert_eq!(
            r.abilities_header.y - (r.stamina.y + r.stamina.h),
            RosterManager::STAMINA_ABILITIES_GAP_CELLS * 4,
            "stamina->abilities gap"
        );
        let bottom_row_bottom = r.ability_cells[2].y + r.ability_cells[2].h;
        assert_eq!(
            r.instructions_header.y - bottom_row_bottom,
            RosterManager::ABILITIES_INSTRUCTIONS_GAP_CELLS * 4,
            "grid->instructions gap"
        );
    }

    #[test]
    fn underline_row_reserved_beneath_each_header() {
        let area = Rect::new(0, 0, 80, 30);
        let r = RosterManager::panel_interior_regions(area);

        assert_eq!(
            r.ability_cells[0].y - r.abilities_header.y,
            RosterManager::HEADER_BAND_H_CELLS * 4,
            "abilities header underline row: cell0.y={} header.y={}", r.ability_cells[0].y, r.abilities_header.y
        );
        assert_eq!(
            r.preview.y - r.instructions_header.y,
            RosterManager::HEADER_BAND_H_CELLS * 4,
            "instructions header underline row: preview.y={} header.y={}", r.preview.y, r.instructions_header.y
        );
    }

    #[test]
    fn instructions_header_splits_left_header_right_edit_button() {
        let area = Rect::new(0, 0, 80, 30);
        let interior = interior(area);
        let r = RosterManager::panel_interior_regions(area);

        assert_eq!(r.instructions_header.x, interior.x, "header must be left-aligned to interior");
        assert!(
            r.edit_button.x > r.instructions_header.x,
            "edit_button.x ({}) must be right of instructions_header.x ({})",
            r.edit_button.x, r.instructions_header.x
        );
        assert_eq!(
            r.edit_button.w,
            (RosterManager::EDIT_BUTTON_W_CELLS - RosterManager::EDIT_BUTTON_RIGHT_MARGIN_CELLS) * 2,
            "edit button width must be the slot minus the right margin"
        );
        assert_eq!(
            r.edit_button.x + r.edit_button.w,
            interior.x + interior.w - RosterManager::EDIT_BUTTON_RIGHT_MARGIN_CELLS * 2,
            "edit button must sit a 1-cell margin off the interior's right edge"
        );
    }

    #[test]
    fn degenerate_area_does_not_panic() {
        let area = Rect::new(0, 0, 10, 6);
        let r = RosterManager::panel_interior_regions(area);

        let all = [r.stamina, r.abilities_header, r.instructions_header, r.edit_button, r.preview]
            .into_iter()
            .chain(r.ability_cells);
        for rect in all {
            assert!(rect.w >= 0, "w must be >=0: {rect:?}");
            assert!(rect.h >= 0, "h must be >=0: {rect:?}");
        }
    }
}
