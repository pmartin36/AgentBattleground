//! Regression coverage for `PANEL_GOLD_TINT`/`ICON_AMBER_TINT`: `BUTTON_PANEL`
//! and every bundled icon are pure opaque white, so without pre-tinting both
//! layers, an icon's rendered color would be indistinguishable from the
//! panel's (the bug that made the arrow icon unreadable once composited) and
//! everything would stay grayscale (the follow-up complaint that the result
//! was "boring, no color variation"). Split out of `button_tests.rs` (b1-t1)
//! into its own concern-partitioned sibling file, byte-for-byte unchanged.

use super::*;
use super::button_test_fixtures::*;
use ratatui::style::Color;

fn make_buf(w: u16, h: u16) -> Buffer {
    Buffer::empty(Rect::new(0, 0, w, h))
}

fn luma(c: Color) -> u32 {
    match c {
        Color::Rgb(r, g, b) => r as u32 + g as u32 + b as u32,
        _ => 0,
    }
}

/// The icon (centered, aspect-fit) must render measurably darker than a
/// panel-only cell near the button's edge — proving `ICON_AMBER_TINT`
/// actually creates the contrast the braille rasterizer's per-cell
/// adaptive luma threshold needs to distinguish icon from panel.
#[test]
fn icon_is_darker_than_panel_only_cell() {
    let rect = Rect::new(0, 0, 8, 4);
    let b = Button::new(rect, panel_bytes()).icon(icon_bytes());
    let mut buf = make_buf(8, 4);
    b.render(&mut buf);

    let center = buf.cell((4, 2)).expect("center cell must exist");
    let edge = buf.cell((0, 0)).expect("edge cell must exist");

    assert_ne!(center.symbol(), " ", "center (icon) cell must be painted");
    assert_ne!(edge.symbol(), " ", "edge (panel-only) cell must be painted");
    assert!(
        luma(center.fg) < luma(edge.fg),
        "icon cell (luma {}) must be darker than a panel-only cell (luma {}) for the icon to read as a distinct shape",
        luma(center.fg),
        luma(edge.fg)
    );
}

/// Both the panel and the icon must render with real hue (R/G/B channels
/// not all equal) — a regression guard for "boring, no color variation":
/// a prior version tinted the icon a flat gray, which stayed grayscale
/// no matter what `ButtonState` multiplied on top.
#[test]
fn panel_and_icon_have_real_hue_not_grayscale() {
    let rect = Rect::new(0, 0, 8, 4);
    let b = Button::new(rect, panel_bytes()).icon(icon_bytes());
    let mut buf = make_buf(8, 4);
    b.render(&mut buf);

    let is_grayscale = |c: Color| matches!(c, Color::Rgb(r, g, b) if r == g && g == b);

    let center = buf.cell((4, 2)).expect("center cell must exist").fg;
    let edge = buf.cell((0, 0)).expect("edge cell must exist").fg;

    assert!(!is_grayscale(center), "icon color {center:?} must have real hue, not grayscale");
    assert!(!is_grayscale(edge), "panel color {edge:?} must have real hue, not grayscale");
}
