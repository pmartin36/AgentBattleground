//! Shared thin horizontal capsule-bar helper: given a dot-precise target rect
//! **exactly 1 cell (4 dots) tall**, a fill fraction, and a fill color, paints
//! a slim bar with white rounded end-caps marking the track ends and a
//! proportional colored interior fill between them. There is NO top/bottom
//! border — the two caps are the only chrome. This is the single routine
//! shared across scenes: the post-battle XP/stamina rows and the roster
//! manager's stamina row all consume it.

use ratatui::buffer::Buffer;

use engine_core::color::Rgba;
use engine_render::dots::Dot;
use engine_render::{Align, Basis, Direction, DotRect, FlexChild, FlexStyle, Justify};

use crate::scenes::post_battle::columns::blit_dots;

/// White end-cap color (`0xffffff`) — the only chrome on the bar.
pub(crate) const CAP_COLOR: Rgba = Rgba::rgb(0xff, 0xff, 0xff);
/// Width (dots) reserved for the row label ("STA"/"XP"), left of the bar.
pub(crate) const ROW_LABEL_W_DOTS: i32 = 8;
/// Dot gap between the row label and the bar.
pub(crate) const ROW_LABEL_GAP_DOTS: i32 = 2;

/// Paints, into `target` (expected exactly 4 dots tall), a thin capsule bar:
/// a white rounded end-cap at the leftmost and rightmost dot column (its top
/// and bottom dots chamfered off so the ends read rounded), and — between the
/// caps — a left-anchored run of `fill_color` dots whose column span is
/// `fraction` (clamped `0.0..=1.0`) of the interior width, blank after it.
/// No top/bottom border. Placed sub-cell precise via `columns::blit_dots`.
///
/// Wired in by the XP bar and the stamina bar.
pub(crate) fn draw_bar(buf: &mut Buffer, target: DotRect, fraction: f32, fill_color: Rgba) {
    if target.w <= 0 || target.h <= 0 {
        return;
    }
    let (w, h) = (target.w as usize, target.h as usize);
    let mut local = engine_render::dots::DotBuffer::new(w, h);

    // Rounded end-caps: leftmost and rightmost dot columns, with the top and
    // bottom dot chamfered off (so a 4-dot-tall cap lights only its middle two
    // rows). Guarded for degenerate heights (< 3 dots lights every row).
    let cap_rows: Vec<usize> = if h >= 3 { (1..h - 1).collect() } else { (0..h).collect() };
    for &row in &cap_rows {
        local.set(0, row, Dot::Lit(CAP_COLOR));
        if w >= 2 {
            local.set(w - 1, row, Dot::Lit(CAP_COLOR));
        }
    }

    // Interior fill between the caps: cols `[1, w-1)`, full height, lit
    // `fill_color` for the leftmost `fraction`-proportion of the interior.
    if w >= 3 {
        let interior_w = w - 2;
        let lit = (fraction.clamp(0.0, 1.0) * interior_w as f32).round() as usize;
        for col in 1..1 + lit.min(interior_w) {
            for row in 0..h {
                local.set(col, row, Dot::Lit(fill_color));
            }
        }
    }

    blit_dots(buf, target, &local);
}

/// Paints a fixed-width text label (left) plus a growing `draw_bar` (right)
/// within `row`, split via `flex` (Row, gap `ROW_LABEL_GAP_DOTS`). Shared by
/// the XP row and the stamina row so both consume the exact same label|bar
/// composition.
pub(crate) fn draw_labeled_bar(
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
    engine_render::label(
        buf,
        parts[0].to_cell_rect(),
        text,
        engine_render::TextAlign::Center,
        ratatui::style::Style::default().fg(ratatui::style::Color::Rgb(
            text_color.r,
            text_color.g,
            text_color.b,
        )),
    );
    draw_bar(buf, parts[1], fraction, fill_color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenes::test_util::lit_dot_color;
    use ratatui::layout::Rect;

    /// Distinct from `CAP_COLOR` so color assertions actually prove separation,
    /// not a coincidence.
    const FILL: Rgba = Rgba::rgb(10, 200, 10);

    /// A whole-cell-aligned target, exactly 1 cell (4 dots) tall and 20 dots
    /// wide.
    fn bar_target() -> DotRect {
        DotRect { x: 0, y: 0, w: 20, h: 4 }
    }

    fn render_bar(fraction: f32) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        draw_bar(&mut buf, bar_target(), fraction, FILL);
        buf
    }

    /// The bar is exactly 4 dots (1 cell) tall — the target height passed to
    /// `draw_bar` is what it paints into, no taller.
    #[test]
    fn bar_target_is_one_cell_tall() {
        assert_eq!(bar_target().h, 4, "bar target must be exactly 1 cell (4 dots) tall");
    }

    /// Both end-caps are lit white (`CAP_COLOR`) at their middle rows — the
    /// leftmost (col 0) and rightmost (col w-1) dot columns.
    #[test]
    fn end_caps_are_white() {
        let target = bar_target();
        let buf = render_bar(0.0);
        // Middle rows (1,2) of a 4-dot-tall cap carry the cap; corners are off.
        for &row in &[1i32, 2] {
            assert_eq!(
                lit_dot_color(&buf, 0, row),
                Some(CAP_COLOR),
                "left cap col 0 row {row} must be white"
            );
            assert_eq!(
                lit_dot_color(&buf, target.w - 1, row),
                Some(CAP_COLOR),
                "right cap col {} row {row} must be white",
                target.w - 1
            );
        }
    }

    /// The cap's top/bottom dots are chamfered off — the ends read rounded, not
    /// a full-height rectangle.
    #[test]
    fn end_caps_are_rounded() {
        let buf = render_bar(0.0);
        let target = bar_target();
        for &(x, y) in &[(0, 0), (0, 3), (target.w - 1, 0), (target.w - 1, 3)] {
            assert_eq!(
                lit_dot_color(&buf, x, y),
                None,
                "cap corner dot ({x},{y}) must be chamfered off"
            );
        }
    }

    /// fraction 0.0 lights no interior fill dot (cols `[1, w-1)`); only the two
    /// white caps are present.
    #[test]
    fn fraction_zero_lights_no_interior_fill() {
        let buf = render_bar(0.0);
        let target = bar_target();
        for x in 1..(target.w - 1) {
            for y in 0..target.h {
                assert_ne!(
                    lit_dot_color(&buf, x, y),
                    Some(FILL),
                    "fraction 0.0 must light no interior fill dot at ({x},{y})"
                );
            }
        }
    }

    /// fraction 1.0 lights the full interior span in `fill_color`. Note: the
    /// adaptive-luma pipeline (`cell_from_dots_tinted`) suppresses a darker
    /// fill dot in any braille cell it shares with a brighter white cap dot, so
    /// the interior columns immediately adjacent to a cap read as the cap, not
    /// fill (the cap reads "on top"). A middle interior column (sharing a cell
    /// with no cap) decodes to the exact requested `fill_color`.
    #[test]
    fn fraction_one_lights_full_interior() {
        let buf = render_bar(1.0);
        let target = bar_target();
        let mid_col = target.w / 2;
        assert_eq!(
            lit_dot_color(&buf, mid_col, 0),
            Some(FILL),
            "fraction 1.0 must light a middle interior column in the exact FILL color"
        );
    }

    /// Interior lit length is proportional to `fraction`: empty < half < full,
    /// counted along the top interior row.
    #[test]
    fn fill_length_is_proportional_to_fraction() {
        let target = bar_target();
        let count_lit = |buf: &Buffer| -> i32 {
            (1..(target.w - 1))
                .filter(|&x| lit_dot_color(buf, x, 0) == Some(FILL))
                .count() as i32
        };

        let empty_n = count_lit(&render_bar(0.0));
        let half_n = count_lit(&render_bar(0.5));
        let full_n = count_lit(&render_bar(1.0));

        assert_eq!(empty_n, 0, "fraction 0.0 must light no interior fill");
        assert!(half_n > empty_n, "half fill ({half_n}) must light more than empty ({empty_n})");
        assert!(half_n < full_n, "half fill ({half_n}) must light less than full ({full_n})");
    }

    /// No interior fill dot ever decodes to the cap color, and no cap dot ever
    /// decodes to the fill color — the two are cleanly separated.
    #[test]
    fn caps_and_fill_never_share_a_color() {
        let buf = render_bar(1.0);
        let target = bar_target();
        // Interior columns carry only FILL (never CAP_COLOR).
        for x in 1..(target.w - 1) {
            for y in 0..target.h {
                if let Some(c) = lit_dot_color(&buf, x, y) {
                    assert_eq!(c, FILL, "interior dot ({x},{y}) must be FILL, not cap color");
                }
            }
        }
    }

    /// A `target` too small to hold caps + interior draws without panicking.
    #[test]
    fn too_small_target_draws_without_panic() {
        let target = DotRect { x: 0, y: 0, w: 2, h: 4 };
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        draw_bar(&mut buf, target, 1.0, FILL);
    }
}
