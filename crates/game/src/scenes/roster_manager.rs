use std::cell::RefCell;
use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::Rect;
use scene_core::scene_id::SceneId;
use scene_core::Inspectable;
use serde_json::Value as JsonValue;

use crate::scene::{EngineCtx, InputEvent, Scene, Transition};

#[derive(Inspectable)]
pub struct RosterManager {
    current_index: usize,
    #[inspect(hidden)]
    creatures: Vec<render::creature::Creature>,
    #[inspect(hidden)]
    elapsed: Duration,
    /// Mouse-driven navigation buttons beside the sprite (b4-t2). `RefCell`
    /// because `render(&self, ..)` must mutate their rect/state from an
    /// immutable receiver (see research.md b4-t2 blueprint point 1).
    #[inspect(hidden)]
    left_button: RefCell<render::Button>,
    #[inspect(hidden)]
    right_button: RefCell<render::Button>,
    /// Top-right button that transitions back to `MainHub` (b4-t3). `RefCell`
    /// for the same immutable-render-mutates-button-state reason as the
    /// arrows.
    #[inspect(hidden)]
    home_button: RefCell<render::Button>,
}

impl RosterManager {
    /// Width/height of the left/right arrow buttons flanking the sprite.
    const ARROW_W: u16 = 6;
    const ARROW_H: u16 = 3;

    /// Width/height of the top-right home button.
    const HOME_W: u16 = 6;
    const HOME_H: u16 = 3;

    /// Splits `area` into `(sprite_rect, name_rect, dots_rect)`: the bottom
    /// two rows are reserved for the name label and the 6-dot position
    /// indicator row, with the sprite occupying the remaining region above,
    /// inset horizontally by `ARROW_W` on each side to make room for the
    /// left/right arrow buttons (b4-t2). Sprite centering is delegated to
    /// `draw_grid`; name centering to `label`; dot-row slotting to
    /// `dot_slots`.
    fn layout(area: Rect) -> (Rect, Rect, Rect) {
        let reserved = 2.min(area.height);
        let sprite_h = area.height - reserved;
        let sprite_rect = Rect::new(
            area.x + Self::ARROW_W,
            area.y,
            area.width.saturating_sub(2 * Self::ARROW_W),
            sprite_h,
        );
        let name_rect = Rect::new(area.x, area.y + sprite_h, area.width, reserved.min(1));
        let dots_rect = Rect::new(
            area.x,
            area.y + sprite_h + reserved.min(1),
            area.width,
            reserved.saturating_sub(1),
        );
        (sprite_rect, name_rect, dots_rect)
    }

    /// The 6 equal-width dot slots across `row`, centered as a group.
    fn dot_slots(row: Rect) -> [Rect; 6] {
        const N: u16 = 6;
        let slot_w = (row.width / N).max(1);
        let group_w = slot_w * N;
        let x0 = row.x + row.width.saturating_sub(group_w) / 2;
        std::array::from_fn(|i| Rect::new(x0 + i as u16 * slot_w, row.y, slot_w, row.height))
    }

    pub fn new() -> Self {
        Self {
            current_index: 0,
            creatures: render::creature::all(),
            elapsed: Duration::ZERO,
            left_button: RefCell::new(render::Button::new(Rect::default(), render::assets::ICON_ARROW_LEFT)),
            right_button: RefCell::new(render::Button::new(Rect::default(), render::assets::ICON_ARROW_RIGHT)),
            home_button: RefCell::new(render::Button::new(Rect::default(), render::assets::ICON_HOME)),
        }
    }

    /// Left/right arrow button rects beside the sprite for the current
    /// `area` — the sole place button positioning is computed; `render()`
    /// and tests both call this rather than re-deriving it. Both rects are
    /// vertically centered on the sprite band established by `layout()`.
    fn arrow_rects(area: Rect) -> (Rect, Rect) {
        let (sprite_rect, _, _) = Self::layout(area);
        let h = Self::ARROW_H.min(sprite_rect.height);
        let y = sprite_rect.y + sprite_rect.height.saturating_sub(Self::ARROW_H) / 2;
        let left_rect = Rect::new(area.x, y, Self::ARROW_W, h);
        let right_rect = Rect::new(
            area.right().saturating_sub(Self::ARROW_W),
            y,
            Self::ARROW_W,
            h,
        );
        (left_rect, right_rect)
    }

    /// Top-right rect for the home button — sole place its position is
    /// computed; `render()` and tests both call this.
    fn home_rect(area: Rect) -> Rect {
        Rect::new(
            area.right().saturating_sub(Self::HOME_W),
            area.top(),
            Self::HOME_W.min(area.width),
            Self::HOME_H.min(area.height),
        )
    }

    /// Advances/retreats `current_index` with wraparound. The sole place
    /// carousel index arithmetic lives — mouse (b4-t2) and slide-direction
    /// (b5-t1) paths must call this, never re-derive `(idx±1)%n`.
    fn navigate(&mut self, dir: Direction) {
        let n = self.creatures.len();
        self.current_index = match dir {
            Direction::Right => (self.current_index + 1) % n,
            Direction::Left => (self.current_index + n - 1) % n,
        };
    }
}

/// Shared carousel direction — the sole type b4-t2 (mouse) and b5-t1 (slide)
/// must also consume, never re-derive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Direction {
    Left,
    Right,
}

impl Default for RosterManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene for RosterManager {
    fn id(&self) -> SceneId {
        SceneId::RosterManager
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}

    fn update(&mut self, _ctx: &mut EngineCtx, dt: Duration) -> Option<Transition> {
        self.elapsed += dt;
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let (sprite_rect, name_rect, dots_rect) = Self::layout(area);
        let creature = &self.creatures[self.current_index];
        if let Some(sprite) = creature.animation(render::AnimationKind::Idle) {
            let grid = render::convert(sprite.frame_at(self.elapsed), sprite_rect);
            render::draw_grid(frame.buffer_mut(), sprite_rect, &grid);
        }
        render::label(frame.buffer_mut(), name_rect, creature.name());

        for (i, slot) in Self::dot_slots(dots_rect).iter().enumerate() {
            let bytes = if i == self.current_index {
                render::assets::DOT_FILLED
            } else {
                render::assets::DOT_UNFILLED
            };
            render::draw_asset(frame.buffer_mut(), *slot, bytes);
        }

        let (left_rect, right_rect) = Self::arrow_rects(area);
        {
            let mut left = self.left_button.borrow_mut();
            left.set_rect(left_rect);
            left.render(frame.buffer_mut());
        }
        {
            let mut right = self.right_button.borrow_mut();
            right.set_rect(right_rect);
            right.render(frame.buffer_mut());
        }

        let home_rect = Self::home_rect(area);
        {
            let mut home = self.home_button.borrow_mut();
            home.set_rect(home_rect);
            home.render(frame.buffer_mut());
        }
    }

    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
        use crossterm::event::KeyCode;

        match ev {
            InputEvent::Key(key) => match key.code {
                KeyCode::Left => self.navigate(Direction::Left),
                KeyCode::Right => self.navigate(Direction::Right),
                _ => {}
            },
            InputEvent::Mouse(me) => {
                let hit_left = self.left_button.get_mut().handle_mouse(&me);
                let hit_right = self.right_button.get_mut().handle_mouse(&me);
                let hit_home = self.home_button.get_mut().handle_mouse(&me);
                if hit_home {
                    return Some(Transition {
                        target: SceneId::MainHub,
                        params: None,
                    });
                }
                if hit_right {
                    self.navigate(Direction::Right);
                }
                if hit_left {
                    self.navigate(Direction::Left);
                }
            }
        }
        None
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {}

    fn inspect(&mut self) -> &mut dyn scene_core::Inspectable {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_seeds_full_roster_at_index_zero_with_zero_elapsed() {
        let rm = RosterManager::new();
        assert_eq!(
            rm.creatures.len(),
            6,
            "RosterManager::new() must seed all 6 creatures from render::creature::all()"
        );
        assert_eq!(rm.creatures[0].name(), "Ember Wolf");
        assert_eq!(rm.current_index, 0);
        assert_eq!(rm.elapsed, Duration::ZERO);
    }

    #[test]
    fn schema_exposes_only_current_index() {
        let names: Vec<String> = <RosterManager as Inspectable>::schema()
            .children
            .iter()
            .map(|c| c.name.clone())
            .collect();
        assert_eq!(
            names,
            vec!["current_index".to_string()],
            "creatures/elapsed must be #[inspect(hidden)]; only current_index is editable"
        );
    }
}

#[cfg(test)]
mod sprite_and_name_render_tests {
    use super::*;
    use crate::scene::EngineCtx;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Render `scene` into a fresh `TestBackend` and return the buffer.
    fn render_to_buffer(scene: &RosterManager, w: u16, h: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                scene.render(f, area);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// Row index of the first row whose text contains `needle`, if any.
    fn row_containing(buf: &ratatui::buffer::Buffer, w: u16, h: u16, needle: &str) -> Option<u16> {
        (0..h).find(|&y| {
            let row_text: String = (0..w)
                .map(|x| buf.cell((x, y)).unwrap().symbol().to_string())
                .collect();
            row_text.contains(needle)
        })
    }

    /// A fresh `RosterManager::new()` (current_index == 0) renders the Ember
    /// Wolf idle sprite centered above a name row that reads "Ember Wolf".
    #[test]
    fn renders_index0_sprite_and_name() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let name_row = row_containing(&buf, w, h, "Ember Wolf")
            .expect("render must place the current creature's name ('Ember Wolf') somewhere on screen");

        let sprite_has_non_space = (0..name_row).any(|y| {
            (0..w).any(|x| buf.cell((x, y)).unwrap().symbol() != " ")
        });
        assert!(
            sprite_has_non_space,
            "render must paint at least one non-space cell in the sprite region above the name row"
        );
    }

    /// `update()` accumulating `dt` across multiple ticks past a frame
    /// boundary must change the composited idle frame — the animation
    /// genuinely progresses over `elapsed`, not a static first frame.
    #[test]
    fn idle_frame_advances_with_update() {
        let (w, h) = (40u16, 20u16);
        let mut ctx = EngineCtx;

        let still = RosterManager::new();
        let buf_at_zero = render_to_buffer(&still, w, h);

        let mut advanced = RosterManager::new();
        // Ember Wolf's idle frame_dur is 80ms; tick across multiple dt calls
        // summing well past one frame boundary.
        for _ in 0..5 {
            advanced.update(&mut ctx, Duration::from_millis(20));
        }
        let buf_after = render_to_buffer(&advanced, w, h);

        assert_ne!(
            buf_at_zero, buf_after,
            "composited sprite must change after update() crosses a frame boundary (elapsed advanced by more than one frame_dur)"
        );
    }

    /// Setting `current_index` to each of the 6 roster slots renders that
    /// creature's name on screen.
    #[test]
    fn name_label_tracks_current_index() {
        let (w, h) = (40u16, 20u16);
        let all = render::creature::all();

        for (i, creature) in all.iter().enumerate() {
            let mut scene = RosterManager::new();
            scene.current_index = i;
            let buf = render_to_buffer(&scene, w, h);
            assert!(
                row_containing(&buf, w, h, creature.name()).is_some(),
                "current_index == {i} must render the name '{}' somewhere on screen",
                creature.name()
            );
        }
    }
}

#[cfg(test)]
mod dot_row_render_tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use ratatui::Terminal;

    fn render_to_buffer(scene: &RosterManager, w: u16, h: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                scene.render(f, area);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// The fg color of the first non-space cell found inside `slot`, or
    /// `None` if the slot has no painted cell.
    fn sample_fg(buf: &ratatui::buffer::Buffer, slot: Rect) -> Option<Color> {
        (slot.top()..slot.bottom())
            .flat_map(|y| (slot.left()..slot.right()).map(move |x| (x, y)))
            .find_map(|(x, y)| {
                let cell = buf.cell((x, y))?;
                if cell.symbol() != " " {
                    Some(cell.fg)
                } else {
                    None
                }
            })
    }

    /// At `current_index == 0` (fresh `new()`), the dot row paints 6 distinct
    /// non-space dot-cell groups — one per `dot_slots` slot on `layout`'s
    /// `dots_rect` — and slot 0 (filled) paints a different fg than each of
    /// the other 5 (unfilled) slots.
    #[test]
    fn dot_row_six_groups_filled_at_index0() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let (_, _, dots_rect) = RosterManager::layout(area);
        let slots = RosterManager::dot_slots(dots_rect);

        let fgs: Vec<Color> = slots
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                sample_fg(&buf, *slot)
                    .unwrap_or_else(|| panic!("dot slot {i} must paint at least one non-space cell"))
            })
            .collect();

        for i in 1..6 {
            assert_ne!(
                fgs[0], fgs[i],
                "slot 0 (filled) fg must differ from slot {i} (unfilled) fg"
            );
        }
    }

    /// Setting `current_index = 3` moves the filled/brighter dot to slot 3;
    /// slot 0 now renders the unfilled color, distinct from slot 3.
    #[test]
    fn filled_dot_follows_current_index() {
        let mut scene = RosterManager::new();
        scene.current_index = 3;
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let (_, _, dots_rect) = RosterManager::layout(area);
        let slots = RosterManager::dot_slots(dots_rect);

        let fg0 = sample_fg(&buf, slots[0]).expect("slot 0 must paint at least one non-space cell");
        let fg3 = sample_fg(&buf, slots[3]).expect("slot 3 must paint at least one non-space cell");
        assert_ne!(
            fg0, fg3,
            "slot 0 (now unfilled) and slot 3 (now filled) fg must differ"
        );
    }
}

#[cfg(test)]
mod arrow_key_navigation_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key_event(code: KeyCode) -> InputEvent {
        InputEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    /// Right arrow from the last index (5) wraps to 0.
    #[test]
    fn right_arrow_wraps_from_last_to_zero() {
        let mut scene = RosterManager::new();
        scene.current_index = 5;
        let transition = scene.handle_input(key_event(KeyCode::Right));
        assert_eq!(scene.current_index, 0, "right arrow at index 5 must wrap to 0");
        assert!(transition.is_none(), "arrow keys must not produce a Transition");
    }

    /// Left arrow from index 0 wraps to the last index (5).
    #[test]
    fn left_arrow_wraps_from_zero_to_last() {
        let mut scene = RosterManager::new();
        scene.current_index = 0;
        let transition = scene.handle_input(key_event(KeyCode::Left));
        assert_eq!(scene.current_index, 5, "left arrow at index 0 must wrap to 5");
        assert!(transition.is_none(), "arrow keys must not produce a Transition");
    }

    /// Right arrow from a middle index advances by 1 (non-wrap case).
    #[test]
    fn right_arrow_advances_without_wrap() {
        let mut scene = RosterManager::new();
        scene.current_index = 2;
        let transition = scene.handle_input(key_event(KeyCode::Right));
        assert_eq!(scene.current_index, 3, "right arrow at index 2 must advance to 3");
        assert!(transition.is_none(), "arrow keys must not produce a Transition");
    }

    /// A key unrelated to navigation leaves `current_index` unchanged.
    #[test]
    fn unrelated_key_leaves_index_unchanged() {
        let mut scene = RosterManager::new();
        scene.current_index = 2;
        let transition = scene.handle_input(key_event(KeyCode::Char('q')));
        assert_eq!(scene.current_index, 2, "unrelated key must not change current_index");
        assert!(transition.is_none(), "unrelated key must not produce a Transition");
    }
}

#[cfg(test)]
mod arrow_button_tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::Terminal;

    fn render_to_buffer(scene: &RosterManager, w: u16, h: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                scene.render(f, area);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> InputEvent {
        InputEvent::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        })
    }

    /// True if any cell inside `rect` is non-space.
    fn has_non_space(buf: &ratatui::buffer::Buffer, rect: Rect) -> bool {
        (rect.top()..rect.bottom())
            .flat_map(|y| (rect.left()..rect.right()).map(move |x| (x, y)))
            .any(|(x, y)| buf.cell((x, y)).unwrap().symbol() != " ")
    }

    /// Leftmost column (if any) within `rect` painted non-space.
    fn leftmost_non_space(buf: &ratatui::buffer::Buffer, rect: Rect) -> Option<u16> {
        (rect.left()..rect.right()).find(|&x| {
            (rect.top()..rect.bottom()).any(|y| buf.cell((x, y)).unwrap().symbol() != " ")
        })
    }

    /// Rightmost column (if any) within `rect` painted non-space.
    fn rightmost_non_space(buf: &ratatui::buffer::Buffer, rect: Rect) -> Option<u16> {
        (rect.left()..rect.right()).rev().find(|&x| {
            (rect.top()..rect.bottom()).any(|y| buf.cell((x, y)).unwrap().symbol() != " ")
        })
    }

    /// `render()` paints the left arrow button strictly to the left of the
    /// sprite's leftmost occupied column.
    #[test]
    fn left_button_renders_beside_sprite() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let (left_rect, _) = RosterManager::arrow_rects(area);
        let (sprite_rect, _, _) = RosterManager::layout(area);

        assert!(
            has_non_space(&buf, left_rect),
            "left arrow button must paint at least one non-space cell within its rect"
        );
        let sprite_left = leftmost_non_space(&buf, sprite_rect)
            .expect("sprite must paint at least one non-space cell");
        let button_right = rightmost_non_space(&buf, left_rect)
            .expect("left button must paint at least one non-space cell");
        assert!(
            button_right < sprite_left,
            "left arrow button's rightmost painted column ({button_right}) must be strictly left of the sprite's leftmost painted column ({sprite_left})"
        );
    }

    /// `render()` paints the right arrow button strictly to the right of the
    /// sprite's rightmost occupied column.
    #[test]
    fn right_button_renders_beside_sprite() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let (_, right_rect) = RosterManager::arrow_rects(area);
        let (sprite_rect, _, _) = RosterManager::layout(area);

        assert!(
            has_non_space(&buf, right_rect),
            "right arrow button must paint at least one non-space cell within its rect"
        );
        let sprite_right = rightmost_non_space(&buf, sprite_rect)
            .expect("sprite must paint at least one non-space cell");
        let button_left = leftmost_non_space(&buf, right_rect)
            .expect("right button must paint at least one non-space cell");
        assert!(
            button_left > sprite_right,
            "right arrow button's leftmost painted column ({button_left}) must be strictly right of the sprite's rightmost painted column ({sprite_right})"
        );
    }

    /// A completed click on the right button drives the SAME `navigate()`
    /// as the right-arrow key (b4-t1): wraps 5 -> 0.
    #[test]
    fn mouse_click_right_button_wraps_like_right_key() {
        let mut scene = RosterManager::new();
        scene.current_index = 5;
        let (w, h) = (40u16, 20u16);
        // Render once so the buttons' rects are set to this frame's `area`
        // (handle_input hit-tests against the PREVIOUS frame's render).
        let _ = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let (_, right_rect) = RosterManager::arrow_rects(area);
        let (cx, cy) = (right_rect.x, right_rect.y);

        let t1 = scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        let t2 = scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
        let t3 = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

        assert_eq!(scene.current_index, 0, "a completed click on the right button at index 5 must wrap current_index to 0");
        assert!(t1.is_none() && t2.is_none() && t3.is_none(), "arrow buttons must never produce a Transition");
    }

    /// A completed click on the left button drives the SAME `navigate()` as
    /// the left-arrow key (b4-t1): wraps 0 -> 5.
    #[test]
    fn mouse_click_left_button_wraps_like_left_key() {
        let mut scene = RosterManager::new();
        scene.current_index = 0;
        let (w, h) = (40u16, 20u16);
        let _ = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let (left_rect, _) = RosterManager::arrow_rects(area);
        let (cx, cy) = (left_rect.x, left_rect.y);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
        let t = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

        assert_eq!(scene.current_index, 5, "a completed click on the left button at index 0 must wrap current_index to 5");
        assert!(t.is_none(), "arrow buttons must never produce a Transition");
    }

    /// A click sequence that completes outside both button rects leaves
    /// `current_index` unchanged.
    #[test]
    fn click_outside_buttons_is_noop() {
        let mut scene = RosterManager::new();
        scene.current_index = 2;
        let (w, h) = (40u16, 20u16);
        let _ = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let (left_rect, right_rect) = RosterManager::arrow_rects(area);
        // Horizontal midpoint of `area`, at the buttons' own row: between
        // the two edge-hugging buttons, outside both rects.
        let (ox, oy) = (area.width / 2, left_rect.y);
        assert!(!left_rect.contains(ratatui::layout::Position { x: ox, y: oy }));
        assert!(!right_rect.contains(ratatui::layout::Position { x: ox, y: oy }));

        scene.handle_input(mouse_event(MouseEventKind::Moved, ox, oy));
        scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), ox, oy));
        let t = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), ox, oy));

        assert_eq!(scene.current_index, 2, "a click completed outside both button rects must not change current_index");
        assert!(t.is_none());
    }
}

#[cfg(test)]
mod home_button_tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::Terminal;

    fn render_to_buffer(scene: &RosterManager, w: u16, h: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                scene.render(f, area);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> InputEvent {
        InputEvent::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        })
    }

    /// True if any cell inside `rect` is non-space.
    fn has_non_space(buf: &ratatui::buffer::Buffer, rect: Rect) -> bool {
        (rect.top()..rect.bottom())
            .flat_map(|y| (rect.left()..rect.right()).map(move |x| (x, y)))
            .any(|(x, y)| buf.cell((x, y)).unwrap().symbol() != " ")
    }

    /// `render()` paints the home button, and its rect sits top-right of
    /// `area` — flush with the top edge and the right edge — distinct from
    /// the arrow buttons' beside-center position and the dot row's bottom
    /// position.
    #[test]
    fn home_button_renders_top_right() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::home_rect(area);

        assert!(
            has_non_space(&buf, rect),
            "home button must paint at least one non-space cell within its rect"
        );
        assert_eq!(rect.right(), area.right(), "home button rect must be flush with the right edge of area");
        assert_eq!(rect.top(), area.top(), "home button rect must be flush with the top edge of area");
    }

    /// A completed click (Moved+Down+Up, all inside the home button's rect)
    /// returns a `Transition` to `MainHub` with no params.
    #[test]
    fn home_click_transitions_to_main_hub() {
        let mut scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        // Render once so the button's rect is set to this frame's `area`
        // (handle_input hit-tests against the PREVIOUS frame's render).
        let _ = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::home_rect(area);
        let (cx, cy) = (rect.x, rect.y);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
        let t = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

        let t = t.expect("a completed click on the home button must return a Transition");
        assert_eq!(t.target, SceneId::MainHub, "home button must transition to MainHub");
        assert!(t.params.is_none(), "home button transition must carry no params");
    }

    /// A click that does not complete inside the home button's rect (Down
    /// inside, Up outside) must not transition and must not touch
    /// `current_index`.
    #[test]
    fn home_click_not_completed_returns_none() {
        let mut scene = RosterManager::new();
        scene.current_index = 2;
        let (w, h) = (40u16, 20u16);
        let _ = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::home_rect(area);
        let (cx, cy) = (rect.x, rect.y);
        // Bottom-left corner: far from the top-right home rect.
        let (ox, oy) = (0u16, h - 1);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
        let t = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), ox, oy));

        assert!(
            t.is_none(),
            "a click that does not complete inside the home button rect must not return a Transition"
        );
        assert_eq!(
            scene.current_index, 2,
            "an incomplete home-button click must not change current_index"
        );
    }
}
