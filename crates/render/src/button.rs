//! Mouse-driven button state machine plus composed panel+icon render.
//!
//! See `specs/22-braille-ui-chrome.md` lines 15-19 for the mouse transition
//! table and lines 6-14 for the render/tint contract.

use std::ops::{Deref, DerefMut};

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

/// Shared mouse-driven interaction core: owns the hit-test rect and current
/// [`ButtonState`], independent of how a button paints itself. Reused by
/// [`Button`] (panel+icon render) and `FrameButton` (bordered frame + text
/// label render).
pub struct ButtonCore {
    rect: Rect,
    state: ButtonState,
}

impl ButtonCore {
    /// New core over `rect`, starting `Idle`.
    pub fn new(rect: Rect) -> Self {
        Self {
            rect,
            state: ButtonState::Idle,
        }
    }

    /// Current visual state.
    pub fn state(&self) -> ButtonState {
        self.state
    }

    /// Current hit-test rect.
    pub fn rect(&self) -> Rect {
        self.rect
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

/// Multiply-tint applied to the background/panel layer before compositing,
/// giving every button a warm gold body instead of `BUTTON_PANEL`/
/// `FRAME_PANEL`'s native near-white. Applied to both `Button`'s solid panel
/// and `FrameButton`'s hollow frame ring (its transparent interior is
/// unaffected — alpha-zero pixels stay alpha-zero regardless of tint).
const PANEL_GOLD_TINT: Rgba = Rgba::rgb(0xc9, 0xa0, 0x3c);

/// Multiply-tint applied to an icon layer before compositing it over the
/// (now gold) panel, so the icon reads as a distinct shape instead of
/// disappearing into it, AND carries real color instead of grayscale.
/// `BUTTON_PANEL` and the bundled icons are pure opaque white (confirmed by
/// sampling both directly) — with zero brightness difference, the braille
/// rasterizer's per-cell adaptive luma threshold (`dots::cell_from_dots`: a
/// dot's bit is set only if its luma is `>=` the cell's average) can't tell
/// icon pixels from panel pixels within a mixed cell, smearing the icon's
/// edges into the panel instead of showing a clean silhouette. This amber is
/// deliberately darker than `PANEL_GOLD_TINT` (luma ≈85 vs ≈161) to preserve
/// that same contrast relationship, just with real hue on both sides instead
/// of grayscale. The whole composed result is still multiplied by the
/// button's `ButtonState` tint afterward, so both the gold/amber relationship
/// and the icon/panel contrast hold across Idle/Hover/Pressed.
const ICON_AMBER_TINT: Rgba = Rgba::rgb(0x8a, 0x4a, 0x00);

/// Shared render sequence for a [`ButtonCore`]-backed widget: stretch-fit
/// `background` to `rect` (pre-tinted gold via `PANEL_GOLD_TINT`), optionally
/// composite an aspect-fit-centered `icon` on top (depth 1 over the
/// background's depth 0, pre-tinted amber via `ICON_AMBER_TINT` so it doesn't
/// disappear into the gold panel), tint the result by `tint_color`, and blit
/// it into `buf`. Cells outside `rect` are left untouched; a zero-area or
/// oversized `rect` must not panic. This is the sequence [`Button`] and
/// `FrameButton` both used to duplicate — they now differ only in whether
/// they pass an icon and whether they draw a label afterward.
fn render_tinted(
    buf: &mut Buffer,
    rect: Rect,
    background: &DynamicImage,
    icon: Option<&DynamicImage>,
    tint_color: Rgba,
) {
    let dot_cols = rect.width as usize * 2;
    let dot_rows = rect.height as usize * 4;
    if dot_cols == 0 || dot_rows == 0 {
        return;
    }

    let bg_dots_raw = crate::dots::sprite_to_dots(background, dot_cols as u32, dot_rows as u32);
    let bg_dots = crate::dots::tint(&bg_dots_raw, PANEL_GOLD_TINT);

    let composed = match icon {
        Some(icon_img) => {
            // Icon is aspect-fit + centered (not stretched) — reuse
            // `convert`'s fit formula to get the icon's fitted dot dims
            // without re-deriving it.
            let fitted = crate::convert::convert(icon_img, rect);
            let icon_cols = fitted.cols() * 2;
            let icon_rows = fitted.rows() * 4;
            let icon_dots_raw = crate::dots::sprite_to_dots(icon_img, icon_cols as u32, icon_rows as u32);
            let icon_dots = crate::dots::tint(&icon_dots_raw, ICON_AMBER_TINT);

            let placements = [
                crate::composite::DotPlacement {
                    dots: &bg_dots,
                    dot_x: 0,
                    dot_y: 0,
                    depth: 0,
                },
                crate::composite::DotPlacement {
                    dots: &icon_dots,
                    dot_x: ((dot_cols.saturating_sub(icon_cols)) / 2) as i32,
                    dot_y: ((dot_rows.saturating_sub(icon_rows)) / 2) as i32,
                    depth: 1,
                },
            ];
            crate::composite::composite_dots(dot_cols, dot_rows, &placements)
        }
        None => bg_dots,
    };

    let tinted = crate::dots::tint(&composed, tint_color);
    let grid = crate::dots::dots_to_grid(&tinted);
    crate::grid::draw_grid(buf, rect, &grid);
}

/// A clickable, hoverable on-screen button. Owns its interaction [`ButtonCore`]
/// (accessed via `Deref`/`DerefMut` — `state()`/`set_rect()`/`handle_mouse()`
/// all resolve there) and its decoded panel/icon images for rendering;
/// mutated by feeding it mouse events.
pub struct Button {
    core: ButtonCore,
    panel: DynamicImage,
    icon: DynamicImage,
}

impl Button {
    /// New button over `rect`, starting `Idle`. `icon` is the bundled icon
    /// asset bytes (e.g. `assets::ICON_HOME`) composited on top of the
    /// shared `assets::BUTTON_PANEL` background at render time.
    pub fn new(rect: Rect, icon: &[u8]) -> Self {
        Self {
            core: ButtonCore::new(rect),
            panel: image::load_from_memory(assets::BUTTON_PANEL)
                .expect("BUTTON_PANEL must decode — bundled first-party asset"),
            icon: image::load_from_memory(icon)
                .expect("icon bytes must decode — bundled first-party asset"),
        }
    }

    /// Paint the composed, state-tinted panel+icon onto `self.rect` in
    /// `buf`, via the existing dot pipeline (`sprite_to_dots` →
    /// `composite_dots` → `tint` → `dots_to_grid` → `draw_grid`; see
    /// research.md's blueprint for b3-t2). Cells outside `self.rect` are
    /// left untouched; a zero-area or oversized `self.rect` must not panic.
    pub fn render(&self, buf: &mut Buffer) {
        render_tinted(
            buf,
            self.core.rect(),
            &self.panel,
            Some(&self.icon),
            self.core.state().tint_color(),
        );
    }
}

impl Deref for Button {
    type Target = ButtonCore;
    fn deref(&self) -> &ButtonCore {
        &self.core
    }
}

impl DerefMut for Button {
    fn deref_mut(&mut self) -> &mut ButtonCore {
        &mut self.core
    }
}

/// Bordered hollow frame + centered text label — the second consumer of
/// [`ButtonCore`] (spec 25 line 28), accessed via `Deref`/`DerefMut` the same
/// way [`Button`] is.
pub struct FrameButton {
    core: ButtonCore,
    frame: DynamicImage,
    label: String,
}

impl FrameButton {
    /// New frame button over `rect`, starting `Idle`, labeled with `label`.
    pub fn new(rect: Rect, label: impl Into<String>) -> Self {
        Self {
            core: ButtonCore::new(rect),
            frame: image::load_from_memory(assets::FRAME_PANEL)
                .expect("FRAME_PANEL must decode — bundled first-party asset"),
            label: label.into(),
        }
    }

    /// Near-white. `FRAME_PANEL` is a HOLLOW frame — only its border ring is
    /// opaque/tinted by `ButtonState`; the interior (where the label sits)
    /// is alpha-transparent, showing whatever's actually behind the button
    /// in the scene (a dark background for every current caller), not the
    /// panel's own tint. A light label color is what reads there — a dark
    /// one (this constant's original, wrong, value) was invisible against
    /// that dark backdrop, confirmed by rendering a real buffer and looking
    /// at it.
    const LABEL_COLOR: Rgba = Rgba::rgb(0xf0, 0xf0, 0xf0);

    /// Paint the state-tinted bordered frame plus centered label onto
    /// `self.rect` in `buf`.
    pub fn render(&self, buf: &mut Buffer) {
        let rect = self.core.rect();
        render_tinted(buf, rect, &self.frame, None, self.core.state().tint_color());
        crate::label(buf, rect, &self.label, Self::LABEL_COLOR);
    }
}

impl Deref for FrameButton {
    type Target = ButtonCore;
    fn deref(&self) -> &ButtonCore {
        &self.core
    }
}

impl DerefMut for FrameButton {
    fn deref_mut(&mut self) -> &mut ButtonCore {
        &mut self.core
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

/// Regression coverage for `PANEL_GOLD_TINT`/`ICON_AMBER_TINT`: `BUTTON_PANEL`
/// and every bundled icon are pure opaque white, so without pre-tinting both
/// layers, an icon's rendered color would be indistinguishable from the
/// panel's (the bug that made the arrow icon unreadable once composited) and
/// everything would stay grayscale (the follow-up complaint that the result
/// was "boring, no color variation"). Separate module so `mod tests` above
/// stays untouched (its own byte-for-byte constraint from the b1-t1
/// refactor).
#[cfg(test)]
mod icon_contrast_tests {
    use super::*;
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
        let b = Button::new(rect, assets::ICON_ARROW_LEFT);
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
        let b = Button::new(rect, assets::ICON_ARROW_LEFT);
        let mut buf = make_buf(8, 4);
        b.render(&mut buf);

        let is_grayscale = |c: Color| matches!(c, Color::Rgb(r, g, b) if r == g && g == b);

        let center = buf.cell((4, 2)).expect("center cell must exist").fg;
        let edge = buf.cell((0, 0)).expect("edge cell must exist").fg;

        assert!(!is_grayscale(center), "icon color {center:?} must have real hue, not grayscale");
        assert!(!is_grayscale(edge), "panel color {edge:?} must have real hue, not grayscale");
    }
}

/// b3-t1: `FrameButton` tests. Separate module from `mod tests` above, which
/// must stay byte-for-byte unmodified per b1-t1's refactor constraint.
#[cfg(test)]
mod frame_button_tests {
    use super::*;
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

    /// Renders a fresh `FrameButton` in `state` and returns the painted fg
    /// color of a top-center BORDER cell (not the transparent interior, not
    /// the label's center row).
    fn render_top_border_fg(state: ButtonState) -> Color {
        let rect = frame_rect();
        let mut b = FrameButton::new(rect, "Go");
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
        let b = FrameButton::new(rect, "Go");
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
        let mut b = FrameButton::new(rect, "Go");

        b.handle_mouse(&ev(MouseEventKind::Moved, inside.0, inside.1));
        b.handle_mouse(&ev(MouseEventKind::Down(MouseButton::Left), inside.0, inside.1));
        let fired = b.handle_mouse(&ev(MouseEventKind::Up(MouseButton::Left), inside.0, inside.1));

        assert!(fired, "Up inside while Pressed must report a completed click");
        assert_eq!(b.state(), ButtonState::Hover);
    }
}
