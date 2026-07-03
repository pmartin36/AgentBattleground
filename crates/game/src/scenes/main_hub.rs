use std::cell::RefCell;
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::Frame;
use ratatui::layout::Rect;
use scene_core::color::Rgba;
use scene_core::scene_id::SceneId;
use scene_core::Inspectable;
use serde_json::Value as JsonValue;

use render::{anchor, stack, Anchor, ButtonState, FrameButton, StackAxis};

use crate::scene::{EngineCtx, InputEvent, Scene, Transition};

#[derive(Inspectable)]
pub struct MainHub {
    /// The 3 menu buttons (index 0 Roster, 1 Battle, 2 Exit — matches
    /// `button_rects`' order). `RefCell` because `render(&self, ..)` must
    /// mutate each button's rect/state through an immutable receiver —
    /// mirrors `RosterManager`'s button fields.
    #[inspect(hidden)]
    buttons: [RefCell<FrameButton>; 3],

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
                RefCell::new(FrameButton::new(Rect::default(), "Roster")),
                RefCell::new(FrameButton::new(Rect::default(), "Battle")),
                RefCell::new(FrameButton::new(Rect::default(), "Exit")),
            ],
            cursor_index: 0,
            quit_requested: false,
        }
    }
}

impl MainHub {
    pub const COLOR: Rgba = Rgba::rgb(0x1e, 0x3a, 0xc8);

    /// Title box size (b5-t1).
    const TITLE_W: u16 = 40;
    const TITLE_H: u16 = 8;

    /// One menu button's size and the vertical gap between stacked buttons.
    const BUTTON_W: u16 = 20;
    const BUTTON_H: u16 = 3;
    const MENU_GAP: u16 = 1;

    /// Menu container size — width is a single button's width; height MUST
    /// equal the stacked group's total height (3 buttons + 2 gaps) so
    /// `stack` fills the container exactly rather than leaving slack.
    const MENU_W: u16 = Self::BUTTON_W;
    const MENU_H: u16 = 3 * Self::BUTTON_H + 2 * Self::MENU_GAP;

    /// Selection-cursor arrow size and the gap between it and its target
    /// button.
    const CURSOR_W: u16 = 2;
    const CURSOR_GAP: u16 = 1;

    /// Title box rect for `area` — sole place its position is computed;
    /// `render()` and tests both call this.
    fn title_rect(area: Rect) -> Rect {
        anchor(area, (Self::TITLE_W, Self::TITLE_H), Anchor::TopCenter)
    }

    /// Menu group container rect for `area` — sole place its position is
    /// computed; feeds `button_rects` via `stack`.
    fn menu_container(area: Rect) -> Rect {
        anchor(area, (Self::MENU_W, Self::MENU_H), Anchor::Center)
    }

    /// The 3 menu-button rects for `area`, top-to-bottom (index 0 Roster, 1
    /// Battle, 2 Exit — labels/roles assigned by b5-t3, this fixes geometry
    /// and order only).
    fn button_rects(area: Rect) -> [Rect; 3] {
        let container = Self::menu_container(area);
        let v = stack(
            container,
            &[(Self::BUTTON_W, Self::BUTTON_H); 3],
            Self::MENU_GAP,
            StackAxis::Vertical,
        );
        v.try_into().expect("stack of 3 sizes must yield 3 rects")
    }

    /// Paint `assets::FRAME_PANEL` stretched to fill `rect` exactly (same
    /// stretch-fit routine `FrameButton::render` uses for its panel), static
    /// (no `ButtonState` tint). Early-returns on a zero-dim rect.
    fn draw_title_frame(buf: &mut Buffer, rect: Rect) {
        let dot_cols = rect.width as usize * 2;
        let dot_rows = rect.height as usize * 4;
        if dot_cols == 0 || dot_rows == 0 {
            return;
        }

        let frame = image::load_from_memory(render::assets::FRAME_PANEL)
            .expect("FRAME_PANEL must decode — bundled first-party asset");
        let dots = render::dots::sprite_to_dots(&frame, dot_cols as u32, dot_rows as u32);
        let grid = render::dots::dots_to_grid(&dots);
        render::draw_grid(buf, rect, &grid);
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
            0 => Some(Transition { target: SceneId::RosterManager, params: None }),
            1 => Some(Transition { target: SceneId::BattleViewer, params: None }),
            2 => {
                self.quit_requested = true;
                None
            }
            _ => None,
        }
    }
}

impl Scene for MainHub {
    fn id(&self) -> SceneId {
        SceneId::MainHub
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}

    fn update(&mut self, _ctx: &mut EngineCtx, _dt: Duration) -> Option<Transition> {
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let buf = frame.buffer_mut();
        let title = Self::title_rect(area);
        Self::draw_title_frame(buf, title);
        render::draw_asset(buf, Self::title_interior(title), render::assets::LOGO);

        let rects = Self::button_rects(area);
        for (button, rect) in self.buttons.iter().zip(rects) {
            let mut b = button.borrow_mut();
            b.set_rect(rect);
            b.render(buf);
        }

        render::draw_asset(
            buf,
            Self::cursor_rect(rects[self.cursor_index]),
            render::assets::ICON_ARROW_RIGHT,
        );
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
                KeyCode::Enter => return self.activate(self.cursor_index),
                _ => {}
            },
            InputEvent::Mouse(me) => {
                let mut clicked: Option<usize> = None;
                let mut hovered: Option<usize> = None;
                for (i, button) in self.buttons.iter().enumerate() {
                    let mut b = button.borrow_mut();
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

    fn inspect(&mut self) -> &mut dyn scene_core::Inspectable {
        self
    }

    fn quit_requested(&self) -> bool {
        self.quit_requested
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::scene::Scene;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_to_buffer(w: u16, h: u16) -> (ratatui::buffer::Buffer, Rect) {
        let scene = MainHub::default();
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        let mut area = Rect::new(0, 0, 0, 0);
        terminal
            .draw(|f| {
                area = f.area();
                scene.render(f, area);
            })
            .unwrap();
        (terminal.backend().buffer().clone(), area)
    }

    /// b5-t2 deliverable: the title box paints real content (frame + logo,
    /// non-space cells) and the bare display-name text "Main Hub" appears
    /// nowhere in the rendered buffer — the logo is the title, not a label.
    #[test]
    fn main_hub_title_box_paints_and_has_no_text() {
        let (buf, area) = render_to_buffer(120, 50);
        let title = MainHub::title_rect(area);

        let painted = (title.top()..title.bottom()).any(|y| {
            (title.left()..title.right())
                .any(|x| buf.cell((x, y)).unwrap().symbol() != " ")
        });
        assert!(
            painted,
            "title rect {title:?} must contain at least one non-space painted cell"
        );

        let full_text: String = (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf.cell((x, y)).unwrap().symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !full_text.contains("Main Hub"),
            "rendered buffer must not contain the bare display-name text \"Main Hub\", got:\n{full_text}"
        );
    }

    /// b5-t3 deliverable: the 3 menu buttons render their exact label text
    /// on the center row of their respective `button_rects`, top-to-bottom
    /// in order Roster, Battle, Exit.
    #[test]
    fn main_hub_renders_three_menu_button_labels() {
        let (buf, area) = render_to_buffer(120, 50);
        let rects = MainHub::button_rects(area);
        let labels = ["Roster", "Battle", "Exit"];

        for (rect, label) in rects.iter().zip(labels.iter()) {
            let y = rect.y + rect.height / 2;
            let row_text: String = (rect.left()..rect.right())
                .map(|x| buf.cell((x, y)).unwrap().symbol().to_string())
                .collect();
            assert!(
                row_text.contains(label),
                "button rect {rect:?} center row must contain label {label:?}, got {row_text:?}"
            );
        }

        assert!(
            rects[0].y < rects[1].y && rects[1].y < rects[2].y,
            "button rects must be strictly ordered top-to-bottom Roster, Battle, Exit"
        );
    }

    /// b5-t4 deliverable: with `cursor_index = 1`, the selection-cursor arrow
    /// paints in the column band immediately left of the Battle button's
    /// rect, and that same band is untouched (all spaces) to the left of the
    /// Roster and Exit rects.
    #[test]
    fn cursor_appears_left_of_focused_button() {
        let scene = MainHub {
            cursor_index: 1,
            ..Default::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(120, 50)).unwrap();
        let mut area = Rect::new(0, 0, 0, 0);
        terminal
            .draw(|f| {
                area = f.area();
                scene.render(f, area);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();

        let rects = MainHub::button_rects(area);
        let cursor_w: u16 = 2;
        let cursor_gap: u16 = 1;
        let band_painted = |rect: Rect| -> bool {
            let x0 = rect.x.saturating_sub(cursor_gap + cursor_w);
            let x1 = rect.x.saturating_sub(cursor_gap);
            (rect.top()..rect.bottom())
                .any(|y| (x0..x1).any(|x| buf.cell((x, y)).unwrap().symbol() != " "))
        };

        assert!(
            band_painted(rects[1]),
            "left-of-Battle band must contain a painted cursor cell when cursor_index == 1"
        );
        assert!(
            !band_painted(rects[0]),
            "left-of-Roster band must be empty when cursor_index == 1"
        );
        assert!(
            !band_painted(rects[2]),
            "left-of-Exit band must be empty when cursor_index == 1"
        );
    }
}

#[cfg(test)]
mod keyboard_input_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key_event(code: KeyCode) -> InputEvent {
        InputEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn hub_at(cursor_index: usize) -> MainHub {
        MainHub {
            cursor_index,
            ..Default::default()
        }
    }

    /// Up at index 0 wraps to 2.
    #[test]
    fn up_wraps_from_zero_to_two() {
        let mut scene = hub_at(0);
        let transition = scene.handle_input(key_event(KeyCode::Up));
        assert_eq!(scene.cursor_index, 2, "Up at index 0 must wrap to 2");
        assert!(transition.is_none(), "arrow-key navigation must not transition");
    }

    /// Down at index 2 wraps to 0.
    #[test]
    fn down_wraps_from_two_to_zero() {
        let mut scene = hub_at(2);
        let transition = scene.handle_input(key_event(KeyCode::Down));
        assert_eq!(scene.cursor_index, 0, "Down at index 2 must wrap to 0");
        assert!(transition.is_none(), "arrow-key navigation must not transition");
    }

    /// Mid-range steps do not wrap: Down 0->1, Up 1->0.
    #[test]
    fn mid_range_steps_do_not_wrap() {
        let mut scene = hub_at(0);
        scene.handle_input(key_event(KeyCode::Down));
        assert_eq!(scene.cursor_index, 1, "Down at index 0 must step to 1");

        scene.handle_input(key_event(KeyCode::Up));
        assert_eq!(scene.cursor_index, 0, "Up at index 1 must step to 0");
    }

    /// Enter at cursor_index 0 (Roster) transitions to SceneId::RosterManager.
    #[test]
    fn enter_on_roster_transitions_to_roster_manager() {
        let mut scene = hub_at(0);
        let transition = scene
            .handle_input(key_event(KeyCode::Enter))
            .expect("Enter on Roster must return a Transition");
        assert_eq!(transition.target, SceneId::RosterManager);
        assert!(!scene.quit_requested(), "Roster activation must not request quit");
    }

    /// Enter at cursor_index 1 (Battle) transitions to SceneId::BattleViewer.
    #[test]
    fn enter_on_battle_transitions_to_battle_viewer() {
        let mut scene = hub_at(1);
        let transition = scene
            .handle_input(key_event(KeyCode::Enter))
            .expect("Enter on Battle must return a Transition");
        assert_eq!(transition.target, SceneId::BattleViewer);
        assert!(!scene.quit_requested(), "Battle activation must not request quit");
    }

    /// Enter at cursor_index 2 (Exit) requests quit and returns no Transition.
    #[test]
    fn enter_on_exit_sets_quit_requested_and_no_transition() {
        let mut scene = hub_at(2);
        let transition = scene.handle_input(key_event(KeyCode::Enter));
        assert!(transition.is_none(), "Exit activation must not return a Transition");
        assert!(scene.quit_requested(), "Exit activation must set quit_requested");
    }

    /// An unrelated key changes nothing and requests nothing.
    #[test]
    fn unrelated_key_is_noop() {
        let mut scene = hub_at(0);
        let transition = scene.handle_input(key_event(KeyCode::Char('x')));
        assert_eq!(scene.cursor_index, 0, "unrelated key must not move the cursor");
        assert!(transition.is_none(), "unrelated key must not transition");
        assert!(!scene.quit_requested(), "unrelated key must not request quit");
    }
}

#[cfg(test)]
mod mouse_input_tests {
    use super::*;
    use crate::scene::Scene;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::Terminal;
    use render::ButtonState;

    const W: u16 = 120;
    const H: u16 = 50;

    /// Renders `scene` once (populating each `FrameButton`'s rect via
    /// `set_rect`, per b5-t6 research's "render before dispatch" note) and
    /// returns the fixed `area` used by `button_rects`.
    fn render_once(scene: &MainHub) -> Rect {
        let mut terminal = Terminal::new(TestBackend::new(W, H)).unwrap();
        let mut area = Rect::new(0, 0, 0, 0);
        terminal
            .draw(|f| {
                area = f.area();
                scene.render(f, area);
            })
            .unwrap();
        area
    }

    fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> InputEvent {
        InputEvent::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        })
    }

    fn center(rect: Rect) -> (u16, u16) {
        (rect.x + rect.width / 2, rect.y + rect.height / 2)
    }

    /// A `Moved` landing inside the Battle button's rect (index 1) sets
    /// `cursor_index` to 1 and produces no transition.
    #[test]
    fn moved_inside_battle_sets_cursor_index_without_transition() {
        let mut scene = MainHub::default();
        let area = render_once(&scene);
        let rects = MainHub::button_rects(area);
        let (cx, cy) = center(rects[1]);

        let transition = scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));

        assert_eq!(
            scene.cursor_index, 1,
            "Moved inside the Battle button's rect must set cursor_index to 1"
        );
        assert!(transition.is_none(), "hover must not produce a Transition");
        assert_eq!(
            scene.buttons[1].borrow().state(),
            ButtonState::Hover,
            "the Battle button itself must report Hover state after the Moved event"
        );
    }

    /// A completed click (Moved+Down+Up, all inside the Exit button's rect)
    /// fires the quit signal via `activate`, with no transition.
    #[test]
    fn click_on_exit_requests_quit() {
        let mut scene = MainHub::default();
        let area = render_once(&scene);
        let rects = MainHub::button_rects(area);
        let (cx, cy) = center(rects[2]);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            cx,
            cy,
        ));
        let transition = scene.handle_input(mouse_event(
            MouseEventKind::Up(MouseButton::Left),
            cx,
            cy,
        ));

        assert!(
            transition.is_none(),
            "a completed click on Exit must not return a Transition"
        );
        assert!(
            scene.quit_requested(),
            "a completed click on Exit must set quit_requested"
        );
    }

    /// A completed click on the Roster button returns the same Transition
    /// Enter produces at cursor_index 0 — the shared `activate` dispatch.
    #[test]
    fn click_on_roster_transitions_to_roster_manager() {
        let mut scene = MainHub::default();
        let area = render_once(&scene);
        let rects = MainHub::button_rects(area);
        let (cx, cy) = center(rects[0]);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            cx,
            cy,
        ));
        let transition = scene
            .handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy))
            .expect("a completed click on Roster must return a Transition");

        assert_eq!(transition.target, SceneId::RosterManager);
        assert!(!scene.quit_requested(), "Roster activation must not request quit");
    }

    /// A completed click on the Battle button returns the same Transition
    /// Enter produces at cursor_index 1.
    #[test]
    fn click_on_battle_transitions_to_battle_viewer() {
        let mut scene = MainHub::default();
        let area = render_once(&scene);
        let rects = MainHub::button_rects(area);
        let (cx, cy) = center(rects[1]);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            cx,
            cy,
        ));
        let transition = scene
            .handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy))
            .expect("a completed click on Battle must return a Transition");

        assert_eq!(transition.target, SceneId::BattleViewer);
        assert!(!scene.quit_requested(), "Battle activation must not request quit");
    }

    /// A click sequence completed at a point outside all 3 button rects is a
    /// no-op: cursor_index unchanged, no transition, no quit.
    #[test]
    fn click_outside_all_buttons_is_noop() {
        let mut scene = MainHub {
            cursor_index: 0,
            ..Default::default()
        };
        let _area = render_once(&scene);
        // Top-left corner of the screen — outside the centered title/menu.
        let (ox, oy) = (0u16, 0u16);

        scene.handle_input(mouse_event(MouseEventKind::Moved, ox, oy));
        scene.handle_input(mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            ox,
            oy,
        ));
        let transition = scene.handle_input(mouse_event(
            MouseEventKind::Up(MouseButton::Left),
            ox,
            oy,
        ));

        assert_eq!(
            scene.cursor_index, 0,
            "a click sequence outside all button rects must not move cursor_index"
        );
        assert!(transition.is_none(), "a click outside all buttons must not transition");
        assert!(
            !scene.quit_requested(),
            "a click outside all buttons must not request quit"
        );
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use render::{anchor, stack, Anchor, StackAxis};

    /// Fixed screen area used by every case in this module.
    fn area() -> Rect {
        Rect::new(0, 0, 120, 50)
    }

    /// `title_rect` is exactly `anchor(area, (TITLE_W, TITLE_H),
    /// Anchor::TopCenter)` — the mechanism, not hand-derived arithmetic.
    #[test]
    fn title_rect_is_top_center_anchor() {
        let a = area();
        let expected = anchor(a, (MainHub::TITLE_W, MainHub::TITLE_H), Anchor::TopCenter);
        assert_eq!(MainHub::title_rect(a), expected);
    }

    /// `button_rects` is exactly `stack(menu_container(area), ..)` — proves
    /// the 3 button rects come from the stack mechanism applied to
    /// `menu_container`'s own output, not independently derived.
    #[test]
    fn button_rects_match_stack_of_menu_container() {
        let a = area();
        let container = MainHub::menu_container(a);
        let expected = stack(
            container,
            &[(MainHub::BUTTON_W, MainHub::BUTTON_H); 3],
            MainHub::MENU_GAP,
            StackAxis::Vertical,
        );
        let got = MainHub::button_rects(a);
        assert_eq!(got.as_slice(), expected.as_slice());
    }

    /// The 3 button rects are strictly ordered top-to-bottom (index 0
    /// Roster, 1 Battle, 2 Exit) and non-overlapping.
    #[test]
    fn button_rects_are_ordered_and_non_overlapping() {
        let rects = MainHub::button_rects(area());

        assert!(rects[0].y < rects[1].y, "rect 0 must be above rect 1");
        assert!(rects[1].y < rects[2].y, "rect 1 must be above rect 2");
        assert!(
            rects[0].bottom() <= rects[1].y,
            "rect 0 must not overlap rect 1"
        );
        assert!(
            rects[1].bottom() <= rects[2].y,
            "rect 1 must not overlap rect 2"
        );
    }

    /// `menu_container`'s height must be at least the stacked group's total
    /// height (3 buttons + 2 gaps) — guards the container/stack desync
    /// pitfall the blueprint calls out.
    #[test]
    fn menu_container_height_fits_three_buttons() {
        let container = MainHub::menu_container(area());
        assert!(
            container.height >= 3 * MainHub::BUTTON_H + 2 * MainHub::MENU_GAP,
            "menu_container height {} must fit 3 buttons + 2 gaps",
            container.height
        );
    }
}
