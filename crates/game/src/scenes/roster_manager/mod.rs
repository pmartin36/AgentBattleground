use std::cell::RefCell;
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::Frame;
use ratatui::layout::Rect;
use engine_render::tween::Tween;
use engine_render::dots::{dots_to_grid, Dot, DotBuffer};
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
    /// The creature currently marked for a select-and-swap (b3-t1).
    /// `Some(current_index)` while blinking; `None` when nothing is
    /// selected.
    #[inspect(hidden)]
    selected_index: Option<usize>,
    /// Mouse-driven hit-test core for the CURRENT creature's dot slot
    /// (b3-t1) — reuses `engine_render::ButtonCore`'s completed-click state
    /// machine rather than hand-rolling one (see research.md's reuse
    /// check). `RefCell` for the same immutable-render-mutates-button-state
    /// reason as `left_button`/`right_button`/`home_button`.
    #[inspect(hidden)]
    current_dot: RefCell<engine_render::ButtonCore>,
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

/// The 7 named panel rects `layout()` splits `area` into (b1-t1,
/// research.md): `name` and `level` stacked tight at the top, a blank
/// `HEADER_GAP_H` row, then the body — a 2:1 LEFT/RIGHT column split with
/// `stat_bar` above `sprite` on the LEFT and `stamina` above
/// `ability_list` (the details panel) on the RIGHT — then `dot_row` at the
/// bottom. Only `sprite` is offset during a slide; every other rect is drawn
/// statically at the resting column regardless of `col_offset`.
#[derive(Clone, Copy, Debug)]
struct RosterLayout {
    name: Rect,
    level: Rect,
    sprite: Rect,
    dot_row: Rect,
}

impl RosterManager {
    const HOME_H: u16 = 3;

    /// Inset (in whole terminal cells) of the home/arrow buttons from the
    /// edges of `area` they anchor to (spec `Decisions (v1)`).
    const EDGE_MARGIN: u16 = 1;

    /// Extra rightward inset (in cells) of the details panel beyond
    /// `EDGE_MARGIN`, pulling the whole panel LEFT off the screen's right edge.
    /// One cell == 2 braille dots wide, so this shifts the panel left by 2
    /// dots (its width is unchanged — only `details_x` moves).
    const DETAILS_LEFT_SHIFT: u16 = 1;

    /// Duration of the slide transition between roster positions.
    const SLIDE_DUR: Duration = Duration::from_millis(300);

    /// Per-toggle interval of the selected dot's blink (b3-t1): the dot
    /// holds each of filled/unfilled for this long before flipping. A full
    /// blink cycle is 2x this. Not a spec-mandated value (spec doesn't state
    /// a period) — see research.md's ADVERSARIAL verdict.
    const BLINK_PERIOD: Duration = Duration::from_millis(400);

    /// Chrome color for procedural thin borders (details panel + stat-bar
    /// outlines, b1-t5/b1-t6). Not `FRAME_PANEL` — drawn via the dot
    /// pipeline (`draw_dot_border`, CLAUDE.md constraint 4).
    const BORDER_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0x88, 0x88, 0x88);

    /// Total height (in rows) of the `stat_bar` band: exactly one bar outline
    /// plus the label row directly below it — no gap row, no padding cell. The
    /// `sprite` band directly below fills all remaining vertical space down to
    /// its pinned baseline above `dot_row`.
    const STAT_BAR_BAND_H: u16 =
        Self::STAT_BAR_OUTLINE_H + Self::STAT_LABEL_H;
    /// Height (in cells) of each stat-bar OUTLINE. 3 cells = 12 dot rows: the
    /// green fill occupies exactly the MIDDLE cell (dot rows 4-7, see
    /// `stat_slice_parts`), and a rounded `STAT_BAR_HUG_CAP_DOTS`-thick grey
    /// cap sits directly above and below it — the top cell's bottom
    /// `STAT_BAR_HUG_CAP_DOTS` dots, and the bottom cell's top
    /// `STAT_BAR_HUG_CAP_DOTS` dots — with 1-dot left/right sides connecting
    /// them. Because the fill is confined to its own single cell and the caps
    /// live in the cells directly above/below it, no braille cell ever
    /// contains both a border dot and a fill dot, so the border always
    /// renders as a complete, crisp shape at any fill amount.
    const STAT_BAR_OUTLINE_H: u16 = 3;
    /// Height (in rows) of the label row at the bottom of each stat-bar
    /// slice (b1-t6).
    const STAT_LABEL_H: u16 = 1;
    /// Horizontal gap (in cells) between adjacent role clusters in the dot
    /// row (b2-t6, widened b1-t3). `5` is spec 38's explicit, final pin —
    /// not a range to tune. It guarantees the 3 role labels — each wider
    /// than its dot cluster — stay visibly separated by a real blank-column
    /// margin (>=2 columns) between every adjacent pair, at both 40-col and
    /// 80-col widths, without the widened label group overlapping b1-t2's
    /// flanking arrow buttons (which also occupy the `dot_row` band).
    const CLUSTER_GAP: u16 = 5;
    /// Role-label text colour (b2-t6) — white, matching
    /// `LEVEL_COLOR`/`STAMINA_COLOR`/`ABILITY_COLOR` chrome.
    const DOT_LABEL_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0xff, 0xff, 0xff);
    /// The 3 role clusters the dot row is grouped into, in roster-index
    /// order: `(slot count, label)`. Slot counts are derived FROM
    /// `crate::squad_role`'s constants — never a hardcoded 3/1/2 — so a
    /// change to the squad-role split automatically re-groups the dot row
    /// (b2-t6).
    const CLUSTERS: [(usize, &str); 3] = [
        (crate::squad_role::ACTIVE_SLOTS, "Active"),
        (crate::squad_role::BENCH_SLOTS, "Bench"),
        (crate::squad_role::RESERVE_SLOTS, "Reserve"),
    ];

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
            selected_index: None,
            current_dot: RefCell::new(engine_render::ButtonCore::new(Rect::default())),
        }
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

    /// Whether the selected dot should currently paint as "filled" for its
    /// blink (b3-t1) — alternates every `BLINK_PERIOD` of `elapsed`. Pure
    /// over `&self`, keyed off the shared `elapsed` clock (no separate
    /// per-selection timer field).
    fn blink_on(&self) -> bool {
        (self.elapsed.as_millis() / Self::BLINK_PERIOD.as_millis()).is_multiple_of(2)
    }

    /// The single Space/current-dot-click action (b3-t1): selects the
    /// current creature if nothing is selected, cancels the selection if
    /// the current creature is already selected, and is a no-op if a
    /// DIFFERENT creature is selected (navigated-away-then-selected — b3-t2's
    /// swap fills this branch in).
    fn toggle_selection(&mut self) {
        match self.selected_index {
            None => self.selected_index = Some(self.current_index),
            Some(sel) if sel == self.current_index => self.selected_index = None,
            Some(sel) => {
                self.creatures.swap(sel, self.current_index);
                self.selected_index = None;
            }
        }
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
        let (name_idx, name_color) = self.name_display();
        self.render_name(frame.buffer_mut(), l.name, name_idx, name_color);
        self.render_level(frame.buffer_mut(), l.level, self.current_index);
        self.render_stat_bars(frame.buffer_mut(), Self::left_col_dots(area)[0]);
        if self.active_slide().is_none() {
            let (border, sta_text, ability_text) = Self::details_panel_rects(area);
            Self::draw_dot_border(frame.buffer_mut(), border, Self::BORDER_COLOR);
            self.render_stamina(frame.buffer_mut(), sta_text, self.current_index);
            self.render_ability_list(frame.buffer_mut(), ability_text, self.current_index);
        }
        self.render_dot_row(frame.buffer_mut(), Self::top_bands_dots(area)[4]);

        let (left_dr, right_dr) = Self::arrow_dot_rects(area);
        {
            let mut left = self.left_button.borrow_mut();
            left.set_rect(left_dr.to_cell_rect());
            left.set_dot_offset_down(left_dr.cell_remainder().1);
            left.render(frame.buffer_mut());
        }
        {
            let mut right = self.right_button.borrow_mut();
            right.set_rect(right_dr.to_cell_rect());
            right.set_dot_offset_down(right_dr.cell_remainder().1);
            right.render(frame.buffer_mut());
        }

        let home_dr = Self::home_dot_rect(area);
        {
            let mut home = self.home_button.borrow_mut();
            home.set_rect(home_dr.to_cell_rect());
            home.set_dot_offset_down(home_dr.cell_remainder().1);
            home.render(frame.buffer_mut());
        }
    }

    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
        use crossterm::event::KeyCode;

        match ev {
            InputEvent::Key(key) => match key.code {
                KeyCode::Left => self.navigate(Direction::Left),
                KeyCode::Right => self.navigate(Direction::Right),
                KeyCode::Char(' ') => self.toggle_selection(),
                _ => {}
            },
            InputEvent::Mouse(me) => {
                let hit_left = self.left_button.get_mut().handle_mouse(&me);
                let hit_right = self.right_button.get_mut().handle_mouse(&me);
                let hit_home = self.home_button.get_mut().handle_mouse(&me);
                let hit_dot = self.current_dot.get_mut().handle_mouse(&me);
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
                if hit_dot {
                    self.toggle_selection();
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

mod borders;
mod chrome;
mod details_panel;
mod dot_row;
mod layout;
mod sprite_name;
mod stat_bar;

#[cfg(test)]
mod regression_tests;

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
    /// per-creature RPG fields (stats/level/abilities/stamina) must match
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
                rm.creatures[i].stamina(),
                demo[i].stamina(),
                "creature {i} stamina must match demo_roster()"
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

    /// The sprite band the slide tests measure. The arrow buttons flank the
    /// dot-cluster group and so now paint in the MIDDLE columns of the frame,
    /// but they live entirely within `dot_row`'s row range — disjoint from
    /// (directly below) the sprite's rows — so scanning `layout(area).sprite`
    /// never picks up an arrow glyph, and no column-narrowing is needed. (The
    /// details-panel border shares the sprite's rightmost column at rest, but
    /// the tests measure the LEFTMOST painted column, which is always sprite
    /// content, so that right-edge border cell never interferes.)
    fn sprite_measure_rect(area: Rect) -> Rect {
        RosterManager::layout(area).sprite
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
        let sprite_rect = sprite_measure_rect(area);

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
        let sprite_rect = sprite_measure_rect(area);

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

    /// Per b1-t3/b2-t1: during an active slide, at the SAME `current_index`,
    /// the name rect and dot-row rect paint identical COLUMNS whether or not
    /// a slide is active — only the sprite region's painted columns differ
    /// (still slides). This is the shared layout contract every b2 rendering
    /// task depends on. The name's fg COLOUR, however, DOES change during an
    /// active slide (b2-t1): it cross-fades toward the background and back,
    /// keyed off the same `Slide`/`elapsed` window (colour-only, position
    /// never moves). Sampled at 200ms (~67% progress) — unambiguously past
    /// the 150ms/50% prev/current cross-fade boundary, so `current_index`'s
    /// name is definitely the one shown, still mid-fade-in.
    #[test]
    fn name_and_dot_row_do_not_slide_but_sprite_does() {
        fn painted_columns(buf: &Buffer, rect: Rect) -> std::collections::BTreeSet<u16> {
            (rect.left()..rect.right())
                .filter(|&x| {
                    (rect.top()..rect.bottom()).any(|y| buf.cell((x, y)).unwrap().symbol() != " ")
                })
                .collect()
        }
        fn first_painted_fg(buf: &Buffer, rect: Rect) -> Option<ratatui::style::Color> {
            (rect.top()..rect.bottom())
                .flat_map(|y| (rect.left()..rect.right()).map(move |x| (x, y)))
                .find_map(|(x, y)| {
                    let cell = buf.cell((x, y))?;
                    if cell.symbol() != " " { Some(cell.fg) } else { None }
                })
        }
        fn channel_sum(c: ratatui::style::Color) -> u32 {
            match c {
                ratatui::style::Color::Rgb(r, g, b) => r as u32 + g as u32 + b as u32,
                _ => 0,
            }
        }

        let (w, h) = (80u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let l = RosterManager::layout(area);
        let mut ctx = EngineCtx;

        // Mid-slide render: nav right from index 0 -> 1, sample at ~67%
        // progress (200ms of the 300ms SLIDE_DUR).
        let mut mid_slide_scene = RosterManager::new();
        mid_slide_scene.handle_input(key_event(KeyCode::Right));
        mid_slide_scene.update(&mut ctx, Duration::from_millis(200));
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

        let mid_name_fg = first_painted_fg(&mid_slide_buf, l.name)
            .expect("name rect must paint at least one cell mid-slide");
        let rest_name_fg = first_painted_fg(&rest_buf, l.name)
            .expect("name rect must paint at least one cell at rest");
        assert!(
            channel_sum(mid_name_fg) < channel_sum(rest_name_fg),
            "name fg during an active slide ({mid_name_fg:?}, sum={}) must be darker (closer to background) than its resting colour ({rest_name_fg:?}, sum={}) — b2-t1's colour cross-fade",
            channel_sum(mid_name_fg), channel_sum(rest_name_fg)
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

/// b3-t1: selection state (Space / current-dot click sets/cancels
/// `selected_index`) and the selected dot's blink render. See research.md's
/// blueprint — `handle_input`/`render_dot_row` are not yet wired to
/// `selected_index`, so these are RED until the code-writer wires them.
#[cfg(test)]
mod selection_tests {
    use super::*;
    use crate::scenes::test_util::{key_event, mouse_event, render_to_buffer, sample_fg};
    use crossterm::event::KeyCode;
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    use engine_core::scene::EngineCtx;

    /// Space with no prior selection selects the current creature.
    #[test]
    fn space_selects_current_when_none_selected() {
        let mut scene = RosterManager::new();
        assert_eq!(scene.selected_index, None);

        scene.handle_input(key_event(KeyCode::Char(' ')));

        assert_eq!(
            scene.selected_index,
            Some(scene.current_index),
            "Space with no prior selection must set selected_index to the current creature"
        );
    }

    /// A completed click (Moved/Down/Up) on the CURRENT creature's dot does
    /// the same as Space.
    #[test]
    fn click_current_dot_selects() {
        let mut scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        // Render once so the dot hit rects are set for this frame's area
        // (handle_input hit-tests against the PREVIOUS frame's render, per
        // the arrow/home button pattern).
        let _ = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let dots_rect = RosterManager::layout(area).dot_row;
        let slot = RosterManager::dot_slots(dots_rect)[scene.current_index];
        let (cx, cy) = (slot.x, slot.y);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
        scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

        assert_eq!(
            scene.selected_index,
            Some(scene.current_index),
            "a completed click on the current creature's dot must select it"
        );
    }

    /// While selected, the selected dot's painted fg alternates between two
    /// `BLINK_PERIOD`-separated `elapsed` samples; every other dot's fg is
    /// unaffected.
    #[test]
    fn selected_dot_blinks_across_elapsed() {
        let mut scene = RosterManager::new();
        scene.selected_index = Some(scene.current_index);

        let (w, h) = (40u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let dots_rect = RosterManager::layout(area).dot_row;
        let slots = RosterManager::dot_slots(dots_rect);
        let selected_slot = slots[scene.current_index];
        let other_slot = slots[(scene.current_index + 1) % slots.len()];

        let buf_a = render_to_buffer(&scene, w, h);
        let selected_fg_a = sample_fg(&buf_a, selected_slot).expect("selected dot must paint");
        let other_fg_a = sample_fg(&buf_a, other_slot).expect("other dot must paint");

        let mut ctx = EngineCtx;
        scene.update(&mut ctx, RosterManager::BLINK_PERIOD);
        let buf_b = render_to_buffer(&scene, w, h);
        let selected_fg_b = sample_fg(&buf_b, selected_slot).expect("selected dot must paint");
        let other_fg_b = sample_fg(&buf_b, other_slot).expect("other dot must paint");

        assert_ne!(
            selected_fg_a, selected_fg_b,
            "selected dot's fg must alternate (blink) across a BLINK_PERIOD-separated elapsed sample"
        );
        assert_eq!(
            other_fg_a, other_fg_b,
            "a non-selected dot's fg must be unaffected by the blink timer"
        );
    }

    /// Space again with `selected_index == Some(current_index)` (no
    /// navigation between) cancels the selection back to `None`.
    #[test]
    fn space_again_cancels_selection() {
        let mut scene = RosterManager::new();
        scene.selected_index = Some(scene.current_index);

        scene.handle_input(key_event(KeyCode::Char(' ')));

        assert_eq!(
            scene.selected_index, None,
            "Space again at the same current_index (no nav between) must cancel the selection"
        );
    }

    /// b3-t2: select index 0, navigate to index 1, select again -> the two
    /// creatures (by name/identity) are swapped and the selection clears.
    #[test]
    fn select_navigate_select_swaps_creatures() {
        let mut scene = RosterManager::new();
        let name_at_0_before = scene.creatures[0].name().to_string();
        let name_at_1_before = scene.creatures[1].name().to_string();

        scene.handle_input(key_event(KeyCode::Char(' '))); // select current (0)
        assert_eq!(scene.selected_index, Some(0));

        scene.handle_input(key_event(KeyCode::Right)); // navigate 0 -> 1
        assert_eq!(scene.current_index, 1);

        scene.handle_input(key_event(KeyCode::Char(' '))); // select again -> swap

        assert_eq!(
            scene.selected_index, None,
            "a completed swap must clear the selection"
        );
        assert_eq!(
            scene.creatures[0].name(),
            name_at_1_before,
            "the creature originally at index 1 must now be at index 0"
        );
        assert_eq!(
            scene.creatures[1].name(),
            name_at_0_before,
            "the creature originally at index 0 must now be at index 1"
        );
    }

    /// b3-t2: swapping an active-slot index with a reserve-slot index flips
    /// each CREATURE's squad role (tracked by identity through the swap, not
    /// by asserting on the fixed slot indices themselves — `squad_role` is a
    /// pure positional lookup, so slot 0 is always Active and the last slot
    /// is always Reserve; what must flip is which creature sits where).
    #[test]
    fn swap_active_with_reserve_flips_role_by_creature_identity() {
        use crate::squad_role::{squad_role, SquadRole, ACTIVE_SLOTS, ROSTER_SIZE};

        let mut scene = RosterManager::new();
        let active_index = 0;
        let reserve_index = ROSTER_SIZE - 1;
        assert!(active_index < ACTIVE_SLOTS, "index 0 must be an active slot");
        assert_eq!(
            squad_role(reserve_index),
            SquadRole::Reserve,
            "the last roster slot must be a reserve slot"
        );

        let active_creature_name = scene.creatures[active_index].name().to_string();
        let reserve_creature_name = scene.creatures[reserve_index].name().to_string();

        // Select the active creature, then jump the cursor directly to the
        // reserve index (bypassing navigate()'s slide-guard, which is not
        // under test here) and select again to swap.
        scene.selected_index = Some(active_index);
        scene.current_index = reserve_index;
        scene.handle_input(key_event(KeyCode::Char(' ')));

        assert_eq!(scene.selected_index, None, "swap must clear the selection");

        let new_index_of_active_creature = scene
            .creatures
            .iter()
            .position(|c| c.name() == active_creature_name)
            .expect("the originally-active creature must still be present");
        let new_index_of_reserve_creature = scene
            .creatures
            .iter()
            .position(|c| c.name() == reserve_creature_name)
            .expect("the originally-reserve creature must still be present");

        assert_eq!(new_index_of_active_creature, reserve_index);
        assert_eq!(new_index_of_reserve_creature, active_index);
        assert_eq!(
            squad_role(new_index_of_active_creature),
            SquadRole::Reserve,
            "the creature moved into the reserve slot must now report Reserve"
        );
        assert_eq!(
            squad_role(new_index_of_reserve_creature),
            SquadRole::Active,
            "the creature moved into the active slot must now report Active"
        );
    }

    /// b3-t2 guard: re-selecting at the SAME current_index as selected_index
    /// (no navigation between) must cancel per b3-t1, NOT swap — the roster
    /// order must be unchanged.
    #[test]
    fn reselect_same_index_cancels_without_reordering_roster() {
        let mut scene = RosterManager::new();
        let names_before: Vec<String> =
            scene.creatures.iter().map(|c| c.name().to_string()).collect();

        scene.handle_input(key_event(KeyCode::Char(' '))); // select current (0)
        assert_eq!(scene.selected_index, Some(0));

        scene.handle_input(key_event(KeyCode::Char(' '))); // same index again -> cancel

        assert_eq!(scene.selected_index, None);
        let names_after: Vec<String> =
            scene.creatures.iter().map(|c| c.name().to_string()).collect();
        assert_eq!(
            names_before, names_after,
            "cancelling a selection must never reorder the roster"
        );
    }
}

