//! Shared horizontal stat-bar helper (b4-t2): given a dot-precise target
//! rect, a fill fraction, and a fill color, paints a bordered bar where the
//! border ring and the proportional fill live in SEPARATE braille cells, so
//! the border stays crisp at any fill level. This is the single routine both
//! the XP bar (b4-t3) and the stamina bar (b4-t4) consume — see research.md.

use ratatui::buffer::Buffer;

use engine_core::color::Rgba;
use engine_render::dots::Dot;
use engine_render::{Align, Basis, Direction, DotRect, FlexChild, FlexStyle, Justify};

use super::columns::blit_dots;
use super::PostBattle;

/// Border ring thickness, in dots.
pub(super) const BAR_THICKNESS: usize = 1;
/// Corner chamfer radius, in dots.
pub(super) const BAR_CORNER_RADIUS: usize = 1;
/// Bar border color — reuses the existing portrait-frame chrome color rather
/// than minting a second grey.
pub(super) const BAR_BORDER_COLOR: Rgba = PostBattle::FRAME_COLOR;
/// Width (dots) reserved for the row label ("STA"/"XP"), left of the bar.
pub(super) const ROW_LABEL_W_DOTS: i32 = 8;
/// Dot gap between the row label and the bar.
pub(super) const ROW_LABEL_GAP_DOTS: i32 = 2;

/// Paints, into `target`, a chamfered border ring (`BAR_BORDER_COLOR`) plus a
/// left-anchored proportional fill (`fill_color`) whose lit dot-column span
/// is `fraction` (clamped `0.0..=1.0`) of the available interior width. The
/// interior fill region is inset a full braille cell (2 dots horizontal, 4
/// dots vertical) from every edge of `target`, so no fill dot can ever share
/// a braille cell with a border dot at any fill level. Placed sub-cell
/// precise via `columns::blit_dots`. A `target` too small to hold a full
/// inset (`w < 4 || h < 8`) draws the border only, with no fill and no
/// panic.
///
/// Wired in by b4-t3 (XP bar) / b4-t4 (stamina bar).
pub(super) fn draw_bar(buf: &mut Buffer, target: DotRect, fraction: f32, fill_color: Rgba) {
    if target.w <= 0 || target.h <= 0 {
        return;
    }
    let (w, h) = (target.w as usize, target.h as usize);

    let mut local =
        engine_render::rounded_rect(w, h, BAR_THICKNESS, BAR_CORNER_RADIUS, BAR_BORDER_COLOR, Dot::Transparent);

    if target.w >= 4 && target.h >= 8 {
        let fill_span_dots = (target.w - 4).max(0);
        let lit = (fraction.clamp(0.0, 1.0) * fill_span_dots as f32).round() as i32;
        for row in 4..(target.h - 4) {
            for col in 2..(2 + lit) {
                local.set(col as usize, row as usize, Dot::Lit(fill_color));
            }
        }
    }

    blit_dots(buf, target, &local);
}

/// Paints a fixed-width text label (left) plus a growing `draw_bar` (right)
/// within `row`, split via `flex` (Row, gap `ROW_LABEL_GAP_DOTS`). Shared by
/// b4-t3's XP row and b4-t4's stamina row so both consume the exact same
/// label|bar composition.
pub(super) fn draw_labeled_bar(
    buf: &mut Buffer,
    row: DotRect,
    text: &str,
    text_color: Rgba,
    fraction: f32,
    fill_color: Rgba,
) {
    let style = FlexStyle {
        direction: Direction::Row,
        justify_content: Justify::Start,
        align_items: Align::Stretch,
        gap: ROW_LABEL_GAP_DOTS,
    };
    let children = [
        FlexChild { basis: Basis::Fixed(ROW_LABEL_W_DOTS), grow: 0.0, shrink: 0.0 },
        FlexChild { basis: Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
    ];
    let parts = engine_render::flex(row, style, &children);
    if parts.len() < 2 {
        return;
    }
    engine_render::label(buf, parts[0].to_cell_rect(), text, text_color);
    draw_bar(buf, parts[1], fraction, fill_color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenes::test_util::lit_dot_color;
    use ratatui::layout::Rect;

    /// Distinct from `BAR_BORDER_COLOR` so color assertions actually prove
    /// separation, not a coincidence.
    const FILL: Rgba = Rgba::rgb(10, 200, 10);

    /// A whole-cell-aligned target (10x3 cells = 20x12 dots), per research's
    /// suggested test rig.
    fn bar_target() -> DotRect {
        DotRect { x: 0, y: 0, w: 20, h: 12 }
    }

    fn render_bar(fraction: f32) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
        draw_bar(&mut buf, bar_target(), fraction, FILL);
        buf
    }

    /// fraction 0.0 lights no dot anywhere in the interior fill region
    /// (cols `[2, w-2)`, rows `[4, h-4)`); the border ring may still be lit.
    #[test]
    fn fraction_zero_lights_no_fill_dots() {
        let buf = render_bar(0.0);
        let target = bar_target();
        for x in 2..(target.w - 2) {
            for y in 4..(target.h - 4) {
                assert_eq!(
                    lit_dot_color(&buf, x, y),
                    None,
                    "fraction 0.0 must light no interior fill dot at ({x},{y})"
                );
            }
        }
    }

    /// fraction 1.0 lights the full fill span: the last interior fill column
    /// carries a lit `FILL`-colored dot.
    #[test]
    fn fraction_one_lights_full_fill_span() {
        let buf = render_bar(1.0);
        let target = bar_target();
        let last_fill_col = target.w - 3; // strictly inside the border (see research.md math)
        let mid_row = target.h / 2;
        assert_eq!(
            lit_dot_color(&buf, last_fill_col, mid_row),
            Some(FILL),
            "fraction 1.0 must light the last interior fill column"
        );
    }

    /// fraction 0.5 lights a prefix strictly between the empty (0.0) and
    /// full (1.0) counts — proportional, not all-or-nothing.
    #[test]
    fn fraction_half_lights_proportional_prefix() {
        let target = bar_target();
        let mid_row = target.h / 2;
        let count_lit = |buf: &Buffer| -> i32 {
            (2..(target.w - 2))
                .filter(|&x| lit_dot_color(buf, x, mid_row) == Some(FILL))
                .count() as i32
        };

        let empty_n = count_lit(&render_bar(0.0));
        let half_n = count_lit(&render_bar(0.5));
        let full_n = count_lit(&render_bar(1.0));

        assert!(half_n > empty_n, "half fill ({half_n}) must light more than empty ({empty_n})");
        assert!(half_n < full_n, "half fill ({half_n}) must light less than full ({full_n})");
    }

    /// No braille cell is ever both a lit border cell and a lit fill cell:
    /// every `FILL`-colored dot lies inside the inset interior region, and
    /// every non-`FILL` lit dot lies outside it.
    #[test]
    fn border_and_fill_never_share_a_braille_cell() {
        let buf = render_bar(1.0);
        let target = bar_target();
        for x in 0..target.w {
            for y in 0..target.h {
                let Some(color) = lit_dot_color(&buf, x, y) else { continue };
                let in_fill_region =
                    (2..(target.w - 2)).contains(&x) && (4..(target.h - 4)).contains(&y);
                if color == FILL {
                    assert!(
                        in_fill_region,
                        "a fill-colored dot at ({x},{y}) must lie inside the inset interior"
                    );
                } else {
                    assert!(
                        !in_fill_region,
                        "a non-fill lit dot at ({x},{y}) must not lie inside the interior fill region"
                    );
                }
            }
        }
    }

    /// The border ring is lit `BAR_BORDER_COLOR` at a chamfer-clear point
    /// (top edge midpoint).
    #[test]
    fn border_ring_is_lit_border_color() {
        let buf = render_bar(1.0);
        let target = bar_target();
        let mid_col = target.w / 2;
        let color = lit_dot_color(&buf, mid_col, 0).expect("top border midpoint must be lit");
        assert_eq!(color, BAR_BORDER_COLOR, "border ring must be lit BAR_BORDER_COLOR");
    }

    /// The fill decodes to the exact requested `fill_color`, not merely "some
    /// color".
    #[test]
    fn fill_color_decodes_to_requested_color() {
        let buf = render_bar(1.0);
        let target = bar_target();
        let mid_row = target.h / 2;
        let color = lit_dot_color(&buf, 2, mid_row)
            .expect("interior first fill col must be lit at fraction 1.0");
        assert_eq!(color, FILL, "fill dot must decode to the exact requested fill_color");
    }

    /// A `target` too small to fit a full one-cell inset (fill span would be
    /// non-positive) draws without panicking.
    #[test]
    fn too_small_target_draws_without_panic() {
        let target = DotRect { x: 0, y: 0, w: 3, h: 4 };
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        draw_bar(&mut buf, target, 1.0, FILL);
    }
}
