//! Mouse-driven button state machine plus composed panel+icon render.
//!
//! See `specs/22-braille-ui-chrome.md` lines 15-19 for the mouse transition
//! table and lines 6-14 for the render/tint contract.

use std::ops::{Deref, DerefMut};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Style};
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

/// Per-state colors for a [`Button`]: `background`/`icon` are multiply tints
/// fed to `dots::tint` (same semantics as [`ButtonState::tint_color`]);
/// `label` is the absolute foreground color of the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateColors {
    pub background: Rgba,
    pub icon: Rgba,
    pub label: Rgba,
}

/// Per-state color scheme for a [`Button`]. `Default` must reproduce today's
/// look exactly (spec's lossless-migration guarantee) — see b2-t1 research.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonColors {
    pub idle: StateColors,
    pub hover: StateColors,
    pub pressed: StateColors,
}

impl Default for ButtonColors {
    fn default() -> Self {
        const LABEL: Rgba = Rgba::rgb(0xf0, 0xf0, 0xf0); // today's default label color
        Self {
            idle: StateColors {
                background: ButtonState::Idle.tint_color(),
                icon: ButtonState::Idle.tint_color(),
                label: LABEL,
            },
            hover: StateColors {
                background: ButtonState::Hover.tint_color(),
                icon: ButtonState::Hover.tint_color(),
                label: LABEL,
            },
            pressed: StateColors {
                background: ButtonState::Pressed.tint_color(),
                icon: ButtonState::Pressed.tint_color(),
                label: LABEL,
            },
        }
    }
}

impl ButtonColors {
    /// The [`StateColors`] to use for `state` (b3-t1).
    pub fn for_state(&self, state: ButtonState) -> StateColors {
        match state {
            ButtonState::Idle => self.idle,
            ButtonState::Hover => self.hover,
            ButtonState::Pressed => self.pressed,
        }
    }
}

/// Shared mouse-driven interaction core: owns the hit-test rect and current
/// [`ButtonState`], independent of how a button paints itself. Reused by
/// [`Button`] (panel+icon+label render).
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
/// `FRAME_PANEL`'s native near-white. Applied to `Button`'s background layer,
/// whether a solid panel or a hollow frame ring (its transparent interior is
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

/// Per-state 3-layer composite render, shared by every [`Button`]: stretch-fit
/// `background` to `rect` (pre-tinted gold via `PANEL_GOLD_TINT`, then the
/// per-state `bg_state_tint`), optionally composite an aspect-fit-centered
/// `icon` on top (pre-tinted amber via `ICON_AMBER_TINT`, then the per-state
/// `icon_state_tint`), and blit the result into `buf`. Cells outside `rect`
/// are left untouched; a zero-area or oversized `rect` must not panic.
///
/// Builds TWO composites so the glyph mask stays state-independent while the
/// color layer still recolors per state (b3-t1 blueprint): `mask_composed`
/// composites only the FIXED pre-tints (no state tint) and drives the shape;
/// `color_composed` composites those same layers each additionally tinted by
/// `bg_state_tint`/`icon_state_tint` and drives the color. `dots::tint` never
/// changes which dots are `Lit`, so both composites share identical lit-dot
/// topology — only their colors differ — which is what keeps
/// `dots_to_grid_tinted`'s mask immune to per-state rounding.
fn render_button(
    buf: &mut Buffer,
    rect: Rect,
    background: &'static [u8],
    icon: Option<&'static [u8]>,
    bg_state_tint: Rgba,
    icon_state_tint: Rgba,
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

    let extra_cells = (dot_down.unsigned_abs() as usize).div_ceil(4);
    let target_rows = content_rows + extra_cells * 4;
    let (target_y, down) = if dot_down >= 0 {
        (rect.y, dot_down)
    } else {
        (rect.y.saturating_sub(extra_cells as u16), (extra_cells * 4) as i32 + dot_down)
    };
    let target_rect = Rect::new(rect.x, target_y, rect.width, rect.height + extra_cells as u16);

    let bg_dots_raw = crate::asset_cache::sprite_to_dots(background, dot_cols as u32, content_rows as u32);
    let bg_fixed = crate::dots::tint(&bg_dots_raw, PANEL_GOLD_TINT);
    let bg_colored = crate::dots::tint(&bg_fixed, bg_state_tint);

    let (mask_composed, color_composed) = match icon {
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
            let icon_fixed = crate::dots::tint(&icon_dots_raw, ICON_AMBER_TINT);
            let icon_colored = crate::dots::tint(&icon_fixed, icon_state_tint);

            let icon_dot_x = ((dot_cols.saturating_sub(icon_cols)) / 2) as i32;
            let icon_dot_y = ((content_rows.saturating_sub(icon_rows)) / 2) as i32 + down;

            let mask_placements = [
                crate::composite::DotPlacement {
                    dots: &bg_fixed,
                    dot_x: 0,
                    dot_y: down,
                    depth: 0,
                },
                crate::composite::DotPlacement {
                    dots: &icon_fixed,
                    dot_x: icon_dot_x,
                    dot_y: icon_dot_y,
                    depth: 1,
                },
            ];
            let mask_composed = crate::composite::composite_dots(dot_cols, target_rows, &mask_placements);

            let color_placements = [
                crate::composite::DotPlacement {
                    dots: &bg_colored,
                    dot_x: 0,
                    dot_y: down,
                    depth: 0,
                },
                crate::composite::DotPlacement {
                    dots: &icon_colored,
                    dot_x: icon_dot_x,
                    dot_y: icon_dot_y,
                    depth: 1,
                },
            ];
            let color_composed = crate::composite::composite_dots(dot_cols, target_rows, &color_placements);

            (mask_composed, color_composed)
        }
        None => {
            let mask_placement = [crate::composite::DotPlacement {
                dots: &bg_fixed,
                dot_x: 0,
                dot_y: down,
                depth: 0,
            }];
            let mask_composed = crate::composite::composite_dots(dot_cols, target_rows, &mask_placement);

            let color_placement = [crate::composite::DotPlacement {
                dots: &bg_colored,
                dot_x: 0,
                dot_y: down,
                depth: 0,
            }];
            let color_composed = crate::composite::composite_dots(dot_cols, target_rows, &color_placement);

            (mask_composed, color_composed)
        }
    };

    let grid = crate::dots::dots_to_grid_tinted(&mask_composed, &color_composed);
    crate::grid::draw_grid(buf, target_rect, &grid);
}

/// A clickable, hoverable on-screen button. Owns its interaction [`ButtonCore`]
/// (accessed via `Deref`/`DerefMut` — `state()`/`set_rect()`/`handle_mouse()`
/// all resolve there) and its decoded panel/icon images for rendering;
/// mutated by feeding it mouse events.
pub struct Button {
    core: ButtonCore,
    background: &'static [u8],
    icon: Option<&'static [u8]>,
    label: Option<String>,
    colors: ButtonColors,
    /// Sub-cell render nudge, in braille dots. `0` (the default) renders the
    /// button flush in its rect; a positive value shifts the composed
    /// background+icon DOWN by that many dots, a negative value shifts it UP —
    /// finer than a whole cell either way — spilling into the cell-row below
    /// or above the rect respectively. Positioning-only: the hit-test rect
    /// (`core.rect()`) is unaffected.
    dot_down: i32,
}

impl Button {
    /// New button over `rect`, starting `Idle`, with no icon/label and the
    /// default (lossless, spec-pinned) color scheme. `background` is
    /// caller-supplied `'static` raster bytes (e.g.
    /// `game::assets::BUTTON_PANEL` / `game::assets::FRAME_PANEL`) —
    /// `engine-render` no longer owns or bundles any asset bytes itself. The
    /// bytes are stored as-is (no eager decode): rasterization happens
    /// lazily on first render, through the shared process-lifetime
    /// `asset_cache`, so repeated/duplicate `Button`s built from the same
    /// bytes share one rasterization instead of each decoding independently.
    /// Chain `.icon()`, `.label()`, `.colors()`, `.dot_offset_down()` to
    /// configure further (b3-t1).
    pub fn new(rect: Rect, background: &'static [u8]) -> Self {
        Self {
            core: ButtonCore::new(rect),
            background,
            icon: None,
            label: None,
            colors: ButtonColors::default(),
            dot_down: 0,
        }
    }

    /// Composite `bytes` as an aspect-fit, centered icon layer over the
    /// background (b3-t1).
    pub fn icon(mut self, bytes: &'static [u8]) -> Self {
        self.icon = Some(bytes);
        self
    }

    /// Draw `text` as a centered label, absolute-colored per state, after
    /// the background/icon composite (b3-t1).
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// Override the default per-state color scheme (b3-t1).
    pub fn colors(mut self, colors: ButtonColors) -> Self {
        self.colors = colors;
        self
    }

    /// Builder form of [`Self::set_dot_offset_down`] (b3-t1).
    pub fn dot_offset_down(mut self, dots: i32) -> Self {
        self.dot_down = dots;
        self
    }

    /// Set the sub-cell render nudge (in braille dots) — positive down,
    /// negative up. See `dot_down`. Positioning-only — never touches the
    /// hit-test rect. Kept as a `&mut` setter (alongside the `.dot_offset_down`
    /// builder) because callers recompute this every frame from a live
    /// sub-cell remainder (roster's per-frame nudge), which a `self`-consuming
    /// builder can't serve.
    pub fn set_dot_offset_down(&mut self, dots: i32) {
        self.dot_down = dots;
    }

    /// Paint the composed, per-state-tinted background layer (always),
    /// optional icon layer (iff `.icon()` was set), and optional label (iff
    /// `.label()` was set) onto `self.rect` in `buf`. Driving
    /// `ButtonCore`'s state Idle->Hover->Pressed recolors all three layers
    /// together, each to that state's `ButtonColors` value (b3-t1
    /// blueprint). Cells outside `self.rect` are left untouched; a
    /// zero-area or oversized `self.rect` must not panic.
    pub fn render(&self, buf: &mut Buffer) {
        let rect = self.core.rect();
        let sc = self.colors.for_state(self.core.state());
        render_button(
            buf,
            rect,
            self.background,
            self.icon,
            sc.background,
            sc.icon,
            self.dot_down,
        );
        if let Some(text) = &self.label {
            let style = Style::default().fg(Color::Rgb(sc.label.r, sc.label.g, sc.label.b));
            crate::label(buf, rect, text, crate::TextAlign::Center, style);
        }
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

// Test modules split from a single over-budget `button_tests.rs` (b1-t1)
// into concern-partitioned sibling files, sharing `button_test_fixtures`.
#[cfg(test)]
#[path = "button_test_fixtures.rs"]
mod button_test_fixtures;

#[cfg(test)]
#[path = "button_state_tests.rs"]
mod button_state_tests;

#[cfg(test)]
#[path = "button_icon_contrast_tests.rs"]
mod button_icon_contrast_tests;

#[cfg(test)]
#[path = "button_frame_tests.rs"]
mod button_frame_tests;

#[cfg(test)]
#[path = "button_glyph_mask_tests.rs"]
mod button_glyph_mask_tests;

#[cfg(test)]
#[path = "button_unified_tests.rs"]
mod button_unified_tests;
