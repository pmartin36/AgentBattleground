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
        }
    }
}

impl MainHub {
    /// The bundled logo's own aspect ratio (width/height in dots — the same
    /// space `engine_render::convert`'s aspect-preserving fit operates in), used to
    /// size the title box's interior so the fit doesn't leave large empty
    /// margins. `crates/render/src/assets/logo.png` is 1212×481 ≈ 2.52:1.
    /// Measured directly against the bundled asset in a test below rather
    /// than trusted as a magic number.
    const LOGO_ASPECT: f32 = 1212.0 / 481.0;

    /// Fraction of the render area the title box's width/height occupy.
    /// Deliberately large — the previous fixed 40×8 size rendered the logo
    /// illegibly small on any real terminal (confirmed by rendering it and
    /// looking at the result).
    const TITLE_W_FRAC: f32 = 0.8;
    const TITLE_H_MAX_FRAC: f32 = 0.55;

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

    /// Title box size for `area`: width is `TITLE_W_FRAC` of `area.width`
    /// (with a sane floor so it's never absurdly small on a tiny terminal),
    /// height derived from `LOGO_ASPECT` so the logo's own aspect ratio
    /// fills the interior without large empty margins, capped at
    /// `TITLE_H_MAX_FRAC` of `area.height` so there's always real room left
    /// for the menu below.
    fn title_size(area: Rect) -> (u16, u16) {
        let w = ((area.width as f32 * Self::TITLE_W_FRAC) as u16).max(20);
        // Interior (after the 1-cell border inset each side) should match
        // LOGO_ASPECT in DOT space: (interior_w_cells*2) / (interior_h_cells*4)
        // == LOGO_ASPECT  =>  interior_h_cells == interior_w_cells / (2*LOGO_ASPECT).
        let interior_w = w.saturating_sub(2).max(1) as f32;
        let interior_h = (interior_w / (2.0 * Self::LOGO_ASPECT)).max(1.0);
        let h_from_aspect = (interior_h as u16).saturating_add(2);
        let h_cap = ((area.height as f32 * Self::TITLE_H_MAX_FRAC) as u16).max(6);
        // -1 whole cell shorter than the aspect/cap formula would otherwise
        // produce. Applied AFTER the `.min(h_cap)` (not to `h_from_aspect`
        // alone) so the shrink is visible even on short terminals where
        // `h_cap` (not the aspect ratio) is the binding constraint — shaving
        // `h_from_aspect` there would be silently absorbed by the cap and
        // produce no visible change. `.max(6)` keeps a sane floor matching
        // `h_cap`'s own floor.
        (w, h_from_aspect.min(h_cap).saturating_sub(1).max(6))
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

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}

    fn update(&mut self, _ctx: &mut EngineCtx, _dt: Duration) -> Option<Transition> {
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let buf = frame.buffer_mut();
        let title = Self::title_rect(area);
        self.draw_title_frame(buf, title);
        let interior = Self::title_interior(title);
        let grid = engine_render::asset_cache::convert(crate::assets::LOGO, interior);
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
                    let n = self.buttons.len();
                    self.cursor_index = (self.cursor_index + n - 1) % n;
                }
                KeyCode::Down => {
                    let n = self.buttons.len();
                    self.cursor_index = (self.cursor_index + 1) % n;
                }
                KeyCode::Enter | KeyCode::Char(' ') => return self.activate(self.cursor_index),
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
