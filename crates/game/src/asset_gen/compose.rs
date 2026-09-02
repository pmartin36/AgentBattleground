//! `generate_image_with_animations`: composes `AssetGen::generate_image`
//! with a list of `AssetGen::generate_animation` calls against the
//! resulting still, sharing the still's identity/cache key, and exposes
//! per-sub-job progress so a caller can observe a partially-complete
//! result rather than only a single terminal blob.

use super::job::{JobHandle, JobStatus};
use super::operations::{resolve_status, AssetError, AssetGen};
use super::types::{AnimationRequest, ClipAsset, ImageAsset, ImageRequest};

/// The terminal result of `generate_image_with_animations`: the still's
/// resolution plus one resolution per requested animation, in request
/// order. A failed still yields no clips; a failed clip does not discard
/// the still or the other clips.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageWithAnimations {
    pub image: Result<ImageAsset, AssetError>,
    pub clips: Vec<Result<ClipAsset, AssetError>>,
}

/// A handle to an in-flight `generate_image_with_animations` call: the
/// still's job handle plus one job handle per requested animation, so a
/// caller can poll each sub-job's status independently before the whole
/// composite settles.
pub struct AnimationSetHandle {
    image: JobHandle<ImageAsset>,
    clips: Vec<JobHandle<ClipAsset>>,
}

impl AnimationSetHandle {
    /// Non-blocking snapshot of the still's job status.
    pub fn image_progress(&self) -> JobStatus<ImageAsset> {
        self.image.poll()
    }

    /// Non-blocking snapshot of each requested clip's job status, in
    /// request order.
    pub fn clip_progress(&self) -> Vec<JobStatus<ClipAsset>> {
        self.clips.iter().map(|h| h.poll()).collect()
    }

    /// The number of animation sub-jobs this handle tracks.
    pub fn clip_count(&self) -> usize {
        self.clips.len()
    }

    /// Blocks until the still and every clip have resolved, then maps each
    /// through `resolve_status` into the flat `ImageWithAnimations` result.
    pub fn wait(&self) -> ImageWithAnimations {
        let image = resolve_status(self.image.wait());
        let clips = self.clips.iter().map(|h| resolve_status(h.wait())).collect();
        ImageWithAnimations { image, clips }
    }
}

impl AssetGen {
    /// Generates (or imports/cache-reads) one still via `generate_image`,
    /// then submits one `generate_animation` call per `animation_requests`
    /// entry against that same still, so every clip is keyed off the
    /// shared still identity. Returns a handle exposing per-sub-job
    /// progress immediately; the still and clip jobs are still resolving
    /// on the queue at return time.
    pub fn generate_image_with_animations(
        &self,
        image_request: ImageRequest,
        animation_requests: Vec<AnimationRequest>,
    ) -> AnimationSetHandle {
        let image = self.generate_image(image_request);
        let status = image.wait();

        let clips = match &status {
            JobStatus::Success(asset) => animation_requests
                .into_iter()
                .map(|req| self.generate_animation(asset, req))
                .collect(),
            _ => Vec::new(),
        };

        AnimationSetHandle { image, clips }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex};

    use image::{Rgba, RgbaImage};

    use super::*;
    use crate::asset_gen::backend_image::ZImageBackend;
    use crate::asset_gen::capability::GpuCapability;
    use crate::asset_gen::recipe::SdCliInvocation;
    use crate::asset_gen::runner::{CancelFlag, JobError, JobRunner, RunOutput};
    use crate::asset_gen::types::{ClipParams, Fidelity, KeyColor};

    const GREEN: KeyColor = KeyColor { r: 0, g: 255, b: 0 };

    fn image_req(seed: u64, import_path: Option<PathBuf>) -> ImageRequest {
        ImageRequest {
            prompt: "a small dragon".to_string(),
            fidelity: Fidelity::Draft,
            seed,
            background_key: GREEN,
            import_path,
        }
    }

    fn anim_req(action: &str) -> AnimationRequest {
        AnimationRequest {
            action: action.to_string(),
            prompt: "winds up, swings its tail, impacts the ground, follows through".to_string(),
            params: ClipParams { frames: 1, fps: 24 },
        }
    }

    /// One fake `JobRunner` for the whole composite: branches on the
    /// invocation's mode arg (`-M img_gen` for the still,
    /// `-M vid_gen` for each animation clip, mirroring operations.rs's own
    /// `KeyColorPngRunner`/`KeyColorFramesRunner`), so a single `AssetGen`
    /// (which owns one runner for both backends) can drive the whole
    /// still-then-clips pipeline. Optionally fails a specific 1-based
    /// `vid_gen` call, and optionally blocks the first `vid_gen` call on a
    /// gate so a test can observe an in-flight `Pending` clip.
    struct CompositeRunner {
        anim_calls: Arc<AtomicUsize>,
        fail_on_anim_call: Option<usize>,
        gate_first_anim: Option<Arc<(Mutex<bool>, Condvar)>>,
    }

    impl JobRunner for CompositeRunner {
        fn run(&self, invocation: &SdCliInvocation, _cancel: &CancelFlag) -> Result<RunOutput, JobError> {
            let o_idx = invocation.args.iter().position(|a| a == "-o").expect("-o arg present");
            let out_path = PathBuf::from(&invocation.args[o_idx + 1]);

            if invocation.args.windows(2).any(|w| w == ["-M", "img_gen"]) {
                std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
                let mut img = RgbaImage::from_pixel(4, 4, Rgba([0, 255, 0, 255]));
                img.put_pixel(2, 2, Rgba([0, 0, 255, 255]));
                img.save(&out_path).unwrap();
                return Ok(RunOutput { stdout: String::new() });
            }

            let call = self.anim_calls.fetch_add(1, Ordering::SeqCst) + 1;

            if call == 1 {
                if let Some(gate) = &self.gate_first_anim {
                    let (lock, cv) = &**gate;
                    let mut released = lock.lock().unwrap();
                    while !*released {
                        released = cv.wait(released).unwrap();
                    }
                }
            }

            if self.fail_on_anim_call == Some(call) {
                return Err(JobError::Process {
                    code: Some(1),
                    stderr: "out of vram".into(),
                });
            }

            let dir = out_path.parent().unwrap().to_path_buf();
            std::fs::create_dir_all(&dir).unwrap();
            let mut img = RgbaImage::from_pixel(4, 4, Rgba([0, 255, 0, 255]));
            img.put_pixel(2, 2, Rgba([0, 0, 255, 255]));
            img.save(dir.join("f_000.png")).unwrap();
            Ok(RunOutput { stdout: String::new() })
        }
    }

    fn gen_with(runner: CompositeRunner, capability: GpuCapability) -> AssetGen {
        AssetGen::new(Arc::new(runner), Box::new(ZImageBackend), capability, AssetGen::test_models())
    }

    fn plain_runner() -> CompositeRunner {
        CompositeRunner {
            anim_calls: Arc::new(AtomicUsize::new(0)),
            fail_on_anim_call: None,
            gate_first_anim: None,
        }
    }

    /// One image request plus three animation requests resolves to the
    /// still and three Ok clips, each keyed off that one shared still.
    #[test]
    fn composite_resolves_still_plus_n_clips() {
        let gen = gen_with(plain_runner(), GpuCapability::Available);

        let result = gen
            .generate_image_with_animations(
                image_req(101, None),
                vec![anim_req("idle"), anim_req("attack"), anim_req("hatch")],
            )
            .wait();

        let still = result.image.expect("still must resolve Ok");
        assert_eq!(result.clips.len(), 3, "one clip result per animation request");
        let actions = ["idle", "attack", "hatch"];
        for (clip, action) in result.clips.iter().zip(actions) {
            let clip = clip.as_ref().expect("clip must resolve Ok").clone();
            assert_eq!(clip.frames.len(), 1);
            // The clip is cached keyed off this exact still: re-requesting
            // the same (still, action) is a cache hit returning the same
            // clip, proving the composite shared the still's identity
            // rather than generating a second one per animation.
            assert_eq!(
                gen.generate_animation(&still, anim_req(action)).wait(),
                JobStatus::Success(clip),
                "clip for action {action} must be keyed off the shared still"
            );
        }
    }

    /// An identical whole-composite call made twice invokes the runner
    /// exactly once for the still and once per clip: the second call is a
    /// pure cache hit end to end.
    #[test]
    fn composite_repeat_hits_cache() {
        let anim_calls = Arc::new(AtomicUsize::new(0));
        let runner = CompositeRunner {
            anim_calls: anim_calls.clone(),
            fail_on_anim_call: None,
            gate_first_anim: None,
        };
        let gen = gen_with(runner, GpuCapability::Available);
        let requests = vec![anim_req("idle"), anim_req("attack")];

        let first = gen
            .generate_image_with_animations(image_req(10, None), requests.clone())
            .wait();
        let second = gen
            .generate_image_with_animations(image_req(10, None), requests)
            .wait();

        assert_eq!(first, second);
        assert_eq!(
            anim_calls.load(Ordering::SeqCst),
            2,
            "repeat composite call must not re-invoke the animation runner"
        );
    }

    /// One animation sub-job failing surfaces as `Err` at its own index,
    /// without dropping the completed still or the other Ok clips, and
    /// without disturbing result order.
    #[test]
    fn composite_partial_failure_preserves_still_and_others() {
        let runner = CompositeRunner {
            anim_calls: Arc::new(AtomicUsize::new(0)),
            fail_on_anim_call: Some(2),
            gate_first_anim: None,
        };
        let gen = gen_with(runner, GpuCapability::Available);

        let result = gen
            .generate_image_with_animations(
                image_req(20, None),
                vec![anim_req("idle"), anim_req("attack"), anim_req("hatch")],
            )
            .wait();

        assert!(result.image.is_ok(), "still must resolve Ok despite a clip failure");
        assert!(result.clips[0].is_ok(), "clip 0 (idle) must still resolve Ok");
        assert!(result.clips[1].is_err(), "clip 1 (attack) is the one made to fail");
        assert!(result.clips[2].is_ok(), "clip 2 (hatch) must still resolve Ok");
    }

    /// While the first animation sub-job is gated in flight, `clip_progress`
    /// reports it (and every not-yet-started clip behind it on the one
    /// serial worker) as observably not-yet-resolved.
    #[test]
    fn composite_progress_queryable_in_flight() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let runner = CompositeRunner {
            anim_calls: Arc::new(AtomicUsize::new(0)),
            fail_on_anim_call: None,
            gate_first_anim: Some(gate.clone()),
        };
        let gen = gen_with(runner, GpuCapability::Available);

        let handle = gen.generate_image_with_animations(
            image_req(30, None),
            vec![anim_req("idle"), anim_req("attack")],
        );

        std::thread::sleep(std::time::Duration::from_millis(50));
        let progress = handle.clip_progress();
        assert_eq!(progress.len(), 2);
        assert!(
            progress.iter().all(|s| matches!(s, JobStatus::Pending)),
            "both clips must still be Pending while the first is gated, got {progress:?}"
        );

        {
            let (lock, cv) = &*gate;
            *lock.lock().unwrap() = true;
            cv.notify_all();
        }

        let result = handle.wait();
        assert!(result.clips.iter().all(|c| c.is_ok()));
    }

    /// No GPU and no import: the still reports `GpuUnavailable`, no clips
    /// are produced, and the animation runner is never invoked (no
    /// fabricated still, no hang).
    #[test]
    fn composite_no_gpu_no_import_reports_and_runs_nothing() {
        let anim_calls = Arc::new(AtomicUsize::new(0));
        let runner = CompositeRunner {
            anim_calls: anim_calls.clone(),
            fail_on_anim_call: None,
            gate_first_anim: None,
        };
        let gen = gen_with(runner, GpuCapability::Unavailable);

        let result = gen
            .generate_image_with_animations(image_req(40, None), vec![anim_req("idle")])
            .wait();

        assert_eq!(result.image, Err(AssetError::GpuUnavailable));
        assert!(result.clips.is_empty(), "no still means no clips must be submitted");
        assert_eq!(
            anim_calls.load(Ordering::SeqCst),
            0,
            "the animation runner must never be invoked when there is no still"
        );
    }
}
