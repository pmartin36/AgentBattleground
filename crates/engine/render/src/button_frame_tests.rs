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
