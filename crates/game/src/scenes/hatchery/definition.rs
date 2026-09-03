//! The Done orchestration pipeline: submits the parts prompt, constructs the
//! creature on text success, stores it on the egg, submits the still image
//! request, and starts incubation.

use std::time::SystemTime;

use crate::ability::Element;
use crate::asset_gen::types::{ImageAsset, ImageRequest};
use crate::text_gen::job::JobHandle as TextJobHandle;
use crate::text_gen::operation::TextGen;

/// The single in-flight Done pipeline slot: a submitted parts-text job
/// awaiting resolution, or a submitted still-image job awaiting resolution.
pub(crate) enum PendingDefinition {
    AwaitingText {
        egg: usize,
        sentence: String,
        /// Kept alive so its job queue's worker can resolve `handle`; never
        /// read directly. Boxed to keep this variant close in size to
        /// `AwaitingImage`.
        #[allow(dead_code)]
        text_gen: Box<TextGen>,
        handle: TextJobHandle<String>,
    },
    AwaitingImage {
        /// Identifies which egg the resolving still image belongs to.
        egg: usize,
        handle: crate::asset_gen::JobHandle<ImageAsset>,
    },
}

/// Surfaced via `draw_definition_error` when Done is pressed with no
/// resolved model config.
pub(crate) const NO_MODEL_MESSAGE: &str = "no model configured";

/// Surfaced via `draw_definition_error` when Done is pressed against a
/// selected model_id whose weights have not been downloaded yet — distinct
/// from `NO_MODEL_MESSAGE` so the player knows to install rather than
/// configure a provider.
pub(crate) const NOT_DOWNLOADED_MESSAGE: &str = "model not downloaded";

/// Surfaced via `draw_definition_error` when Done is pressed against a
/// selected `model_id` that does not exist in the registry.
pub(crate) const UNKNOWN_MODEL_MESSAGE: &str = "unknown model";

/// Surfaced when the parts text job fails, times out, or parses to nothing
/// usable.
pub(crate) const GEN_FAILED_MESSAGE: &str = "creature generation failed";

/// Maps a `ConfigError` variant to its distinct, player-facing message: each
/// variant names a different reason nothing usable is configured, so none of
/// them may collapse into another.
fn config_error_message(err: &crate::model_config::ConfigError) -> &'static str {
    use crate::model_config::ConfigError;
    match err {
        ConfigError::NotConfigured => NO_MODEL_MESSAGE,
        ConfigError::NotDownloaded { .. } => NOT_DOWNLOADED_MESSAGE,
        ConfigError::UnknownModel { .. } => UNKNOWN_MODEL_MESSAGE,
    }
}

/// A stable seed for a given egg + completed sentence, so a construction is
/// reproducible for the same inputs. Mirrors `asset_gen::operations`'s
/// `DefaultHasher`-over-the-inputs pattern.
pub(crate) fn derive_seed(egg_index: usize, sentence: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    egg_index.hash(&mut hasher);
    sentence.hash(&mut hasher);
    hasher.finish()
}

/// Builds the still-image request for a defined egg: the completed sentence
/// as the prompt (framed per `CREATURE_FRAMING`), a draft-fidelity pass, the
/// derived seed, and a background key derived from the egg's element tint.
pub(crate) fn egg_still_request(sentence: &str, element: Element, seed: u64) -> ImageRequest {
    let tint = crate::scenes::palette::element_color(element);
    ImageRequest {
        prompt: format!("{sentence}, {}", crate::asset_gen::CREATURE_FRAMING),
        fidelity: crate::asset_gen::types::Fidelity::Draft,
        seed,
        background_key: crate::asset_gen::key_color_for([tint.r, tint.g, tint.b]),
        import_path: None,
    }
}

/// One non-blocking poll's outcome, computed while `self.definition` is
/// still borrowed, so the mutation that follows never overlaps that borrow.
enum PollOutcome {
    Pending,
    TextReady { egg: usize, sentence: String, text: String },
    TextGaveUp,
    ImageReady { egg: usize, asset: ImageAsset },
    ImageFailed,
}

impl super::Hatchery {
    /// Entry point for a completed sentence submission on the egg under
    /// edit: resolves the model config, submits the parts-text job, and
    /// stores the in-flight pending state. A no-op-with-error while a
    /// pipeline is already in flight.
    #[allow(dead_code)]
    pub(super) fn begin_definition(&mut self, sentence: String) {
        if self.definition.is_some() {
            // A second Done while one pipeline is already in flight: known,
            // safe first-pass limitation (single in-flight slot).
            return;
        }

        let Some(egg) = self.editing_egg() else { return };

        self.definition_error = None;

        let config = match &self.model_config {
            Ok(config) => config.clone(),
            Err(e) => {
                self.definition_error = Some(config_error_message(e).to_string());
                return;
            }
        };

        if let Some(e) = self.eggs.get_mut(egg) {
            e.mad_lib = Some(sentence.clone());
        }
        self.persist_eggs();

        let text_gen = (self.text_gen_factory)(&config);
        let handle = text_gen.generate_text(
            crate::scenes::hatchery::parts::build_parts_prompt(&sentence),
            crate::text_gen::operation::CachePolicy::Off,
        );
        self.definition =
            Some(PendingDefinition::AwaitingText { egg, sentence, text_gen: Box::new(text_gen), handle });
    }

    /// Advances the in-flight pipeline by one non-blocking poll. Called
    /// every frame from `update`.
    pub(super) fn poll_definition(&mut self, now: SystemTime) {
        use crate::asset_gen::JobStatus as AssetStatus;
        use crate::text_gen::JobStatus as TextStatus;

        let outcome = match &self.definition {
            None => PollOutcome::Pending,
            Some(PendingDefinition::AwaitingText { egg, sentence, handle, .. }) => match handle.poll() {
                TextStatus::Pending => PollOutcome::Pending,
                TextStatus::Success(text) => {
                    PollOutcome::TextReady { egg: *egg, sentence: sentence.clone(), text }
                }
                TextStatus::Failed(_) | TextStatus::TimedOut => PollOutcome::TextGaveUp,
            },
            Some(PendingDefinition::AwaitingImage { egg, handle }) => match handle.poll() {
                AssetStatus::Pending => PollOutcome::Pending,
                AssetStatus::Success(asset) => PollOutcome::ImageReady { egg: *egg, asset },
                AssetStatus::Failed(e) => {
                    tracing::warn!("egg {egg} still-image generation failed: {e:?}");
                    PollOutcome::ImageFailed
                }
                AssetStatus::TimedOut => {
                    tracing::warn!("egg {egg} still-image generation timed out");
                    PollOutcome::ImageFailed
                }
            },
        };

        match outcome {
            PollOutcome::Pending => {}
            PollOutcome::TextGaveUp => {
                self.definition = None;
                self.definition_error = Some(GEN_FAILED_MESSAGE.to_string());
            }
            PollOutcome::ImageReady { egg, asset } => {
                self.definition = None;
                self.apply_egg_art(egg, asset);
            }
            PollOutcome::ImageFailed => {
                self.definition = None;
            }
            PollOutcome::TextReady { egg, sentence, text } => {
                self.definition = None;
                self.on_text_success(egg, sentence, text, now);
            }
        }
    }

    /// Applies a resolved still-image asset to `egg`: stores it, re-decodes
    /// the scene's `art_cache` entry from it, and persists. A stale
    /// out-of-range `egg` index is a no-op.
    fn apply_egg_art(&mut self, egg: usize, asset: ImageAsset) {
        let decoded = super::Hatchery::decode_egg_art_one(&asset);
        let Some(e) = self.eggs.get_mut(egg) else { return };
        e.egg_art = Some(asset);
        if let Some(slot) = self.art_cache.get_mut(egg) {
            *slot = decoded;
        }
        self.persist_eggs();
    }

    /// On a successful parts-text completion: parses parts, constructs the
    /// creature, stores it on the egg, submits the still-image job, and
    /// starts incubation.
    fn on_text_success(&mut self, egg: usize, sentence: String, text: String, now: SystemTime) {
        let parts = match crate::scenes::hatchery::parts::parse_parts(&text) {
            Ok(parts) => parts,
            Err(_) => {
                self.definition_error = Some(GEN_FAILED_MESSAGE.to_string());
                return;
            }
        };

        let Some(element) = self.eggs.get(egg).map(|e| e.element) else { return };
        let seed = derive_seed(egg, &sentence);
        let request = crate::construction::ConstructionRequest::new(
            parts.name,
            parts.description,
            parts.weighting,
            parts.archetype,
            element,
            seed,
        );
        let creature = crate::construction::construct_creature(&request, None, None, None);
        let hatchling = crate::player_data::creature_to_persisted(&creature);
        if let Some(e) = self.eggs.get_mut(egg) {
            e.hatchling = Some(hatchling);
        }

        let handle = self.asset_gen.generate_image(egg_still_request(&sentence, element, seed));
        self.definition = Some(PendingDefinition::AwaitingImage { egg, handle });

        self.start_incubation(egg, now);
        self.exit_edit();
    }

    /// Renders the current definition error, if any, as a plain text line
    /// along the bottom of `area`.
    pub(super) fn draw_definition_error(&self, buf: &mut ratatui::buffer::Buffer, area: ratatui::layout::Rect) {
        let Some(message) = &self.definition_error else { return };

        let height = 2u16.min(area.height);
        let rect = ratatui::layout::Rect {
            x: area.x,
            y: area.y + area.height.saturating_sub(height),
            width: area.width,
            height,
        };
        engine_render::wrapped_text(
            buf,
            rect,
            message,
            engine_render::TextAlign::Center,
            ratatui::style::Style::default().fg(ratatui::style::Color::Rgb(0xff, 0x55, 0x55)),
            true,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset_gen::recipe::SdCliInvocation;
    use crate::asset_gen::{
        AssetGen, CancelFlag as AssetCancelFlag, GpuCapability, JobError, JobRunner, RunOutput,
        ZImageBackend, CREATURE_FRAMING,
    };
    use crate::construction::{construct_creature, ConstructionRequest};
    use crate::model_config::ConfigError;
    use crate::player_data::{creature_to_persisted, Egg, EggState, PlayerData, PlayerStore};
    use crate::scenes::hatchery::parts::parse_parts;
    use crate::scenes::palette::element_color;
    use crate::text_gen::backend::TextBackend;
    use crate::text_gen::job::CancelFlag as TextCancelFlag;
    use crate::text_gen::{Provider, ResolvedModelConfig, TextError, TextRequest};
    use engine_core::scene::{EngineCtx, Scene};
    use image::{Rgba, RgbaImage};
    use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Unique per-test temp dir, mirroring the sibling `hatchery` tests'
    /// hermetic no-`tempfile`-crate pattern.
    fn temp_store_dir(tag: &str) -> std::path::PathBuf {
        let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "game-hatchery-definition-test-{}-{}-{}",
            std::process::id(),
            tag,
            n
        ))
    }

    fn undefined_egg() -> Egg {
        Egg { element: Element::Fire, state: EggState::Undefined, mad_lib: None, egg_art: None, hatchling: None }
    }

    /// A well-formed parts completion matching `parts.rs`'s labeled-line
    /// format.
    const WELL_FORMED_PARTS: &str = "NAME: Ember\n\
DESCRIPTION: A tiny beast with smoldering eyes.\n\
STRENGTH: 8\n\
DEXTERITY: 4\n\
INTELLIGENCE: 2\n\
VITALITY: 6\n\
ARCHETYPE: Ranged\n";

    /// A completion carrying every field but a usable name — `parse_parts`'s
    /// sole `Err` case.
    const NAMELESS_PARTS: &str = "DESCRIPTION: A tiny beast.\nARCHETYPE: Ranged\n";

    fn present_model_config() -> ResolvedModelConfig {
        ResolvedModelConfig::new(Provider::Local, "m", None, Some("bin".to_string()), None)
    }

    /// A `TextBackend` fixture that always returns a fixed string.
    struct FixedBackend(String);
    impl TextBackend for FixedBackend {
        fn generate(&self, _request: &TextRequest, _cancel: &TextCancelFlag) -> Result<String, TextError> {
            Ok(self.0.clone())
        }
    }

    /// A `TextBackend` fixture that always errors.
    struct ErrBackend;
    impl TextBackend for ErrBackend {
        fn generate(&self, _request: &TextRequest, _cancel: &TextCancelFlag) -> Result<String, TextError> {
            Err(TextError::Transport("boom".to_string()))
        }
    }

    fn text_gen_factory_yielding(text: &'static str) -> super::super::TextGenFactory {
        Box::new(move |cfg: &ResolvedModelConfig| {
            TextGen::with_backend_factory(
                cfg.clone(),
                Box::new(move |_cfg: &ResolvedModelConfig| -> Box<dyn TextBackend> {
                    Box::new(FixedBackend(text.to_string()))
                }),
                Duration::from_secs(2),
            )
        })
    }

    fn text_gen_factory_erroring() -> super::super::TextGenFactory {
        Box::new(|cfg: &ResolvedModelConfig| {
            TextGen::with_backend_factory(
                cfg.clone(),
                Box::new(|_cfg: &ResolvedModelConfig| -> Box<dyn TextBackend> { Box::new(ErrBackend) }),
                Duration::from_secs(2),
            )
        })
    }

    /// Writes a synthetic solid-green PNG (with an opaque blue subject rect)
    /// to the invocation's `-o` path, and counts how many times it ran —
    /// mirrors `asset_gen::operations`'s `KeyColorPngRunner`.
    struct RecordingRunner {
        calls: Arc<AtomicUsize>,
    }
    impl JobRunner for RecordingRunner {
        fn run(&self, invocation: &SdCliInvocation, _cancel: &AssetCancelFlag) -> Result<RunOutput, JobError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let o_idx = invocation.args.iter().position(|a| a == "-o").expect("-o arg present");
            let path = std::path::PathBuf::from(&invocation.args[o_idx + 1]);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut img = RgbaImage::from_pixel(4, 4, Rgba([0, 255, 0, 255]));
            img.put_pixel(2, 2, Rgba([0, 0, 255, 255]));
            img.save(&path).unwrap();
            Ok(RunOutput { stdout: String::new() })
        }
    }

    fn fake_asset_gen(calls: Arc<AtomicUsize>) -> AssetGen {
        AssetGen::new(
            Arc::new(RecordingRunner { calls }),
            Box::new(ZImageBackend),
            GpuCapability::Available,
            AssetGen::test_models(),
        )
    }

    /// Constructs a hermetic `Hatchery` with one `Undefined` egg, the given
    /// model config, and a text-gen factory, then enters edit mode for egg
    /// 0. Returns the scene and the asset-gen call counter.
    fn scene_with_undefined_egg(
        tag: &str,
        model_config: Option<ResolvedModelConfig>,
        text_gen_factory: super::super::TextGenFactory,
    ) -> (super::super::Hatchery, Arc<AtomicUsize>) {
        let dir = temp_store_dir(tag);
        let seed = PlayerData { roster: Vec::new(), eggs: vec![undefined_egg()] };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let calls = Arc::new(AtomicUsize::new(0));
        let mut scene = super::super::Hatchery::from_store_with_gen(
            PlayerStore::with_dir(&dir),
            std::time::SystemTime::now(),
            fake_asset_gen(calls.clone()),
            model_config.ok_or(ConfigError::NotConfigured),
            text_gen_factory,
        );
        scene.enter_edit(0);
        (scene, calls)
    }

    /// Pumps `update` up to `max_ticks` times (sleeping briefly between each
    /// to let the background job queues make progress), stopping early once
    /// `done` reports true.
    fn pump(scene: &mut super::super::Hatchery, max_ticks: u32, mut done: impl FnMut(&super::super::Hatchery) -> bool) {
        let mut ctx = EngineCtx;
        for _ in 0..max_ticks {
            scene.update(&mut ctx, Duration::from_millis(5));
            if done(scene) {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// With no resolved model config, Done surfaces the generic no-model
    /// error (not the distinct not-downloaded message), starts no pipeline,
    /// and leaves the egg `Undefined` — the model/image APIs are never
    /// touched.
    #[test]
    fn done_no_config_surfaces_error_and_leaves_egg_undefined() {
        let (mut scene, calls) =
            scene_with_undefined_egg("no-config", None, text_gen_factory_yielding(WELL_FORMED_PARTS));

        scene.begin_definition("A small brave creature.".to_string());

        assert_eq!(
            scene.definition_error.as_deref(),
            Some(NO_MODEL_MESSAGE),
            "an unconfigured model must surface the generic no-model message, not a model-specific one"
        );
        assert!(scene.definition.is_none(), "an absent config must not start a pipeline");
        assert_eq!(scene.eggs[0].state, EggState::Undefined, "the egg must stay Undefined");
        assert_eq!(calls.load(Ordering::SeqCst), 0, "no image job may be submitted without a config");
    }

    /// A selected model_id whose weights have not been downloaded surfaces a
    /// message distinct from the generic no-model-configured case, starts no
    /// pipeline, and leaves the egg `Undefined`.
    #[test]
    fn done_absent_weights_surfaces_not_downloaded_message() {
        let (mut scene, calls) =
            scene_with_undefined_egg("absent-weights", None, text_gen_factory_yielding(WELL_FORMED_PARTS));
        scene.model_config =
            Err(ConfigError::NotDownloaded { model_id: "qwen3-4b-instruct".to_string() });

        scene.begin_definition("A small brave creature.".to_string());

        assert_eq!(
            scene.definition_error.as_deref(),
            Some(NOT_DOWNLOADED_MESSAGE),
            "absent weights must surface a distinct 'model not downloaded' message, not the generic \
             no-model message"
        );
        assert!(scene.definition.is_none(), "absent weights must not start a pipeline");
        assert_eq!(scene.eggs[0].state, EggState::Undefined, "the egg must stay Undefined");
        assert_eq!(calls.load(Ordering::SeqCst), 0, "no image job may be submitted without installed weights");
    }

    /// A completed Done with a present config and a well-formed parts
    /// completion drives the egg to `Incubating`, populates its hatchling
    /// and mad_lib, returns to Browsing, and submits an image job.
    #[test]
    fn done_success_incubates_with_hatchling_madlib_and_submits_image() {
        let sentence = "A small brave creature.".to_string();
        let (mut scene, calls) = scene_with_undefined_egg(
            "success",
            Some(present_model_config()),
            text_gen_factory_yielding(WELL_FORMED_PARTS),
        );

        scene.begin_definition(sentence.clone());
        pump(&mut scene, 200, |s| matches!(s.eggs[0].state, EggState::Incubating { .. }));

        assert!(
            matches!(scene.eggs[0].state, EggState::Incubating { .. }),
            "a successful Done must incubate the egg, got {:?}",
            scene.eggs[0].state
        );
        assert!(scene.eggs[0].hatchling.is_some(), "a successful Done must populate the hatchling");
        assert_eq!(scene.eggs[0].mad_lib, Some(sentence), "the egg's mad_lib must be the completed sentence");
        assert!(
            matches!(scene.mode, super::super::selection::HatcheryMode::Browsing { .. }),
            "a successful Done must return to Browsing, got {:?}",
            scene.mode
        );
        assert!(calls.load(Ordering::SeqCst) >= 1, "a successful Done must submit an image job");
    }

    /// With two `Undefined` eggs, `begin_definition` after `enter_edit(1)`
    /// defines egg 1 — not egg 0 — proving the pipeline sources the egg from
    /// the editing state rather than a fixed index.
    #[test]
    fn begin_definition_sources_egg_from_editing_state() {
        let sentence = "A small brave creature.".to_string();
        let dir = temp_store_dir("editing-state-sourcing");
        let seed = PlayerData { roster: Vec::new(), eggs: vec![undefined_egg(), undefined_egg()] };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let calls = Arc::new(AtomicUsize::new(0));
        let mut scene = super::super::Hatchery::from_store_with_gen(
            PlayerStore::with_dir(&dir),
            std::time::SystemTime::now(),
            fake_asset_gen(calls),
            Ok(present_model_config()),
            text_gen_factory_yielding(WELL_FORMED_PARTS),
        );
        scene.enter_edit(1);

        scene.begin_definition(sentence.clone());
        pump(&mut scene, 200, |s| matches!(s.eggs[1].state, EggState::Incubating { .. }));

        assert_eq!(scene.eggs[0].state, EggState::Undefined, "the egg not under edit must stay Undefined");
        assert_eq!(
            scene.eggs[1].mad_lib,
            Some(sentence),
            "the edited egg (index 1) must receive the completed sentence, not egg 0"
        );
        assert!(
            matches!(scene.eggs[1].state, EggState::Incubating { .. }),
            "the edited egg (index 1) must incubate, got {:?}",
            scene.eggs[1].state
        );
    }

    /// A text-generation failure surfaces an error and leaves the egg
    /// `Undefined` with no hatchling.
    #[test]
    fn done_text_failure_surfaces_error_and_leaves_egg_undefined() {
        let (mut scene, _calls) =
            scene_with_undefined_egg("text-failure", Some(present_model_config()), text_gen_factory_erroring());

        scene.begin_definition("A small brave creature.".to_string());
        pump(&mut scene, 200, |s| s.definition_error.is_some());

        assert!(scene.definition_error.is_some(), "a text failure must surface a definition error");
        assert_eq!(scene.eggs[0].state, EggState::Undefined, "the egg must stay Undefined");
        assert!(scene.eggs[0].hatchling.is_none(), "no hatchling may be stored on a text failure");
    }

    /// A well-formed-but-nameless parts completion is treated the same as a
    /// text failure: error set, egg stays `Undefined`, no incubation.
    #[test]
    fn done_nameless_completion_is_treated_as_failure() {
        let (mut scene, _calls) = scene_with_undefined_egg(
            "nameless",
            Some(present_model_config()),
            text_gen_factory_yielding(NAMELESS_PARTS),
        );

        scene.begin_definition("A small brave creature.".to_string());
        pump(&mut scene, 200, |s| s.definition_error.is_some());

        assert!(scene.definition_error.is_some(), "a nameless completion must surface a definition error");
        assert_eq!(scene.eggs[0].state, EggState::Undefined, "the egg must stay Undefined");
    }

    /// The stored hatchling equals `creature_to_persisted` applied to
    /// `construct_creature`'s output for the same parsed parts, egg
    /// element, and derived seed — the hatchling is the real builders'
    /// output, not a bespoke shortcut.
    #[test]
    fn stored_hatchling_matches_construct_creature_output() {
        let sentence = "A small brave creature.".to_string();
        let (mut scene, _calls) = scene_with_undefined_egg(
            "determinism",
            Some(present_model_config()),
            text_gen_factory_yielding(WELL_FORMED_PARTS),
        );

        scene.begin_definition(sentence.clone());
        pump(&mut scene, 200, |s| matches!(s.eggs[0].state, EggState::Incubating { .. }));

        let parts = parse_parts(WELL_FORMED_PARTS).expect("fixture parts must parse");
        let seed = derive_seed(0, &sentence);
        let request = ConstructionRequest::new(
            parts.name,
            parts.description,
            parts.weighting,
            parts.archetype,
            Element::Fire,
            seed,
        );
        let expected = creature_to_persisted(&construct_creature(&request, None, None, None));

        assert_eq!(
            scene.eggs[0].hatchling,
            Some(expected),
            "the stored hatchling must equal construct_creature's own output for the same inputs"
        );
    }

    /// The still-image request's prompt carries the completed sentence and
    /// the shared creature-framing clause, and its background key is
    /// derived from the egg's element tint via `key_color_for` — the one
    /// key-color rule, not a re-derived one.
    #[test]
    fn egg_still_request_builds_prompt_from_sentence_and_element_key() {
        let sentence = "A small brave creature.";
        let request = egg_still_request(sentence, Element::Fire, 7);

        assert!(request.prompt.contains(sentence), "prompt must contain the completed sentence: {}", request.prompt);
        assert!(
            request.prompt.contains(CREATURE_FRAMING),
            "prompt must contain the shared creature framing clause: {}",
            request.prompt
        );
        let tint = element_color(Element::Fire);
        let expected_key = crate::asset_gen::key_color_for([tint.r, tint.g, tint.b]);
        assert_eq!(request.background_key, expected_key, "background key must derive from the element's tint");
        assert_eq!(request.seed, 7, "seed must be threaded through verbatim");
        assert!(request.import_path.is_none(), "a freshly generated still must never carry an import path");
    }

    /// A `JobRunner` that always fails, so the still-image job resolves to
    /// `Failed` rather than a decodable asset.
    struct FailingRunner;
    impl JobRunner for FailingRunner {
        fn run(&self, _invocation: &SdCliInvocation, _cancel: &AssetCancelFlag) -> Result<RunOutput, JobError> {
            Err(JobError::Process { code: Some(1), stderr: "out of vram".into() })
        }
    }

    /// Once the still-image job submitted on text success resolves, the
    /// egg's `egg_art` is populated, the scene's decoded `art_cache` entry
    /// for that egg is re-decoded to `Some`, and the pipeline slot clears.
    #[test]
    fn image_success_sets_egg_art_and_redecodes_art_cache() {
        let sentence = "A small brave creature.".to_string();
        let (mut scene, _calls) = scene_with_undefined_egg(
            "image-success",
            Some(present_model_config()),
            text_gen_factory_yielding(WELL_FORMED_PARTS),
        );

        scene.begin_definition(sentence);
        pump(&mut scene, 400, |s| s.eggs[0].egg_art.is_some());

        assert!(scene.eggs[0].egg_art.is_some(), "a resolved still-image job must set the egg's egg_art");
        assert!(
            scene.art_cache.first().and_then(|a| a.as_ref()).is_some(),
            "a resolved still-image job must re-decode the scene's art_cache entry for that egg"
        );
        assert!(scene.definition.is_none(), "the pipeline slot must clear once the image job settles");
        assert!(
            matches!(scene.eggs[0].state, EggState::Incubating { .. }),
            "applying the resolved art must not change the egg's incubation state, got {:?}",
            scene.eggs[0].state
        );
    }

    /// Once the still-image job resolves, the tray render switches the
    /// incubating egg's slot from the untinted `?` placeholder to its
    /// element-tinted art.
    #[test]
    fn image_success_switches_tray_render_from_placeholder_to_tinted_art() {
        let sentence = "A small brave creature.".to_string();
        let (mut scene, _calls) = scene_with_undefined_egg(
            "image-success-render",
            Some(present_model_config()),
            text_gen_factory_yielding(WELL_FORMED_PARTS),
        );

        scene.begin_definition(sentence);
        pump(&mut scene, 400, |s| s.eggs[0].egg_art.is_some());
        assert!(scene.eggs[0].egg_art.is_some(), "fixture setup must resolve the image job before checking render");

        let (w, h) = (40u16, 20u16);
        let buf = crate::scenes::test_util::render_to_buffer(&scene, w, h);
        let area = ratatui::layout::Rect::new(0, 0, w, h);
        let slot = super::super::tray::tray_slots(super::super::tray::tray_band(area), 1)[0].to_cell_rect();

        let is_yellow_gold = |r: u8, g: u8, b: u8| r > 190 && g > 150 && b < 90;
        let mut found_placeholder = false;
        for y in slot.y..slot.y + slot.height {
            for x in slot.x..slot.x + slot.width {
                if let Some((_, color)) = engine_render::decode_braille_cell(&buf, x, y) {
                    if is_yellow_gold(color.r, color.g, color.b) {
                        found_placeholder = true;
                    }
                }
            }
        }
        assert!(
            !found_placeholder,
            "the egg's slot must no longer show the untinted `?` placeholder once its art resolves"
        );
    }

    /// A still-image job that fails leaves the egg's `egg_art` and the
    /// scene's `art_cache` entry untouched (the `?` placeholder stays), the
    /// egg remains `Incubating`, and nothing panics.
    #[test]
    fn image_failure_leaves_placeholder_and_does_not_panic() {
        let sentence = "A small brave creature.".to_string();
        let dir = temp_store_dir("image-failure");
        let seed = PlayerData { roster: Vec::new(), eggs: vec![undefined_egg()] };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let asset_gen = AssetGen::new(
            Arc::new(FailingRunner),
            Box::new(ZImageBackend),
            GpuCapability::Available,
            AssetGen::test_models(),
        );
        let mut scene = super::super::Hatchery::from_store_with_gen(
            PlayerStore::with_dir(&dir),
            std::time::SystemTime::now(),
            asset_gen,
            Ok(present_model_config()),
            text_gen_factory_yielding(WELL_FORMED_PARTS),
        );
        scene.enter_edit(0);

        scene.begin_definition(sentence);
        pump(&mut scene, 400, |s| matches!(s.eggs[0].state, EggState::Incubating { .. }) && s.definition.is_none());

        assert!(scene.eggs[0].egg_art.is_none(), "a failed still-image job must not set egg_art");
        assert!(
            scene.art_cache.first().and_then(|a| a.as_ref()).is_none(),
            "a failed still-image job must leave the art_cache entry as the placeholder"
        );
        assert!(scene.definition.is_none(), "the pipeline slot must clear once the failed image job settles");
        assert!(
            matches!(scene.eggs[0].state, EggState::Incubating { .. }),
            "an image failure must leave the egg Incubating, got {:?}",
            scene.eggs[0].state
        );
    }

    /// A resolved still-image job's `egg_art` durably persists: reloading the
    /// store shows the same egg with `egg_art` set.
    #[test]
    fn image_success_persists_egg_art() {
        let sentence = "A small brave creature.".to_string();
        let dir = temp_store_dir("image-success-persist");
        let seed = PlayerData { roster: Vec::new(), eggs: vec![undefined_egg()] };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

        let calls = Arc::new(AtomicUsize::new(0));
        let mut scene = super::super::Hatchery::from_store_with_gen(
            PlayerStore::with_dir(&dir),
            std::time::SystemTime::now(),
            fake_asset_gen(calls),
            Ok(present_model_config()),
            text_gen_factory_yielding(WELL_FORMED_PARTS),
        );
        scene.enter_edit(0);

        scene.begin_definition(sentence);
        pump(&mut scene, 400, |s| s.eggs[0].egg_art.is_some());
        assert!(scene.eggs[0].egg_art.is_some(), "fixture setup must resolve the image job before checking persistence");

        let reloaded = PlayerStore::with_dir(&dir)
            .load(|| panic!("must not fall back to seed"))
            .into_data();
        assert!(reloaded.eggs[0].egg_art.is_some(), "the resolved still must be persisted to disk");
    }
}
