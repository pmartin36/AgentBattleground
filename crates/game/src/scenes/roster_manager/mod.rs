use std::cell::RefCell;
use std::path::PathBuf;
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
    home_button: RefCell<engine_render::ButtonCore>,
    /// Edit button in the details panel's Instructions header (b3-t1),
    /// opens `prompt_editor` on a completed click. `RefCell` for the same
    /// immutable-render-mutates-button-state reason as the other buttons.
    #[inspect(hidden)]
    edit_button: RefCell<engine_render::ButtonCore>,
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
    /// Cached instructions-file contents for the current creature (b1-t4).
    /// Loaded once at construction and reloaded only when a navigation
    /// slide settles — never re-read per frame/render.
    #[inspect(hidden)]
    current_instructions: String,
    /// Base dir for the instructions cache's IO (b1-t4). `None` in
    /// production (runtime resolver via `crate::instructions::read_instructions`);
    /// `Some` in tests (`read_instructions_in`, hermetic temp dir).
    #[inspect(hidden)]
    instructions_base: Option<PathBuf>,
    /// Set by the Edit button click (b3-t1); `None` until then. Full
    /// popup behavior (rendering, text editors) is spec 51 — this field
    /// only tracks the open/closed state spec 48's Testing Guidance
    /// (line 87) requires.
    #[inspect(hidden)]
    prompt_editor: Option<prompt_editor::PromptEditor>,
    /// The ability grid cell index currently under the cursor (b3-t2);
    /// hover-only, no keyboard focus. `None` when the cursor is outside
    /// every populated ability cell. Consumed by spec 49's tooltip
    /// (out of scope here). Mutated in `handle_input(&mut self, ..)`, so a
    /// plain field (unlike `ability_hit_rects`, which is read from
    /// `render(&self, ..)`).
    #[inspect(hidden)]
    hovered_ability: Option<usize>,
    /// The current frame's per-ability hit-test rects (b3-t2), in cell
    /// space, refreshed each `render(&self, ..)` from
    /// `panel_interior_regions(area).ability_cells`. `Some(rect)` for a
    /// populated ability slot, `None` for an empty slot or while the
    /// details panel is suppressed during a slide. `RefCell` because
    /// `render(&self, ..)` must mutate it from an immutable receiver, same
    /// reason as the button fields.
    #[inspect(hidden)]
    ability_hit_rects: RefCell<[Option<Rect>; 4]>,
    /// Cached lint output for `current_instructions` (b4-t1). Refreshed
    /// exactly when `current_instructions` reloads (construction,
    /// slide-settle, popup-close) via `reload_instructions` — never during
    /// `render(&self, ..)`.
    #[inspect(hidden)]
    diagnostics: Vec<crate::diagnostics::Diagnostic>,
    /// Number of times `reload_instructions` has run the lint (b4-t1,
    /// spec:223's explicit call-count assertion). Test-only observation
    /// point; not read by production logic.
    #[inspect(hidden)]
    lint_runs: usize,
    /// The current frame's `[!!]` badge hit-test rect (b4-t1), in cell
    /// space, refreshed each `render(&self, ..)` from
    /// `diagnostics_ui::header_badge_slot`. `None` when no badge was drawn
    /// (clean instructions, too-narrow terminal, or a slide in flight).
    /// `RefCell` for the same immutable-render-mutates-hit-rect reason as
    /// `ability_hit_rects`.
    #[inspect(hidden)]
    badge_hit_rect: RefCell<Option<Rect>>,
    /// Whether the cursor is currently over the drawn `[!!]` badge (b4-t1);
    /// hover-only, mirrors `hovered_ability`. Mutated in
    /// `handle_input(&mut self, ..)`, so a plain field.
    #[inspect(hidden)]
    hovered_badge: bool,
    /// The store this scene persists roster/egg mutations through.
    /// `None` for `new()`/`new_with_instructions_base` (store-less,
    /// hermetic construction used by every non-store test); `Some` only
    /// for a scene built via `from_store`/`from_store_in`.
    #[inspect(hidden)]
    store: Option<crate::player_data::PlayerStore>,
    /// The loaded egg list, carried alongside `creatures` so a roster save
    /// never drops it. Empty for store-less construction.
    #[inspect(hidden)]
    eggs: Vec<crate::player_data::Egg>,
    /// Entry button that transitions to the Hatchery scene. `RefCell` for
    /// the same immutable-render-mutates-button-state reason as the other
    /// buttons.
    #[inspect(hidden)]
    hatchery_button: RefCell<engine_render::ButtonCore>,
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
        crate::scenes::stat_bar::STAT_BAR_OUTLINE_H + crate::scenes::stat_bar::STAT_LABEL_H;
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
        Self::build(None)
    }

    /// Shared construction path for `new()` and `new_with_instructions_base`
    /// (b1-t4): builds every field identically, then performs the initial
    /// instructions-cache load against `instructions_base` so the first load
    /// already uses the correct base.
    fn build(instructions_base: Option<PathBuf>) -> Self {
        let mut scene = Self {
            current_index: 0,
            creatures: crate::creatures::demo_roster(),
            elapsed: Duration::ZERO,
            left_button: RefCell::new(
                engine_render::Button::new(Rect::default(), crate::assets::BUTTON_PANEL)
                    .icon(crate::assets::ICON_ARROW_LEFT),
            ),
            right_button: RefCell::new(
                engine_render::Button::new(Rect::default(), crate::assets::BUTTON_PANEL)
                    .icon(crate::assets::ICON_ARROW_RIGHT),
            ),
            home_button: RefCell::new(engine_render::ButtonCore::new(Rect::default())),
            edit_button: RefCell::new(engine_render::ButtonCore::new(Rect::default())),
            slide: None,
            selected_index: None,
            current_dot: RefCell::new(engine_render::ButtonCore::new(Rect::default())),
            current_instructions: String::new(),
            instructions_base,
            prompt_editor: None,
            hovered_ability: None,
            ability_hit_rects: RefCell::new([None; 4]),
            diagnostics: Vec::new(),
            lint_runs: 0,
            badge_hit_rect: RefCell::new(None),
            hovered_badge: false,
            store: None,
            eggs: Vec::new(),
            hatchery_button: RefCell::new(engine_render::ButtonCore::new(Rect::default())),
        };
        scene.reload_instructions();
        scene
    }

    /// Test-only constructor that pins the instructions-cache base dir to a
    /// hermetic temp directory instead of the runtime resolver (b1-t4).
    #[cfg(test)]
    fn new_with_instructions_base(base: PathBuf) -> Self {
        Self::build(Some(base))
    }

    /// Store-backed construction used by the production scene factory
    /// (`registry::construct`): loads `PlayerData` from `store` (walking
    /// main -> .bak -> a first-run seed built from `demo_roster()`),
    /// writes the seed on first run, hydrates the persisted roster back
    /// into runtime `Creature`s (re-attaching bundled sprites by name),
    /// and carries `store`/the loaded eggs so subsequent mutations persist.
    pub fn from_store(store: crate::player_data::PlayerStore) -> Self {
        Self::from_store_in(store, None)
    }

    /// `from_store` with an injectable instructions-cache base dir, so
    /// tests never touch the runtime instructions resolver. Loads
    /// `PlayerData` from `store` (walking main -> .bak -> a first-run seed
    /// built from `demo_roster()`), writes the seed back on first run,
    /// hydrates the persisted roster into runtime `Creature`s, and carries
    /// `store`/the loaded eggs so `persist()` can save future mutations.
    fn from_store_in(
        store: crate::player_data::PlayerStore,
        instructions_base: Option<PathBuf>,
    ) -> Self {
        let loaded = store.load(Self::seed_player_data);
        let seeded = matches!(loaded, crate::player_data::Loaded::Seeded(_));
        let data = loaded.into_data();
        if seeded {
            if let Err(e) = store.save(&data) {
                tracing::warn!("player-data save failed: {e}");
            }
        }
        let creatures = Self::hydrate_roster(&data.roster);

        let mut scene = Self::build(instructions_base);
        scene.creatures = creatures;
        scene.eggs = data.eggs;
        scene.store = Some(store);
        scene.current_index = 0;
        scene.reload_instructions();
        scene
    }

    /// First-run seed: `demo_roster()` in persisted form (no art handles —
    /// `hydrate_roster` re-attaches the matching bundled sprite by name).
    fn seed_player_data() -> crate::player_data::PlayerData {
        crate::player_data::default_seed()
    }

    /// Converts a persisted roster into runtime `Creature`s, re-attaching
    /// each bundled creature's decoded sprite BY NAME rather than resolving
    /// a handle from disk. `AnimatedSprite` is not `Clone`, so a bundled
    /// sprite must be moved out of a fresh `crate::creatures::all()`
    /// instance, not cloned onto a `creature_from_persisted` result — this
    /// is why hydration starts from the bundled `Creature`, not the
    /// persisted RPG data. A generated creature (one that already carries an
    /// idle handle) or a name with no bundled match resolves normally via
    /// `creature_from_persisted`.
    fn hydrate_roster(
        persisted: &[crate::player_data::PersistedCreature],
    ) -> Vec<crate::creatures::Creature> {
        let mut bundled: std::collections::HashMap<String, crate::creatures::Creature> =
            crate::creatures::all()
                .into_iter()
                .map(|c| (c.name().to_string(), c))
                .collect();

        persisted
            .iter()
            .map(|p| match bundled.remove(&p.name) {
                Some(base) if p.idle.is_none() => {
                    crate::player_data::apply_persisted_rpg(base, p)
                }
                _ => crate::player_data::creature_from_persisted(p),
            })
            .collect()
    }

    /// Re-serializes the current roster and eggs and writes them through
    /// `store` — the single save site every roster-content mutation must
    /// call through. A no-op for a store-less scene (`new()` /
    /// `new_with_instructions_base`).
    fn persist(&self) {
        if let Some(store) = &self.store {
            let data = crate::player_data::PlayerData {
                roster: self.creatures.iter().map(crate::player_data::creature_to_persisted).collect(),
                eggs: self.eggs.clone(),
            };
            if let Err(e) = store.save(&data) {
                tracing::warn!("player-data save failed: {e}");
            }
        }
    }

    /// Reloads `current_instructions` from disk for the CURRENT creature
    /// (b1-t4). Called once at construction (`build`) and again only when a
    /// navigation slide settles — never per frame/render (`render(&self, ..)`
    /// cannot call this; it takes `&self`).
    fn reload_instructions(&mut self) {
        let name = self.creatures[self.current_index].name().to_string();
        let read =
            crate::instructions::read_instructions_maybe(self.instructions_base.as_deref(), &name);
        self.current_instructions = read.unwrap_or_default();

        let vocab = crate::mention::Vocabulary::new(&self.creatures[self.current_index], &self.creatures);
        self.diagnostics = crate::diagnostics::lint(&self.current_instructions, &vocab);
        self.lint_runs += 1;
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
                self.reload_instructions();
                self.persist();
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
                self.reload_instructions();
            }
        }
        if let Some(popup) = self.prompt_editor.as_mut() {
            popup.update(dt);
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
            let border = Self::details_panel_rects(area);
            Self::draw_dot_border(frame.buffer_mut(), border, Self::BORDER_COLOR);

            let regions = Self::panel_interior_regions(area);
            let idx = self.current_index;
            self.render_stamina_row(frame.buffer_mut(), regions.stamina, idx);
            self.render_abilities(frame.buffer_mut(), regions.abilities_header, regions.ability_cells, idx);
            self.render_instructions(frame.buffer_mut(), regions.instructions_header, regions.preview);

            {
                let slot = diagnostics_ui::header_badge_slot(
                    regions.instructions_header,
                    Self::INSTRUCTIONS_HEADER_TEXT,
                );
                let drawn = slot.and_then(|s| diagnostics_ui::draw_badge(frame.buffer_mut(), s, &self.diagnostics));
                *self.badge_hit_rect.borrow_mut() = drawn;
            }

            {
                let state = {
                    let mut edit = self.edit_button.borrow_mut();
                    edit.set_rect(regions.edit_button.to_cell_rect());
                    edit.state()
                };
                Self::render_edit_button(frame.buffer_mut(), regions.edit_button, state);
            }

            let n = self.creatures[idx].abilities().len();
            let mut hits = self.ability_hit_rects.borrow_mut();
            for i in 0..4 {
                hits[i] = (i < n).then(|| regions.ability_cells[i].to_cell_rect());
            }
        } else {
            *self.ability_hit_rects.borrow_mut() = [None; 4];
            *self.badge_hit_rect.borrow_mut() = None;
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
            crate::scenes::home_button::draw_home_button(frame.buffer_mut(), home_dr, home.state());
        }

        {
            let hdr = Self::hatchery_dot_rect(area);
            let mut hb = self.hatchery_button.borrow_mut();
            hb.set_rect(Self::hatchery_button_rect(area));
            crate::scenes::home_button::draw_badge_button(
                frame.buffer_mut(),
                hdr,
                hb.state(),
                crate::assets::ICON_EGG,
            );
        }

        // Ability hover tooltip (spec 49) — topmost overlay, hover-only.
        // Suppressed while a modal is open or a slide is in flight (the details
        // panel and its ability cells are not drawn during a slide, and the
        // stale hovered index may point past the new creature's abilities).
        if self.prompt_editor.is_none() && self.active_slide().is_none() {
            if let Some(hi) = self.hovered_ability {
                let abilities = self.creatures[self.current_index].abilities();
                if hi < abilities.len() {
                    let cell = Self::panel_interior_regions(area).ability_cells[hi];
                    tooltip::render_tooltip(frame.buffer_mut(), &abilities[hi], cell);
                }
            }
        }

        // Diagnostics warning-card overlay (spec 60, b4-t1) — topmost overlay,
        // hover-only, same suppression guards as the ability tooltip plus
        // `hovered_badge`.
        if self.prompt_editor.is_none() && self.active_slide().is_none() && self.hovered_badge {
            if let Some(badge) = *self.badge_hit_rect.borrow() {
                diagnostics_ui::render_warning_card(
                    frame.buffer_mut(),
                    Self::cell_rect_to_dots(badge),
                    &self.diagnostics,
                );
            }
        }

        // Prompt-editor popup (spec 51) — topmost overlay, occludes the
        // roster beneath while open.
        if let Some(popup) = &self.prompt_editor {
            popup.render(frame, area);
        }
    }

    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
        use crossterm::event::{KeyCode, MouseEventKind};
        use ratatui::layout::Position;

        if self.prompt_editor.is_some() {
            let close = self.prompt_editor.as_mut().unwrap().handle_input(&ev);
            if close {
                self.prompt_editor.as_mut().unwrap().flush_pending();
                self.prompt_editor = None;
                self.reload_instructions();
            }
            return None;
        }

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
                let hit_edit = self.edit_button.get_mut().handle_mouse(&me);
                let hit_hatchery = self.hatchery_button.get_mut().handle_mouse(&me);
                if me.kind == MouseEventKind::Moved {
                    let pos = Position { x: me.column, y: me.row };
                    self.hovered_ability = self
                        .ability_hit_rects
                        .borrow()
                        .iter()
                        .position(|r| r.is_some_and(|rc| rc.contains(pos)));
                    self.hovered_badge = self.badge_hit_rect.borrow().is_some_and(|r| r.contains(pos));
                }
                if hit_home {
                    return Some(Transition {
                        target: SceneId::MainHub.into(),
                        params: None,
                    });
                }
                if hit_hatchery {
                    return Some(Transition {
                        target: SceneId::Hatchery.into(),
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
                if hit_edit {
                    let name = self.creatures[self.current_index].name().to_string();
                    let base = self.instructions_base.clone();
                    self.prompt_editor = Some(prompt_editor::PromptEditor::new(
                        self.current_index,
                        &name,
                        base.as_deref(),
                        &self.creatures,
                    ));
                }
            }
        }
        None
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {}

    fn inspect(&mut self) -> &mut dyn engine_core::Inspectable {
        self
    }

    /// The prompt-editor popup owns this scene's only text fields, so it is
    /// the sole holder of the break key: delegate to it while it is open, and
    /// report `false` once it closes so Ctrl-C quits normally again.
    fn consumes_break(&self) -> bool {
        self.prompt_editor
            .as_ref()
            .is_some_and(|popup| popup.consumes_break())
    }
}

mod borders;
mod chrome;
mod details_panel;
mod diagnostics_ui;
mod dot_row;
mod layout;
mod panel_layout;
mod prompt_editor;
mod sprite_name;
mod stat_bar;
mod tooltip;

#[cfg(test)]
mod regression_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod arrow_key_navigation_tests;
#[cfg(test)]
mod slide_transition_tests;
#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod instructions_cache_tests;
#[cfg(test)]
mod edit_button_tests;
#[cfg(test)]
mod ability_hover_tests;
#[cfg(test)]
mod tooltip_integration_tests;
#[cfg(test)]
mod prompt_editor_modal_tests;
#[cfg(test)]
mod diagnostics_ui_tests;
#[cfg(test)]
mod diagnostics_integration_tests;
#[cfg(test)]
mod store_backed_tests;
#[cfg(test)]
mod hatchery_button_tests;
