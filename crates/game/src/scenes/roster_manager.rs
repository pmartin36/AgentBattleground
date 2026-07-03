use std::cell::RefCell;
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::Frame;
use ratatui::layout::Rect;
use render::tween::Tween;
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
    /// Transient scene-internal slide transition (b5-t1), armed by
    /// `navigate()` and driven by `elapsed`. `None` when no slide is active.
    #[inspect(hidden)]
    slide: Option<Slide>,
}

/// Transient bookkeeping for an in-flight slide transition: the group that is
/// leaving (`prev_index`), the direction of travel, and the `elapsed` value
/// at which the slide started. Scene-internal only — never a field on
/// `Creature` or any shared type.
#[derive(Clone, Copy, Debug)]
struct Slide {
    prev_index: usize,
    dir: Direction,
    start: Duration,
}

impl RosterManager {
    /// Width/height of the left/right arrow buttons flanking the sprite.
    const ARROW_W: u16 = 6;
    const ARROW_H: u16 = 3;

    /// Width/height of the top-right home button.
    const HOME_W: u16 = 6;
    const HOME_H: u16 = 3;

    /// Inset (in whole terminal cells) of the home/arrow buttons from the
    /// edges of `area` they anchor to (spec `Decisions (v1)`).
    const EDGE_MARGIN: u16 = 1;

    /// Duration of the slide transition between roster positions.
    const SLIDE_DUR: Duration = Duration::from_millis(300);

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
        // 2 cells wide × the row's 1-cell height = 4×4 dots per indicator —
        // enough resolution for a recognizable filled/unfilled circle.
        // Dividing row.width/N instead (the old approach) gave each slot far
        // more width than the aspect-fit circle could ever use (capped by
        // the 4-dot height regardless), scattering the 6 dots across the
        // full row instead of sitting compactly together.
        const SLOT_W: u16 = 2;
        let slot_w = SLOT_W.min(row.width.max(1));
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
            slide: None,
        }
    }

    /// Left/right arrow button rects beside the sprite for the current
    /// `area` — the sole place button positioning is computed; `render()`
    /// and tests both call this rather than re-deriving it. Both rects are
    /// vertically centered on the sprite band established by `layout()`.
    fn arrow_rects(area: Rect) -> (Rect, Rect) {
        let (sprite_rect, _, _) = Self::layout(area);
        let band = Rect::new(area.x, sprite_rect.y, area.width, sprite_rect.height);
        // `layout()` reserves exactly `ARROW_W` columns beside the sprite on
        // each side, flush against it. Insetting a full-`ARROW_W`-wide button
        // from `area`'s edge by `EDGE_MARGIN` would push its far edge past
        // that reservation and into the sprite band, so the button width is
        // shrunk by `EDGE_MARGIN`: its far edge (against the sprite) stays
        // exactly where the reserved space ends, while its near edge (against
        // `area`'s edge) moves inward by the margin.
        let size = (
            Self::ARROW_W.saturating_sub(Self::EDGE_MARGIN),
            Self::ARROW_H,
        );
        let left_rect = render::anchor_with_margin(
            band,
            size,
            render::Anchor::CenterLeft,
            (Self::EDGE_MARGIN, 0),
        );
        let right_rect = render::anchor_with_margin(
            band,
            size,
            render::Anchor::CenterRight,
            (Self::EDGE_MARGIN, 0),
        );
        (left_rect, right_rect)
    }

    /// Top-right rect for the home button — sole place its position is
    /// computed; `render()` and tests both call this.
    fn home_rect(area: Rect) -> Rect {
        render::anchor_with_margin(
            area,
            (Self::HOME_W, Self::HOME_H),
            render::Anchor::TopRight,
            (Self::EDGE_MARGIN, Self::EDGE_MARGIN),
        )
    }

    /// Advances/retreats `current_index` with wraparound. The sole place
    /// carousel index arithmetic lives — mouse (b4-t2) and slide-direction
    /// (b5-t1) paths must call this, never re-derive `(idx±1)%n`. A nav fired
    /// while a slide is already active is ignored until it settles (b5-t1's
    /// SCOPE_QUESTION default).
    fn navigate(&mut self, dir: Direction) {
        if self.active_slide().is_some() {
            return;
        }
        let prev_index = self.current_index;
        let n = self.creatures.len();
        self.current_index = match dir {
            Direction::Right => (self.current_index + 1) % n,
            Direction::Left => (self.current_index + n - 1) % n,
        };
        self.slide = Some(Slide {
            prev_index,
            dir,
            start: self.elapsed,
        });
    }

    /// The currently in-flight slide, if any — `None` once `elapsed` has
    /// reached/exceeded `SLIDE_DUR` past the slide's `start`, regardless of
    /// whether `update()` has run the settle cleanup yet.
    fn active_slide(&self) -> Option<&Slide> {
        self.slide
            .as_ref()
            .filter(|s| self.elapsed.saturating_sub(s.start) < Self::SLIDE_DUR)
    }

    /// Column offsets `(outgoing, incoming)` for an active slide `s` at the
    /// current `elapsed`, eased via `render::tween::Tween`/`ease_in_out`.
    /// A right-nav exits the outgoing group LEFT and enters the incoming
    /// group from the RIGHT; a left-nav is the mirror.
    fn slide_offsets(&self, area: Rect, s: &Slide) -> (i32, i32) {
        let progress = self.elapsed.saturating_sub(s.start);
        let w = area.width as f32;
        let (out_t, in_t) = match s.dir {
            Direction::Right => (
                Tween::new(0.0, -w, Self::SLIDE_DUR).at(progress),
                Tween::new(w, 0.0, Self::SLIDE_DUR).at(progress),
            ),
            Direction::Left => (
                Tween::new(0.0, w, Self::SLIDE_DUR).at(progress),
                Tween::new(-w, 0.0, Self::SLIDE_DUR).at(progress),
            ),
        };
        (out_t.round() as i32, in_t.round() as i32)
    }

    /// Renders the creature at `index`'s group (sprite + name + dot row) into
    /// a throwaway zero-origin buffer sized like `area`, then blits every
    /// non-space cell into `buf` shifted by `col_offset` columns — a true
    /// screen-space translation that works with `Rect`'s unsigned `x`.
    fn render_group(&self, buf: &mut Buffer, area: Rect, index: usize, col_offset: i32) {
        let zero_area = Rect::new(0, 0, area.width, area.height);
        let mut tmp = Buffer::empty(zero_area);
        let (sprite_rect, name_rect, dots_rect) = Self::layout(zero_area);

        let creature = &self.creatures[index];
        if let Some(sprite) = creature.animation(render::AnimationKind::Idle) {
            let (cols, rows) = render::convert::fit_dot_dims(sprite.frame_at(self.elapsed), sprite_rect);
            if cols > 0 && rows > 0 {
                let buf = sprite.dots_at(self.elapsed, cols * 2, rows * 4);
                let grid = render::dots::dots_to_grid(&buf);
                render::draw_grid(&mut tmp, sprite_rect, &grid);
            }
        }
        // White — reads against the scene's dark/transparent background
        // (there's no light panel behind this label the way FrameButton has).
        render::label(
            &mut tmp,
            name_rect,
            creature.name(),
            scene_core::color::Rgba::rgb(0xff, 0xff, 0xff),
        );

        for (i, slot) in Self::dot_slots(dots_rect).iter().enumerate() {
            let bytes = if i == index {
                render::assets::DOT_FILLED
            } else {
                render::assets::DOT_UNFILLED
            };
            render::draw_asset(&mut tmp, *slot, bytes);
        }

        for y in 0..area.height {
            for x in 0..area.width {
                let cell = match tmp.cell((x, y)) {
                    Some(c) => c,
                    None => continue,
                };
                if cell.symbol() == " " {
                    continue;
                }
                let dest_x = area.x as i32 + x as i32 + col_offset;
                if dest_x < area.left() as i32 || dest_x >= area.right() as i32 {
                    continue;
                }
                let dest_y = area.y + y;
                if let Some(dest_cell) = buf.cell_mut((dest_x as u16, dest_y)) {
                    *dest_cell = cell.clone();
                }
            }
        }
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
        if let Some(s) = &self.slide {
            if self.elapsed.saturating_sub(s.start) >= Self::SLIDE_DUR {
                self.slide = None;
            }
        }
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        if let Some(slide) = self.active_slide() {
            let (out_off, in_off) = self.slide_offsets(area, slide);
            self.render_group(frame.buffer_mut(), area, slide.prev_index, out_off);
            self.render_group(frame.buffer_mut(), area, self.current_index, in_off);
        } else {
            self.render_group(frame.buffer_mut(), area, self.current_index, 0);
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
    use crate::scenes::test_util::{render_to_buffer, row_containing};

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
    use crate::scenes::test_util::render_to_buffer;
    use ratatui::style::Color;

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
    use crate::scenes::test_util::key_event;
    use crossterm::event::KeyCode;

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
    use crate::scenes::test_util::{has_non_space, mouse_event, render_to_buffer};
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};

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
        assert_eq!(
            left_rect.x,
            area.x + RosterManager::EDGE_MARGIN,
            "left arrow button rect must be inset from area's left edge by EDGE_MARGIN"
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
        assert_eq!(
            right_rect.right(),
            area.right() - RosterManager::EDGE_MARGIN,
            "right arrow button rect must be inset from area's right edge by EDGE_MARGIN"
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
    use crate::scenes::test_util::{has_non_space, mouse_event, render_to_buffer};
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};

    /// `render()` paints the home button, and its rect sits top-right of
    /// `area` — inset from the top edge and the right edge by
    /// `RosterManager::EDGE_MARGIN` cells (no longer flush) — distinct from
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
        assert_eq!(
            rect.right(),
            area.right() - RosterManager::EDGE_MARGIN,
            "home button rect must be inset from the right edge of area by EDGE_MARGIN"
        );
        assert_eq!(
            rect.top(),
            area.top() + RosterManager::EDGE_MARGIN,
            "home button rect must be inset from the top edge of area by EDGE_MARGIN"
        );
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

/// Slide transition (b5-t1): navigating slides the outgoing creature's
/// group off-screen in the direction of travel while the incoming
/// creature's group slides in from the opposite edge, eased via
/// `render::tween`. Timings below assume the blueprint's documented
/// `SLIDE_DUR = 300ms` (research.md b5-t1): 75ms/225ms/425ms total elapsed
/// land at ~25%/~75%/past-100% progress.
#[cfg(test)]
mod slide_transition_tests {
    use super::*;
    use crate::scene::EngineCtx;
    use crate::scenes::test_util::{key_event, render_to_buffer, row_containing};
    use crossterm::event::KeyCode;
    use ratatui::buffer::Buffer;

    /// Leftmost non-space column in row `y`, if any.
    fn leftmost_non_space_in_row(buf: &Buffer, w: u16, y: u16) -> Option<u16> {
        (0..w).find(|&x| buf.cell((x, y)).unwrap().symbol() != " ")
    }

    /// A right-nav slides the outgoing creature's group out to the left and
    /// the incoming creature's group in from the right, eased over time, and
    /// settles with only the incoming creature painted at its resting column.
    #[test]
    fn right_nav_slide_animates_and_settles() {
        let (w, h) = (80u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let (_, name_rect, _) = RosterManager::layout(area);
        let name_y = name_rect.y;

        // Resting (no-slide) column of each creature, rendered standalone.
        let out_rest_left = {
            let baseline = RosterManager::new(); // index 0: Ember Wolf
            let buf = render_to_buffer(&baseline, w, h);
            leftmost_non_space_in_row(&buf, w, name_y)
                .expect("Ember Wolf must paint the name row at rest")
        };
        let in_rest_left = {
            let mut baseline = RosterManager::new();
            baseline.current_index = 1; // Frost Lizard
            let buf = render_to_buffer(&baseline, w, h);
            leftmost_non_space_in_row(&buf, w, name_y)
                .expect("Frost Lizard must paint the name row at rest")
        };

        let mut ctx = EngineCtx;
        let mut scene = RosterManager::new();
        let t = scene.handle_input(key_event(KeyCode::Right));
        assert!(t.is_none(), "arrow keys must not produce a Transition");
        assert_eq!(
            scene.current_index, 1,
            "current_index must update immediately on nav (b4 contract), even though a slide starts"
        );

        // Instant of trigger (no update yet): outgoing still at rest,
        // incoming fully off the right edge (not painted).
        let buf0 = render_to_buffer(&scene, w, h);
        assert_eq!(
            leftmost_non_space_in_row(&buf0, w, name_y),
            Some(out_rest_left),
            "immediately after nav, the outgoing creature (Ember Wolf) must still be painted at its resting column"
        );
        assert!(
            row_containing(&buf0, w, h, "Frost Lizard").is_none(),
            "immediately after nav, the incoming creature must be fully off the right edge (not painted yet)"
        );

        // ~25% progress: outgoing has slid measurably left of rest.
        scene.update(&mut ctx, Duration::from_millis(75));
        let buf1 = render_to_buffer(&scene, w, h);
        let out_mid_left = leftmost_non_space_in_row(&buf1, w, name_y)
            .expect("outgoing creature must still be partially on-screen at ~25% progress");
        assert!(
            out_mid_left < out_rest_left,
            "outgoing creature's painted column ({out_mid_left}) must have moved left of its resting column ({out_rest_left})"
        );

        // ~75% progress: incoming has slid in from the right, not yet settled.
        scene.update(&mut ctx, Duration::from_millis(150)); // total elapsed 225ms
        let buf2 = render_to_buffer(&scene, w, h);
        let in_mid_left = leftmost_non_space_in_row(&buf2, w, name_y)
            .expect("incoming creature must be partially visible at ~75% progress");
        assert!(
            in_mid_left > in_rest_left,
            "incoming creature's painted column ({in_mid_left}) must still be offset right of its resting column ({in_rest_left}) mid-transition"
        );

        // Past the slide duration: only the incoming creature remains, at rest.
        scene.update(&mut ctx, Duration::from_millis(200)); // total elapsed 425ms
        let buf3 = render_to_buffer(&scene, w, h);
        assert_eq!(
            leftmost_non_space_in_row(&buf3, w, name_y),
            Some(in_rest_left),
            "once settled, the incoming creature must render at the exact resting column b3-t2 established"
        );
        assert!(
            row_containing(&buf3, w, h, "Ember Wolf").is_none(),
            "once settled, the outgoing creature must no longer be painted anywhere"
        );
    }

    /// Mirror of the right-nav case: a left-nav slides the outgoing creature
    /// out to the right and the incoming creature in from the left.
    #[test]
    fn left_nav_slide_animates_and_settles() {
        let (w, h) = (80u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let (_, name_rect, _) = RosterManager::layout(area);
        let name_y = name_rect.y;

        let out_rest_left = {
            let baseline = RosterManager::new(); // index 0: Ember Wolf
            let buf = render_to_buffer(&baseline, w, h);
            leftmost_non_space_in_row(&buf, w, name_y)
                .expect("Ember Wolf must paint the name row at rest")
        };
        let in_rest_left = {
            let mut baseline = RosterManager::new();
            baseline.current_index = 5; // Shadow Cat (left-wrap from 0)
            let buf = render_to_buffer(&baseline, w, h);
            leftmost_non_space_in_row(&buf, w, name_y)
                .expect("Shadow Cat must paint the name row at rest")
        };

        let mut ctx = EngineCtx;
        let mut scene = RosterManager::new();
        let t = scene.handle_input(key_event(KeyCode::Left));
        assert!(t.is_none(), "arrow keys must not produce a Transition");
        assert_eq!(
            scene.current_index, 5,
            "left nav from index 0 must wrap current_index to 5 immediately, even though a slide starts"
        );

        let buf0 = render_to_buffer(&scene, w, h);
        assert_eq!(
            leftmost_non_space_in_row(&buf0, w, name_y),
            Some(out_rest_left),
            "immediately after nav, the outgoing creature (Ember Wolf) must still be painted at its resting column"
        );
        assert!(
            row_containing(&buf0, w, h, "Shadow Cat").is_none(),
            "immediately after nav, the incoming creature must be fully off the left edge (not painted yet)"
        );

        scene.update(&mut ctx, Duration::from_millis(75));
        let buf1 = render_to_buffer(&scene, w, h);
        let out_mid_left = leftmost_non_space_in_row(&buf1, w, name_y)
            .expect("outgoing creature must still be partially on-screen at ~25% progress");
        assert!(
            out_mid_left > out_rest_left,
            "outgoing creature's painted column ({out_mid_left}) must have moved right of its resting column ({out_rest_left}) for a left-nav exit"
        );

        scene.update(&mut ctx, Duration::from_millis(150)); // total elapsed 225ms
        let buf2 = render_to_buffer(&scene, w, h);
        let in_mid_left = leftmost_non_space_in_row(&buf2, w, name_y)
            .expect("incoming creature must be partially visible at ~75% progress");
        assert!(
            in_mid_left < in_rest_left,
            "incoming creature's painted column ({in_mid_left}) must still be offset left of its resting column ({in_rest_left}) mid-transition"
        );

        scene.update(&mut ctx, Duration::from_millis(200)); // total elapsed 425ms
        let buf3 = render_to_buffer(&scene, w, h);
        assert_eq!(
            leftmost_non_space_in_row(&buf3, w, name_y),
            Some(in_rest_left),
            "once settled, the incoming creature must render at the exact resting column b3-t2 established"
        );
        assert!(
            row_containing(&buf3, w, h, "Ember Wolf").is_none(),
            "once settled, the outgoing creature must no longer be painted anywhere"
        );
    }

    /// A second nav fired before the first nav's slide has settled must be
    /// ignored (research.md's SCOPE_QUESTION default) — `current_index`
    /// reflects only the first nav.
    #[test]
    fn nav_ignored_during_active_slide() {
        let mut ctx = EngineCtx;
        let mut scene = RosterManager::new();
        scene.handle_input(key_event(KeyCode::Right));
        assert_eq!(scene.current_index, 1, "first nav must update current_index immediately");

        // Still well within the slide's transition window.
        scene.update(&mut ctx, Duration::from_millis(50));

        scene.handle_input(key_event(KeyCode::Right));
        assert_eq!(
            scene.current_index, 1,
            "a nav fired while a slide transition is active must be ignored until it settles"
        );
    }
}
