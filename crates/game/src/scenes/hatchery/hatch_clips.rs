//! Per-creature idle/attack clip generation kicked off once an egg's still
//! resolves during incubation: submits exactly two `generate_animation`
//! jobs per egg (idle, attack), polls them to completion, and writes the
//! resolved `ClipAsset`s onto the egg's hatchling. Submission is idempotent
//! per `(egg, action)` for the scene's lifetime, and a failed/no-GPU job
//! leaves the handle `None` without resubmitting every frame.

use crate::asset_gen::types::ClipAsset;
use crate::asset_gen::{JobHandle, JobStatus};
use crate::player_data::{EggState, PersistedCreature};

/// The two clip kinds a hatchling needs; each maps to one `ACTION_TEMPLATES`
/// entry and one `PersistedCreature` handle field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ClipKind {
    Idle,
    Attack,
}

impl ClipKind {
    const ALL: [ClipKind; 2] = [ClipKind::Idle, ClipKind::Attack];

    /// The `ACTION_TEMPLATES`/`animation_request` action string for this
    /// kind.
    fn action(self) -> &'static str {
        match self {
            ClipKind::Idle => "idle",
            ClipKind::Attack => "attack",
        }
    }

    /// Whether `hatchling` already carries a resolved handle for this kind.
    fn has_handle(self, hatchling: &PersistedCreature) -> bool {
        match self {
            ClipKind::Idle => hatchling.idle.is_some(),
            ClipKind::Attack => hatchling.attack.is_some(),
        }
    }

    /// Writes `clip` onto `hatchling`'s handle for this kind.
    fn write_handle(self, hatchling: &mut PersistedCreature, clip: ClipAsset) {
        match self {
            ClipKind::Idle => hatchling.idle = Some(clip),
            ClipKind::Attack => hatchling.attack = Some(clip),
        }
    }
}

/// One submitted idle/attack clip job, tracked so it is never resubmitted
/// once recorded — a settled failure stays `settled` rather than being
/// removed, so it is not retried on the very next tick.
pub(super) struct ClipJob {
    egg: usize,
    kind: ClipKind,
    handle: JobHandle<ClipAsset>,
    settled: bool,
}

impl super::Hatchery {
    /// Single per-`update()` entry point: submits any missing idle/attack
    /// jobs for eggs whose still has resolved, then advances in-flight
    /// jobs, writing resolved clips onto their egg's hatchling and
    /// persisting.
    pub(super) fn advance_hatch_clips(&mut self) {
        self.ensure_clip_jobs();
        self.poll_clip_jobs();
    }

    /// Keeps `clip_jobs` in step with an egg removed from `self.eggs` at
    /// `removed_index`: drops jobs that belonged to the removed egg and
    /// shifts down the `egg` index of every job for an egg that sat above
    /// it, mirroring the parallel-collection removal already applied to
    /// `art_cache`/`egg_buttons`.
    pub(super) fn remove_egg_from_clip_jobs(&mut self, removed_index: usize) {
        self.clip_jobs.retain_mut(|job| match job.egg.cmp(&removed_index) {
            std::cmp::Ordering::Equal => false,
            std::cmp::Ordering::Greater => {
                job.egg -= 1;
                true
            }
            std::cmp::Ordering::Less => true,
        });
    }

    /// Scans `self.eggs` for an `Incubating` egg with a resolved `egg_art`
    /// and a hatchling missing an idle or attack clip, submitting a
    /// `generate_animation` job for each missing action not already
    /// recorded in `clip_jobs` this session.
    fn ensure_clip_jobs(&mut self) {
        for egg_index in 0..self.eggs.len() {
            let egg = &self.eggs[egg_index];
            if !matches!(egg.state, EggState::Incubating { .. }) {
                continue;
            }
            let Some(egg_art) = egg.egg_art.clone() else { continue };
            let Some(hatchling) = &egg.hatchling else { continue };

            for kind in ClipKind::ALL {
                if kind.has_handle(hatchling) {
                    continue;
                }
                let already_recorded =
                    self.clip_jobs.iter().any(|job| job.egg == egg_index && job.kind == kind);
                if already_recorded {
                    continue;
                }

                let description = egg.mad_lib.clone().unwrap_or_else(|| hatchling.name.clone());
                let Some(request) = crate::asset_gen::preset::animation_request(kind.action(), &description)
                else {
                    continue;
                };
                let handle = self.asset_gen.generate_animation(&egg_art, request);
                self.clip_jobs.push(ClipJob { egg: egg_index, kind, handle, settled: false });
            }
        }
    }

    /// Advances every unsettled recorded job: on `Success` writes the
    /// resolved `ClipAsset` to the owning egg's hatchling and persists; on
    /// `Failed`/`TimedOut` marks the job settled and logs, never writing a
    /// handle and never resubmitting on a later tick. Mirrors
    /// `definition::poll_definition`'s compute-then-mutate borrow shape.
    fn poll_clip_jobs(&mut self) {
        enum Outcome {
            Pending,
            Ready { egg: usize, kind: ClipKind, clip: ClipAsset },
            GaveUp,
        }

        let outcomes: Vec<(usize, Outcome)> = self
            .clip_jobs
            .iter()
            .enumerate()
            .filter(|(_, job)| !job.settled)
            .map(|(i, job)| {
                let outcome = match job.handle.poll() {
                    JobStatus::Pending => Outcome::Pending,
                    JobStatus::Success(clip) => Outcome::Ready { egg: job.egg, kind: job.kind, clip },
                    JobStatus::Failed(e) => {
                        tracing::warn!(
                            "egg {} {} clip generation failed: {e:?}",
                            job.egg,
                            job.kind.action()
                        );
                        Outcome::GaveUp
                    }
                    JobStatus::TimedOut => {
                        tracing::warn!("egg {} {} clip generation timed out", job.egg, job.kind.action());
                        Outcome::GaveUp
                    }
                };
                (i, outcome)
            })
            .collect();

        let mut wrote_a_clip = false;
        for (i, outcome) in outcomes {
            match outcome {
                Outcome::Pending => {}
                Outcome::GaveUp => {
                    self.clip_jobs[i].settled = true;
                }
                Outcome::Ready { egg, kind, clip } => {
                    self.clip_jobs[i].settled = true;
                    if let Some(hatchling) = self.eggs.get_mut(egg).and_then(|e| e.hatchling.as_mut()) {
                        kind.write_handle(hatchling, clip);
                        wrote_a_clip = true;
                    }
                }
            }
        }
        if wrote_a_clip {
            self.persist_eggs();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use image::{Rgba, RgbaImage};

    use crate::ability::Element;
    use crate::asset_gen::recipe::SdCliInvocation;
    use crate::asset_gen::types::ImageAsset;
    use crate::asset_gen::{
        AssetGen, CancelFlag, GpuCapability, JobError, JobRunner, RunOutput, ZImageBackend,
    };
    use crate::model_config::ConfigError;
    use crate::player_data::{Egg, EggState, PersistedCreature, PlayerData, PlayerStore};
    use crate::stamina::Stamina;
    use crate::stats::Stats;
    use crate::text_gen::operation::TextGen;
    use crate::text_gen::ResolvedModelConfig;

    /// A text-gen factory that must never be invoked: none of this file's
    /// tests exercise the Done pipeline, so the factory closure is not
    /// exercised.
    fn unused_text_gen_factory() -> super::super::TextGenFactory {
        Box::new(|_cfg: &ResolvedModelConfig| -> TextGen {
            unreachable!("no text-gen pipeline exercised by clip tests")
        })
    }

    static TMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Unique per-test temp dir, mirroring the sibling `definition` tests'
    /// hermetic no-`tempfile`-crate pattern.
    fn temp_store_dir(tag: &str) -> PathBuf {
        let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("game-hatchery-clips-test-{}-{}-{}", std::process::id(), tag, n))
    }

    /// Writes a synthetic opaque still PNG to a unique temp path, standing
    /// in for an already-resolved `egg_art`.
    fn synthetic_still(tag: &str) -> ImageAsset {
        let path = std::env::temp_dir().join(format!(
            "game-hatchery-clips-still-{}-{}.png",
            std::process::id(),
            tag
        ));
        let mut img = RgbaImage::from_pixel(4, 4, Rgba([200, 60, 40, 255]));
        img.put_pixel(2, 2, Rgba([0, 0, 255, 255]));
        img.save(&path).unwrap();
        ImageAsset { path }
    }

    /// A freshly-constructed hatchling with no clip handles yet, mirroring
    /// `on_text_success`'s stored shape before any clip resolves.
    fn clipless_hatchling() -> PersistedCreature {
        PersistedCreature::new(
            "Ember",
            Element::Fire,
            Stats { strength: 8, dexterity: 4, intelligence: 2, vitality: 6 },
            1,
            0,
            vec![],
            Stamina::default(),
            None,
            None,
            None,
        )
    }

    /// An `Incubating` egg with the given still and hatchling, matching the
    /// entry condition `advance_hatch_clips` scans for.
    fn incubating_egg(egg_art: Option<ImageAsset>, hatchling: Option<PersistedCreature>) -> Egg {
        Egg {
            element: Element::Fire,
            state: EggState::Incubating { started_at: SystemTime::now() },
            mad_lib: Some("a small brave creature".to_string()),
            egg_art,
            hatchling,
        }
    }

    /// A `JobRunner` that counts invocations and either fails every call or
    /// writes one synthetic frame to the invocation's `-o` output directory.
    /// Every invocation this scene submits IS an animation invocation (the
    /// egg's still is pre-resolved, so `generate_image` is never called
    /// here), so a plain call count IS the animation-submission count.
    struct ClipRunner {
        calls: Arc<AtomicUsize>,
        fail: bool,
    }

    impl JobRunner for ClipRunner {
        fn run(&self, invocation: &SdCliInvocation, _cancel: &CancelFlag) -> Result<RunOutput, JobError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err(JobError::Process { code: Some(1), stderr: "out of vram".into() });
            }
            let o_idx = invocation.args.iter().position(|a| a == "-o").expect("-o arg present");
            let out_path = PathBuf::from(&invocation.args[o_idx + 1]);
            std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
            let mut img = RgbaImage::from_pixel(4, 4, Rgba([0, 255, 0, 255]));
            img.put_pixel(2, 2, Rgba([0, 0, 255, 255]));
            img.save(&out_path).unwrap();
            Ok(RunOutput { stdout: String::new() })
        }
    }

    fn fake_asset_gen(calls: Arc<AtomicUsize>, fail: bool, capability: GpuCapability) -> AssetGen {
        AssetGen::new(
            Arc::new(ClipRunner { calls, fail }),
            Box::new(ZImageBackend),
            capability,
            AssetGen::test_models(),
        )
        .with_extractor(AssetGen::test_extractor())
    }

    /// Builds a hermetic store seeded with one egg and a `Hatchery` scene
    /// over it with the given `AssetGen`.
    fn scene_with_egg(tag: &str, egg: Egg, asset_gen: AssetGen) -> super::super::Hatchery {
        let dir = temp_store_dir(tag);
        let seed = PlayerData { roster: Vec::new(), eggs: vec![egg] };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");
        super::super::Hatchery::from_store_with_gen(
            PlayerStore::with_dir(&dir),
            SystemTime::now(),
            asset_gen,
            Err(ConfigError::NotConfigured),
            unused_text_gen_factory(),
        )
    }

    /// Same as `scene_with_egg` but also returns the store's directory, so a
    /// test can rebuild a second scene over the same on-disk data or reload
    /// it directly.
    fn scene_with_egg_dir(tag: &str, egg: Egg, asset_gen: AssetGen) -> (super::super::Hatchery, PathBuf) {
        let dir = temp_store_dir(tag);
        let seed = PlayerData { roster: Vec::new(), eggs: vec![egg] };
        PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");
        let scene = super::super::Hatchery::from_store_with_gen(
            PlayerStore::with_dir(&dir),
            SystemTime::now(),
            asset_gen,
            Err(ConfigError::NotConfigured),
            unused_text_gen_factory(),
        );
        (scene, dir)
    }

    /// Calls `advance_hatch_clips` up to `max_ticks` times (sleeping briefly
    /// between calls so the background job queue can make progress),
    /// stopping early once `done` reports true.
    fn pump(
        scene: &mut super::super::Hatchery,
        max_ticks: u32,
        mut done: impl FnMut(&super::super::Hatchery) -> bool,
    ) {
        for _ in 0..max_ticks {
            scene.advance_hatch_clips();
            if done(scene) {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn has_both_clips(scene: &super::super::Hatchery) -> bool {
        scene.eggs[0]
            .hatchling
            .as_ref()
            .map(|h| h.idle.is_some() && h.attack.is_some())
            .unwrap_or(false)
    }

    /// Once an `Incubating` egg's still is resolved, exactly two animation
    /// jobs (idle + attack) are submitted, and on success both the egg's
    /// `hatchling.idle` and `.attack` become populated.
    #[test]
    fn resolves_still_then_submits_idle_and_attack_once() {
        let egg_art = synthetic_still("submit-once");
        let calls = Arc::new(AtomicUsize::new(0));
        let mut scene = scene_with_egg(
            "submit-once",
            incubating_egg(Some(egg_art), Some(clipless_hatchling())),
            fake_asset_gen(calls.clone(), false, GpuCapability::Available),
        );

        pump(&mut scene, 200, has_both_clips);

        assert!(
            has_both_clips(&scene),
            "a resolved still must yield both idle and attack clips on the hatchling"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "exactly one job per action (idle, attack) must be submitted"
        );
    }

    /// After both clips are populated, further ticks submit no additional
    /// jobs (in-session idempotency).
    #[test]
    fn second_tick_after_clips_ready_submits_no_more() {
        let egg_art = synthetic_still("no-more");
        let calls = Arc::new(AtomicUsize::new(0));
        let mut scene = scene_with_egg(
            "no-more",
            incubating_egg(Some(egg_art), Some(clipless_hatchling())),
            fake_asset_gen(calls.clone(), false, GpuCapability::Available),
        );

        pump(&mut scene, 200, has_both_clips);
        assert!(has_both_clips(&scene), "fixture setup must resolve both clips before checking idempotency");
        let after_ready = calls.load(Ordering::SeqCst);

        for _ in 0..20 {
            scene.advance_hatch_clips();
        }

        assert_eq!(
            calls.load(Ordering::SeqCst),
            after_ready,
            "no further job may be submitted once both clips are already resolved"
        );
    }

    /// A fresh scene rebuilt over a store that already has both clips
    /// persisted submits no jobs at all (cross-session idempotency).
    #[test]
    fn reload_with_persisted_clips_submits_none() {
        let egg_art = synthetic_still("reload-none");
        let first_calls = Arc::new(AtomicUsize::new(0));
        let (mut scene, dir) = scene_with_egg_dir(
            "reload-none",
            incubating_egg(Some(egg_art), Some(clipless_hatchling())),
            fake_asset_gen(first_calls.clone(), false, GpuCapability::Available),
        );
        pump(&mut scene, 200, has_both_clips);
        assert!(has_both_clips(&scene), "fixture setup must resolve both clips before the reload check");

        let second_calls = Arc::new(AtomicUsize::new(0));
        let mut reloaded = super::super::Hatchery::from_store_with_gen(
            PlayerStore::with_dir(&dir),
            SystemTime::now(),
            fake_asset_gen(second_calls.clone(), false, GpuCapability::Available),
            Err(ConfigError::NotConfigured),
            unused_text_gen_factory(),
        );

        for _ in 0..20 {
            reloaded.advance_hatch_clips();
        }

        assert_eq!(
            second_calls.load(Ordering::SeqCst),
            0,
            "an egg whose clips are already persisted must submit nothing on a fresh scene"
        );
    }

    /// A resolved clip pair durably persists: reloading the store directly
    /// (bypassing the scene entirely) shows the same egg with both handles
    /// set.
    #[test]
    fn clips_persist_round_trip() {
        let egg_art = synthetic_still("persist-round-trip");
        let calls = Arc::new(AtomicUsize::new(0));
        let (mut scene, dir) = scene_with_egg_dir(
            "persist-round-trip",
            incubating_egg(Some(egg_art), Some(clipless_hatchling())),
            fake_asset_gen(calls, false, GpuCapability::Available),
        );
        pump(&mut scene, 200, has_both_clips);
        assert!(has_both_clips(&scene), "fixture setup must resolve both clips before checking persistence");

        let reloaded = PlayerStore::with_dir(&dir).load(|| panic!("must not fall back to seed")).into_data();
        let hatchling = reloaded.eggs[0].hatchling.as_ref().expect("hatchling must survive reload");
        assert!(hatchling.idle.is_some(), "idle clip must be persisted to disk");
        assert!(hatchling.attack.is_some(), "attack clip must be persisted to disk");
    }

    /// A failing runner leaves both clip handles `None`, the egg stays
    /// `Incubating`, nothing panics, and a settled failure is not
    /// resubmitted on every subsequent tick.
    #[test]
    fn clip_job_failure_leaves_handles_none_and_does_not_loop() {
        let egg_art = synthetic_still("failure");
        let calls = Arc::new(AtomicUsize::new(0));
        let mut scene = scene_with_egg(
            "failure",
            incubating_egg(Some(egg_art), Some(clipless_hatchling())),
            fake_asset_gen(calls.clone(), true, GpuCapability::Available),
        );

        for _ in 0..50 {
            scene.advance_hatch_clips();
            std::thread::sleep(Duration::from_millis(2));
        }

        let hatchling = scene.eggs[0].hatchling.as_ref().expect("hatchling must remain present");
        assert!(hatchling.idle.is_none(), "a failed job must leave the idle handle None");
        assert!(hatchling.attack.is_none(), "a failed job must leave the attack handle None");
        assert!(
            matches!(scene.eggs[0].state, EggState::Incubating { .. }),
            "a clip failure must not change the egg's incubation state, got {:?}",
            scene.eggs[0].state
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "a settled failure must not be resubmitted on every subsequent tick"
        );
    }

    /// With no GPU available, both clip handles stay `None` and incubation
    /// is not blocked (no panic, egg stays `Incubating`).
    #[test]
    fn no_gpu_leaves_handles_none_without_blocking() {
        let egg_art = synthetic_still("no-gpu");
        let calls = Arc::new(AtomicUsize::new(0));
        let mut scene = scene_with_egg(
            "no-gpu",
            incubating_egg(Some(egg_art), Some(clipless_hatchling())),
            fake_asset_gen(calls, false, GpuCapability::Unavailable),
        );

        for _ in 0..10 {
            scene.advance_hatch_clips();
        }

        let hatchling = scene.eggs[0].hatchling.as_ref().expect("hatchling must remain present");
        assert!(hatchling.idle.is_none(), "no-GPU must leave the idle handle None");
        assert!(hatchling.attack.is_none(), "no-GPU must leave the attack handle None");
        assert!(
            matches!(scene.eggs[0].state, EggState::Incubating { .. }),
            "no-GPU must not block or change incubation, got {:?}",
            scene.eggs[0].state
        );
    }

    /// An `Incubating` egg whose still has not yet resolved (`egg_art ==
    /// None`) submits nothing — there is nothing to animate yet.
    #[test]
    fn still_unresolved_submits_nothing() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut scene = scene_with_egg(
            "unresolved",
            incubating_egg(None, Some(clipless_hatchling())),
            fake_asset_gen(calls.clone(), false, GpuCapability::Available),
        );

        for _ in 0..10 {
            scene.advance_hatch_clips();
        }

        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "an egg with no resolved still must submit no animation job"
        );
    }
}
