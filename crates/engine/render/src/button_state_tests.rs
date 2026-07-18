//! Button state-machine, hit-test, and basic render/color tests. Split out
//! of `button_tests.rs` (b1-t1) — the former `mod tests` block, moved to its
//! own concern-partitioned sibling file, byte-for-byte unchanged.

use super::*;
use super::button_test_fixtures::*;
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
use ratatui::style::Color;


// Fixed test rect: covers x in [2,6), y in [2,5). (3,3) is inside; (0,0)
// is outside.
fn rect() -> Rect {
    Rect::new(2, 2, 4, 3)
}

fn ev(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

const INSIDE: (u16, u16) = (3, 3);
const OUTSIDE: (u16, u16) = (0, 0);

/// `Moved` inside from `Idle` transitions to `Hover`.
#[test]
fn moved_inside_from_idle_transitions_to_hover() {
    let mut b = Button::new(rect(), panel_bytes()).icon(icon_bytes());
    assert_eq!(b.state(), ButtonState::Idle);
    let fired = b.handle_mouse(&ev(MouseEventKind::Moved, INSIDE.0, INSIDE.1));
    assert!(!fired);
    assert_eq!(b.state(), ButtonState::Hover);
}

/// `Moved` outside from `Hover` reverts to `Idle`.
#[test]
fn moved_outside_from_hover_reverts_to_idle() {
    let mut b = Button::new(rect(), panel_bytes()).icon(icon_bytes());
    b.handle_mouse(&ev(MouseEventKind::Moved, INSIDE.0, INSIDE.1));
    assert_eq!(b.state(), ButtonState::Hover);
    let fired = b.handle_mouse(&ev(MouseEventKind::Moved, OUTSIDE.0, OUTSIDE.1));
    assert!(!fired);
    assert_eq!(b.state(), ButtonState::Idle);
}

/// `Moved` outside while `Pressed` does NOT revert — state stays
/// `Pressed` (the asymmetric exception in the transition table).
#[test]
fn moved_outside_while_pressed_stays_pressed() {
    let mut b = Button::new(rect(), panel_bytes()).icon(icon_bytes());
    b.handle_mouse(&ev(MouseEventKind::Moved, INSIDE.0, INSIDE.1));
    b.handle_mouse(&ev(
        MouseEventKind::Down(MouseButton::Left),
        INSIDE.0,
        INSIDE.1,
    ));
    assert_eq!(b.state(), ButtonState::Pressed);
    let fired = b.handle_mouse(&ev(MouseEventKind::Moved, OUTSIDE.0, OUTSIDE.1));
    assert!(!fired);
    assert_eq!(
        b.state(),
        ButtonState::Pressed,
        "Pressed must not revert when the pointer moves outside"
    );
}

/// `Down(Left)` inside from `Hover` transitions to `Pressed`.
#[test]
fn down_left_inside_from_hover_transitions_to_pressed() {
    let mut b = Button::new(rect(), panel_bytes()).icon(icon_bytes());
    b.handle_mouse(&ev(MouseEventKind::Moved, INSIDE.0, INSIDE.1));
    let fired = b.handle_mouse(&ev(
        MouseEventKind::Down(MouseButton::Left),
        INSIDE.0,
        INSIDE.1,
    ));
    assert!(!fired);
    assert_eq!(b.state(), ButtonState::Pressed);
}

/// `Up(Left)` inside while `Pressed` completes the click: returns
/// `true` and reverts to `Hover`.
#[test]
fn up_left_inside_while_pressed_completes_click_and_reverts_to_hover() {
    let mut b = Button::new(rect(), panel_bytes()).icon(icon_bytes());
    b.handle_mouse(&ev(MouseEventKind::Moved, INSIDE.0, INSIDE.1));
    b.handle_mouse(&ev(
        MouseEventKind::Down(MouseButton::Left),
        INSIDE.0,
        INSIDE.1,
    ));
    let fired = b.handle_mouse(&ev(
        MouseEventKind::Up(MouseButton::Left),
        INSIDE.0,
        INSIDE.1,
    ));
    assert!(fired, "Up inside while Pressed must report a completed click");
    assert_eq!(b.state(), ButtonState::Hover);
}

/// `Up(Left)` outside while `Pressed` cancels the click: returns
/// `false` and reverts to `Idle` (not `Hover`, since the pointer is
/// outside).
#[test]
fn up_left_outside_while_pressed_cancels_click_and_reverts_to_idle() {
    let mut b = Button::new(rect(), panel_bytes()).icon(icon_bytes());
    b.handle_mouse(&ev(MouseEventKind::Moved, INSIDE.0, INSIDE.1));
    b.handle_mouse(&ev(
        MouseEventKind::Down(MouseButton::Left),
        INSIDE.0,
        INSIDE.1,
    ));
    let fired = b.handle_mouse(&ev(
        MouseEventKind::Up(MouseButton::Left),
        OUTSIDE.0,
        OUTSIDE.1,
    ));
    assert!(
        !fired,
        "Up outside while Pressed must NOT report a completed click"
    );
    assert_eq!(b.state(), ButtonState::Idle);
}

/// `Down` with a non-Left button (e.g. Right) must not press the
/// button — state and return value unchanged.
#[test]
fn down_right_inside_is_ignored() {
    let mut b = Button::new(rect(), panel_bytes()).icon(icon_bytes());
    b.handle_mouse(&ev(MouseEventKind::Moved, INSIDE.0, INSIDE.1));
    assert_eq!(b.state(), ButtonState::Hover);
    let fired = b.handle_mouse(&ev(
        MouseEventKind::Down(MouseButton::Right),
        INSIDE.0,
        INSIDE.1,
    ));
    assert!(!fired);
    assert_eq!(b.state(), ButtonState::Hover);
}

/// `Drag` events are ignored entirely — state unchanged, never fires.
#[test]
fn drag_is_ignored() {
    let mut b = Button::new(rect(), panel_bytes()).icon(icon_bytes());
    b.handle_mouse(&ev(MouseEventKind::Moved, INSIDE.0, INSIDE.1));
    b.handle_mouse(&ev(
        MouseEventKind::Down(MouseButton::Left),
        INSIDE.0,
        INSIDE.1,
    ));
    assert_eq!(b.state(), ButtonState::Pressed);
    let fired = b.handle_mouse(&ev(
        MouseEventKind::Drag(MouseButton::Left),
        OUTSIDE.0,
        OUTSIDE.1,
    ));
    assert!(!fired);
    assert_eq!(b.state(), ButtonState::Pressed);
}

/// Each `ButtonState` maps to its exact spec-mandated tint color
/// (spec 22 lines 12-14): Idle ~78% gray, Hover full-brightness
/// pass-through, Pressed ~55% gray.
#[test]
fn tint_color_matches_spec_constants_per_state() {
    assert_eq!(ButtonState::Idle.tint_color(), Rgba::rgb(0xc8, 0xc8, 0xc8));
    assert_eq!(ButtonState::Hover.tint_color(), Rgba::rgb(0xff, 0xff, 0xff));
    assert_eq!(
        ButtonState::Pressed.tint_color(),
        Rgba::rgb(0x8c, 0x8c, 0x8c)
    );
}

/// `ButtonColors::default()` must reproduce today's look exactly: each
/// state's `background`/`icon` equal `ButtonState::_.tint_color()`, and
/// `label` is the constant 0xf0f0f0 (today's default label color) across
/// all three states (spec's Decisions, lines 55-61; b2-t1 research.md).
#[test]
fn button_colors_default_matches_current_look() {
    let colors = ButtonColors::default();
    const LABEL: Rgba = Rgba::rgb(0xf0, 0xf0, 0xf0);

    assert_eq!(colors.idle.background, ButtonState::Idle.tint_color());
    assert_eq!(colors.idle.icon, ButtonState::Idle.tint_color());
    assert_eq!(colors.idle.label, LABEL);

    assert_eq!(colors.hover.background, ButtonState::Hover.tint_color());
    assert_eq!(colors.hover.icon, ButtonState::Hover.tint_color());
    assert_eq!(colors.hover.label, LABEL);

    assert_eq!(colors.pressed.background, ButtonState::Pressed.tint_color());
    assert_eq!(colors.pressed.icon, ButtonState::Pressed.tint_color());
    assert_eq!(colors.pressed.label, LABEL);
}

/// `set_rect` updates the hit-test area used by subsequent events.
#[test]
fn set_rect_updates_hit_test_area() {
    let mut b = Button::new(Rect::new(0, 0, 1, 1), panel_bytes()).icon(icon_bytes());
    b.set_rect(rect());
    let fired = b.handle_mouse(&ev(MouseEventKind::Moved, INSIDE.0, INSIDE.1));
    assert!(!fired);
    assert_eq!(
        b.state(),
        ButtonState::Hover,
        "handle_mouse must hit-test against the rect set via set_rect"
    );
}

// ---------------------------------------------------------------- render

fn make_buf(w: u16, h: u16) -> Buffer {
    Buffer::empty(Rect::new(0, 0, w, h))
}

/// Button rect used by the render tests, placed away from the buffer
/// origin so "outside the rect" and "outside the buffer" are distinct.
fn render_rect() -> Rect {
    Rect::new(2, 1, 8, 4)
}

/// Renders a fresh `Button` in `state` and returns the painted fg color
/// of the button rect's center cell (the panel's opaque body — never a
/// transparent corner — so a `Glyph` must have been written there in
/// every state).
fn render_center_fg(state: ButtonState) -> Color {
    let rect = render_rect();
    let mut b = Button::new(rect, panel_bytes()).icon(icon_bytes());
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

    let cx = rect.x + rect.width / 2;
    let cy = rect.y + rect.height / 2;
    let cell = buf
        .cell((cx, cy))
        .unwrap_or_else(|| panic!("center cell ({cx},{cy}) must exist in the buffer"));
    assert_ne!(
        cell.symbol(),
        " ",
        "center cell must be painted (panel body is opaque there) in state {state:?}"
    );
    cell.fg
}

/// `render` must produce a visibly different painted color for each of
/// the three `ButtonState`s at the same on-screen cell — proving the
/// per-state tint (b3-t1) is actually wired through the composed
/// panel+icon dot pipeline, not just "doesn't panic".
#[test]
fn render_tints_differ_across_all_three_states() {
    let idle = render_center_fg(ButtonState::Idle);
    let hover = render_center_fg(ButtonState::Hover);
    let pressed = render_center_fg(ButtonState::Pressed);

    assert_ne!(idle, hover, "Idle and Hover must paint different colors");
    assert_ne!(idle, pressed, "Idle and Pressed must paint different colors");
    assert_ne!(hover, pressed, "Hover and Pressed must paint different colors");
}

/// `render` must leave buffer cells outside `self.rect` untouched, and
/// must not panic on a zero-area rect or a rect larger than the
/// destination buffer.
#[test]
fn render_paints_only_within_rect_and_no_panic() {
    let rect = render_rect();
    let b = Button::new(rect, panel_bytes()).icon(icon_bytes());
    let mut buf = make_buf(16, 8);
    b.render(&mut buf);

    let outside = buf
        .cell((15u16, 7u16))
        .expect("cell (15,7) must exist in a 16x8 buffer");
    assert_eq!(
        outside.symbol(),
        " ",
        "cell well outside the button rect must be untouched"
    );

    let zero = Button::new(Rect::new(0, 0, 0, 0), panel_bytes()).icon(icon_bytes());
    let mut buf_zero = make_buf(4, 4);
    zero.render(&mut buf_zero); // must not panic

    let oversized = Button::new(Rect::new(0, 0, 50, 50), panel_bytes()).icon(icon_bytes());
    let mut buf_small = make_buf(5, 5);
    oversized.render(&mut buf_small); // must not panic
}
