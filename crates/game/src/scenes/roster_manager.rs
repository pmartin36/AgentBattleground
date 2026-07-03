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
}

impl RosterManager {
    /// Splits `area` into `(sprite_rect, name_rect, dots_rect)`: the bottom
    /// two rows are reserved for the name label and the 6-dot position
    /// indicator row, with the sprite occupying the remaining region above.
    /// Sprite centering is delegated to `draw_grid`; name centering to
    /// `label`; dot-row slotting to `dot_slots`.
    fn layout(area: Rect) -> (Rect, Rect, Rect) {
        let reserved = 2.min(area.height);
        let sprite_h = area.height - reserved;
        let sprite_rect = Rect::new(area.x, area.y, area.width, sprite_h);
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
        }
    }
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
    }

    fn handle_input(&mut self, _ev: InputEvent) -> Option<Transition> {
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
