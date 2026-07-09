//! Mouse-driven button state machine plus composed panel+icon render.
//!
//! See `specs/22-braille-ui-chrome.md` lines 15-19 for the mouse transition
//! table and lines 6-14 for the render/tint contract.

use std::ops::{Deref, DerefMut};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use engine_core::color::Rgba;

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
    background: &'static [u8],
    icon: Option<&'static [u8]>,
    tint_color: Rgba,
    dot_down: i32,
) {
    let dot_cols = rect.width as usize * 2;
    let content_rows = rect.height as usize * 4;
    if dot_cols == 0 || content_rows == 0 {
        return;
    }

    // Test-only: `button.rs`'s own render tests run in the same lib-test
    // binary as `asset_cache.rs`'s counter-delta tests and now route through
    // that same shared, process-global cache -- serialize against them so an
    // unrelated test's counter-sampling window is never polluted by a render
    // happening concurrently on another thread. See `cache_test_lock`'s doc
    // comment. No-op / not compiled outside test builds.
    #[cfg(test)]
    let _cache_test_guard = crate::asset_cache::cache_test_lock();

    // `dot_down` nudges the whole composed button by that many sub-cell
    // braille dots — positive shifts DOWN, negative shifts UP — a genuine
    // offset finer than a whole cell, applied to the composited dots BEFORE
    // `dots_to_grid` (the same "offset the raw dots, then convert" precision
    // technique used for sub-cell sprite/dot placement elsewhere). The render
    // target grows by just enough whole cells to hold the shifted content:
    // extending downward (`target_rect.y == rect.y`) for `dot_down >= 0`, or
    // extending upward (`target_rect.y` moves up, spilling into the cell-row
    // above `rect`) for `dot_down < 0` — since `dots_to_grid` only converts
    // whole cells; the panel/icon keep their natural `content_rows` size
    // (never stretched to the taller target). `dot_down == 0` leaves the
    // target == `rect` and every layer at dot_y 0 — byte-identical to an
    // un-nudged render.
    let extra_cells = (dot_down.unsigned_abs() as usize).div_ceil(4);
    let target_rows = content_rows + extra_cells * 4;
    let (target_y, down) = if dot_down >= 0 {
        (rect.y, dot_down)
    } else {
        (rect.y.saturating_sub(extra_cells as u16), (extra_cells * 4) as i32 + dot_down)
    };
    let target_rect = Rect::new(rect.x, target_y, rect.width, rect.height + extra_cells as u16);

    let bg_dots_raw = crate::asset_cache::sprite_to_dots(background, dot_cols as u32, content_rows as u32);
    let bg_dots = crate::dots::tint(&bg_dots_raw, PANEL_GOLD_TINT);

    let composed = match icon {
        Some(icon_bytes) => {
            // Icon is aspect-fit + centered (not stretched) — reuse
            // `convert`'s fit formula to get the icon's fitted dot dims
            // without re-deriving it (and without rasterizing twice: the
            // decode-cache `Arc` clone is cheap on a hit).
            let icon_img = crate::asset_cache::decoded(icon_bytes);
            let (fit_cols, fit_rows) = crate::convert::fit_dot_dims(&icon_img, rect);
            let icon_cols = (fit_cols * 2) as usize;
            let icon_rows = (fit_rows * 4) as usize;
            let icon_dots_raw =
                crate::asset_cache::sprite_to_dots(icon_bytes, icon_cols as u32, icon_rows as u32);
            let icon_dots = crate::dots::tint(&icon_dots_raw, ICON_AMBER_TINT);

            let placements = [
                crate::composite::DotPlacement {
                    dots: &bg_dots,
                    dot_x: 0,
                    dot_y: down,
                    depth: 0,
                },
                crate::composite::DotPlacement {
                    dots: &icon_dots,
                    dot_x: ((dot_cols.saturating_sub(icon_cols)) / 2) as i32,
                    dot_y: ((content_rows.saturating_sub(icon_rows)) / 2) as i32 + down,
                    depth: 1,
                },
            ];
            crate::composite::composite_dots(dot_cols, target_rows, &placements)
        }
        None => {
            let placement = [crate::composite::DotPlacement {
                dots: &bg_dots,
                dot_x: 0,
                dot_y: down,
                depth: 0,
            }];
            crate::composite::composite_dots(dot_cols, target_rows, &placement)
        }
    };

    let tinted = crate::dots::tint(&composed, tint_color);
    let grid = crate::dots::dots_to_grid_tinted(&composed, &tinted);
    crate::grid::draw_grid(buf, target_rect, &grid);
}

/// A clickable, hoverable on-screen button. Owns its interaction [`ButtonCore`]
/// (accessed via `Deref`/`DerefMut` — `state()`/`set_rect()`/`handle_mouse()`
/// all resolve there) and its decoded panel/icon images for rendering;
/// mutated by feeding it mouse events.
pub struct Button {
    core: ButtonCore,
    panel: &'static [u8],
    icon: &'static [u8],
    /// Sub-cell render nudge, in braille dots. `0` (the default) renders the
    /// button flush in its rect; a positive value shifts the composed
    /// panel+icon DOWN by that many dots, a negative value shifts it UP —
    /// finer than a whole cell either way — spilling into the cell-row below
    /// or above the rect respectively. Positioning-only: the hit-test rect
    /// (`core.rect()`) is unaffected.
    dot_down: i32,
}

impl Button {
    /// New button over `rect`, starting `Idle`. `panel` and `icon` are
    /// caller-supplied `'static` raster bytes (e.g. `game::assets::BUTTON_PANEL`
    /// / `game::assets::ICON_HOME`) composited together at render time —
    /// `engine-render` no longer owns or bundles any asset bytes itself.
    /// The bytes are stored as-is (no eager decode): rasterization happens
    /// lazily on first render, through the shared process-lifetime
    /// `asset_cache`, so repeated/duplicate `Button`s built from the same
    /// bytes share one rasterization instead of each decoding independently.
    pub fn new(rect: Rect, panel: &'static [u8], icon: &'static [u8]) -> Self {
        Self {
            core: ButtonCore::new(rect),
            panel,
            icon,
            dot_down: 0,
        }
    }

    /// Set the sub-cell render nudge (in braille dots) — positive down,
    /// negative up. See `dot_down`. Positioning-only — never touches the
    /// hit-test rect.
    pub fn set_dot_offset_down(&mut self, dots: i32) {
        self.dot_down = dots;
    }

    /// Paint the composed, state-tinted panel+icon onto `self.rect` in
    /// `buf`, via the existing dot pipeline (`sprite_to_dots` →
    /// `composite_dots` → `tint` → `dots_to_grid_tinted` → `draw_grid`; see
    /// research.md's blueprint for b3-t2). Cells outside `self.rect` are
    /// left untouched; a zero-area or oversized `self.rect` must not panic.
    /// A non-zero `dot_down` shifts the render into the cell-row directly
    /// below (positive) or above (negative) `self.rect`, so those cells may
    /// also be painted.
    pub fn render(&self, buf: &mut Buffer) {
        render_tinted(
            buf,
            self.core.rect(),
            self.panel,
            Some(self.icon),
            self.core.state().tint_color(),
            self.dot_down,
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
    frame: &'static [u8],
    label: String,
}

impl FrameButton {
    /// New frame button over `rect`, starting `Idle`, labeled with `label`.
    /// `frame` is caller-supplied `'static` raster bytes (e.g.
    /// `game::assets::FRAME_PANEL`) — `engine-render` no longer owns or
    /// bundles any asset bytes itself. The bytes are stored as-is (no eager
    /// decode); rasterization is lazy, shared via the process-lifetime
    /// `asset_cache` on first render.
    pub fn new(rect: Rect, frame: &'static [u8], label: impl Into<String>) -> Self {
        Self {
            core: ButtonCore::new(rect),
            frame,
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
        render_tinted(buf, rect, self.frame, None, self.core.state().tint_color(), 0);
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
#[path = "button_tests.rs"]
mod button_tests;
