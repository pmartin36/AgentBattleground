//! Hatchery scene shell: reachable from the roster, with a back button that
//! returns to it, an egg tray rendering every owned egg, and a tap-to-focus
//! view that centers an `Incubating` egg with its countdown while the
//! remaining eggs relocate to a bottom strip.

mod focus;
mod hatch;
mod hatch_clips;
#[cfg(debug_assertions)]
mod hatch_dev;
mod hatch_layout;
mod hatch_render;
mod hatch_roster;
mod lifecycle;
#[cfg(test)]
mod local_model_e2e_tests;
mod tray;
pub mod define_modal;
pub mod definition;
pub mod mad_lib;
pub mod parts;

use std::cell::RefCell;
use std::time::{Duration, SystemTime};

use ratatui::layout::Rect;
use ratatui::Frame;
use serde_json::Value as JsonValue;

use engine_core::scene::{EngineCtx, InputEvent, Scene, Transition};
use engine_core::Inspectable;
use engine_core::SceneKey;

use crate::asset_gen::{capability, AssetGen, SdCliRunner, ZImageBackend};
use crate::model_config::{resolve_model_config, ConfigError};
use crate::text_gen::operation::TextGen;
use crate::text_gen::ResolvedModelConfig;
use define_modal::{DefineModal, ModalAction};

/// Produces a `TextGen` from a resolved model config, so the scene can defer
/// building the real backend until it is actually needed.
type TextGenFactory = Box<dyn Fn(&ResolvedModelConfig) -> TextGen>;

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
    /// Set by a completed tap on a `Ready` egg or a dev force-hatch. Persists
    /// as the generating-wait flag (`render` draws "Generating..." while it
    /// is set and `self.hatch` is `None`) until `advance_hatch`'s readiness
    /// gate clears it and launches the sequence.
    #[inspect(hidden)]
    pending_hatch: Option<usize>,
    /// One click-state core per egg, index-aligned with `eggs`; its rect is
    /// set every render to the egg's current on-screen slot (tray or focus).
    #[inspect(hidden)]
    egg_buttons: RefCell<Vec<engine_render::ButtonCore>>,
    /// The open mad-lib definition modal, or `None` when no `Undefined` egg
    /// is being defined.
    #[inspect(hidden)]
    define_modal: Option<DefineModal>,
    /// The egg index `define_modal` is defining; set on open, cleared on
    /// close.
    #[inspect(hidden)]
    defining_egg: Option<usize>,
    /// Image-generation dependency, read by the definition pipeline once a
    /// modal is submitted.
    #[inspect(hidden)]
    asset_gen: AssetGen,
    /// The resolved text-generation model config, or the distinct reason
    /// nothing usable is configured; read by the definition pipeline.
    #[inspect(hidden)]
    model_config: Result<ResolvedModelConfig, ConfigError>,
    /// Builds a `TextGen` from a resolved model config on demand, so the
    /// definition pipeline can construct it lazily and tests can inject a
    /// fake-backend `TextGen`.
    #[inspect(hidden)]
    text_gen_factory: TextGenFactory,
    /// The in-flight Done pipeline (parts-text or still-image job awaiting
    /// resolution), or `None` when no definition is in progress.
    #[inspect(hidden)]
    definition: Option<definition::PendingDefinition>,
    /// The most recent definition-pipeline error (no config / generation
    /// failure), or `None`; cleared by the next attempted Done.
    #[inspect(hidden)]
    definition_error: Option<String>,
    /// Recorded idle/attack clip jobs, one per `(egg, kind)` submitted this
    /// session; a settled entry suppresses resubmission even after failure.
    #[inspect(hidden)]
    clip_jobs: Vec<hatch_clips::ClipJob>,
    /// The in-progress hatch sequence launched from a tapped `Ready` egg, or
    /// `None` when no hatch is underway.
    #[inspect(hidden)]
    hatch: Option<hatch_render::HatchState>,
    /// The post-hatch Keep/Discard action state, or `None` before the
    /// active hatch completes.
    #[inspect(hidden)]
    roster_action: Option<hatch_roster::RosterAction>,
}

impl Hatchery {
    /// Store-less constructor for hermetic tests: no eggs, no persistence.
    /// Generation deps are built the same way the production path builds
    /// them (see `production_asset_gen`/`production_text_gen_factory`) since
    /// no job is ever submitted by construction alone.
    pub fn new() -> Self {
        Self {
            eggs: Vec::new(),
            store: None,
            back_button: RefCell::new(engine_render::ButtonCore::new(Rect::default())),
            elapsed: Duration::ZERO,
            art_cache: Vec::new(),
            focused: None,
            pending_hatch: None,
            egg_buttons: RefCell::new(Vec::new()),
            define_modal: None,
            defining_egg: None,
            asset_gen: Self::production_asset_gen(),
            model_config: resolve_model_config(),
            text_gen_factory: Self::production_text_gen_factory(),
            definition: None,
            definition_error: None,
            clip_jobs: Vec::new(),
            hatch: None,
            roster_action: None,
        }
    }

    /// Store-backed construction used by the production scene factory:
    /// loads `eggs` from the persisted `PlayerData`, keeps `store`, and
    /// promotes any egg that has already finished incubating (persisting
    /// the promotion if it fires). Generation deps are the real production
    /// ones — see `from_store_with_gen`.
    pub fn from_store(store: crate::player_data::PlayerStore) -> Self {
        Self::from_store_at(store, SystemTime::now())
    }

    /// `from_store` with an injectable `now`, so construction-time
    /// promotion (an elapsed `Incubating` egg becomes `Ready` and the
    /// promotion is persisted) is deterministic under test.
    pub(crate) fn from_store_at(store: crate::player_data::PlayerStore, now: SystemTime) -> Self {
        Self::from_store_with_gen(
            store,
            now,
            Self::production_asset_gen(),
            resolve_model_config(),
            Self::production_text_gen_factory(),
        )
    }

    /// Full injectable constructor: `from_store_at`'s load/promote body plus
    /// caller-supplied generation dependencies, so a test can drive the
    /// definition pipeline with fakes instead of the real `AssetGen`/
    /// `TextGen` backends.
    pub(crate) fn from_store_with_gen(
        store: crate::player_data::PlayerStore,
        now: SystemTime,
        asset_gen: AssetGen,
        model_config: Result<ResolvedModelConfig, ConfigError>,
        text_gen_factory: TextGenFactory,
    ) -> Self {
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
            pending_hatch: None,
            egg_buttons,
            define_modal: None,
            defining_egg: None,
            asset_gen,
            model_config,
            text_gen_factory,
            definition: None,
            definition_error: None,
            clip_jobs: Vec::new(),
            hatch: None,
            roster_action: None,
        };
        scene.tick(now);
        scene
    }

    /// Production `AssetGen`: the real `sd-cli` sibling runner (falling back
    /// to a bare `"sd-cli"` lookup on the rare `current_exe` error, rather
    /// than panicking), the real image backend, and a live GPU-capability
    /// probe.
    fn production_asset_gen() -> AssetGen {
        let runner = SdCliRunner::sibling()
            .unwrap_or_else(|_| SdCliRunner::with_bin(std::path::PathBuf::from("sd-cli")));
        AssetGen::with_env_models(std::sync::Arc::new(runner), Box::new(ZImageBackend), capability())
    }

    /// Production `TextGenFactory`: builds a real `TextGen` from whatever
    /// `ResolvedModelConfig` it is given.
    fn production_text_gen_factory() -> TextGenFactory {
        Box::new(|config: &ResolvedModelConfig| TextGen::new(config.clone()))
    }

    /// Decodes each egg's `egg_art` path once, index-aligned with `eggs`.
    /// An egg with no art, or art that fails to decode, gets `None` (a
    /// visible `EGG_UNKNOWN` placeholder in the tray render, never a panic).
    fn decode_egg_art(eggs: &[crate::player_data::Egg]) -> Vec<Option<image::DynamicImage>> {
        eggs.iter().map(|egg| egg.egg_art.as_ref().and_then(Self::decode_egg_art_one)).collect()
    }

    /// Decodes a single `ImageAsset`'s path. The sole `image::open` site for
    /// egg art; a decode failure warns and yields `None` (never a panic).
    fn decode_egg_art_one(asset: &crate::asset_gen::types::ImageAsset) -> Option<image::DynamicImage> {
        match image::open(&asset.path) {
            Ok(img) => Some(img),
            Err(e) => {
                tracing::warn!("failed to decode egg art at {:?}: {e}", asset.path);
                None
            }
        }
    }

    /// The seed used whenever a load falls through to a fresh `PlayerData`.
    /// Never written to disk on its own — see `persist_eggs`'s reload-merge.
    fn egg_seed() -> crate::player_data::PlayerData {
        crate::player_data::default_seed()
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

    /// Consumes and returns the pending "hatch this egg" request left by a
    /// completed tap on a `Ready` egg, or `None` if there is none.
    pub fn take_hatch_request(&mut self) -> Option<usize> {
        self.pending_hatch.take()
    }

    /// Opens the mad-lib definition modal for egg `index`, keyed by the same
    /// index the tray uses to select its unfilled-sentence preview.
    fn open_define_modal(&mut self, index: usize) {
        self.define_modal = Some(DefineModal::new(mad_lib::select_template(index)));
        self.defining_egg = Some(index);
    }

    /// Closes the open definition modal, if any.
    fn close_define_modal(&mut self) {
        self.define_modal = None;
        self.defining_egg = None;
    }

    /// Routes a completed tap on egg `index` by its current state: an
    /// `Incubating` egg toggles/swaps the centered focus view; an
    /// `Undefined` egg opens the definition modal; a `Ready` egg records a
    /// hatch request instead of entering focus.
    fn on_egg_tapped(&mut self, index: usize) {
        let Some(egg) = self.eggs.get(index) else { return };
        match focus::route_tap(&egg.state) {
            focus::TapRoute::Define => self.open_define_modal(index),
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
        self.poll_definition(SystemTime::now());
        self.advance_hatch_clips();
        self.advance_hatch(dt);
        self.maybe_offer_dock_actions();
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        engine_render::fill(frame.buffer_mut(), area, Self::COLOR);

        if self.hatch.is_some() {
            self.draw_hatch(frame, area);
            self.draw_dock_actions(frame, area);
            return;
        }
        if self.pending_hatch.is_some() {
            self.draw_hatch_generating(frame, area);
            return;
        }

        let dr = Self::back_dot_rect(area);
        let mut b = self.back_button.borrow_mut();
        b.set_rect(dr.to_cell_rect());
        crate::scenes::home_button::draw_badge_button(
            frame.buffer_mut(),
            dr,
            b.state(),
            crate::assets::ICON_ARROW_LEFT,
        );

        let mut buttons = self.egg_buttons.borrow_mut();
        match self.focused {
            None => {
                let slots = tray::tray_slots(tray::tray_band(area), self.eggs.len());
                for (i, slot) in slots.iter().enumerate() {
                    tray::draw_egg(
                        frame.buffer_mut(),
                        *slot,
                        &self.eggs[i],
                        self.art_cache.get(i).and_then(|a| a.as_ref()),
                        self.elapsed,
                    );
                    tray::draw_unfilled_sentence(frame.buffer_mut(), *slot, &self.eggs[i], i);
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
                    tray::draw_unfilled_sentence(frame.buffer_mut(), *slot, &self.eggs[i], i);
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

        if let Some(modal) = &self.define_modal {
            modal.render(frame, area);
        }

        self.draw_definition_error(frame.buffer_mut(), area);
    }

    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
        if let Some(h) = self.hatch.as_ref() {
            if h.seq.is_active() {
                return None;
            }
            return self.handle_post_hatch_input(ev);
        }

        if let Some(modal) = self.define_modal.as_mut() {
            let action = modal.handle_input(&ev);
            match action {
                ModalAction::Close => self.close_define_modal(),
                ModalAction::Submit(sentence) => self.begin_definition(sentence),
                ModalAction::None => {}
            }
            return None;
        }

        #[cfg(debug_assertions)]
        if let InputEvent::Key(key) = &ev {
            if self.handle_debug_hotkey(key.code) {
                return None;
            }
        }

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
    use crate::scenes::test_util::{key_event, mouse_event, render_to_buffer};
    use engine_core::scene::Scene;
    use ratatui::crossterm::event::{KeyCode, MouseButton, MouseEventKind};

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
    /// frame there is a cell carrying the bundled `?` sprite's warm yellow-gold,
    /// untinted. In the resting tray the egg is small, so per-cell braille
    /// averaging blends the `?`'s thin strokes with the dark egg body toward a
    /// muted gold (~201,160,60) rather than the sprite's pure yellow — but it
    /// stays unmistakably yellow-gold, which no element tint (reddish Fire,
    /// bluish Ice, ...) would produce from a bright mark.
    #[test]
    fn render_draws_undefined_eggs_bright_yellow_marker() {
        let dir = temp_store_dir("render-undefined-marker");
        let seed = PlayerData { roster: Vec::new(), eggs: vec![undefined_egg()] };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        // Warm yellow-gold: strong red+green, low blue. Loose enough for the
        // small-egg per-cell blend, tight enough to exclude element tints.
        let is_yellow_gold = |r: u8, g: u8, b: u8| r > 190 && g > 150 && b < 90;
        let mut found = false;
        'scan: for y in 0..h {
            for x in 0..w {
                if let Some((_, color)) = engine_render::decode_braille_cell(&buf, x, y) {
                    if is_yellow_gold(color.r, color.g, color.b) {
                        found = true;
                        break 'scan;
                    }
                }
            }
        }
        assert!(found, "expected a warm yellow-gold cell somewhere in the rendered frame for the undefined egg's `?`");
    }

    /// The Hatchery tray shows an `Undefined` egg's unfilled mad-lib
    /// sentence beside it: the selected template's literal words appear
    /// somewhere in the rendered frame.
    #[test]
    fn render_shows_unfilled_sentence_for_undefined_egg() {
        let dir = temp_store_dir("render-unfilled-sentence");
        let seed = PlayerData { roster: Vec::new(), eggs: vec![undefined_egg()] };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let template = mad_lib::select_template(0);
        let word = template
            .segments()
            .iter()
            .filter_map(|s| match s {
                mad_lib::Segment::Literal(text) => Some(*text),
                mad_lib::Segment::Blank { .. } => None,
            })
            .flat_map(str::split_whitespace)
            .find(|word| word.len() > 3)
            .expect("selected template must contain a literal word longer than 3 characters");

        let text = crate::scenes::test_util::rect_text(&buf, Rect::new(0, 0, w, h));
        assert!(
            text.contains(word),
            "expected the rendered frame to contain the unfilled sentence's {word:?}, got {text:?}"
        );
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

        let rect = tray::tray_slots(tray::tray_band(area), 1)[0].to_cell_rect();
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
        let tray_rect = tray::tray_slots(tray::tray_band(area), 1)[0].to_cell_rect();
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
        let slot0 = tray::tray_slots(tray::tray_band(area), 2)[0].to_cell_rect();
        tap_at(&mut scene, slot0.x, slot0.y);
        assert_eq!(scene.focused, Some(0));

        let _ = render_to_buffer(&scene, w, h);
        let (_, strip) = focus::focus_layout(area);
        let strip_slot1 = tray::tray_slots(strip, 2)[1].to_cell_rect();
        tap_at(&mut scene, strip_slot1.x, strip_slot1.y);
        assert_eq!(scene.focused, Some(1), "tapping a different Incubating egg must swap focus");
    }

    /// Tapping an `Undefined` egg opens the define modal directly (rather
    /// than only recording a pending request) and never enters focus.
    #[test]
    fn tap_undefined_egg_opens_define_modal() {
        let dir = temp_store_dir("tap-undefined");
        let seed = PlayerData { roster: Vec::new(), eggs: vec![undefined_egg()] };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
        let (w, h) = (40u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let _ = render_to_buffer(&scene, w, h);
        let rect = tray::tray_slots(tray::tray_band(area), 1)[0].to_cell_rect();
        tap_at(&mut scene, rect.x, rect.y);

        assert!(scene.focused.is_none(), "an Undefined egg must never enter focus");
        assert!(scene.define_modal.is_some(), "tapping an Undefined egg must open the define modal");
        assert_eq!(scene.defining_egg, Some(0), "the open modal must be keyed to the tapped egg's index");
    }

    /// With the modal open, `render` draws it over the tray: its "Done"
    /// control appears in the frame (the tray alone never draws "Done").
    #[test]
    fn render_shows_open_define_modal() {
        let mut scene = Hatchery::new();
        scene.define_modal = Some(define_modal::DefineModal::new(mad_lib::select_template(0)));
        scene.defining_egg = Some(0);

        let (w, h) = (80u16, 30u16);
        let buf = render_to_buffer(&scene, w, h);
        let text = crate::scenes::test_util::rect_text(&buf, Rect::new(0, 0, w, h));

        assert!(text.contains("Done"), "an open define modal must render its Done control, got {text:?}");
    }

    /// `Key(Esc)` while the modal is open closes it: `define_modal` and
    /// `defining_egg` both clear.
    #[test]
    fn esc_closes_open_define_modal() {
        let mut scene = Hatchery::new();
        scene.define_modal = Some(define_modal::DefineModal::new(mad_lib::select_template(0)));
        scene.defining_egg = Some(0);

        let t = scene.handle_input(key_event(KeyCode::Esc));

        assert!(scene.define_modal.is_none(), "Esc must close the open define modal");
        assert!(scene.defining_egg.is_none(), "closing the modal must clear defining_egg");
        assert!(t.is_none(), "closing the modal must not itself request a scene Transition");
    }

    /// A completed click on the modal's X (after one render seeds its
    /// layout) closes it.
    #[test]
    fn x_click_closes_open_define_modal() {
        let mut scene = Hatchery::new();
        let template = mad_lib::select_template(0);
        scene.define_modal = Some(define_modal::DefineModal::new(template));
        scene.defining_egg = Some(0);

        let (w, h) = (80u16, 30u16);
        let _ = render_to_buffer(&scene, w, h);
        let area = Rect::new(0, 0, w, h);
        let close = define_modal::DefineModal::compute_layout(area, template).close.to_cell_rect();
        let (cx, cy) = (close.x + close.width / 2, close.y + close.height / 2);
        tap_at(&mut scene, cx, cy);

        assert!(scene.define_modal.is_none(), "a completed click on the modal's X must close it");
    }

    /// While the modal is open, input is consumed by it and never reaches
    /// the tray: tapping another egg's tray slot neither enters focus nor
    /// closes the modal.
    #[test]
    fn input_while_open_is_consumed_not_routed_to_tray() {
        let dir = temp_store_dir("modal-consumes-input");
        let now = SystemTime::now();
        let seed = PlayerData {
            roster: Vec::new(),
            eggs: vec![undefined_egg(), incubating_egg(now - Duration::from_secs(3600))],
        };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);
        scene.define_modal = Some(define_modal::DefineModal::new(mad_lib::select_template(0)));
        scene.defining_egg = Some(0);

        let (w, h) = (40u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let _ = render_to_buffer(&scene, w, h);
        let slot1 = tray::tray_slots(tray::tray_band(area), 2)[1].to_cell_rect();
        tap_at(&mut scene, slot1.x, slot1.y);

        assert!(scene.focused.is_none(), "input while the modal is open must not reach the tray");
        assert!(scene.define_modal.is_some(), "the modal must remain open through unrelated tray input");
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
        let rect = tray::tray_slots(tray::tray_band(area), 1)[0].to_cell_rect();
        tap_at(&mut scene, rect.x, rect.y);

        assert!(scene.focused.is_none(), "a Ready egg must never enter focus");
        assert_eq!(scene.take_hatch_request(), Some(0));
    }

    // Hatch sequence render + scene wiring tests live in
    // `tests/hatch_sequence_tests.rs` (kept out of this file to stay under
    // the project's file-size budget).
    mod hatch_sequence_tests;
    // Settled-placement layout + stats-dock tests live in
    // `tests/hatch_settled_tests.rs`, kept out of this file for the same
    // reason as hatch_sequence_tests.
    mod hatch_settled_tests;
    // Post-hatch Keep/Discard action tests live in
    // `tests/hatch_roster_tests.rs`, kept out of this file for the same
    // reason as hatch_sequence_tests.
    mod hatch_roster_tests;
    // Slide-phase choreography tests live in `tests/hatch_slide_tests.rs`,
    // kept out of this file for the same reason as hatch_sequence_tests.
    mod hatch_slide_tests;
    // Dev-only debug hotkey tests live in `tests/hatch_dev_tests.rs`,
    // compiled only alongside the debug-only `hatch_dev` module itself.
    #[cfg(debug_assertions)]
    mod hatch_dev_tests;
}
