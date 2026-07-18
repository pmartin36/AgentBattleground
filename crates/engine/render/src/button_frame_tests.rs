//! Label `Button` tests (`Button::new(rect, frame).label(text)`). Split out
//! of `button_tests.rs` (b1-t1) into its own concern-partitioned sibling
//! file, byte-for-byte unchanged.

use super::*;
use super::button_test_fixtures::*;
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
use ratatui::style::Color;

fn make_buf(w: u16, h: u16) -> Buffer {
    Buffer::empty(Rect::new(0, 0, w, h))
}

fn ev(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

/// Rect large enough for a solid top border clear of the centered label.
fn frame_rect() -> Rect {
    Rect::new(2, 1, 8, 4)
}

/// Renders a fresh label `Button` in `state` and returns the painted fg
/// color of a top-center BORDER cell (not the transparent interior, not
/// the label's center row).
fn render_top_border_fg(state: ButtonState) -> Color {
    let rect = frame_rect();
    let mut b = Button::new(rect, frame_bytes()).label("Go");
    let inside = (rect.x + 1, rect.y + 1);
    match state {
        ButtonState::Idle => {}
        ButtonState::Hover => {
            b.handle_mouse(&ev(MouseEventKind::Moved, inside.0, inside.1));
        }
        ButtonState::Pressed => {
            b.handle_mouse(&ev(MouseEventKind::Moved, inside.0, inside.1));
            b.handle_mouse(&ev(
                MouseEventKind::Down(MouseButton::Left),
                inside.0,
                inside.1,
            ));
        }
    }
    assert_eq!(b.state(), state, "test setup must reach the target state");

    let mut buf = make_buf(16, 8);
    b.render(&mut buf);

    let bx = rect.x + rect.width / 2;
    let by = rect.y;
    let cell = buf
        .cell((bx, by))
        .unwrap_or_else(|| panic!("top-border cell ({bx},{by}) must exist in the buffer"));
    assert_ne!(
        cell.symbol(),
        " ",
        "top-border cell must be painted (border ring is opaque there) in state {state:?}"
    );
    cell.fg
}

/// `render` must produce a visibly different painted color for each of
/// the three `ButtonState`s at the same border cell, proving the
/// per-state tint is wired through the frame's dot pipeline.
#[test]
fn frame_button_render_tints_differ_across_all_three_states() {
    let idle = render_top_border_fg(ButtonState::Idle);
    let hover = render_top_border_fg(ButtonState::Hover);
    let pressed = render_top_border_fg(ButtonState::Pressed);

    assert_ne!(idle, hover, "Idle and Hover must paint different colors");
    assert_ne!(idle, pressed, "Idle and Pressed must paint different colors");
    assert_ne!(hover, pressed, "Hover and Pressed must paint different colors");
}

/// `render` draws the exact label text centered on the rect's middle
/// row, matching `label`'s own centering formula.
#[test]
fn frame_button_render_draws_centered_label() {
    let rect = frame_rect();
    let b = Button::new(rect, frame_bytes()).label("Go");
    let mut buf = make_buf(16, 8);
    b.render(&mut buf);

    let text_len: u16 = 2; // "Go"
    let expected_y = rect.y + rect.height / 2;
    let expected_x = rect.x + (rect.width - text_len) / 2;

    let first = buf.cell((expected_x, expected_y)).unwrap();
    assert_eq!(
        first.symbol(),
        "G",
        "first char of label 'Go' must be at ({expected_x},{expected_y})"
    );
    let second = buf.cell((expected_x + 1, expected_y)).unwrap();
    assert_eq!(
        second.symbol(),
        "o",
        "second char of label 'Go' must be at ({},{expected_y})",
        expected_x + 1
    );
}

/// A completed click (Moved-inside, Down-inside, Up-inside) reuses
/// `ButtonCore`'s transition table: `Up` returns `true` and state ends
/// `Hover`, matching `Button`'s equivalent test.
#[test]
fn frame_button_handle_mouse_completes_click() {
    let rect = frame_rect();
    let inside = (rect.x + 1, rect.y + 1);
    let mut b = Button::new(rect, frame_bytes()).label("Go");

    b.handle_mouse(&ev(MouseEventKind::Moved, inside.0, inside.1));
    b.handle_mouse(&ev(MouseEventKind::Down(MouseButton::Left), inside.0, inside.1));
    let fired = b.handle_mouse(&ev(MouseEventKind::Up(MouseButton::Left), inside.0, inside.1));

    assert!(fired, "Up inside while Pressed must report a completed click");
    assert_eq!(b.state(), ButtonState::Hover);
}

/// Render a frame `Button` at `rect` into a buffer with a 1-cell margin on
/// every side, so cells just outside the button rect are addressable.
fn render_frame_button(rect: Rect) -> Buffer {
    let b = Button::new(rect, frame_bytes());
    let mut buf = make_buf(rect.x + rect.width + 2, rect.y + rect.height + 2);
    b.render(&mut buf);
    buf
}

/// Decision 1: the procedural border must light every corner cell of the
/// button rect — no 4-corner gap — at multiple sizes, verified on decoded
/// dots (not `Rect` field comparison). `(20, 3)` is the hub-representative
/// size (already gap-free even under today's stretched raster); `(20, 10)`
/// is a taller, near-square dot-aspect size where today's stretch-fit
/// raster genuinely loses the extreme corner dot to Lanczos3 downscale
/// blending (empirically confirmed against current `render_button`) — the
/// procedural border must stay corner-complete there too.
#[test]
fn frame_button_border_lights_all_four_corner_cells() {
    for rect in [Rect::new(1, 1, 20, 3), Rect::new(1, 1, 20, 10), Rect::new(1, 1, 4, 1)] {
        let buf = render_frame_button(rect);
        let corners = [
            (rect.x, rect.y),
            (rect.x + rect.width - 1, rect.y),
            (rect.x, rect.y + rect.height - 1),
            (rect.x + rect.width - 1, rect.y + rect.height - 1),
        ];
        for (cx, cy) in corners {
            let decoded = crate::decode_braille_cell(&buf, cx, cy);
            assert!(
                decoded.is_some(),
                "corner cell ({cx},{cy}) of rect {rect:?} must have a lit dot"
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// b2-t3: active-style override (border + label recolor, alpha fade)
// ─────────────────────────────────────────────────────────────────────────

/// Distinct sentinel colors for the override — deliberately NOT the spec's
/// real active-white (b4 wires the real palette); only distinctness from
/// the default gold border / `#f0f0f0` label matters here.
const OVERRIDE_BORDER: Rgba = Rgba::rgb(0x11, 0x22, 0x33);
const OVERRIDE_LABEL: Rgba = Rgba::rgb(0x44, 0x55, 0x66);

/// Top-center border cell + label first-char cell coordinates for
/// `frame_rect()`, matching `render_top_border_fg`'s / the centered-label
/// test's own math.
fn border_cell(rect: Rect) -> (u16, u16) {
    (rect.x + rect.width / 2, rect.y)
}
fn label_cell(rect: Rect) -> (u16, u16) {
    let text_len: u16 = 2; // "Go"
    (rect.x + (rect.width - text_len) / 2, rect.y + rect.height / 2)
}

/// `set_active_style` must recolor BOTH the frame border and the label to
/// the override's absolute colors while `ButtonState::Idle` — bypassing the
/// gold border tint and the default `#f0f0f0` label.
#[test]
fn active_style_overrides_border_and_label_color_while_idle() {
    let rect = frame_rect();
    let mut b = Button::new(rect, frame_bytes()).label("Go");
    assert_eq!(b.state(), ButtonState::Idle, "test setup must stay Idle");
    b.set_active_style(Some(ActiveStyle {
        border: OVERRIDE_BORDER,
        label: OVERRIDE_LABEL,
    }));

    let mut buf = make_buf(16, 8);
    b.render(&mut buf);

    let (bx, by) = border_cell(rect);
    let (_, decoded_border) = crate::decode_braille_cell(&buf, bx, by)
        .expect("top-border cell must still be lit under an active-style override");
    assert_eq!(
        decoded_border, OVERRIDE_BORDER,
        "border must paint the override color, bypassing the gold/state tint"
    );

    let (lx, ly) = label_cell(rect);
    let label_fg = buf.cell((lx, ly)).unwrap().fg;
    assert_eq!(
        label_fg,
        Color::Rgb(OVERRIDE_LABEL.r, OVERRIDE_LABEL.g, OVERRIDE_LABEL.b),
        "label must paint the override color, bypassing StateColors.label"
    );
}

/// An override border with `a < 255` renders TRANSLUCENT: the border glyph
/// keeps the same lit-dot mask as the fully-opaque override (alpha reduces
/// color, not coverage), but its decoded color is `border.over(black)` — the
/// backdrop-blended color, not the opaque override color.
#[test]
fn active_style_translucent_border_keeps_mask_but_blends_color() {
    let rect = frame_rect();

    let mut b_opaque = Button::new(rect, frame_bytes());
    b_opaque.set_active_style(Some(ActiveStyle {
        border: OVERRIDE_BORDER,
        label: OVERRIDE_LABEL,
    }));
    let mut buf_opaque = make_buf(16, 8);
    b_opaque.render(&mut buf_opaque);

    let translucent_border = Rgba::new(OVERRIDE_BORDER.r, OVERRIDE_BORDER.g, OVERRIDE_BORDER.b, 0x80);
    let mut b_translucent = Button::new(rect, frame_bytes());
    b_translucent.set_active_style(Some(ActiveStyle {
        border: translucent_border,
        label: OVERRIDE_LABEL,
    }));
    let mut buf_translucent = make_buf(16, 8);
    b_translucent.render(&mut buf_translucent);

    let (bx, by) = border_cell(rect);
    let (mask_opaque, _) = crate::decode_braille_cell(&buf_opaque, bx, by)
        .expect("opaque override border cell must be lit");
    let (mask_translucent, color_translucent) = crate::decode_braille_cell(&buf_translucent, bx, by)
        .expect("translucent override border cell must still be lit — alpha reduces color, not coverage");

    assert_eq!(
        mask_translucent, mask_opaque,
        "alpha must not change which dots are lit"
    );

    // `decode_braille_cell` reads back a `ratatui::Color::Rgb`, which carries
    // no alpha — its returned `Rgba` is always opaque (`a==255`) regardless
    // of the alpha that was blended upstream. Compare only the blended RGB.
    let blended = translucent_border.over(Rgba::new(0, 0, 0, 0xFF));
    let expected = Rgba::rgb(blended.r, blended.g, blended.b);
    assert_eq!(
        color_translucent, expected,
        "translucent border must blend toward the backdrop via Rgba::over, not paint the opaque override color"
    );
}

/// An override whose border AND label alpha are both `0` must paint NO
/// visible dots and NO label glyph at all (not merely blend to invisible).
#[test]
fn active_style_full_transparent_paints_nothing() {
    let rect = frame_rect();
    let mut b = Button::new(rect, frame_bytes()).label("Go");
    b.set_active_style(Some(ActiveStyle {
        border: Rgba::new(OVERRIDE_BORDER.r, OVERRIDE_BORDER.g, OVERRIDE_BORDER.b, 0),
        label: Rgba::new(OVERRIDE_LABEL.r, OVERRIDE_LABEL.g, OVERRIDE_LABEL.b, 0),
    }));

    let mut buf = make_buf(16, 8);
    b.render(&mut buf);

    let (bx, by) = border_cell(rect);
    assert!(
        crate::decode_braille_cell(&buf, bx, by).is_none(),
        "border must paint no dots when the active-style override is fully transparent"
    );

    let (lx, ly) = label_cell(rect);
    assert_eq!(
        buf.cell((lx, ly)).unwrap().symbol(),
        " ",
        "label must paint no glyph when the active-style override is fully transparent"
    );
}
