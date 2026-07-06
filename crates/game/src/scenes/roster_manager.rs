use std::cell::RefCell;
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::Frame;
use ratatui::layout::Rect;
use engine_render::tween::Tween;
use engine_core::Inspectable;
use engine_core::SceneKey;
use serde_json::Value as JsonValue;

use engine_core::scene::{EngineCtx, InputEvent, Scene, Transition};
use crate::scene_id::SceneId;

#[derive(Inspectable)]
pub struct RosterManager {
    current_index: usize,
    #[inspect(hidden)]
    creatures: Vec<crate::creatures::Creature>,
    #[inspect(hidden)]
    elapsed: Duration,
    /// Mouse-driven navigation buttons beside the sprite (b4-t2). `RefCell`
    /// because `render(&self, ..)` must mutate their rect/state from an
    /// immutable receiver (see research.md b4-t2 blueprint point 1).
    #[inspect(hidden)]
    left_button: RefCell<engine_render::Button>,
    #[inspect(hidden)]
    right_button: RefCell<engine_render::Button>,
    /// Top-right button that transitions back to `MainHub` (b4-t3). `RefCell`
    /// for the same immutable-render-mutates-button-state reason as the
    /// arrows.
    #[inspect(hidden)]
    home_button: RefCell<engine_render::Button>,
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

/// The 7 named panel rects `layout()` splits `area` into (b1-t3,
/// research.md). `name`/`sprite`/`dot_row` are the pre-existing bands;
/// `level`/`stat_bar`/`exhaustion`/`ability_list` are new — b2 tasks render
/// into them. Only `sprite` is offset during a slide; every other rect is
/// drawn statically at the resting column regardless of `col_offset`.
#[derive(Clone, Copy, Debug)]
struct RosterLayout {
    name: Rect,
    // Rendered starting b2-t2..t5 (level bar, stat bar, exhaustion meter,
    // ability list); unread this task by design — keep the fields, not the
    // lint suppression, once those tasks land.
    #[allow(dead_code)]
    level: Rect,
    #[allow(dead_code)]
    stat_bar: Rect,
    #[allow(dead_code)]
    exhaustion: Rect,
    #[allow(dead_code)]
    ability_list: Rect,
    sprite: Rect,
    dot_row: Rect,
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

    /// Height (in rows) of the `name` band at the top of the frame.
    const NAME_H: u16 = 3;
    /// Height of the `level` band directly below `name`.
    const LEVEL_H: u16 = 1;
    /// Height of the `stat_bar`/`exhaustion` panel row below `level`.
    const PANEL_H: u16 = 5;
    /// Height of the `dot_row` band at the bottom of the frame.
    const DOT_H: u16 = 2;

    /// Splits `area` into the 7 named panel rects (b1-t3, research.md
    /// blueprint), top to bottom: `name`, `level`, the `stat_bar`/
    /// `exhaustion` panel row (split left/right), `sprite` (center band —
    /// the only region that still slides), `dot_row`. `ability_list` sits
    /// below `exhaustion`, sharing its column. Uses saturating arithmetic
    /// throughout so small `area`s degrade to zero-height rects instead of
    /// panicking.
    fn layout(area: Rect) -> RosterLayout {
        let name_h = Self::NAME_H.min(area.height);
        let level_h = Self::LEVEL_H.min(area.height.saturating_sub(name_h));
        let panel_h = Self::PANEL_H.min(area.height.saturating_sub(name_h + level_h));
        let dot_h = Self::DOT_H.min(area.height.saturating_sub(name_h + level_h + panel_h));
        let sprite_h = area
            .height
            .saturating_sub(name_h + level_h + panel_h + dot_h);

        let name = Rect::new(area.x, area.y, area.width, name_h);
        let level = Rect::new(area.x, area.y + name_h, area.width, level_h);

        let panel_y = area.y + name_h + level_h;
        let stat_bar_w = area.width / 2;
        let stat_bar = Rect::new(area.x, panel_y, stat_bar_w, panel_h);
        let exhaustion = Rect::new(
            area.x + stat_bar_w,
            panel_y,
            area.width.saturating_sub(stat_bar_w),
            panel_h,
        );

        let sprite_y = panel_y + panel_h;
        let sprite = Rect::new(
            area.x + Self::ARROW_W,
            sprite_y,
            area.width.saturating_sub(2 * Self::ARROW_W),
            sprite_h,
        );

        let ability_list = Rect::new(
            exhaustion.x,
            exhaustion.y + panel_h,
            exhaustion.width,
            sprite_h,
        );

        let dot_row = Rect::new(area.x, sprite_y + sprite_h, area.width, dot_h);

        RosterLayout {
            name,
            level,
            stat_bar,
            exhaustion,
            ability_list,
            sprite,
            dot_row,
        }
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
            creatures: crate::creatures::demo_roster(),
            elapsed: Duration::ZERO,
            left_button: RefCell::new(engine_render::Button::new(
                Rect::default(),
                crate::assets::BUTTON_PANEL,
                crate::assets::ICON_ARROW_LEFT,
            )),
            right_button: RefCell::new(engine_render::Button::new(
                Rect::default(),
                crate::assets::BUTTON_PANEL,
                crate::assets::ICON_ARROW_RIGHT,
            )),
            home_button: RefCell::new(engine_render::Button::new(
                Rect::default(),
                crate::assets::BUTTON_PANEL,
                crate::assets::ICON_HOME,
            )),
            slide: None,
        }
    }

    /// Left/right arrow button rects beside the sprite for the current
    /// `area` — the sole place button positioning is computed; `render()`
    /// and tests both call this rather than re-deriving it. Both rects are
    /// vertically centered on the sprite band established by `layout()`.
    fn arrow_rects(area: Rect) -> (Rect, Rect) {
        let sprite_rect = Self::layout(area).sprite;
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
        let left_rect = engine_render::anchor_with_margin(
            band,
            size,
            engine_render::Anchor::CenterLeft,
            (Self::EDGE_MARGIN, 0),
        );
        let right_rect = engine_render::anchor_with_margin(
            band,
            size,
            engine_render::Anchor::CenterRight,
            (Self::EDGE_MARGIN, 0),
        );
        (left_rect, right_rect)
    }

    /// Top-right rect for the home button — sole place its position is
    /// computed; `render()` and tests both call this.
    fn home_rect(area: Rect) -> Rect {
        engine_render::anchor_with_margin(
            area,
            (Self::HOME_W, Self::HOME_H),
            engine_render::Anchor::TopRight,
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
    /// current `elapsed`, eased via `engine_render::tween::Tween`/`ease_in_out`.
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

    /// Renders the creature at `index`'s SPRITE ONLY (b1-t3: name and dot row
    /// are static panels drawn separately, never offset) into a throwaway
    /// zero-origin buffer sized like `area`, then blits every non-space cell
    /// into `buf` shifted by `col_offset` columns — a true screen-space
    /// translation that works with `Rect`'s unsigned `x`.
    fn render_sprite(&self, buf: &mut Buffer, area: Rect, index: usize, col_offset: i32) {
        let zero_area = Rect::new(0, 0, area.width, area.height);
        let mut tmp = Buffer::empty(zero_area);
        let sprite_rect = Self::layout(zero_area).sprite;

        let creature = &self.creatures[index];
        if let Some(sprite) = creature.animation(crate::creatures::AnimationKind::Idle) {
            let (cols, rows) = engine_render::convert::fit_dot_dims(sprite.frame_at(self.elapsed), sprite_rect);
            if cols > 0 && rows > 0 {
                let buf = sprite.dots_at(self.elapsed, cols * 2, rows * 4);
                let grid = engine_render::dots::dots_to_grid(&buf);
                engine_render::draw_grid(&mut tmp, sprite_rect, &grid);
            }
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

    /// Draws `creatures[index]`'s name statically into `name_rect` — no
    /// `col_offset`, so it never travels with an in-flight sprite slide
    /// (b1-t3: name updates immediately with `current_index` regardless of
    /// slide state).
    fn render_name(&self, buf: &mut Buffer, name_rect: Rect, index: usize) {
        let creature = &self.creatures[index];
        // White — reads against the scene's dark/transparent background
        // (there's no light panel behind this label the way FrameButton has).
        engine_render::label(
            buf,
            name_rect,
            creature.name(),
            engine_core::color::Rgba::rgb(0xff, 0xff, 0xff),
        );
    }

    /// Draws the 6-slot dot row statically into `dot_row_rect`, filled at
    /// `self.current_index` — no `col_offset`, so it never travels with an
    /// in-flight sprite slide (b1-t3).
    fn render_dot_row(&self, buf: &mut Buffer, dot_row_rect: Rect) {
        for (i, slot) in Self::dot_slots(dot_row_rect).iter().enumerate() {
            let bytes = if i == self.current_index {
                crate::assets::DOT_FILLED
            } else {
                crate::assets::DOT_UNFILLED
            };
            let grid = engine_render::asset_cache::convert(bytes, *slot);
            engine_render::draw_grid(buf, *slot, &grid);
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
    fn id(&self) -> SceneKey {
        SceneId::RosterManager.into()
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
        let l = Self::layout(area);

        if let Some(slide) = self.active_slide() {
            let (out_off, in_off) = self.slide_offsets(area, slide);
            self.render_sprite(frame.buffer_mut(), area, slide.prev_index, out_off);
            self.render_sprite(frame.buffer_mut(), area, self.current_index, in_off);
        } else {
            self.render_sprite(frame.buffer_mut(), area, self.current_index, 0);
        }
        // Static panels — no col_offset, so they never travel with an
        // in-flight sprite slide (b1-t3).
        self.render_name(frame.buffer_mut(), l.name, self.current_index);
        self.render_dot_row(frame.buffer_mut(), l.dot_row);

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
                        target: SceneId::MainHub.into(),
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

    fn inspect(&mut self) -> &mut dyn engine_core::Inspectable {
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
            "RosterManager::new() must seed all 6 creatures from crate::creatures::demo_roster()"
        );
        assert_eq!(rm.creatures[0].name(), "Ember Wolf");
        assert_eq!(rm.current_index, 0);
        assert_eq!(rm.elapsed, Duration::ZERO);
    }

    /// `RosterManager::new()` must source its roster from
    /// `crate::creatures::demo_roster()`, not `crate::creatures::all()` — the
    /// per-creature RPG fields (stats/level/abilities/exhaustion) must match
    /// `demo_roster()` element-for-element, not `Creature::new`'s defaults
    /// (level 1, `Stats::default()`, empty abilities).
    #[test]
    fn new_sources_rpg_fields_from_demo_roster() {
        let rm = RosterManager::new();
        let demo = crate::creatures::demo_roster();

        for i in [0usize, 2usize] {
            assert_eq!(
                rm.creatures[i].level(),
                demo[i].level(),
                "creature {i} level must match demo_roster()"
            );
            assert_eq!(
                rm.creatures[i].stats(),
                demo[i].stats(),
                "creature {i} stats must match demo_roster()"
            );
            assert_eq!(
                rm.creatures[i].abilities(),
                demo[i].abilities(),
                "creature {i} abilities must match demo_roster()"
            );
            assert_eq!(
                rm.creatures[i].exhaustion(),
                demo[i].exhaustion(),
                "creature {i} exhaustion must match demo_roster()"
            );
        }

        // Guard against a missed swap: `all()`'s defaults would leave Ember
        // Wolf at level 1, which demo_roster() overrides to level 5. This is
        // the assertion that actually fails if `new()` still calls `all()`.
        assert_ne!(
            rm.creatures[0].level(),
            1,
            "RosterManager::new() must use demo_roster(), not all() (which defaults to level 1)"
        );
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

/// b1-t3: `layout()`'s expanded 7-rect contract — the shared layout every
/// b2 rendering task renders into.
#[cfg(test)]
mod layout_tests {
    use super::*;

    /// `layout(area)` must order its bands top-to-bottom: name < level <
    /// stat_bar/exhaustion row < sprite < dot_row, with `exhaustion` above
    /// `ability_list`, and `stat_bar`/`exhaustion` must not horizontally
    /// overlap (they share the same row, split left/right).
    #[test]
    fn layout_rects_ordered_top_to_bottom() {
        let area = Rect::new(0, 0, 80, 30);
        let l = RosterManager::layout(area);

        assert!(l.name.y < l.level.y, "name.y ({}) must be above level.y ({})", l.name.y, l.level.y);
        assert!(l.level.y < l.stat_bar.y, "level.y ({}) must be above stat_bar.y ({})", l.level.y, l.stat_bar.y);
        assert_eq!(l.stat_bar.y, l.exhaustion.y, "stat_bar and exhaustion must share the same row (y={} vs y={})", l.stat_bar.y, l.exhaustion.y);
        assert!(l.exhaustion.y < l.ability_list.y, "exhaustion.y ({}) must be above ability_list.y ({})", l.exhaustion.y, l.ability_list.y);
        assert!(l.stat_bar.y < l.sprite.y, "stat_bar/exhaustion row ({}) must be above sprite.y ({})", l.stat_bar.y, l.sprite.y);
        assert!(l.sprite.y < l.dot_row.y, "sprite.y ({}) must be above dot_row.y ({})", l.sprite.y, l.dot_row.y);

        assert!(
            l.stat_bar.right() <= l.exhaustion.left() || l.exhaustion.right() <= l.stat_bar.left(),
            "stat_bar ({:?}) and exhaustion ({:?}) must not horizontally overlap",
            l.stat_bar, l.exhaustion
        );
    }
}

#[cfg(test)]
mod sprite_and_name_render_tests {
    use super::*;
    use engine_core::scene::EngineCtx;
    use crate::scenes::test_util::{render_to_buffer, row_containing};

    /// A fresh `RosterManager::new()` (current_index == 0) renders the "Ember
    /// Wolf" name row at the TOP of the frame (b1-t3 layout inversion from
    /// `24`, where the name sat below the sprite), with the sprite painting
    /// non-space cells inside `layout().sprite` — the band below the name.
    #[test]
    fn renders_index0_name_top_and_sprite_below() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let sprite_rect = RosterManager::layout(area).sprite;

        let name_row = row_containing(&buf, w, h, "Ember Wolf")
            .expect("render must place the current creature's name ('Ember Wolf') somewhere on screen");
        assert!(
            name_row < sprite_rect.y,
            "name row ({name_row}) must be above the sprite rect (starting at y={}) — name sits at the TOP band per b1-t3",
            sprite_rect.y
        );

        let sprite_has_non_space = (sprite_rect.top()..sprite_rect.bottom()).any(|y| {
            (0..w).any(|x| buf.cell((x, y)).unwrap().symbol() != " ")
        });
        assert!(
            sprite_has_non_space,
            "render must paint at least one non-space cell inside the sprite rect"
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
        let all = crate::creatures::all();

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
        let dots_rect = RosterManager::layout(area).dot_row;
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
        let dots_rect = RosterManager::layout(area).dot_row;
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
        let sprite_rect = RosterManager::layout(area).sprite;

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
        let sprite_rect = RosterManager::layout(area).sprite;

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
        assert_eq!(t.target, SceneKey::from(SceneId::MainHub), "home button must transition to MainHub");
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
/// `engine_render::tween`. Timings below assume the blueprint's documented
/// `SLIDE_DUR = 300ms` (research.md b5-t1): 75ms/225ms/425ms total elapsed
/// land at ~25%/~75%/past-100% progress.
#[cfg(test)]
mod slide_transition_tests {
    use super::*;
    use engine_core::scene::EngineCtx;
    use crate::scenes::test_util::{key_event, render_to_buffer};
    use crossterm::event::KeyCode;
    use ratatui::buffer::Buffer;

    /// Leftmost non-space column across every row of `rect`, if any. Used to
    /// track the SPRITE region's slide position (b1-t3: only the sprite is
    /// offset during a slide; name/dot-row are static and no longer a valid
    /// signal for slide progress).
    fn leftmost_non_space_in_rect(buf: &Buffer, rect: Rect) -> Option<u16> {
        (rect.left()..rect.right()).find(|&x| {
            (rect.top()..rect.bottom()).any(|y| buf.cell((x, y)).unwrap().symbol() != " ")
        })
    }

    /// A right-nav slides the outgoing creature's SPRITE out to the left and
    /// the incoming creature's SPRITE in from the right, eased over time, and
    /// settles with only the incoming creature's sprite painted at its
    /// resting column. Per b1-t3, name/dot-row no longer travel with the
    /// slide (they update immediately with `current_index`, unchanged
    /// columns throughout) — only the sprite region is exercised here.
    #[test]
    fn right_nav_slide_animates_and_settles() {
        let (w, h) = (80u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let sprite_rect = RosterManager::layout(area).sprite;

        // Resting (no-slide) column of each creature's sprite, rendered
        // standalone.
        let out_rest_left = {
            let baseline = RosterManager::new(); // index 0: Ember Wolf
            let buf = render_to_buffer(&baseline, w, h);
            leftmost_non_space_in_rect(&buf, sprite_rect)
                .expect("Ember Wolf's sprite must paint at rest")
        };
        let in_rest_left = {
            let mut baseline = RosterManager::new();
            baseline.current_index = 1; // Frost Lizard
            let buf = render_to_buffer(&baseline, w, h);
            leftmost_non_space_in_rect(&buf, sprite_rect)
                .expect("Frost Lizard's sprite must paint at rest")
        };

        let mut ctx = EngineCtx;
        let mut scene = RosterManager::new();
        let t = scene.handle_input(key_event(KeyCode::Right));
        assert!(t.is_none(), "arrow keys must not produce a Transition");
        assert_eq!(
            scene.current_index, 1,
            "current_index must update immediately on nav (b4 contract), even though a slide starts"
        );

        // Instant of trigger (no update yet): outgoing sprite still at rest.
        let buf0 = render_to_buffer(&scene, w, h);
        assert_eq!(
            leftmost_non_space_in_rect(&buf0, sprite_rect),
            Some(out_rest_left),
            "immediately after nav, the outgoing creature's sprite (Ember Wolf) must still be painted at its resting column"
        );

        // ~25% progress: outgoing sprite has slid measurably left of rest.
        scene.update(&mut ctx, Duration::from_millis(75));
        let buf1 = render_to_buffer(&scene, w, h);
        let out_mid_left = leftmost_non_space_in_rect(&buf1, sprite_rect)
            .expect("outgoing sprite must still be partially on-screen at ~25% progress");
        assert!(
            out_mid_left < out_rest_left,
            "outgoing sprite's painted column ({out_mid_left}) must have moved left of its resting column ({out_rest_left})"
        );

        // ~75% progress: incoming sprite has slid in from the right, not yet settled.
        scene.update(&mut ctx, Duration::from_millis(150)); // total elapsed 225ms
        let buf2 = render_to_buffer(&scene, w, h);
        let in_mid_left = leftmost_non_space_in_rect(&buf2, sprite_rect)
            .expect("incoming sprite must be partially visible at ~75% progress");
        assert!(
            in_mid_left > in_rest_left,
            "incoming sprite's painted column ({in_mid_left}) must still be offset right of its resting column ({in_rest_left}) mid-transition"
        );

        // Past the slide duration: only the incoming sprite remains, at rest.
        scene.update(&mut ctx, Duration::from_millis(200)); // total elapsed 425ms
        let buf3 = render_to_buffer(&scene, w, h);
        assert_eq!(
            leftmost_non_space_in_rect(&buf3, sprite_rect),
            Some(in_rest_left),
            "once settled, the incoming sprite must render at its exact resting column"
        );
    }

    /// Mirror of the right-nav case: a left-nav slides the outgoing
    /// creature's sprite out to the right and the incoming creature's sprite
    /// in from the left.
    #[test]
    fn left_nav_slide_animates_and_settles() {
        let (w, h) = (80u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let sprite_rect = RosterManager::layout(area).sprite;

        let out_rest_left = {
            let baseline = RosterManager::new(); // index 0: Ember Wolf
            let buf = render_to_buffer(&baseline, w, h);
            leftmost_non_space_in_rect(&buf, sprite_rect)
                .expect("Ember Wolf's sprite must paint at rest")
        };
        let in_rest_left = {
            let mut baseline = RosterManager::new();
            baseline.current_index = 5; // Shadow Cat (left-wrap from 0)
            let buf = render_to_buffer(&baseline, w, h);
            leftmost_non_space_in_rect(&buf, sprite_rect)
                .expect("Shadow Cat's sprite must paint at rest")
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
            leftmost_non_space_in_rect(&buf0, sprite_rect),
            Some(out_rest_left),
            "immediately after nav, the outgoing creature's sprite (Ember Wolf) must still be painted at its resting column"
        );

        scene.update(&mut ctx, Duration::from_millis(75));
        let buf1 = render_to_buffer(&scene, w, h);
        let out_mid_left = leftmost_non_space_in_rect(&buf1, sprite_rect)
            .expect("outgoing sprite must still be partially on-screen at ~25% progress");
        assert!(
            out_mid_left > out_rest_left,
            "outgoing sprite's painted column ({out_mid_left}) must have moved right of its resting column ({out_rest_left}) for a left-nav exit"
        );

        scene.update(&mut ctx, Duration::from_millis(150)); // total elapsed 225ms
        let buf2 = render_to_buffer(&scene, w, h);
        let in_mid_left = leftmost_non_space_in_rect(&buf2, sprite_rect)
            .expect("incoming sprite must be partially visible at ~75% progress");
        assert!(
            in_mid_left < in_rest_left,
            "incoming sprite's painted column ({in_mid_left}) must still be offset left of its resting column ({in_rest_left}) mid-transition"
        );

        scene.update(&mut ctx, Duration::from_millis(200)); // total elapsed 425ms
        let buf3 = render_to_buffer(&scene, w, h);
        assert_eq!(
            leftmost_non_space_in_rect(&buf3, sprite_rect),
            Some(in_rest_left),
            "once settled, the incoming sprite must render at its exact resting column"
        );
    }

    /// Per b1-t3: during an active slide, at the SAME `current_index`, the
    /// name rect and dot-row rect paint identical columns whether or not a
    /// slide is active — only the sprite region's painted columns differ
    /// (still slides). This is the shared layout contract every b2 rendering
    /// task depends on.
    #[test]
    fn name_and_dot_row_do_not_slide_but_sprite_does() {
        fn painted_columns(buf: &Buffer, rect: Rect) -> std::collections::BTreeSet<u16> {
            (rect.left()..rect.right())
                .filter(|&x| {
                    (rect.top()..rect.bottom()).any(|y| buf.cell((x, y)).unwrap().symbol() != " ")
                })
                .collect()
        }

        let (w, h) = (80u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let l = RosterManager::layout(area);
        let mut ctx = EngineCtx;

        // Mid-slide render: nav right from index 0 -> 1, sample at ~50%.
        let mut mid_slide_scene = RosterManager::new();
        mid_slide_scene.handle_input(key_event(KeyCode::Right));
        mid_slide_scene.update(&mut ctx, Duration::from_millis(150));
        let mid_slide_buf = render_to_buffer(&mid_slide_scene, w, h);
        assert_eq!(mid_slide_scene.current_index, 1, "slide must not change current_index a second time");

        // No-slide render at the SAME resting current_index (1).
        let mut rest_scene = RosterManager::new();
        rest_scene.current_index = 1;
        let rest_buf = render_to_buffer(&rest_scene, w, h);

        assert_eq!(
            painted_columns(&mid_slide_buf, l.name),
            painted_columns(&rest_buf, l.name),
            "name rect's painted columns must be identical during an active slide vs. at rest — name no longer travels with col_offset"
        );
        assert_eq!(
            painted_columns(&mid_slide_buf, l.dot_row),
            painted_columns(&rest_buf, l.dot_row),
            "dot-row rect's painted columns must be identical during an active slide vs. at rest — dots no longer travel with col_offset"
        );
        assert_ne!(
            painted_columns(&mid_slide_buf, l.sprite),
            painted_columns(&rest_buf, l.sprite),
            "sprite rect's painted columns must differ during an active slide vs. at rest — the sprite is the only region that still slides"
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
