//! Hatchery scene shell: reachable from the roster, with a back button that
//! returns to it, an egg tray rendering every owned egg, and a tap-to-focus
//! view that centers an `Incubating` egg with its countdown while the
//! remaining eggs relocate to a bottom strip.

mod focus;
mod lifecycle;
mod tray;

use std::cell::RefCell;
use std::time::{Duration, SystemTime};

use ratatui::layout::Rect;
use ratatui::Frame;
use serde_json::Value as JsonValue;

use engine_core::scene::{EngineCtx, InputEvent, Scene, Transition};
use engine_core::Inspectable;
use engine_core::SceneKey;

#[derive(Inspectable)]
pub struct Hatchery {
    /// Loaded from the player-data store; read by the egg tray render.
    #[inspect(hidden)]
    eggs: Vec<crate::player_data::Egg>,
    /// The store this scene was constructed from, used by `persist_eggs` to
    /// reload-merge the current on-disk `PlayerData` before saving.
    #[inspect(hidden)]
    store: Option<crate::player_data::PlayerStore>,
    #[inspect(hidden)]
    back_button: RefCell<engine_render::ButtonCore>,
    /// Scene-local clock accumulated in `update`, read by the tray render to
    /// key the `Ready` egg's idle bob.
    #[inspect(hidden)]
    elapsed: Duration,
    /// Each owned egg's `egg_art` decoded once (index-aligned with `eggs`),
    /// so `render` never touches disk.
    #[inspect(hidden)]
    art_cache: Vec<Option<image::DynamicImage>>,
    /// The `Incubating` egg currently shown in the centered focus view, or
    /// `None` when every egg sits in the tray.
    #[inspect(hidden)]
    focused: Option<usize>,
    /// Set by a completed tap on an `Undefined` egg; consumed by
    /// `take_define_request`.
    #[inspect(hidden)]
    pending_define: Option<usize>,
    /// Set by a completed tap on a `Ready` egg; consumed by
    /// `take_hatch_request`.
    #[inspect(hidden)]
    pending_hatch: Option<usize>,
    /// One click-state core per egg, index-aligned with `eggs`; its rect is
    /// set every render to the egg's current on-screen slot (tray or focus).
    #[inspect(hidden)]
    egg_buttons: RefCell<Vec<engine_render::ButtonCore>>,
}

impl Hatchery {
    /// Store-less constructor for hermetic tests: no eggs, no persistence.
    pub fn new() -> Self {
        Self {
            eggs: Vec::new(),
            store: None,
            back_button: RefCell::new(engine_render::ButtonCore::new(Rect::default())),
            elapsed: Duration::ZERO,
            art_cache: Vec::new(),
            focused: None,
            pending_define: None,
            pending_hatch: None,
            egg_buttons: RefCell::new(Vec::new()),
        }
    }

    /// Store-backed construction used by the production scene factory:
    /// loads `eggs` from the persisted `PlayerData`, keeps `store`, and
    /// promotes any egg that has already finished incubating (persisting
    /// the promotion if it fires).
    pub fn from_store(store: crate::player_data::PlayerStore) -> Self {
        Self::from_store_at(store, SystemTime::now())
    }

    /// `from_store` with an injectable `now`, so construction-time
    /// promotion (an elapsed `Incubating` egg becomes `Ready` and the
    /// promotion is persisted) is deterministic under test.
    pub(crate) fn from_store_at(store: crate::player_data::PlayerStore, now: SystemTime) -> Self {
        let data = store.load(Self::egg_seed).into_data();
        // Decode `egg_art` before promotion so the cache stays index-aligned
        // with `eggs` (promotion only ever changes `.state`, never `egg_art`
        // or the Vec's length/order).
        let art_cache = Self::decode_egg_art(&data.eggs);
        // Sized once from `data.eggs.len()`; eggs never grow/reorder in this
        // scene, so the cores stay index-aligned for the scene's life.
        let egg_buttons = RefCell::new(
            (0..data.eggs.len())
                .map(|_| engine_render::ButtonCore::new(Rect::default()))
                .collect(),
        );
        let mut scene = Self {
            eggs: data.eggs,
            store: Some(store),
            back_button: RefCell::new(engine_render::ButtonCore::new(Rect::default())),
            elapsed: Duration::ZERO,
            art_cache,
            focused: None,
            pending_define: None,
            pending_hatch: None,
            egg_buttons,
        };
        scene.tick(now);
        scene
    }

    /// Decodes each egg's `egg_art` path once, index-aligned with `eggs`.
    /// An egg with no art, or art that fails to decode, gets `None` (a
    /// visible `EGG_UNKNOWN` placeholder in the tray render, never a panic).
    fn decode_egg_art(eggs: &[crate::player_data::Egg]) -> Vec<Option<image::DynamicImage>> {
        eggs.iter()
            .map(|egg| {
                egg.egg_art.as_ref().and_then(|asset| match image::open(&asset.path) {
                    Ok(img) => Some(img),
                    Err(e) => {
                        tracing::warn!("failed to decode egg art at {:?}: {e}", asset.path);
                        None
                    }
                })
            })
            .collect()
    }

    /// The seed used whenever a load falls through to a fresh `PlayerData`.
    /// Never written to disk on its own — see `persist_eggs`'s reload-merge.
    fn egg_seed() -> crate::player_data::PlayerData {
        crate::player_data::PlayerData {
            roster: Vec::new(),
            eggs: Vec::new(),
        }
    }

    /// Promotes any elapsed `Incubating` egg to `Ready` and, only when a
    /// promotion actually happened, persists it. The single owner of the
    /// `Incubating` -> `Ready` transition, called both at construction and
    /// per frame. Returns whether anything changed.
    fn tick(&mut self, now: SystemTime) -> bool {
        let changed = lifecycle::promote_ready(&mut self.eggs, now);
        if changed {
            self.persist_eggs();
        }
        changed
    }

    /// The single `store.save` site for this scene: reloads the current
    /// on-disk `PlayerData`, replaces only `.eggs` with the in-memory copy,
    /// and saves — so the on-disk roster is rewritten untouched, never
    /// fabricated from Hatchery's own state.
    fn persist_eggs(&self) {
        if let Some(store) = &self.store {
            let mut data = store.load(Self::egg_seed).into_data();
            data.eggs = self.eggs.clone();
            if let Err(e) = store.save(&data) {
                tracing::warn!("failed to persist hatchery eggs: {e}");
            }
        }
    }

    /// Sets egg `index` to `Incubating { started_at: now }` and persists.
    /// Out-of-range `index` is a no-op.
    pub fn start_incubation(&mut self, index: usize, now: SystemTime) {
        if let Some(egg) = self.eggs.get_mut(index) {
            egg.state = crate::player_data::EggState::Incubating { started_at: now };
            self.persist_eggs();
        }
    }

    /// Consumes and returns the pending "define this egg" request left by a
    /// completed tap on an `Undefined` egg, or `None` if there is none.
    pub fn take_define_request(&mut self) -> Option<usize> {
        self.pending_define.take()
    }

    /// Consumes and returns the pending "hatch this egg" request left by a
    /// completed tap on a `Ready` egg, or `None` if there is none.
    pub fn take_hatch_request(&mut self) -> Option<usize> {
        self.pending_hatch.take()
    }

    /// Routes a completed tap on egg `index` by its current state: an
    /// `Incubating` egg toggles/swaps the centered focus view; `Undefined`/
    /// `Ready` eggs record a define/hatch request instead of entering focus.
    fn on_egg_tapped(&mut self, index: usize) {
        let Some(egg) = self.eggs.get(index) else { return };
        match focus::route_tap(&egg.state) {
            focus::TapRoute::Define => self.pending_define = Some(index),
            focus::TapRoute::Hatch => self.pending_hatch = Some(index),
            focus::TapRoute::Focus => {
                self.focused = (self.focused != Some(index)).then_some(index);
            }
        }
    }

    /// Background fill color — teal, distinct from every other scene's fill.
    const COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0x1a, 0x66, 0x66);

    /// Dot-space placement of the back button within `area`; reuses the
    /// roster's shared home-button geometry (same top-right slot, since the
    /// back button plays the same navigational role here).
    fn back_dot_rect(area: Rect) -> engine_render::DotRect {
        crate::scenes::home_button::home_dot_rect(area)
    }
}

impl Default for Hatchery {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene for Hatchery {
    fn id(&self) -> SceneKey {
        crate::scene_id::SceneId::Hatchery.into()
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}

    fn update(&mut self, _ctx: &mut EngineCtx, dt: Duration) -> Option<Transition> {
        self.elapsed += dt;
        self.tick(SystemTime::now());
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        engine_render::fill(frame.buffer_mut(), area, Self::COLOR);

        let dr = Self::back_dot_rect(area);
        let mut b = self.back_button.borrow_mut();
        b.set_rect(dr.to_cell_rect());
        crate::scenes::home_button::draw_home_button(frame.buffer_mut(), dr, b.state());

        let mut buttons = self.egg_buttons.borrow_mut();
        match self.focused {
            None => {
                let slots = tray::tray_slots(area, self.eggs.len());
                for (i, slot) in slots.iter().enumerate() {
                    tray::draw_egg(
                        frame.buffer_mut(),
                        *slot,
                        &self.eggs[i],
                        self.art_cache.get(i).and_then(|a| a.as_ref()),
                        self.elapsed,
                    );
                    if let Some(btn) = buttons.get_mut(i) {
                        btn.set_rect(slot.to_cell_rect());
                    }
                }
            }
            Some(f) => {
                let (focus_dr, strip) = focus::focus_layout(area);
                let slots = tray::tray_slots(strip, self.eggs.len());
                for (i, slot) in slots.iter().enumerate() {
                    if i == f {
                        continue;
                    }
                    tray::draw_egg(
                        frame.buffer_mut(),
                        *slot,
                        &self.eggs[i],
                        self.art_cache.get(i).and_then(|a| a.as_ref()),
                        self.elapsed,
                    );
                    if let Some(btn) = buttons.get_mut(i) {
                        btn.set_rect(slot.to_cell_rect());
                    }
                }
                tray::draw_egg(
                    frame.buffer_mut(),
                    focus_dr,
                    &self.eggs[f],
                    self.art_cache.get(f).and_then(|a| a.as_ref()),
                    self.elapsed,
                );
                if let Some(btn) = buttons.get_mut(f) {
                    btn.set_rect(focus_dr.to_cell_rect());
                }
                if let Some(rem) = lifecycle::remaining(&self.eggs[f], SystemTime::now()) {
                    focus::draw_countdown(frame.buffer_mut(), focus_dr, rem);
                }
            }
        }
    }

    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
        if let InputEvent::Mouse(me) = ev {
            if self.back_button.get_mut().handle_mouse(&me) {
                return Some(Transition {
                    target: crate::scene_id::SceneId::RosterManager.into(),
                    params: None,
                });
            }

            let mut tapped = None;
            for (i, btn) in self.egg_buttons.get_mut().iter_mut().enumerate() {
                if btn.handle_mouse(&me) {
                    tapped = Some(i);
                }
            }
            if let Some(i) = tapped {
                self.on_egg_tapped(i);
            }
        }
        None
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {}

    fn inspect(&mut self) -> &mut dyn Inspectable {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_id::SceneId;
    use crate::scenes::test_util::{mouse_event, render_to_buffer};
    use engine_core::scene::Scene;
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};

    #[test]
    fn hatchery_id_is_hatchery() {
        let scene = Hatchery::new();
        assert_eq!(scene.id(), SceneKey::from(SceneId::Hatchery));
    }

    /// A completed click (Moved+Down+Up) on the back button returns a
    /// `Transition` to `RosterManager` with no params.
    #[test]
    fn back_button_click_transitions_to_roster_manager() {
        let mut scene = Hatchery::new();
        let (w, h) = (40u16, 20u16);
        // Render once so the button's rect is set for this frame (handle_input
        // hit-tests against the previous frame's render, mirroring the
        // roster's home-button pattern).
        let _ = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let rect = crate::scenes::home_button::home_dot_rect(area).to_cell_rect();
        let (cx, cy) = (rect.x, rect.y);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
        let t = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

        let t = t.expect("a completed click on the back button must return a Transition");
        assert_eq!(
            t.target,
            SceneKey::from(SceneId::RosterManager),
            "back button must transition to RosterManager"
        );
        assert!(t.params.is_none(), "back button transition must carry no params");
    }

    use crate::ability::Element;
    use crate::player_data::{
        Egg, EggState, PersistedCreature, PlayerData, PlayerStore,
    };
    use crate::stamina::Stamina;
    use crate::stats::Stats;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Unique per-test temp dir, mirroring `player_data::store`'s hermetic
    /// no-`tempfile`-crate pattern.
    fn temp_store_dir(tag: &str) -> std::path::PathBuf {
        let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "game-hatchery-lifecycle-test-{}-{}-{}",
            std::process::id(),
            tag,
            n
        ))
    }

    /// A minimal roster member, distinguishable so a reload can confirm the
    /// roster survived an egg-only save untouched.
    fn sample_creature(name: &str) -> PersistedCreature {
        PersistedCreature::new(
            name,
            Element::Fire,
            Stats::default(),
            1,
            0,
            Vec::new(),
            Stamina::default(),
            None,
            None,
            None,
        )
    }

    fn incubating_egg(started_at: SystemTime) -> Egg {
        Egg {
            element: Element::Fire,
            state: EggState::Incubating { started_at },
            mad_lib: None,
            egg_art: None,
            hatchling: None,
        }
    }

    fn undefined_egg() -> Egg {
        Egg {
            element: Element::Fire,
            state: EggState::Undefined,
            mad_lib: None,
            egg_art: None,
            hatchling: None,
        }
    }

    /// Construction promotes an elapsed `Incubating` egg to `Ready` AND
    /// persists it: a fresh load from the same dir also sees `Ready`, and
    /// the on-disk roster is untouched by the egg-only save.
    #[test]
    fn from_store_at_promotes_elapsed_egg_and_persists_without_clobbering_roster() {
        let dir = temp_store_dir("promote-and-persist");
        let now = SystemTime::now();
        let seed = PlayerData {
            roster: vec![sample_creature("Emberling")],
            eggs: vec![incubating_egg(now - Duration::from_secs(24 * 3600 + 1))],
        };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);

        assert_eq!(scene.eggs[0].state, EggState::Ready, "elapsed egg must promote in memory");

        let reloaded = PlayerStore::with_dir(&dir)
            .load(|| panic!("must not fall back to seed"))
            .into_data();
        assert_eq!(reloaded.eggs[0].state, EggState::Ready, "promotion must be persisted, not just in-memory");
        assert_eq!(reloaded.roster.len(), 1, "the on-disk roster must survive an egg-only save untouched");
        assert_eq!(reloaded.roster[0].name, "Emberling");
    }

    /// An `Incubating` egg short of its 24h mark stays `Incubating` after
    /// construction — no premature promotion.
    #[test]
    fn from_store_at_leaves_unelapsed_egg_incubating() {
        let dir = temp_store_dir("not-yet-elapsed");
        let now = SystemTime::now();
        let seed = PlayerData {
            roster: vec![sample_creature("Emberling")],
            eggs: vec![incubating_egg(now - Duration::from_secs(23 * 3600))],
        };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);

        assert!(
            matches!(scene.eggs[0].state, EggState::Incubating { .. }),
            "an egg under 24h must not be promoted"
        );
    }

    /// `start_incubation` sets the targeted egg to `Incubating{started_at}`
    /// and persists it — a reload from the same dir shows it, and the
    /// roster is untouched.
    #[test]
    fn start_incubation_sets_started_at_and_persists() {
        let dir = temp_store_dir("start-incubation");
        let now = SystemTime::now();
        let seed = PlayerData {
            roster: vec![sample_creature("Emberling")],
            eggs: vec![undefined_egg()],
        };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);
        let t0 = now + Duration::from_secs(60);
        scene.start_incubation(0, t0);

        assert_eq!(scene.eggs[0].state, EggState::Incubating { started_at: t0 });

        let reloaded = PlayerStore::with_dir(&dir)
            .load(|| panic!("must not fall back to seed"))
            .into_data();
        assert_eq!(reloaded.eggs[0].state, EggState::Incubating { started_at: t0 }, "must be persisted");
        assert_eq!(reloaded.roster.len(), 1, "the on-disk roster must survive untouched");
    }

    /// An out-of-range index is a no-op: no panic, no state change.
    #[test]
    fn start_incubation_out_of_range_index_is_a_no_op() {
        let dir = temp_store_dir("start-incubation-oob");
        let now = SystemTime::now();
        let seed = PlayerData { roster: vec![sample_creature("Emberling")], eggs: vec![undefined_egg()] };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);
        scene.start_incubation(99, now);

        assert_eq!(scene.eggs[0].state, EggState::Undefined, "out-of-range index must not mutate any egg");
    }

    /// A scene with an `Undefined` egg renders the tray: somewhere in the
    /// frame there is a lit dot carrying the bundled `?` sprite's bright
    /// yellow, untinted (element_color tints would darken it away from this
    /// exact hue).
    #[test]
    fn render_draws_undefined_eggs_bright_yellow_marker() {
        let dir = temp_store_dir("render-undefined-marker");
        let seed = PlayerData { roster: Vec::new(), eggs: vec![undefined_egg()] };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let is_bright_yellow = |r: u8, g: u8, b: u8| r > 200 && g > 150 && b < 50;
        let mut found = false;
        'scan: for y in 0..h {
            for x in 0..w {
                if let Some((_, color)) = engine_render::decode_braille_cell(&buf, x, y) {
                    if is_bright_yellow(color.r, color.g, color.b) {
                        found = true;
                        break 'scan;
                    }
                }
            }
        }
        assert!(found, "expected a bright-yellow lit dot somewhere in the rendered frame for the undefined egg's `?`");
    }

    fn ready_egg() -> Egg {
        Egg {
            element: Element::Fire,
            state: EggState::Ready,
            mad_lib: None,
            egg_art: None,
            hatchling: None,
        }
    }

    /// A completed click (Moved+Down+Up) at `(x, y)`.
    fn tap_at(scene: &mut Hatchery, x: u16, y: u16) {
        scene.handle_input(mouse_event(MouseEventKind::Moved, x, y));
        scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), x, y));
        scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), x, y));
    }

    /// Tapping an `Incubating` egg enters focus: `focused` names it.
    #[test]
    fn tap_incubating_egg_enters_focus() {
        let dir = temp_store_dir("tap-enters-focus");
        let now = SystemTime::now();
        let seed = PlayerData {
            roster: Vec::new(),
            eggs: vec![incubating_egg(now - Duration::from_secs(3600))],
        };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);
        let (w, h) = (40u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let _ = render_to_buffer(&scene, w, h);

        let rect = tray::tray_slots(area, 1)[0].to_cell_rect();
        tap_at(&mut scene, rect.x, rect.y);

        assert_eq!(scene.focused, Some(0), "tapping an Incubating egg must focus it");
    }

    /// Tapping the same focused egg again returns it to the tray.
    #[test]
    fn tapping_focused_egg_again_returns_it_to_the_tray() {
        let dir = temp_store_dir("tap-toggle-off");
        let now = SystemTime::now();
        let seed = PlayerData {
            roster: Vec::new(),
            eggs: vec![incubating_egg(now - Duration::from_secs(3600))],
        };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);
        let (w, h) = (40u16, 20u16);
        let area = Rect::new(0, 0, w, h);

        let _ = render_to_buffer(&scene, w, h);
        let tray_rect = tray::tray_slots(area, 1)[0].to_cell_rect();
        tap_at(&mut scene, tray_rect.x, tray_rect.y);
        assert_eq!(scene.focused, Some(0));

        let _ = render_to_buffer(&scene, w, h);
        let focus_rect = focus::focus_layout(area).0.to_cell_rect();
        tap_at(&mut scene, focus_rect.x, focus_rect.y);
        assert_eq!(scene.focused, None, "tapping the focused egg again must return it to the tray");
    }

    /// Tapping a different `Incubating` egg swaps focus to it.
    #[test]
    fn tap_swaps_focus_between_two_incubating_eggs() {
        let dir = temp_store_dir("tap-swap");
        let now = SystemTime::now();
        let seed = PlayerData {
            roster: Vec::new(),
            eggs: vec![
                incubating_egg(now - Duration::from_secs(3600)),
                incubating_egg(now - Duration::from_secs(7200)),
            ],
        };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);
        let (w, h) = (40u16, 20u16);
        let area = Rect::new(0, 0, w, h);

        let _ = render_to_buffer(&scene, w, h);
        let slot0 = tray::tray_slots(area, 2)[0].to_cell_rect();
        tap_at(&mut scene, slot0.x, slot0.y);
        assert_eq!(scene.focused, Some(0));

        let _ = render_to_buffer(&scene, w, h);
        let (_, strip) = focus::focus_layout(area);
        let strip_slot1 = tray::tray_slots(strip, 2)[1].to_cell_rect();
        tap_at(&mut scene, strip_slot1.x, strip_slot1.y);
        assert_eq!(scene.focused, Some(1), "tapping a different Incubating egg must swap focus");
    }

    /// Tapping an `Undefined` egg records a define request and never enters
    /// focus; the request is consumed exactly once.
    #[test]
    fn tap_undefined_egg_sets_pending_define_not_focus() {
        let dir = temp_store_dir("tap-undefined");
        let seed = PlayerData { roster: Vec::new(), eggs: vec![undefined_egg()] };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
        let (w, h) = (40u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let _ = render_to_buffer(&scene, w, h);
        let rect = tray::tray_slots(area, 1)[0].to_cell_rect();
        tap_at(&mut scene, rect.x, rect.y);

        assert!(scene.focused.is_none(), "an Undefined egg must never enter focus");
        assert_eq!(scene.take_define_request(), Some(0));
        assert_eq!(scene.take_define_request(), None, "a request is consumed exactly once");
    }

    /// Tapping a `Ready` egg records a hatch request and never enters focus.
    #[test]
    fn tap_ready_egg_sets_pending_hatch_not_focus() {
        let dir = temp_store_dir("tap-ready");
        let seed = PlayerData { roster: Vec::new(), eggs: vec![ready_egg()] };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
        let (w, h) = (40u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let _ = render_to_buffer(&scene, w, h);
        let rect = tray::tray_slots(area, 1)[0].to_cell_rect();
        tap_at(&mut scene, rect.x, rect.y);

        assert!(scene.focused.is_none(), "a Ready egg must never enter focus");
        assert_eq!(scene.take_hatch_request(), Some(0));
    }
}
