//! Content-agnostic tooltip card shell (b3-t1): up-left anchor math, edge
//! clamping (`x >= 0`, `y >= 0`), the chamfered `Dot::Occlude` frame, and the
//! interior row `flex`. Callers own row *content*; this module owns geometry
//! and chrome only, so every caller (the existing ability card, and future
//! callers such as a diagnostics warning card) inherits the clamp by
//! default — no opt-in.

use super::{
    ANCHOR_GAP_CELLS, CARD_BORDER_COLOR, CARD_BORDER_THICKNESS_DOTS, CARD_CORNER_RADIUS_DOTS,
    INTERIOR_PADDING_CELLS, INTERIOR_PADDING_H_CELLS, TOOLTIP_WIDTH_CELLS,
};
use engine_render::dots::Dot;
use engine_render::{
    draw_dots, flex, ui_primitives, Align, Basis, Direction, DotRect, FlexChild, FlexStyle, Justify,
};
use ratatui::buffer::Buffer;

/// One content row: `height_cells` tall, preceded by `gap_above_cells` of
/// blank space. The gap is inserted internally and never appears as its own
/// rect in [`ShellLayout::rows`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShellRow {
    pub height_cells: u16,
    pub gap_above_cells: u16,
}

/// Card frame plus one rect per requested [`ShellRow`] (gaps excluded), in
/// request order, absolute dot coordinates.
pub(crate) struct ShellLayout {
    pub card: DotRect,
    pub rows: Vec<DotRect>,
}

/// Lays out the card up-left of `anchor`'s horizontal center,
/// `TOOLTIP_WIDTH_CELLS` wide, height driven by `rows`, clamped so
/// `card.x >= 0 && card.y >= 0`; each returned row rect follows the
/// (possibly clamped) card, inset by the shared interior padding.
pub(crate) fn layout_shell(anchor: DotRect, rows: &[ShellRow]) -> ShellLayout {
    let content_dots: i32 = rows
        .iter()
        .map(|row| (row.gap_above_cells as i32 + row.height_cells as i32) * 4)
        .sum();

    let card_w = TOOLTIP_WIDTH_CELLS as i32 * 2;
    let card_h = content_dots + 2 * INTERIOR_PADDING_CELLS as i32 * 4;
    let dx = ANCHOR_GAP_CELLS as i32 * 2;
    let dy = ANCHOR_GAP_CELLS as i32 * 4;

    // Anchor the card's bottom-right to `anchor`'s horizontal CENTER, same
    // convention as the ability card. Only left/top can overflow off-screen
    // (see research.md verdict #6), so `.max(0)` on x/y is a complete clamp.
    let anchor_x = anchor.x + anchor.w / 2;

    let card = DotRect {
        x: (anchor_x - dx - card_w).max(0),
        y: (anchor.y - dy - card_h).max(0),
        w: card_w,
        h: card_h,
    };

    let interior = card.inset(
        INTERIOR_PADDING_H_CELLS as i32 * 2,
        INTERIOR_PADDING_H_CELLS as i32 * 2,
        INTERIOR_PADDING_CELLS as i32 * 4,
        INTERIOR_PADDING_CELLS as i32 * 4,
    );

    let mut markers: Vec<bool> = Vec::with_capacity(rows.len() * 2);
    let mut children: Vec<FlexChild> = Vec::with_capacity(rows.len() * 2);
    for row in rows {
        if row.gap_above_cells > 0 {
            children.push(FlexChild {
                basis: Basis::Fixed(row.gap_above_cells as i32 * 4),
                grow: 0.0,
                shrink: 0.0,
            });
            markers.push(false);
        }
        children.push(FlexChild {
            basis: Basis::Fixed(row.height_cells as i32 * 4),
            grow: 0.0,
            shrink: 0.0,
        });
        markers.push(true);
    }

    let style = FlexStyle {
        direction: Direction::Column,
        justify_content: Justify::Start,
        align_items: Align::Start,
        gap: 0,
    };
    let rects = flex(interior, style, &children);

    let rows_out = markers
        .into_iter()
        .zip(rects)
        .filter_map(|(is_row, rect)| is_row.then_some(rect))
        .collect();

    ShellLayout { card, rows: rows_out }
}

/// Draws the chamfered `CARD_BORDER_COLOR` `Occlude` frame into `buf` at
/// `card`. Returns `false` (drawing nothing) for a zero-area card.
pub(crate) fn draw_shell_frame(buf: &mut Buffer, card: DotRect) -> bool {
    let card_cell_rect = card.to_cell_rect();
    if card_cell_rect.width == 0 || card_cell_rect.height == 0 {
        return false;
    }

    let frame = ui_primitives::rounded_rect(
        card.w as usize,
        card.h as usize,
        CARD_BORDER_THICKNESS_DOTS,
        CARD_CORNER_RADIUS_DOTS,
        CARD_BORDER_COLOR,
        Dot::Occlude,
    );
    draw_dots(buf, card_cell_rect, &frame);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenes::test_util::lit_dot_color;
    use engine_render::{label, TextAlign};
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    /// Anchor far from every screen edge — the clamp must be a no-op here
    /// (guards the shipped ability card, which always anchors this far out).
    fn far_anchor() -> DotRect {
        DotRect { x: 200, y: 120, w: 20, h: 16 }
    }

    /// Anchor close enough to the left edge that the unclamped card.x would
    /// go negative (-58 for this exact anchor, per research.md verdict #4).
    fn near_left_anchor() -> DotRect {
        DotRect { x: 4, y: 120, w: 20, h: 16 }
    }

    /// Anchor close enough to the top edge that the unclamped card.y would
    /// go negative.
    fn near_top_anchor() -> DotRect {
        DotRect { x: 200, y: 4, w: 20, h: 16 }
    }

    #[test]
    fn layout_shell_clamps_card_left_edge_to_zero() {
        let layout = layout_shell(near_left_anchor(), &[]);
        assert_eq!(layout.card.x, 0, "unclamped card.x is -58; must clamp to 0");
    }

    #[test]
    fn layout_shell_clamps_card_top_edge_to_zero() {
        let layout = layout_shell(near_top_anchor(), &[]);
        assert_eq!(layout.card.y, 0, "unclamped card.y goes negative; must clamp to 0");
    }

    #[test]
    fn layout_shell_rows_stay_inset_inside_clamped_card() {
        let rows = [ShellRow { height_cells: 1, gap_above_cells: 0 }];
        let layout = layout_shell(near_left_anchor(), &rows);

        assert_eq!(layout.card.x, 0);
        assert_eq!(
            layout.rows[0].x,
            layout.card.x + INTERIOR_PADDING_H_CELLS as i32 * 2,
            "row content must stay inset inside the clamped card, not collide with the left border"
        );
    }

    #[test]
    fn layout_shell_anchor_is_up_left_when_far_from_edges() {
        let anchor = far_anchor();
        let layout = layout_shell(anchor, &[]);

        assert_eq!(
            layout.card.x + layout.card.w,
            anchor.x + anchor.w / 2 - ANCHOR_GAP_CELLS as i32 * 2,
            "far from edges, the clamp must be a no-op: card's right edge stays at the anchor's center minus the gap"
        );
        assert_eq!(
            layout.card.y + layout.card.h,
            anchor.y - ANCHOR_GAP_CELLS as i32 * 4,
            "far from edges, the clamp must be a no-op: card's bottom edge stays at the anchor's top"
        );
    }

    /// The real discriminator: content must land inside the frame, and the
    /// frame's left border must still decode as lit at dot col 0 — proving
    /// the clamp deforms geometry consistently rather than letting content
    /// collide with (or float clear of) the border. Decoded via real
    /// rendered dots per CLAUDE.md rule 5, not by comparing rect fields.
    #[test]
    fn clamped_card_row_content_renders_inside_the_frame() {
        let rows = [ShellRow { height_cells: 1, gap_above_cells: 0 }];
        let layout = layout_shell(near_left_anchor(), &rows);
        assert_eq!(layout.card.x, 0, "precondition: card must be left-clamped");

        let mut buf = Buffer::empty(Rect::new(0, 0, 50, 32));
        assert!(
            draw_shell_frame(&mut buf, layout.card),
            "a non-zero-area card must draw and report true"
        );

        let row_cell_rect = layout.rows[0].to_cell_rect();
        label(&mut buf, row_cell_rect, "X", TextAlign::Left, Style::default());

        assert_eq!(
            row_cell_rect.x, INTERIOR_PADDING_H_CELLS,
            "row content must start INTERIOR_PADDING_H_CELLS cells in from the clamped left edge"
        );

        let mid_dot_row = layout.card.y + layout.card.h / 2;
        assert_eq!(
            lit_dot_color(&buf, 0, mid_dot_row),
            Some(CARD_BORDER_COLOR),
            "left border must still decode as a lit border dot at dot col 0 — content must not overwrite it"
        );
    }

    #[test]
    fn layout_shell_no_rows_yields_padding_only_card() {
        let layout = layout_shell(far_anchor(), &[]);

        assert!(layout.rows.is_empty());
        assert_eq!(layout.card.h, 2 * INTERIOR_PADDING_CELLS as i32 * 4);
    }

    #[test]
    fn layout_shell_gap_above_offsets_only_its_own_row() {
        let rows = [
            ShellRow { height_cells: 1, gap_above_cells: 0 },
            ShellRow { height_cells: 1, gap_above_cells: 1 },
        ];
        let layout = layout_shell(far_anchor(), &rows);

        assert_eq!(layout.rows.len(), 2, "the gap must never appear as its own returned rect");
        assert_eq!(
            layout.rows[1].y - (layout.rows[0].y + layout.rows[0].h),
            4,
            "gap_above_cells: 1 must offset only rows[1] by 1 cell (4 dots)"
        );
    }
}
