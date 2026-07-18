use std::cell::RefCell;
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::Frame;
use ratatui::layout::Rect;
use engine_core::Inspectable;
use engine_core::SceneKey;
use serde_json::Value as JsonValue;

use engine_render::{
    flex, Align, Basis, Button, ButtonState, Direction, FlexChild, FlexStyle, Justify,
};

use engine_audio::play_oneshot;
use engine_core::scene::{EngineCtx, InputEvent, Scene, Transition};
use crate::scene_id::SceneId;

#[derive(Inspectable)]
pub struct MainHub {
    /// The 3 menu buttons (index 0 Roster, 1 Battle, 2 Exit — matches
    /// `button_rects`' order). `RefCell` because `render(&self, ..)` must
    /// mutate each button's rect/state through an immutable receiver —
    /// mirrors `RosterManager`'s button fields.
    #[inspect(hidden)]
    buttons: [RefCell<Button>; 3],

    /// Index (0..=2) of the menu item the selection cursor points at.
    /// Rendering the cursor arrow itself is b5-t4's deliverable.
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
                RefCell::new(Button::new(Rect::default(), crate::assets::FRAME_PANEL).label("Exit")),
            ],
            cursor_index: 0,
            quit_requested: false,
            elapsed: 0.0,
        }
    }
}

impl MainHub {
    /// One menu button's size and the vertical gap between stacked buttons.
    const BUTTON_W: u16 = 20;
    const BUTTON_H: u16 = 3;
    const MENU_GAP: u16 = 1;

    /// Menu container size — width is a single button's width; height MUST
    /// equal the stacked group's total height (3 buttons + 2 gaps) so
    /// `flex` fills the container exactly rather than leaving slack.
    const MENU_W: u16 = Self::BUTTON_W;
    const MENU_H: u16 = 3 * Self::BUTTON_H + 2 * Self::MENU_GAP;

    /// Gap kept clear between the bottom of the menu (Exit) and the very
    /// bottom edge of the screen.
    const MENU_BOTTOM_MARGIN: u16 = 2;

    /// Selection-cursor arrow size and the gap between it and its target
    /// button.
    const CURSOR_W: u16 = 2;
    const CURSOR_GAP: u16 = 1;

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
        // Top margin: inset the container by 1 whole text cell (4 dots) off
        // its top edge before running the flex call, so the title box sits
        // 1 cell lower than a zero-margin `Align::Start` would place it.
        let container = Self::cell_rect_to_dots(area).inset(0, 0, 4, 0);
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

    /// The 3 menu-button rects for `area`, top-to-bottom (index 0 Roster, 1
    /// Battle, 2 Exit — labels/roles assigned by b5-t3, this fixes geometry
    /// and order only).
    fn button_rects(area: Rect) -> [Rect; 3] {
        let container = Self::cell_rect_to_dots(Self::menu_container(area));
        let child = || FlexChild {
            // Column: closure returns (main=Y/height, cross=X/width) in dots.
            basis: Basis::Intrinsic(Box::new(|_main| {
                (Self::BUTTON_H as i32 * 4, Self::BUTTON_W as i32 * 2)
            })),
            grow: 0.0,
            shrink: 0.0,
        };
        let children = [child(), child(), child()];
        let style = FlexStyle {
            direction: Direction::Column,
            justify_content: Justify::Start,
            align_items: Align::Start,
            gap: Self::MENU_GAP as i32 * 4,
        };
        let rects = flex(container, style, &children);
        [rects[0].to_cell_rect(), rects[1].to_cell_rect(), rects[2].to_cell_rect()]
    }

    /// Paint `crate::assets::FRAME_PANEL` stretched to fill `rect` exactly (same
    /// stretch-fit routine a `Button`'s background render uses), static
    /// (no `ButtonState` tint). Early-returns on a zero-dim rect.
    fn draw_title_frame(&self, buf: &mut Buffer, rect: Rect) {
        let dot_cols = rect.width as usize * 2;
        let dot_rows = rect.height as usize * 4;
        if dot_cols == 0 || dot_rows == 0 {
            return;
        }

        let dots = engine_render::asset_cache::sprite_to_dots(
            crate::assets::FRAME_PANEL,
            dot_cols as u32,
            dot_rows as u32,
        );
        engine_render::draw_dots(buf, rect, &dots);
    }

    /// `title` inset by 1 cell per side — the interior the logo paints into,
    /// kept clear of the frame's border thickness.
    fn title_interior(title: Rect) -> Rect {
        Rect {
            x: title.x + 1,
            y: title.y + 1,
            width: title.width.saturating_sub(2),
            height: title.height.saturating_sub(2),
        }
    }

    /// Selection-cursor arrow's paint rect — a `CURSOR_W`-wide,
    /// `CURSOR_GAP`-gapped band immediately left of `button`, spanning its
    /// full height. `saturating_sub` guards left-edge underflow (practically
    /// unreachable — the menu is centered well clear of the left edge).
    fn cursor_rect(button: Rect) -> Rect {
        Rect {
            x: button.x.saturating_sub(Self::CURSOR_GAP + Self::CURSOR_W),
            y: button.y,
            width: Self::CURSOR_W,
            height: button.height,
        }
    }

    /// Sole activation dispatch for a menu index (0 Roster, 1 Battle,
    /// 2 Exit). Keyboard Enter (b5-t5) and mouse click (b5-t6) both route
    /// here — never a duplicated match. Index is 0..=2 by construction;
    /// other values are inert.
    fn activate(&mut self, index: usize) -> Option<Transition> {
        match index {
            0 => Some(Transition { target: SceneId::RosterManager.into(), params: None }),
            1 => Some(Transition { target: SceneId::BattleViewer.into(), params: None }),
            2 => {
                self.quit_requested = true;
                None
            }
            _ => None,
        }
    }
}

impl Scene for MainHub {
    fn id(&self) -> SceneKey {
        SceneId::MainHub.into()
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {
        if crate::first_run::is_first_run() {
            self.elapsed = 0.0;
            let _ = crate::first_run::mark_first_run_done();
        } else {
            self.elapsed = crate::scenes::title_logo::ANIM_END;
        }
    }

    fn update(&mut self, _ctx: &mut EngineCtx, dt: Duration) -> Option<Transition> {
        self.elapsed += dt.as_secs_f32();
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let buf = frame.buffer_mut();
        let title = Self::title_rect(area);
        self.draw_title_frame(buf, title);
        let interior = Self::title_interior(title);
        let (grid, _dot_rect) = crate::scenes::title_logo::frame(self.elapsed);
        engine_render::draw_grid(buf, interior, &grid);

        let rects = Self::button_rects(area);
        for (button, rect) in self.buttons.iter().zip(rects) {
            let mut b = button.borrow_mut();
            b.set_rect(rect);
            b.render(buf);
        }

        let cursor_rect = Self::cursor_rect(rects[self.cursor_index]);
        let grid = engine_render::asset_cache::convert(crate::assets::ICON_ARROW_RIGHT, cursor_rect);
        engine_render::draw_grid(buf, cursor_rect, &grid);
    }

    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
        use crossterm::event::{KeyCode, MouseEventKind};
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
