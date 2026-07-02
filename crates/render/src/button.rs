//! Mouse-driven button state machine (hit-testing only — no rendering).
//!
//! See `specs/22-braille-ui-chrome.md` lines 15-19 for the transition table.
//! Rendering (tint + composed panel/icon draw) is implemented separately on
//! top of this module (bucket b3); this module is state-machine only.

use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

/// Visual/interaction state of a [`Button`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonState {
    #[default]
    Idle,
    Hover,
    Pressed,
}

/// A clickable, hoverable on-screen button. Owns its hit-test rect and
/// current [`ButtonState`]; mutated by feeding it mouse events.
pub struct Button {
    rect: Rect,
    state: ButtonState,
}

impl Button {
    /// New button over `rect`, starting `Idle`.
    pub fn new(rect: Rect) -> Self {
        Self {
            rect,
            state: ButtonState::Idle,
        }
    }

    /// Current visual state (b3 reads this to pick the tint).
    pub fn state(&self) -> ButtonState {
        self.state
    }

    /// Update on-screen rect (scenes recompute layout each frame).
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    /// Drive the state machine with one mouse event. Returns `true` exactly
    /// on the call that completes a click (Up while Pressed, inside rect).
    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> bool {
        let inside = self.rect.contains(Position {
            x: ev.column,
            y: ev.row,
        });

        match ev.kind {
            MouseEventKind::Moved => {
                if inside {
                    self.state = ButtonState::Hover;
                } else if self.state != ButtonState::Pressed {
                    self.state = ButtonState::Idle;
                }
                false
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if inside
                    && (self.state == ButtonState::Idle || self.state == ButtonState::Hover)
                {
                    self.state = ButtonState::Pressed;
                }
                false
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.state == ButtonState::Pressed {
                    if inside {
                        self.state = ButtonState::Hover;
                        return true;
                    } else {
                        self.state = ButtonState::Idle;
                    }
                }
                false
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};

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
        let mut b = Button::new(rect());
        assert_eq!(b.state(), ButtonState::Idle);
        let fired = b.handle_mouse(&ev(MouseEventKind::Moved, INSIDE.0, INSIDE.1));
        assert!(!fired);
        assert_eq!(b.state(), ButtonState::Hover);
    }

    /// `Moved` outside from `Hover` reverts to `Idle`.
    #[test]
    fn moved_outside_from_hover_reverts_to_idle() {
        let mut b = Button::new(rect());
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
        let mut b = Button::new(rect());
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
        let mut b = Button::new(rect());
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
        let mut b = Button::new(rect());
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
        let mut b = Button::new(rect());
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
        let mut b = Button::new(rect());
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
        let mut b = Button::new(rect());
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

    /// `set_rect` updates the hit-test area used by subsequent events.
    #[test]
    fn set_rect_updates_hit_test_area() {
        let mut b = Button::new(Rect::new(0, 0, 1, 1));
        b.set_rect(rect());
        let fired = b.handle_mouse(&ev(MouseEventKind::Moved, INSIDE.0, INSIDE.1));
        assert!(!fired);
        assert_eq!(
            b.state(),
            ButtonState::Hover,
            "handle_mouse must hit-test against the rect set via set_rect"
        );
    }
}
