//! Shared card-shell primitive: up-left anchor math, edge clamping
//! (`x >= 0`, `y >= 0`), the chamfered `Dot::Occlude` frame, and the interior
//! row `flex`. Callers own row *content* and card *width*; this module owns
//! geometry and chrome only, so every caller (the roster ability card, the
//! diagnostics warning card, and any future tooltip-shaped card) inherits the
//! clamp by default — no opt-in.

use engine_core::color::Rgba;
use engine_render::dots::Dot;
use engine_render::{
    draw_dots, flex, ui_primitives, wrapped_line_count, wrapped_text, Align, Basis, Direction, DotRect,
    FlexChild, FlexStyle, Justify, TextAlign,
};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Style};

/// One content row: `height_cells` tall, preceded by `gap_above_cells` of
/// blank space. The gap is inserted internally and never appears as its own
/// rect in [`CardLayout::rows`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RowSpec {
    pub height_cells: u16,
    pub gap_above_cells: u16,
}

/// Card frame plus one rect per requested [`RowSpec`] (gaps excluded), in
/// request order, absolute dot coordinates.
pub(crate) struct CardLayout {
    pub card: DotRect,
    pub rows: Vec<DotRect>,
}

/// Whole-cell gap the card is anchored off `anchor`'s top-left corner (card's
/// bottom-right sits this many cells above-left, before clamping). 0 = the
/// card sits right against the anchor point.
pub(crate) const ANCHOR_GAP_CELLS: u16 = 0;
/// Card frame border color — amber.
pub(crate) const CARD_BORDER_COLOR: Rgba = Rgba::rgb(0xff, 0xbf, 0x00);
/// Card frame border ring thickness (dots).
pub(crate) const CARD_BORDER_THICKNESS_DOTS: usize = 1;
/// Card frame corner radius (dots) — chamfer 1, the house default.
pub(crate) const CARD_CORNER_RADIUS_DOTS: usize = 1;
/// Interior vertical (top/bottom) `.inset` padding applied to the card's
/// content area.
pub(crate) const INTERIOR_PADDING_CELLS: u16 = 1;
/// Interior horizontal (left/right) padding — leaves a blank margin cell
/// between the card frame and every content row.
pub(crate) const INTERIOR_PADDING_H_CELLS: u16 = 2;
/// Plain-text foreground the primitive uses for its own convenience content
/// path, legible over the `Occlude` interior fill.
#[allow(dead_code)]
pub(crate) const TEXT_COLOR: Rgba = Rgba::rgb(0xff, 0xff, 0xff);

/// Lays out the card up-left of `anchor`'s horizontal center, `width_cells`
/// wide, height driven by `rows`, clamped so `card.x >= 0 && card.y >= 0`;
/// each returned row rect follows the (possibly clamped) card, inset by the
/// shared interior padding.
pub(crate) fn layout(anchor: DotRect, rows: &[RowSpec], width_cells: u16) -> CardLayout {
    let content_dots: i32 = rows
        .iter()
        .map(|row| (row.gap_above_cells as i32 + row.height_cells as i32) * 4)
        .sum();

    let card_w = width_cells as i32 * 2;
    let card_h = content_dots + 2 * INTERIOR_PADDING_CELLS as i32 * 4;
    let dx = ANCHOR_GAP_CELLS as i32 * 2;
    let dy = ANCHOR_GAP_CELLS as i32 * 4;

    // Anchor the card's bottom-right to `anchor`'s horizontal CENTER. Only
    // left/top can overflow off-screen, so `.max(0)` on x/y is a complete
    // clamp.
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

    CardLayout { card, rows: rows_out }
}

/// Lays out a single word-wrapped text row within `max_width_cells`, then
/// draws it inside a bordered card via [`layout`]/[`draw_frame`]. Returns
/// `false` (drawing nothing) for an empty/whitespace `msg` or a
/// `max_width_cells` too narrow to hold any content.
#[allow(dead_code)]
pub(crate) fn render_text(buf: &mut Buffer, anchor: DotRect, msg: &str, max_width_cells: u16) -> bool {
    let max_content = max_width_cells.saturating_sub(2 * INTERIOR_PADDING_H_CELLS);
    if max_content == 0 || msg.trim().is_empty() {
        return false;
    }

    let content_width = (msg.chars().count() as u16).min(max_content).max(1);
    let card_width = content_width + 2 * INTERIOR_PADDING_H_CELLS;
    let line_count = (wrapped_line_count(msg, content_width as usize).max(1)) as u16;
    let rows = [RowSpec { height_cells: line_count, gap_above_cells: 0 }];
    let laid = layout(anchor, &rows, card_width);
    if !draw_frame(buf, laid.card) {
        return false;
    }

    wrapped_text(
        buf,
        laid.rows[0].to_cell_rect(),
        msg,
        TextAlign::Left,
        Style::default().fg(Color::Rgb(TEXT_COLOR.r, TEXT_COLOR.g, TEXT_COLOR.b)),
        true,
    );
    true
}

/// Draws the chamfered `CARD_BORDER_COLOR` `Occlude` frame into `buf` at
/// `card`. Returns `false` (drawing nothing) for a zero-area card.
pub(crate) fn draw_frame(buf: &mut Buffer, card: DotRect) -> bool {
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
    use crate::scenes::test_util::{lit_dot_color, rect_text};
    use engine_render::{label, TextAlign};
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    const TEST_WIDTH_CELLS: u16 = 36;

    /// Anchor far from every screen edge — the clamp must be a no-op here
    /// (guards the shipped ability card, which always anchors this far out).
    fn far_anchor() -> DotRect {
        DotRect { x: 200, y: 120, w: 20, h: 16 }
    }

    /// Anchor close enough to the left edge that the unclamped card.x would
    /// go negative (-58 for this exact anchor at `TEST_WIDTH_CELLS`).
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
        let layout = layout(near_left_anchor(), &[], TEST_WIDTH_CELLS);
        assert_eq!(layout.card.x, 0, "unclamped card.x is -58; must clamp to 0");
    }

    #[test]
    fn layout_shell_clamps_card_top_edge_to_zero() {
        let layout = layout(near_top_anchor(), &[], TEST_WIDTH_CELLS);
        assert_eq!(layout.card.y, 0, "unclamped card.y goes negative; must clamp to 0");
    }

    #[test]
    fn layout_shell_rows_stay_inset_inside_clamped_card() {
        let rows = [RowSpec { height_cells: 1, gap_above_cells: 0 }];
        let layout = layout(near_left_anchor(), &rows, TEST_WIDTH_CELLS);

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
        let layout = layout(anchor, &[], TEST_WIDTH_CELLS);

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
    /// rendered dots, not by comparing rect fields.
    #[test]
    fn clamped_card_row_content_renders_inside_the_frame() {
        let rows = [RowSpec { height_cells: 1, gap_above_cells: 0 }];
        let layout = layout(near_left_anchor(), &rows, TEST_WIDTH_CELLS);
        assert_eq!(layout.card.x, 0, "precondition: card must be left-clamped");

        let mut buf = Buffer::empty(Rect::new(0, 0, 50, 32));
        assert!(
            draw_frame(&mut buf, layout.card),
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
        let layout = layout(far_anchor(), &[], TEST_WIDTH_CELLS);

        assert!(layout.rows.is_empty());
        assert_eq!(layout.card.h, 2 * INTERIOR_PADDING_CELLS as i32 * 4);
    }

    #[test]
    fn layout_shell_gap_above_offsets_only_its_own_row() {
        let rows = [
            RowSpec { height_cells: 1, gap_above_cells: 0 },
            RowSpec { height_cells: 1, gap_above_cells: 1 },
        ];
        let layout = layout(far_anchor(), &rows, TEST_WIDTH_CELLS);

        assert_eq!(layout.rows.len(), 2, "the gap must never appear as its own returned rect");
        assert_eq!(
            layout.rows[1].y - (layout.rows[0].y + layout.rows[0].h),
            4,
            "gap_above_cells: 1 must offset only rows[1] by 1 cell (4 dots)"
        );
    }

    /// Bounding box (min_col, max_col), in absolute DOT columns, of every lit
    /// dot anywhere in `buf` that decodes to `color`; `None` if no such dot
    /// exists. Used to measure a rendered card's true pixel width and to
    /// prove a "draws nothing" case left no border behind, without comparing
    /// rect fields.
    fn color_dot_col_extent(buf: &Buffer, color: Rgba) -> Option<(i32, i32)> {
        const DOT_DX: [u8; 8] = [0, 0, 0, 1, 1, 1, 0, 1];
        let mut extent: Option<(i32, i32)> = None;
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let Some((mask, cell_color)) = engine_render::decode_braille_cell(buf, x, y) else {
                    continue;
                };
                if cell_color != color {
                    continue;
                }
                for k in 0..8u8 {
                    if mask & (1 << k) != 0 {
                        let dot_col = x as i32 * 2 + DOT_DX[k as usize] as i32;
                        extent = Some(match extent {
                            None => (dot_col, dot_col),
                            Some((lo, hi)) => (lo.min(dot_col), hi.max(dot_col)),
                        });
                    }
                }
            }
        }
        extent
    }

    /// Buffer large enough to hold any card `render_text` draws off
    /// `far_anchor()` in these tests without clamping or clipping.
    fn text_test_buffer() -> Buffer {
        Buffer::empty(Rect::new(0, 0, 110, 32))
    }

    #[test]
    #[allow(clippy::int_plus_one)]
    fn render_text_draws_bordered_card_within_max_width() {
        let mut buf = text_test_buffer();
        let max_width_cells: u16 = 30;

        let drew = render_text(&mut buf, far_anchor(), "Hi", max_width_cells);

        assert!(drew, "a short message within max_width must draw a card");
        let (lo, hi) = color_dot_col_extent(&buf, CARD_BORDER_COLOR)
            .expect("a drawn card must leave at least one amber border dot lit");
        assert!(
            hi - lo + 1 <= max_width_cells as i32 * 2,
            "card's amber border must not exceed max_width_cells ({max_width_cells}) in dots, got {} dots",
            hi - lo + 1
        );
    }

    #[test]
    fn render_text_message_appears_inside_card() {
        let mut buf = text_test_buffer();

        let drew = render_text(&mut buf, far_anchor(), "Hi", 30);

        assert!(drew, "a short message within max_width must draw a card");
        assert!(
            rect_text(&buf, buf.area).contains("Hi"),
            "the message text must render somewhere inside the drawn card"
        );
    }

    #[test]
    #[allow(clippy::int_plus_one)]
    fn render_text_overlong_message_wraps_without_panic() {
        let mut buf = text_test_buffer();
        let max_width_cells: u16 = 10;
        let msg = "This message is far too long to fit on a single narrow line";

        let drew = render_text(&mut buf, far_anchor(), msg, max_width_cells);

        assert!(drew, "a long message within a small max_width must still draw a card");
        let (lo, hi) = color_dot_col_extent(&buf, CARD_BORDER_COLOR)
            .expect("a drawn card must leave at least one amber border dot lit");
        assert!(
            hi - lo + 1 <= max_width_cells as i32 * 2,
            "wrapped card must stay within max_width_cells, not overrun it, got {} dots",
            hi - lo + 1
        );
        assert!(
            rect_text(&buf, buf.area).contains("This"),
            "at least the first word of the wrapped message must still render"
        );
    }

    #[test]
    fn render_text_empty_message_draws_nothing() {
        let mut buf = text_test_buffer();

        let drew = render_text(&mut buf, far_anchor(), "", 30);

        assert!(!drew, "an empty message must draw nothing");
        assert!(
            color_dot_col_extent(&buf, CARD_BORDER_COLOR).is_none(),
            "no amber border dot may be drawn for an empty message"
        );
    }

    #[test]
    fn render_text_zero_max_width_draws_nothing() {
        let mut buf = text_test_buffer();

        let drew = render_text(&mut buf, far_anchor(), "Hi", 0);

        assert!(!drew, "a max_width too small to hold any content must draw nothing");
        assert!(
            color_dot_col_extent(&buf, CARD_BORDER_COLOR).is_none(),
            "no amber border dot may be drawn when max_width can't hold content"
        );
    }
}
