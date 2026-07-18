use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::Rect;
use engine_core::Inspectable;
use engine_core::SceneKey;
use serde_json::Value as JsonValue;

use engine_render::{
    flex, ActiveStyle, Align, Basis, Button, ButtonState, Direction, FlexChild, FlexStyle,
    Justify,
};

use engine_audio::play_oneshot;
use engine_core::scene::{EngineCtx, InputEvent, Scene, Transition};
use crate::scene_id::SceneId;

#[derive(Inspectable)]
pub struct MainHub {
    /// The 4 menu buttons (index 0 Roster, 1 Battle, 2 Settings, 3 Exit —
    /// matches `button_rects`' order). `RefCell` because `render(&self, ..)`
    /// must mutate each button's rect/state through an immutable receiver —
    /// mirrors `RosterManager`'s button fields.
    #[inspect(hidden)]
    buttons: [RefCell<Button>; 4],

    /// Index (0..=3) of the menu item the selection cursor points at.
    /// Selection state only — has no visual effect until b4-t2 recolors the
    /// active button.
    cursor_index: usize,

    /// Set true when the Exit menu item activates; polled by the engine via
    /// `Scene::quit_requested` (b4-t1's poll path). Transient engine signal,
    /// not editable state.
    #[inspect(hidden)]
    quit_requested: bool,

    /// Accumulated animation clock for the procedural title logo
    /// (`title_logo::frame`), advanced by `update(dt)` and read by
    /// `render()`. Transient engine state, not editable.
    #[inspect(hidden)]
    elapsed: f32,
}

impl Default for MainHub {
    fn default() -> Self {
        Self {
            buttons: [
                RefCell::new(Button::new(Rect::default(), crate::assets::FRAME_PANEL).label("Roster")),
                RefCell::new(Button::new(Rect::default(), crate::assets::FRAME_PANEL).label("Battle")),
                RefCell::new(Button::new(Rect::default(), crate::assets::FRAME_PANEL).label("Settings")),
                RefCell::new(Button::new(Rect::default(), crate::assets::FRAME_PANEL).label("Exit")),
            ],
            cursor_index: 0,
            quit_requested: false,
            elapsed: 0.0,
        }
    }
}

impl MainHub {
    /// First-run fade window (Decision 6) for the nav buttons: begins after
    /// `title_logo::SWORD_DROP` seats (`PREROLL + 0.18`) and ramps 0..1 across
    /// `[BTN_FADE.start, BTN_FADE.end)`. Offset by `title_logo::PREROLL` so it
    /// tracks the pre-roll delay. Drives both the buttons' alpha (`render`) and
    /// their input gate (`handle_input`/`buttons_interactive`).
    const BTN_FADE: crate::scenes::title_logo::Beat = crate::scenes::title_logo::Beat::new(
        crate::scenes::title_logo::PREROLL + 0.30,
        crate::scenes::title_logo::PREROLL + 0.62,
    );

    /// One menu button's size and the vertical gap between stacked buttons.
    const BUTTON_W: u16 = 20;
    const BUTTON_H: u16 = 3;
    const MENU_GAP: u16 = 1;

    /// Menu container size — width is a single button's width; height MUST
    /// equal the stacked group's total height (4 buttons + 3 gaps) so
    /// `flex` fills the container exactly rather than leaving slack.
    const MENU_W: u16 = Self::BUTTON_W;
    const MENU_H: u16 = 4 * Self::BUTTON_H + 3 * Self::MENU_GAP;

    /// Gap kept clear between the bottom of the menu (Exit) and the very
    /// bottom edge of the screen. (4 cells: the menu sits 2 cells higher than
    /// the former margin of 2.)
    const MENU_BOTTOM_MARGIN: u16 = 4;

    /// The procedural logo's own cell dims — `div_ceil` of
    /// `title_logo::compute_layout()`'s dot canvas (188×94 dots), matching
    /// exactly what `dots_to_grid_tinted` produces (94×24 cells). NOT a
    /// floored `/2, /4` (that would clip the bottom dot-row — CLAUDE.md #5).
    fn logo_cell_size() -> (u16, u16) {
        let l = crate::scenes::title_logo::compute_layout();
        ((l.canvas_w as usize).div_ceil(2) as u16, (l.canvas_h as usize).div_ceil(4) as u16)
    }

    /// Title box size — FIXED: the logo's own cell dims plus a 1-cell
    /// border each side. `area` is accepted for call-site symmetry with the
    /// rest of the layout functions but is otherwise unused — no aspect-fit,
    /// no dependence on the render area — the procedural logo is a fixed
    /// SCALE=4 composition (Decision 3), unlike the old PNG's
    /// aspect-preserving fit.
    fn title_size(_area: Rect) -> (u16, u16) {
        let (c, r) = Self::logo_cell_size();
        (c + 2, r + 2)
    }

    /// Title box rect for `area` — sole place its position/size is
    /// computed; `render()` and tests both call this.
    fn title_rect(area: Rect) -> Rect {
        let (w, h) = Self::title_size(area);
        let child = FlexChild {
            basis: Basis::Intrinsic(Box::new(move |_main| (w as i32 * 2, h as i32 * 4))),
            grow: 0.0,
            shrink: 0.0,
        };
        let style = FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::Center,
            align_items: Align::Start,
            gap: 0,
        };
        // No top margin: the title box sits flush at the top of `area` (moved
        // up 1 cell from the former 1-cell / 4-dot top inset).
        let container = Self::cell_rect_to_dots(area);
        flex(container, style, std::slice::from_ref(&child))[0].to_cell_rect()
    }

    /// Converts a whole-cell `Rect` into dot space (2 dots wide, 4 dots tall
    /// per cell) — the sole cell->dot boundary `title_rect`/`menu_container`/
    /// `button_rects` use on their way into `flex()` (b2-t1/b3-t1/b3-t2).
    fn cell_rect_to_dots(r: Rect) -> engine_render::DotRect {
        engine_render::DotRect {
            x: r.x as i32 * 2,
            y: r.y as i32 * 4,
            w: r.width as i32 * 2,
            h: r.height as i32 * 4,
        }
    }

    /// Menu group container rect for `area` — anchored near the BOTTOM of
    /// the screen (not dead-center) so Exit sits close to the bottom edge,
    /// leaving the open space above for the much bigger title box. Sole
    /// place its position is computed; feeds `button_rects` via `flex`.
    ///
    /// Insets the dot-space `area` by `MENU_BOTTOM_MARGIN*4` dots on the
    /// bottom edge, then runs a single-child Column flex (`Justify::End`
    /// pins the group's bottom edge at the inset container's bottom edge,
    /// `Align::Center` centers it horizontally) to place the group at the
    /// bottom-center of the container.
    fn menu_container(area: Rect) -> Rect {
        let container =
            Self::cell_rect_to_dots(area).inset(0, 0, 0, Self::MENU_BOTTOM_MARGIN as i32 * 4);
        let child = FlexChild {
            // Column: closure returns (main=Y/height, cross=X/width) in dots.
            basis: Basis::Intrinsic(Box::new(|_main| {
                (Self::MENU_H as i32 * 4, Self::MENU_W as i32 * 2)
            })),
            grow: 0.0,
            shrink: 0.0,
        };
        let style = FlexStyle {
            direction: Direction::Column,
            justify_content: Justify::End,
            align_items: Align::Center,
            gap: 0,
        };
        flex(container, style, std::slice::from_ref(&child))[0].to_cell_rect()
    }

    /// The 4 menu-button rects for `area`, top-to-bottom (index 0 Roster, 1
    /// Battle, 2 Settings, 3 Exit — labels/roles assigned by b5-t3, this
    /// fixes geometry and order only).
    fn button_rects(area: Rect) -> [Rect; 4] {
        let container = Self::cell_rect_to_dots(Self::menu_container(area));
        let child = || FlexChild {
            // Column: closure returns (main=Y/height, cross=X/width) in dots.
            basis: Basis::Intrinsic(Box::new(|_main| {
                (Self::BUTTON_H as i32 * 4, Self::BUTTON_W as i32 * 2)
            })),
            grow: 0.0,
            shrink: 0.0,
        };
        let children = [child(), child(), child(), child()];
        let style = FlexStyle {
            direction: Direction::Column,
            justify_content: Justify::Start,
            align_items: Align::Start,
            gap: Self::MENU_GAP as i32 * 4,
        };
        let rects = flex(container, style, &children);
        [
            rects[0].to_cell_rect(),
            rects[1].to_cell_rect(),
            rects[2].to_cell_rect(),
            rects[3].to_cell_rect(),
        ]
    }

    /// Sole activation dispatch for a menu index (0 Roster, 1 Battle,
    /// 2 Settings, 3 Exit). Keyboard Enter (b5-t5) and mouse click (b5-t6)
    /// both route here — never a duplicated match. Index is 0..=3 by
    /// construction; other values are inert.
    fn activate(&mut self, index: usize) -> Option<Transition> {
        match index {
            0 => Some(Transition { target: SceneId::RosterManager.into(), params: None }),
            1 => Some(Transition { target: SceneId::BattleViewer.into(), params: None }),
            2 => Some(Transition { target: SceneId::Settings.into(), params: None }),
            3 => {
                self.quit_requested = true;
                None
            }
            _ => None,
        }
    }

    /// Whether the nav buttons are past the first-run fade and accept input
    /// (Decision 6) — single guard `handle_input` checks before dispatch.
    fn buttons_interactive(&self) -> bool {
        self.elapsed >= Self::BTN_FADE.end
    }
}

/// Process-scoped intro-played latch (b1-t2). Set on the first `MainHub`
/// `enter` of the process; subsequent entries hold the intro still. Replaces
/// the previous persisted `first_run` flag-file mechanism.
static INTRO_PLAYED: AtomicBool = AtomicBool::new(false);

/// Resets the process-scoped intro-played latch (b1-t2) so a gate test can
/// deterministically observe "first entry of the process" regardless of
/// execution order.
#[cfg(test)]
pub(crate) fn reset_intro_played_for_test() {
    INTRO_PLAYED.store(false, Ordering::Relaxed);
}

impl Scene for MainHub {
    fn id(&self) -> SceneKey {
        SceneId::MainHub.into()
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {
        let already_played = INTRO_PLAYED.swap(true, Ordering::Relaxed);
        self.elapsed = if already_played {
            crate::scenes::title_logo::ANIM_END
        } else {
            0.0
        };
    }

    fn update(&mut self, _ctx: &mut EngineCtx, dt: Duration) -> Option<Transition> {
        self.elapsed += dt.as_secs_f32();
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let buf = frame.buffer_mut();
        let title = Self::title_rect(area);
        let (grid, _dot_rect) = crate::scenes::title_logo::frame(self.elapsed);
        engine_render::draw_grid(buf, title, &grid);

        let fade = Self::BTN_FADE.progress(self.elapsed);
        let alpha = (fade * 255.0).round() as u8;

        let rects = Self::button_rects(area);
        for (i, (button, rect)) in self.buttons.iter().zip(rects).enumerate() {
            let mut b = button.borrow_mut();
            b.set_rect(rect);
            let base = if i == self.cursor_index {
                crate::scenes::title_logo::WHITE_COLOR
            } else {
                crate::scenes::title_logo::GLOW_COLOR
            };
            let color = engine_core::color::Rgba { a: alpha, ..base };
            b.set_active_style(Some(ActiveStyle { border: color, label: color }));
            b.render(buf);
        }
    }

    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
        use crossterm::event::{KeyCode, MouseEventKind};
        if !self.buttons_interactive() {
            return None;
        }
        match ev {
            InputEvent::Key(key) => match key.code {
                KeyCode::Up => {
                    play_oneshot(crate::sounds::UI_CONFIRM);
                    let n = self.buttons.len();
                    self.cursor_index = (self.cursor_index + n - 1) % n;
                }
                KeyCode::Down => {
                    play_oneshot(crate::sounds::UI_CONFIRM);
                    let n = self.buttons.len();
                    self.cursor_index = (self.cursor_index + 1) % n;
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    play_oneshot(crate::sounds::UI_CONFIRM);
                    return self.activate(self.cursor_index);
                }
                _ => {}
            },
            InputEvent::Mouse(me) => {
                let mut clicked: Option<usize> = None;
                let mut hovered: Option<usize> = None;
                for (i, button) in self.buttons.iter_mut().enumerate() {
                    let b = button.get_mut();
                    if b.handle_mouse(&me) {
                        clicked = Some(i);
                    }
                    if b.state() == ButtonState::Hover {
                        hovered = Some(i);
                    }
                }
                if let Some(i) = clicked {
                    return self.activate(i);
                }
                if me.kind == MouseEventKind::Moved {
                    if let Some(i) = hovered {
                        self.cursor_index = i;
                    }
                }
            }
        }
        None
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {}

    fn inspect(&mut self) -> &mut dyn engine_core::Inspectable {
        self
    }

    fn quit_requested(&self) -> bool {
        self.quit_requested
    }
}

#[cfg(test)]
#[path = "main_hub_tests.rs"]
mod main_hub_tests;

#[cfg(test)]
#[path = "main_hub_fade_tests.rs"]
mod main_hub_fade_tests;
