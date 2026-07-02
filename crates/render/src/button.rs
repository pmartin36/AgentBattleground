//! Mouse-driven button state machine plus composed panel+icon render.
//!
//! See `specs/22-braille-ui-chrome.md` lines 15-19 for the mouse transition
//! table and lines 6-14 for the render/tint contract.

use image::DynamicImage;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use scene_core::color::Rgba;

use crate::assets;

/// Visual/interaction state of a [`Button`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonState {
    #[default]
    Idle,
    Hover,
    Pressed,
}

impl ButtonState {
    /// Multiply-blend tint color for this state, fed unmodified into
    /// `dots::tint` (spec 22 lines 12-14).
    pub const fn tint_color(self) -> Rgba {
        match self {
            ButtonState::Idle => Rgba::rgb(0xc8, 0xc8, 0xc8),
            ButtonState::Hover => Rgba::rgb(0xff, 0xff, 0xff),
            ButtonState::Pressed => Rgba::rgb(0x8c, 0x8c, 0x8c),
        }
    }
}

/// A clickable, hoverable on-screen button. Owns its hit-test rect, current
/// [`ButtonState`], and its decoded panel/icon images for rendering; mutated
/// by feeding it mouse events.
pub struct Button {
    rect: Rect,
    state: ButtonState,
    panel: DynamicImage,
    icon: DynamicImage,
}

impl Button {
    /// New button over `rect`, starting `Idle`. `icon` is the bundled icon
    /// asset bytes (e.g. `assets::ICON_HOME`) composited on top of the
    /// shared `assets::BUTTON_PANEL` background at render time.
    pub fn new(rect: Rect, icon: &[u8]) -> Self {
        Self {
            rect,
            state: ButtonState::Idle,
            panel: image::load_from_memory(assets::BUTTON_PANEL)
                .expect("BUTTON_PANEL must decode — bundled first-party asset"),
            icon: image::load_from_memory(icon)
                .expect("icon bytes must decode — bundled first-party asset"),
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

    /// Paint the composed, state-tinted panel+icon onto `self.rect` in
    /// `buf`, via the existing dot pipeline (`sprite_to_dots` →
    /// `composite_dots` → `tint` → `dots_to_grid` → `draw_grid`; see
    /// research.md's blueprint for b3-t2). Cells outside `self.rect` are
    /// left untouched; a zero-area or oversized `self.rect` must not panic.
    pub fn render(&self, buf: &mut Buffer) {
        let rect = self.rect;
        let dot_cols = rect.width as usize * 2;
        let dot_rows = rect.height as usize * 4;
        if dot_cols == 0 || dot_rows == 0 {
            return;
        }

        // Panel stretches to fill the whole button rect.
        let panel = crate::dots::sprite_to_dots(&self.panel, dot_cols as u32, dot_rows as u32);

        // Icon is aspect-fit + centered (not stretched) — reuse `convert`'s
        // fit formula to get the icon's fitted dot dims without re-deriving it.
        let fitted = crate::convert::convert(&self.icon, rect);
        let icon_cols = fitted.cols() * 2;
        let icon_rows = fitted.rows() * 4;
        let icon = crate::dots::sprite_to_dots(&self.icon, icon_cols as u32, icon_rows as u32);

        let placements = [
            crate::composite::DotPlacement {
                dots: &panel,
                dot_x: 0,
                dot_y: 0,
                depth: 0,
            },
            crate::composite::DotPlacement {
                dots: &icon,
                dot_x: ((dot_cols.saturating_sub(icon_cols)) / 2) as i32,
                dot_y: ((dot_rows.saturating_sub(icon_rows)) / 2) as i32,
                depth: 1,
            },
        ];
        let composed = crate::composite::composite_dots(dot_cols, dot_rows, &placements);
        let tinted = crate::dots::tint(&composed, self.state.tint_color());
        let grid = crate::dots::dots_to_grid(&tinted);
        crate::grid::draw_grid(buf, rect, &grid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let mut b = Button::new(rect(), assets::ICON_HOME);
        assert_eq!(b.state(), ButtonState::Idle);
        let fired = b.handle_mouse(&ev(MouseEventKind::Moved, INSIDE.0, INSIDE.1));
        assert!(!fired);
        assert_eq!(b.state(), ButtonState::Hover);
    }

    /// `Moved` outside from `Hover` reverts to `Idle`.
    #[test]
    fn moved_outside_from_hover_reverts_to_idle() {
        let mut b = Button::new(rect(), assets::ICON_HOME);
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
        let mut b = Button::new(rect(), assets::ICON_HOME);
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
        let mut b = Button::new(rect(), assets::ICON_HOME);
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
        let mut b = Button::new(rect(), assets::ICON_HOME);
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
        let mut b = Button::new(rect(), assets::ICON_HOME);
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
        let mut b = Button::new(rect(), assets::ICON_HOME);
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
        let mut b = Button::new(rect(), assets::ICON_HOME);
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

    /// `set_rect` updates the hit-test area used by subsequent events.
    #[test]
    fn set_rect_updates_hit_test_area() {
        let mut b = Button::new(Rect::new(0, 0, 1, 1), assets::ICON_HOME);
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
        let mut b = Button::new(rect, assets::ICON_HOME);
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
        let b = Button::new(rect, assets::ICON_HOME);
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

        let zero = Button::new(Rect::new(0, 0, 0, 0), assets::ICON_HOME);
        let mut buf_zero = make_buf(4, 4);
        zero.render(&mut buf_zero); // must not panic

        let oversized = Button::new(Rect::new(0, 0, 50, 50), assets::ICON_HOME);
        let mut buf_small = make_buf(5, 5);
        oversized.render(&mut buf_small); // must not panic
    }
}
