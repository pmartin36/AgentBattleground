//! Sub-cell dot placement (`draw_dots_at`) and the shared procedural border
//! (`draw_dot_border`) — both decode the actual rendered braille dots via
//! `decode_braille_cell` rather than comparing coordinate fields, since a
//! flooring bug is invisible at the `Rect`/`DotRect` level but visible at the
//! dot level.

use engine_core::color::Rgba;
use engine_render::dots::{Dot, DotBuffer};
use engine_render::{decode_braille_cell, draw_dot_border, draw_dots_at, rounded_rect, DotRect};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn make_buf(w: u16, h: u16) -> Buffer {
    Buffer::empty(Rect::new(0, 0, w, h))
}

/// A `DotRect` with a non-zero `cell_remainder()` must place its dot at the
/// sub-cell row/col the remainder demands, not floor it to the cell origin.
/// `DotRect{x:1,y:2,w:1,h:1}` floors to cell (0,0) with remainder (1,2);
/// `DOTS[5] == (1,2,5)`, so the lit dot must set bit 5 and leave bit 0 (the
/// cell-origin position) clear.
#[test]
fn draw_dots_at_places_dot_at_subcell_position_from_remainder() {
    let mut dots = DotBuffer::new(1, 1);
    dots.set(0, 0, Dot::Lit(Rgba::rgb(200, 100, 50)));

    let mut buf = make_buf(2, 2);
    draw_dots_at(&mut buf, DotRect { x: 1, y: 2, w: 1, h: 1 }, &dots);

    let (mask, _color) = decode_braille_cell(&buf, 0, 0)
        .expect("cell (0,0) must be painted");
    assert_eq!(
        mask & (1 << 5),
        1 << 5,
        "dot must land at the sub-cell position (1,2) the remainder demands (mask={mask:#04x})"
    );
    assert_eq!(
        mask & 0x01,
        0,
        "dot must NOT be floored to the cell origin (0,0) (mask={mask:#04x})"
    );
}

/// A zero-width or zero-height `DotRect` is a no-op: the buffer is left
/// byte-identical to a clone taken before the call.
#[test]
fn draw_dots_at_zero_size_is_noop() {
    let dots = {
        let mut d = DotBuffer::new(2, 4);
        d.set(0, 0, Dot::Lit(Rgba::rgb(1, 2, 3)));
        d
    };

    let mut buf = make_buf(4, 4);
    let before = buf.clone();
    draw_dots_at(&mut buf, DotRect { x: 0, y: 0, w: 0, h: 4 }, &dots);
    assert_eq!(buf, before, "zero-width rect must leave the buffer unchanged");

    draw_dots_at(&mut buf, DotRect { x: 0, y: 0, w: 2, h: 0 }, &dots);
    assert_eq!(buf, before, "zero-height rect must leave the buffer unchanged");
}

/// A `DotRect` far larger than the destination buffer, and one with a
/// negative origin, must both return without panicking.
#[test]
fn draw_dots_at_oversized_and_negative_do_not_panic() {
    let mut buf = make_buf(4, 4);
    let big_dots = DotBuffer::new(40, 40);
    draw_dots_at(&mut buf, DotRect { x: 0, y: 0, w: 40, h: 40 }, &big_dots);

    let mut buf2 = make_buf(4, 4);
    let small_dots = DotBuffer::new(2, 4);
    draw_dots_at(&mut buf2, DotRect { x: -10, y: -20, w: 2, h: 4 }, &small_dots);
    // Reaching here without panicking is the assertion.
}

/// At `thickness=1, corner_radius=1`, a cell-aligned border lights the
/// perimeter with the outer corner dot chamfered off (mirrors the roster's
/// `corners_are_chamfered_not_square`), while a mid-top-edge cell keeps its
/// own top-left dot lit — proving the clip is corner-local, not blanket.
#[test]
fn draw_dot_border_perimeter_lit_corners_chamfered() {
    let border = Rgba::rgb(10, 20, 30);
    // 16x8 dots = 8 cell cols x 2 cell rows.
    let mut buf = make_buf(8, 2);
    draw_dot_border(&mut buf, DotRect { x: 0, y: 0, w: 16, h: 8 }, 1, 1, border);

    let (corner_mask, _) = decode_braille_cell(&buf, 0, 0)
        .expect("top-left corner cell must be painted");
    assert_eq!(
        corner_mask & 0x01,
        0,
        "outer corner dot (bit 0) must be chamfered off (mask={corner_mask:#04x})"
    );
    assert_ne!(corner_mask, 0, "corner cell must still carry the two edges meeting there");

    let (mid_mask, _) = decode_braille_cell(&buf, 4, 0)
        .expect("mid-top-edge cell must be painted");
    assert_eq!(
        mid_mask & 0x01,
        0x01,
        "mid-top-edge cell's top-left dot must stay lit — the chamfer is corner-local (mask={mid_mask:#04x})"
    );
}

/// The interior of a `draw_dot_border` ring is left `Transparent`, so
/// pre-existing content behind it survives the border draw untouched.
#[test]
fn draw_dot_border_interior_preserves_content_behind() {
    let mut buf = make_buf(10, 4);
    engine_render::fill(&mut buf, Rect::new(5, 2, 1, 1), Rgba::rgb(200, 50, 50));
    let before_symbol = buf.cell((5, 2)).unwrap().symbol().to_string();
    let before_fg = buf.cell((5, 2)).unwrap().fg;

    draw_dot_border(&mut buf, DotRect { x: 0, y: 0, w: 20, h: 16 }, 1, 1, Rgba::rgb(10, 20, 30));

    let cell = buf.cell((5, 2)).unwrap();
    assert_eq!(cell.symbol(), before_symbol, "interior content's glyph must survive the border draw");
    assert_eq!(cell.fg, before_fg, "interior content's color must survive the border draw");
}

/// `draw_dot_border` is exactly `rounded_rect` placed via `draw_dots_at` —
/// pins the composition rather than letting the two drift independently.
#[test]
fn draw_dot_border_matches_rounded_rect_via_draw_dots_at() {
    let color = Rgba::rgb(1, 2, 3);
    let rect = DotRect { x: 1, y: 2, w: 16, h: 8 };

    let mut via_border = make_buf(10, 4);
    draw_dot_border(&mut via_border, rect, 1, 1, color);

    let mut via_manual = make_buf(10, 4);
    let expected_dots = rounded_rect(rect.w as usize, rect.h as usize, 1, 1, color, Dot::Transparent);
    draw_dots_at(&mut via_manual, rect, &expected_dots);

    assert_eq!(
        via_border, via_manual,
        "draw_dot_border must equal draw_dots_at(rect, rounded_rect(..)) byte-for-byte"
    );
}
