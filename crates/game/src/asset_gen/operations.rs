//! Shared operations scaffolding (`AssetError`, `resolve_status`, the
//! deterministic asset-path helpers) plus `AssetGen::generate_image`, the
//! image-generation entry point: cache read, import/no-GPU short-circuits,
//! and the GPU-generation path (submit through the job queue, then
//! background-remove and cache the result).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::backend_animation::{H3Job, MiniMaxH3Backend};
use super::backend_image::key_color_for;
use super::bg_removal::{remove_frame_background, remove_still_background};
use super::cache::AssetCache;
use super::capability::GpuCapability;
use super::frame_extract::{FfmpegExtractor, FrameExtractor};
use super::job::{JobHandle, JobQueue, JobStatus};
use super::model_paths::{ModelPathError, ModelPaths};
use super::recipe::RecipeBackend;
use super::runner::{JobError, JobRunner, RunOutput};
use super::types::{AnimationRequest, ClipAsset, ImageAsset, ImageRequest, KeyColor};

/// Generation is minutes-long; a fake runner in tests resolves in
/// milliseconds, well under this bound.
pub const DEFAULT_JOB_TIMEOUT: Duration = Duration::from_secs(600);

/// The caller-facing flat error shape a `JobStatus` resolves to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetError {
    GpuUnavailable,
    TimedOut,
    Generation(JobError),
}

/// Maps a terminal `JobStatus` to the flat `AssetError` form: `Success`
/// unwraps; `Failed(NoGpu)` becomes `GpuUnavailable`; any other `Failed`
/// becomes `Generation`; `TimedOut` passes through.
pub fn resolve_status<T>(status: JobStatus<T>) -> Result<T, AssetError> {
    match status {
        JobStatus::Success(value) => Ok(value),
        JobStatus::Failed(JobError::NoGpu) => Err(AssetError::GpuUnavailable),
        JobStatus::Failed(other) => Err(AssetError::Generation(other)),
        JobStatus::TimedOut => Err(AssetError::TimedOut),
        JobStatus::Pending => Err(AssetError::Generation(JobError::Io(
            "resolve_status called on a Pending status".to_string(),
        ))),
    }
}

/// The deterministic path sd-cli is told to write its raw (pre-background-
/// removal) PNG to for a given request. The image backend's `-o` arg and
/// the operation's materialize step both derive this from the request so
/// they can never disagree.
pub(crate) fn image_raw_path(request: &ImageRequest) -> PathBuf {
    asset_path(request, "raw.png")
}

/// The deterministic path of the final, background-removed asset for a
/// given request.
pub(crate) fn image_asset_path(request: &ImageRequest) -> PathBuf {
    asset_path(request, "png")
}

fn asset_path(request: &ImageRequest, extension: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    request.hash(&mut hasher);
    let hash = hasher.finish();
    std::env::temp_dir()
        .join("abg_assets")
        .join(format!("{hash:x}.{extension}"))
}

/// The pre-cleaned, flat-key-backed still `generate_animation` hands H3 as
/// its `--init-img`, for a given `(image, action)`.
pub(crate) fn clip_clean_still_path(image: &ImageAsset, action: &str) -> PathBuf {
    std::env::temp_dir()
        .join("abg_assets")
        .join(format!("{:x}.clean.png", clip_hash(image, action)))
}

/// The directory the runner is told (`-o`) to leave its raw key-color output
/// frames in, for a given `(image, action)`. The `-o` arg and the
/// materialize read both derive this so they can never disagree.
pub(crate) fn clip_raw_frames_dir(image: &ImageAsset, action: &str) -> PathBuf {
    std::env::temp_dir()
        .join("abg_assets")
        .join(format!("{:x}.raw_frames", clip_hash(image, action)))
}

/// The directory the final, background-removed output frames are written
/// to, for a given `(image, action)`.
pub(crate) fn clip_out_frames_dir(image: &ImageAsset, action: &str) -> PathBuf {
    std::env::temp_dir()
        .join("abg_assets")
        .join(format!("{:x}.frames", clip_hash(image, action)))
}

/// The path the runner's `-o` arg is pointed at for a `vid_gen` invocation's
/// single video output, for a given `(image, action)`. Kept in a directory
/// distinct from `clip_raw_frames_dir` so the video file the extractor reads
/// is never mixed in with (and miscounted as) one of the frames
/// `materialize_clip` reads.
pub(crate) fn clip_video_out_path(image: &ImageAsset, action: &str) -> PathBuf {
    std::env::temp_dir()
        .join("abg_assets")
        .join(format!("{:x}.video", clip_hash(image, action)))
        .join("anim.png")
}

fn clip_hash(image: &ImageAsset, action: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    image.hash(&mut hasher);
    action.hash(&mut hasher);
    hasher.finish()
}

/// Owns the cache, job queue, GPU-capability reading, and image backend
/// behind `generate_image`.
pub struct AssetGen {
    cache: Arc<AssetCache>,
    queue: JobQueue,
    capability: GpuCapability,
    image_backend: Box<dyn RecipeBackend<Request = ImageRequest>>,
    timeout: Duration,
    resolver: Result<ModelPaths, ModelPathError>,
    extractor: Arc<dyn FrameExtractor>,
}

impl AssetGen {
    pub fn new(
        runner: Arc<dyn JobRunner>,
        image_backend: Box<dyn RecipeBackend<Request = ImageRequest>>,
        capability: GpuCapability,
        models: ModelPaths,
    ) -> Self {
        Self::with_timeout(runner, image_backend, capability, models, DEFAULT_JOB_TIMEOUT)
    }

    /// Same as `new`, with an injectable timeout so a test can use a short
    /// bound instead of the real multi-minute default.
    pub fn with_timeout(
        runner: Arc<dyn JobRunner>,
        image_backend: Box<dyn RecipeBackend<Request = ImageRequest>>,
        capability: GpuCapability,
        models: ModelPaths,
        timeout: Duration,
    ) -> Self {
        AssetGen {
            cache: Arc::new(AssetCache::new()),
            queue: JobQueue::new(runner),
            capability,
            image_backend,
            timeout,
            resolver: Ok(models),
            extractor: Arc::new(FfmpegExtractor),
        }
    }

    /// Overrides the frame extractor, for a test that must observe or fake
    /// the video-to-frames step without spawning a real `ffmpeg`.
    #[cfg(test)]
    pub(crate) fn with_extractor(mut self, extractor: Arc<dyn FrameExtractor>) -> Self {
        self.extractor = extractor;
        self
    }

    /// Production entry: the ONLY constructor that reads
    /// `AGENTBATTLEGROUND_SDCLI_MODELS_DIR` (via `ModelPaths::from_env`).
    /// Every test constructs an explicit `ModelPaths` instead (see
    /// `test_models`), so tests stay hermetic.
    pub(crate) fn with_env_models(
        runner: Arc<dyn JobRunner>,
        image_backend: Box<dyn RecipeBackend<Request = ImageRequest>>,
        capability: GpuCapability,
    ) -> Self {
        AssetGen {
            cache: Arc::new(AssetCache::new()),
            queue: JobQueue::new(runner),
            capability,
            image_backend,
            timeout: DEFAULT_JOB_TIMEOUT,
            resolver: ModelPaths::from_env(),
            extractor: Arc::new(FfmpegExtractor),
        }
    }

    /// The one shared non-verifying resolver every test site constructs an
    /// `AssetGen` with, so resolution never needs a real models dir in the
    /// offline gate.
    #[cfg(test)]
    pub(crate) fn test_models() -> ModelPaths {
        ModelPaths::unchecked(std::env::temp_dir().join("abg-test-models"))
    }

    /// The one shared fake extractor every animation test site injects
    /// instead of spawning a real `ffmpeg`.
    #[cfg(test)]
    pub(crate) fn test_extractor() -> Arc<dyn FrameExtractor> {
        Arc::new(crate::asset_gen::frame_extract::DuplicatingFakeExtractor)
    }

    /// The GPU capability this `AssetGen` was constructed with, so a caller
    /// can choose its fallback before submitting.
    pub fn capability(&self) -> GpuCapability {
        self.capability
    }

    /// Generates (or imports, or serves from cache) a single still image.
    /// Branches: cache hit; explicit import (no runner, no chroma-key);
    /// GPU-available generation (submit, then background-remove and cache);
    /// GPU-unavailable with no import (a reported failure, never a hang).
    pub fn generate_image(&self, request: ImageRequest) -> JobHandle<ImageAsset> {
        if let Some(asset) = self.cache.get_image(&request) {
            return JobHandle::resolved(JobStatus::Success(asset));
        }

        if let Some(path) = request.import_path.clone() {
            let asset = ImageAsset { path };
            let stored = self
                .cache
                .image_or_bake(&request, || Ok::<ImageAsset, std::convert::Infallible>(asset.clone()))
                .unwrap_or(asset);
            return JobHandle::resolved(JobStatus::Success(stored));
        }

        if self.capability != GpuCapability::Available {
            return JobHandle::resolved(JobStatus::Failed(JobError::NoGpu));
        }

        let models = match &self.resolver {
            Ok(models) => models,
            Err(e) => return JobHandle::resolved(JobStatus::Failed(JobError::ModelPath(e.clone()))),
        };
        let invocation = match self.image_backend.invocation(&request, models) {
            Ok(invocation) => invocation,
            Err(e) => return JobHandle::resolved(JobStatus::Failed(JobError::ModelPath(e))),
        };
        let cache = Arc::clone(&self.cache);
        let materialize_request = request.clone();
        self.queue.submit(invocation, self.timeout, move |output: RunOutput| {
            materialize_image(&cache, &materialize_request, output)
        })
    }

    /// Generates (or serves from cache) one animation clip for an existing
    /// still. Branches, in order: cache hit; GPU gate (animation has no
    /// no-GPU fallback, unlike `generate_image`'s import route); a
    /// synchronous pre-clean that background-removes the still and flattens
    /// it onto an opaque flat-key background (the `--init-img` H3
    /// preserves); submit through the queue; per-frame background removal
    /// on materialize.
    pub fn generate_animation(&self, image: &ImageAsset, request: AnimationRequest) -> JobHandle<ClipAsset> {
        if let Some(clip) = self.cache.get_clip(image, &request.action) {
            return JobHandle::resolved(JobStatus::Success(clip));
        }

        if self.capability != GpuCapability::Available {
            return JobHandle::resolved(JobStatus::Failed(JobError::NoGpu));
        }

        let models = match &self.resolver {
            Ok(models) => models,
            Err(e) => return JobHandle::resolved(JobStatus::Failed(JobError::ModelPath(e.clone()))),
        };

        let still = match image::open(&image.path) {
            Ok(still) => still.to_rgba8(),
            Err(e) => {
                return JobHandle::resolved(JobStatus::Failed(JobError::Io(format!(
                    "failed to read still at {:?}: {e}",
                    image.path
                ))));
            }
        };

        let key = key_color_for(dominant_opaque_color(&still));
        let clean = remove_still_background(&still, key.clone());
        let flattened = flatten_onto_key(&clean, key.clone());

        let clean_still_path = clip_clean_still_path(image, &request.action);
        if let Some(parent) = clean_still_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return JobHandle::resolved(JobStatus::Failed(JobError::Io(format!(
                    "failed to create clip directory {parent:?}: {e}"
                ))));
            }
        }
        if let Err(e) = flattened.save(&clean_still_path) {
            return JobHandle::resolved(JobStatus::Failed(JobError::Io(format!(
                "failed to write pre-cleaned still to {clean_still_path:?}: {e}"
            ))));
        }

        let job = H3Job {
            init_img: clean_still_path,
            output: clip_video_out_path(image, &request.action),
            prompt: request.prompt.clone(),
            key: key.clone(),
            frames: request.params.frames,
            fps: request.params.fps,
        };
        let invocation = match MiniMaxH3Backend.invocation(&job, models) {
            Ok(invocation) => invocation,
            Err(e) => return JobHandle::resolved(JobStatus::Failed(JobError::ModelPath(e))),
        };

        let extractor = Arc::clone(&self.extractor);
        let video_out = job.output.clone();
        let frames_dir = clip_raw_frames_dir(image, &request.action);
        let frames = request.params.frames;
        let fps = request.params.fps;

        let cache = Arc::clone(&self.cache);
        let materialize_image = image.clone();
        let materialize_action = request.action.clone();
        self.queue.submit(invocation, self.timeout, move |output: RunOutput| {
            if let Err(e) = extractor.extract(&video_out, &frames_dir, frames, fps) {
                tracing::warn!("frame extraction failed for {video_out:?}: {e}");
            }
            materialize_clip(&cache, &materialize_image, &materialize_action, key, output)
        })
    }
}

/// Converts a completed H3 run into an ordered sequence of
/// background-removed clip frames and caches the result: reads the raw
/// key-color frames the runner left in `clip_raw_frames_dir(image, action)`
/// (sorted by file name for playback order), removes the background from
/// each against `key`, writes each cleaned RGBA PNG to
/// `clip_out_frames_dir(image, action)`, and stores the resulting
/// `ClipAsset` via `AssetCache::clip_or_bake`. IO failures are best-effort
/// (logged, returning the intended/partial `ClipAsset`), since the job's
/// `materialize` callback cannot return a `Result`.
fn materialize_clip(
    cache: &AssetCache,
    image: &ImageAsset,
    action: &str,
    key: KeyColor,
    _output: RunOutput,
) -> ClipAsset {
    let raw_dir = clip_raw_frames_dir(image, action);
    let out_dir = clip_out_frames_dir(image, action);

    let mut raw_frames: Vec<PathBuf> = match std::fs::read_dir(&raw_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect(),
        Err(e) => {
            tracing::warn!("failed to read raw clip frames dir {raw_dir:?}: {e}");
            Vec::new()
        }
    };
    raw_frames.sort();

    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        tracing::warn!("failed to create clip output dir {out_dir:?}: {e}");
    }

    let mut frames = Vec::with_capacity(raw_frames.len());
    for (i, raw_path) in raw_frames.iter().enumerate() {
        let out_path = out_dir.join(format!("frame_{i:03}.png"));
        match image::open(raw_path) {
            Ok(raw) => {
                let removed = remove_frame_background(&raw.to_rgba8(), key.clone());
                if let Err(e) = removed.save(&out_path) {
                    tracing::warn!("failed to write background-removed frame to {out_path:?}: {e}");
                }
            }
            Err(e) => {
                tracing::warn!("failed to read raw clip frame at {raw_path:?}: {e}");
            }
        }
        frames.push(out_path);
    }

    let clip = ClipAsset { frames };
    cache
        .clip_or_bake(image, action, || Ok::<ClipAsset, std::convert::Infallible>(clip.clone()))
        .unwrap_or(clip)
}

/// The mean RGB of `img`'s opaque (alpha >= 128) pixels: the subject's
/// dominant color, ignoring an already-removed transparent background so a
/// clean still keys off the subject rather than its removed field.
fn dominant_opaque_color(img: &image::RgbaImage) -> [u8; 3] {
    let mut sum = [0u64; 3];
    let mut count = 0u64;
    for px in img.pixels() {
        let [r, g, b, a] = px.0;
        if a >= 128 {
            sum[0] += r as u64;
            sum[1] += g as u64;
            sum[2] += b as u64;
            count += 1;
        }
    }
    if count == 0 {
        return [0, 0, 0];
    }
    [
        (sum[0] / count) as u8,
        (sum[1] / count) as u8,
        (sum[2] / count) as u8,
    ]
}

/// Composites `subject` (already background-removed; opaque where retained)
/// onto a solid opaque `key`-colored background of the same dimensions,
/// producing the flat-background init image H3's style-preservation prompt
/// describes. Compositing glue, not a second background-removal
/// implementation.
fn flatten_onto_key(subject: &image::RgbaImage, key: KeyColor) -> image::RgbaImage {
    let (w, h) = subject.dimensions();
    let mut out = image::RgbaImage::from_pixel(w, h, image::Rgba([key.r, key.g, key.b, 255]));
    for (x, y, px) in subject.enumerate_pixels() {
        if px.0[3] > 0 {
            out.put_pixel(x, y, *px);
        }
    }
    out
}

/// Converts a completed GPU-generation run into a background-removed
/// `ImageAsset` and caches it: reads the raw key-color PNG the runner wrote
/// to `image_raw_path`, removes the background against the request's key,
/// writes the result to `image_asset_path`, and stores it via
/// `AssetCache::image_or_bake`. IO failures are best-effort: they are logged
/// and the intended asset path is returned rather than propagated, since the
/// job's `materialize` callback cannot return a `Result`.
fn materialize_image(cache: &AssetCache, request: &ImageRequest, _output: RunOutput) -> ImageAsset {
    let raw_path = image_raw_path(request);
    let final_path = image_asset_path(request);
    let asset = ImageAsset { path: final_path.clone() };

    match image::open(&raw_path) {
        Ok(raw) => {
            let removed = remove_still_background(&raw.to_rgba8(), request.background_key.clone());
            if let Err(e) = removed.save(&final_path) {
                tracing::warn!("failed to write background-removed image to {final_path:?}: {e}");
            }
        }
        Err(e) => {
            tracing::warn!("failed to read raw generated image at {raw_path:?}: {e}");
        }
    }

    cache
        .image_or_bake(request, || Ok::<ImageAsset, std::convert::Infallible>(asset.clone()))
        .unwrap_or(asset)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use image::{Rgba, RgbaImage};

    use super::*;
    use crate::asset_gen::backend_image::ZImageBackend;
    use crate::asset_gen::recipe::SdCliInvocation;
    use crate::asset_gen::runner::{CancelFlag, RunOutput};
    use crate::asset_gen::types::{ClipParams, Fidelity, KeyColor};

    const GREEN: KeyColor = KeyColor { r: 0, g: 255, b: 0 };

    fn req(seed: u64, import_path: Option<PathBuf>) -> ImageRequest {
        ImageRequest {
            prompt: "a small dragon".to_string(),
            fidelity: Fidelity::Draft,
            seed,
            background_key: GREEN,
            import_path,
        }
    }

    /// Writes a synthetic transparent-field PNG with a single opaque
    /// `subject` pixel to a unique temp path and returns it as an
    /// `ImageAsset`, standing in for an already-generated (or imported)
    /// still.
    fn synthetic_still(tag: &str, subject: [u8; 4]) -> ImageAsset {
        let path = std::env::temp_dir().join(format!("abg_assets_test_{tag}.png"));
        let mut img = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 0]));
        img.put_pixel(2, 2, Rgba(subject));
        img.save(&path).unwrap();
        ImageAsset { path }
    }

    fn animation_req(action: &str, frames: u32, fps: u32) -> AnimationRequest {
        AnimationRequest {
            action: action.to_string(),
            prompt: "winds up, swings its tail, impacts the ground, follows through".to_string(),
            params: ClipParams { frames, fps },
        }
    }

    /// Stands in for `sd-cli`'s video-generation step: reads the `-o`
    /// invocation arg and writes ONE synthetic solid-`field` PNG (with an
    /// opaque blue subject rect) to that exact path, standing in for the
    /// single video a real `vid_gen` run produces. Captures the `--init-img`
    /// and `-p` args it received, and counts how many times it ran; frame
    /// extraction (multiplying this one image into a sequence) is the
    /// injected `FrameExtractor`'s job, not the runner's.
    struct KeyColorFramesRunner {
        calls: Arc<AtomicUsize>,
        field: [u8; 4],
        captured_init_img: Arc<Mutex<Option<PathBuf>>>,
        captured_prompt: Arc<Mutex<Option<String>>>,
    }

    impl JobRunner for KeyColorFramesRunner {
        fn run(&self, invocation: &SdCliInvocation, _cancel: &CancelFlag) -> Result<RunOutput, JobError> {
            self.calls.fetch_add(1, Ordering::SeqCst);

            let o_idx = invocation.args.iter().position(|a| a == "-o").expect("-o arg present");
            let out_path = PathBuf::from(&invocation.args[o_idx + 1]);
            std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
            let mut img = RgbaImage::from_pixel(4, 4, Rgba(self.field));
            img.put_pixel(2, 2, Rgba([0, 0, 255, 255]));
            img.save(&out_path).unwrap();

            if let Some(idx) = invocation.args.iter().position(|a| a == "--init-img") {
                *self.captured_init_img.lock().unwrap() = Some(PathBuf::from(&invocation.args[idx + 1]));
            }
            if let Some(idx) = invocation.args.iter().position(|a| a == "-p") {
                *self.captured_prompt.lock().unwrap() = Some(invocation.args[idx + 1].clone());
            }

            Ok(RunOutput { stdout: String::new() })
        }
    }

    /// Writes a synthetic solid-green PNG (with an opaque blue subject
    /// rect) to the invocation's `-o` path, and counts how many times it
    /// ran.
    struct KeyColorPngRunner {
        calls: Arc<AtomicUsize>,
    }

    impl JobRunner for KeyColorPngRunner {
        fn run(&self, invocation: &SdCliInvocation, _cancel: &CancelFlag) -> Result<RunOutput, JobError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let o_idx = invocation.args.iter().position(|a| a == "-o").expect("-o arg present");
            let path = PathBuf::from(&invocation.args[o_idx + 1]);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut img = RgbaImage::from_pixel(4, 4, Rgba([0, 255, 0, 255]));
            img.put_pixel(2, 2, Rgba([0, 0, 255, 255]));
            img.save(&path).unwrap();
            Ok(RunOutput { stdout: String::new() })
        }
    }

    /// The GPU path submits, then background-removes: the resulting file's
    /// key field is transparent and the subject region is retained.
    #[test]
    fn generate_image_generates_and_removes_bg() {
        let calls = Arc::new(AtomicUsize::new(0));
        let runner: Arc<dyn JobRunner> = Arc::new(KeyColorPngRunner { calls: calls.clone() });
        let gen = AssetGen::new(runner, Box::new(ZImageBackend), GpuCapability::Available, AssetGen::test_models());

        let asset = match gen.generate_image(req(1, None)).wait() {
            JobStatus::Success(asset) => asset,
            other => panic!("expected Success, got {other:?}"),
        };

        let decoded = image::open(&asset.path).unwrap().to_rgba8();
        assert_eq!(decoded.get_pixel(0, 0).0[3], 0, "key field must be transparent");
        assert!(decoded.get_pixel(2, 2).0[3] > 0, "subject region must be retained");
    }

    /// An explicit import resolves without touching the runner at all.
    #[test]
    fn generate_image_import_skips_runner() {
        let calls = Arc::new(AtomicUsize::new(0));
        let runner: Arc<dyn JobRunner> = Arc::new(KeyColorPngRunner { calls: calls.clone() });
        let gen = AssetGen::new(runner, Box::new(ZImageBackend), GpuCapability::Available, AssetGen::test_models());
        let import = std::env::temp_dir().join("abg_assets_test_import.png");

        match gen.generate_image(req(2, Some(import.clone()))).wait() {
            JobStatus::Success(asset) => assert_eq!(asset.path, import),
            other => panic!("expected Success, got {other:?}"),
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0, "import must not invoke the runner");
    }

    /// A second identical request is a cache hit: the runner runs exactly
    /// once.
    #[test]
    fn generate_image_repeat_hits_cache() {
        let calls = Arc::new(AtomicUsize::new(0));
        let runner: Arc<dyn JobRunner> = Arc::new(KeyColorPngRunner { calls: calls.clone() });
        let gen = AssetGen::new(runner, Box::new(ZImageBackend), GpuCapability::Available, AssetGen::test_models());
        let request = req(3, None);

        let first = gen.generate_image(request.clone()).wait();
        let second = gen.generate_image(request).wait();

        assert_eq!(first, second);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "a repeat request must not re-invoke the runner");
    }

    /// A runner-reported process error resolves to a `Failed` status, not a
    /// hang.
    #[test]
    fn generate_image_error_resolves() {
        struct FailingRunner;
        impl JobRunner for FailingRunner {
            fn run(&self, _invocation: &SdCliInvocation, _cancel: &CancelFlag) -> Result<RunOutput, JobError> {
                Err(JobError::Process {
                    code: Some(1),
                    stderr: "out of vram".into(),
                })
            }
        }
        let gen = AssetGen::new(Arc::new(FailingRunner), Box::new(ZImageBackend), GpuCapability::Available, AssetGen::test_models());
        match gen.generate_image(req(4, None)).wait() {
            JobStatus::Failed(JobError::Process { code: Some(1), .. }) => {}
            other => panic!("expected Failed(Process), got {other:?}"),
        }
    }

    /// A job that runs past the injected timeout resolves to `TimedOut`
    /// promptly, never an indefinite wait.
    #[test]
    fn generate_image_timeout_resolves() {
        struct BlockingRunner;
        impl JobRunner for BlockingRunner {
            fn run(&self, _invocation: &SdCliInvocation, cancel: &CancelFlag) -> Result<RunOutput, JobError> {
                while !cancel.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(JobError::Cancelled)
            }
        }
        let start = Instant::now();
        let gen = AssetGen::with_timeout(
            Arc::new(BlockingRunner),
            Box::new(ZImageBackend),
            GpuCapability::Available,
            AssetGen::test_models(),
            Duration::from_millis(50),
        );
        let status = gen.generate_image(req(5, None)).wait();
        assert_eq!(status, JobStatus::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(2), "took {:?}", start.elapsed());
    }

    /// No GPU and no import: a reported capability failure, never a hang
    /// and never a fabricated still.
    #[test]
    fn generate_image_no_gpu_no_import_errors() {
        let runner: Arc<dyn JobRunner> = Arc::new(KeyColorPngRunner {
            calls: Arc::new(AtomicUsize::new(0)),
        });
        let gen = AssetGen::new(runner, Box::new(ZImageBackend), GpuCapability::Unavailable, AssetGen::test_models());
        assert_eq!(
            gen.generate_image(req(6, None)).wait(),
            JobStatus::Failed(JobError::NoGpu)
        );
    }

    #[test]
    fn resolve_status_maps_success() {
        assert_eq!(resolve_status(JobStatus::Success(42)), Ok(42));
    }

    #[test]
    fn resolve_status_maps_no_gpu() {
        assert_eq!(
            resolve_status::<()>(JobStatus::Failed(JobError::NoGpu)),
            Err(AssetError::GpuUnavailable)
        );
    }

    #[test]
    fn resolve_status_maps_other_failure() {
        let err = JobError::Process {
            code: Some(1),
            stderr: "x".into(),
        };
        assert_eq!(
            resolve_status::<()>(JobStatus::Failed(err.clone())),
            Err(AssetError::Generation(err))
        );
    }

    #[test]
    fn resolve_status_maps_timeout() {
        assert_eq!(resolve_status::<()>(JobStatus::TimedOut), Err(AssetError::TimedOut));
    }

    /// The GPU path submits, then per-frame background-removes: the
    /// resulting clip has the requested frame count, and the first frame's
    /// key field is transparent while the subject region is retained.
    #[test]
    fn generate_animation_produces_removed_frames() {
        let calls = Arc::new(AtomicUsize::new(0));
        let runner: Arc<dyn JobRunner> = Arc::new(KeyColorFramesRunner {
            calls: calls.clone(),
            field: [0, 255, 0, 255],
            captured_init_img: Arc::new(Mutex::new(None)),
            captured_prompt: Arc::new(Mutex::new(None)),
        });
        let gen = AssetGen::new(runner, Box::new(ZImageBackend), GpuCapability::Available, AssetGen::test_models())
            .with_extractor(AssetGen::test_extractor());
        let still = synthetic_still("anim_frames", [0, 0, 255, 255]);

        let clip = match gen
            .generate_animation(&still, animation_req("attack", 3, 24))
            .wait()
        {
            JobStatus::Success(clip) => clip,
            other => panic!("expected Success, got {other:?}"),
        };

        assert_eq!(clip.frames.len(), 3);
        let decoded = image::open(&clip.frames[0]).unwrap().to_rgba8();
        assert_eq!(decoded.get_pixel(0, 0).0[3], 0, "key field must be transparent");
        assert!(decoded.get_pixel(2, 2).0[3] > 0, "subject region must be retained");
    }

    /// Before the invocation runs, the still is pre-cleaned and flattened
    /// onto an opaque flat-key background: the `--init-img` the runner
    /// receives has an opaque key-colored background and the retained
    /// subject, not the caller's transparent still.
    #[test]
    fn generate_animation_precleans_still() {
        let captured_init_img = Arc::new(Mutex::new(None));
        let runner: Arc<dyn JobRunner> = Arc::new(KeyColorFramesRunner {
            calls: Arc::new(AtomicUsize::new(0)),
            field: [0, 255, 0, 255],
            captured_init_img: captured_init_img.clone(),
            captured_prompt: Arc::new(Mutex::new(None)),
        });
        let gen = AssetGen::new(runner, Box::new(ZImageBackend), GpuCapability::Available, AssetGen::test_models())
            .with_extractor(AssetGen::test_extractor());
        let still = synthetic_still("anim_preclean", [0, 0, 255, 255]);

        gen.generate_animation(&still, animation_req("idle", 1, 24)).wait();

        let init_path = captured_init_img
            .lock()
            .unwrap()
            .clone()
            .expect("runner must have captured --init-img");
        let decoded = image::open(&init_path).unwrap().to_rgba8();
        assert_eq!(
            decoded.get_pixel(0, 0).0,
            [0, 255, 0, 255],
            "init image background must be opaque flat key color"
        );
        assert_eq!(
            decoded.get_pixel(2, 2).0,
            [0, 0, 255, 255],
            "init image must retain the subject"
        );
    }

    /// A still whose subject is green-family selects magenta: the
    /// invocation's prompt carries the magenta clause, and the per-frame
    /// removal keys against magenta (a magenta-field synthetic frame comes
    /// back transparent).
    #[test]
    fn generate_animation_green_family_uses_magenta() {
        let captured_prompt = Arc::new(Mutex::new(None));
        let runner: Arc<dyn JobRunner> = Arc::new(KeyColorFramesRunner {
            calls: Arc::new(AtomicUsize::new(0)),
            field: [255, 0, 255, 255],
            captured_init_img: Arc::new(Mutex::new(None)),
            captured_prompt: captured_prompt.clone(),
        });
        let gen = AssetGen::new(runner, Box::new(ZImageBackend), GpuCapability::Available, AssetGen::test_models())
            .with_extractor(AssetGen::test_extractor());
        let still = synthetic_still("anim_green_family", [0, 220, 80, 255]);

        let clip = match gen.generate_animation(&still, animation_req("hatch", 1, 24)).wait() {
            JobStatus::Success(clip) => clip,
            other => panic!("expected Success, got {other:?}"),
        };

        let prompt = captured_prompt
            .lock()
            .unwrap()
            .clone()
            .expect("runner must have captured -p");
        assert!(prompt.contains("magenta"), "got prompt: {prompt}");

        let decoded = image::open(&clip.frames[0]).unwrap().to_rgba8();
        assert_eq!(
            decoded.get_pixel(0, 0).0[3],
            0,
            "magenta field must be removed for a green-family subject"
        );
    }

    /// A second call with the same `(image, action)` returns the same clip
    /// without re-invoking the runner: the clip is baked once.
    #[test]
    fn generate_animation_repeat_hits_cache() {
        let calls = Arc::new(AtomicUsize::new(0));
        let runner: Arc<dyn JobRunner> = Arc::new(KeyColorFramesRunner {
            calls: calls.clone(),
            field: [0, 255, 0, 255],
            captured_init_img: Arc::new(Mutex::new(None)),
            captured_prompt: Arc::new(Mutex::new(None)),
        });
        let gen = AssetGen::new(runner, Box::new(ZImageBackend), GpuCapability::Available, AssetGen::test_models())
            .with_extractor(AssetGen::test_extractor());
        let still = synthetic_still("anim_repeat", [0, 0, 255, 255]);
        let request = animation_req("idle", 2, 24);

        let first = gen.generate_animation(&still, request.clone()).wait();
        let second = gen.generate_animation(&still, request).wait();

        assert_eq!(first, second);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "a repeat (image, action) must not re-invoke the runner"
        );
    }

    /// No GPU available: a reported `NoGpu` failure, never a hang and never
    /// a fabricated clip. Animation has no no-GPU fallback, so the runner
    /// must never be invoked.
    #[test]
    fn generate_animation_no_gpu_errors() {
        let calls = Arc::new(AtomicUsize::new(0));
        let runner: Arc<dyn JobRunner> = Arc::new(KeyColorFramesRunner {
            calls: calls.clone(),
            field: [0, 255, 0, 255],
            captured_init_img: Arc::new(Mutex::new(None)),
            captured_prompt: Arc::new(Mutex::new(None)),
        });
        let gen = AssetGen::new(runner, Box::new(ZImageBackend), GpuCapability::Unavailable, AssetGen::test_models());
        let still = synthetic_still("anim_no_gpu", [0, 0, 255, 255]);

        assert_eq!(
            gen.generate_animation(&still, animation_req("idle", 1, 24)).wait(),
            JobStatus::Failed(JobError::NoGpu)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "no GPU must never invoke the runner");
    }

    /// A runner-reported process error (e.g. out-of-VRAM) resolves to a
    /// `Failed` status, not a hang.
    #[test]
    fn generate_animation_error_resolves() {
        struct FailingRunner;
        impl JobRunner for FailingRunner {
            fn run(&self, _invocation: &SdCliInvocation, _cancel: &CancelFlag) -> Result<RunOutput, JobError> {
                Err(JobError::Process {
                    code: Some(1),
                    stderr: "out of vram".into(),
                })
            }
        }
        let gen = AssetGen::new(Arc::new(FailingRunner), Box::new(ZImageBackend), GpuCapability::Available, AssetGen::test_models());
        let still = synthetic_still("anim_error", [0, 0, 255, 255]);

        match gen.generate_animation(&still, animation_req("idle", 1, 24)).wait() {
            JobStatus::Failed(JobError::Process { code: Some(1), .. }) => {}
            other => panic!("expected Failed(Process), got {other:?}"),
        }
    }

    /// A job that runs past the injected timeout resolves to `TimedOut`
    /// promptly, never an indefinite wait.
    #[test]
    fn generate_animation_timeout_resolves() {
        struct BlockingRunner;
        impl JobRunner for BlockingRunner {
            fn run(&self, _invocation: &SdCliInvocation, cancel: &CancelFlag) -> Result<RunOutput, JobError> {
                while !cancel.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(JobError::Cancelled)
            }
        }
        let start = Instant::now();
        let gen = AssetGen::with_timeout(
            Arc::new(BlockingRunner),
            Box::new(ZImageBackend),
            GpuCapability::Available,
            AssetGen::test_models(),
            Duration::from_millis(50),
        );
        let still = synthetic_still("anim_timeout", [0, 0, 255, 255]);

        let status = gen
            .generate_animation(&still, animation_req("idle", 1, 24))
            .wait();
        assert_eq!(status, JobStatus::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(2), "took {:?}", start.elapsed());
    }

    /// A resolver with no model files present under its configured dir
    /// surfaces `Failed(ModelPath(MissingFile))` from `generate_image`, not
    /// a hang and not a bad argv reaching the runner.
    #[test]
    fn generate_image_missing_model_errors() {
        let dir = std::env::temp_dir().join(format!(
            "abg-test-ops-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let models = crate::asset_gen::model_paths::ModelPaths::from_dir_str(Some(dir.to_str().unwrap()))
            .expect("dir exists");

        let calls = Arc::new(AtomicUsize::new(0));
        let runner: Arc<dyn JobRunner> = Arc::new(KeyColorPngRunner { calls: calls.clone() });
        let gen = AssetGen::new(runner, Box::new(ZImageBackend), GpuCapability::Available, models);

        match gen.generate_image(req(7, None)).wait() {
            JobStatus::Failed(JobError::ModelPath(ModelPathError::MissingFile { .. })) => {}
            other => panic!("expected Failed(ModelPath(MissingFile)), got {other:?}"),
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0, "a missing model must never reach the runner");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Writes a single opaque byte blob to the invocation's `-o` path,
    /// standing in for the one video file a `vid_gen` run produces (as
    /// opposed to a frame sequence written directly there).
    struct VideoFileRunner;

    impl JobRunner for VideoFileRunner {
        fn run(&self, invocation: &SdCliInvocation, _cancel: &CancelFlag) -> Result<RunOutput, JobError> {
            let o_idx = invocation.args.iter().position(|a| a == "-o").expect("-o arg present");
            let out_path = PathBuf::from(&invocation.args[o_idx + 1]);
            std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
            std::fs::write(&out_path, b"not-a-real-video").unwrap();
            Ok(RunOutput { stdout: String::new() })
        }
    }

    /// Records the `video_out` path it was invoked with and writes `frames`
    /// synthetic PNGs into `frames_dir`, standing in for a real
    /// video-to-frames extraction step.
    struct RecordingExtractor {
        captured_video_out: Arc<Mutex<Option<PathBuf>>>,
    }

    impl crate::asset_gen::frame_extract::FrameExtractor for RecordingExtractor {
        fn extract(
            &self,
            video_out: &std::path::Path,
            frames_dir: &std::path::Path,
            frames: u32,
            _fps: u32,
        ) -> Result<(), crate::asset_gen::frame_extract::FrameExtractError> {
            *self.captured_video_out.lock().unwrap() = Some(video_out.to_path_buf());
            std::fs::create_dir_all(frames_dir).unwrap();
            for i in 0..frames {
                let mut img = RgbaImage::from_pixel(4, 4, Rgba([0, 255, 0, 255]));
                img.put_pixel(2, 2, Rgba([0, 0, 255, 255]));
                img.save(frames_dir.join(format!("f_{i:03}.png"))).unwrap();
            }
            Ok(())
        }
    }

    /// The injected frame extractor runs against a video-output path
    /// distinct from `clip_raw_frames_dir`, the directory `materialize_clip`
    /// reads frames from, so the raw video is never miscounted as a frame.
    #[test]
    fn generate_animation_extracts_from_a_dedicated_video_path() {
        let captured_video_out = Arc::new(Mutex::new(None));
        let gen = AssetGen::new(
            Arc::new(VideoFileRunner),
            Box::new(ZImageBackend),
            GpuCapability::Available,
            AssetGen::test_models(),
        )
        .with_extractor(Arc::new(RecordingExtractor {
            captured_video_out: captured_video_out.clone(),
        }));
        let still = synthetic_still("anim_extract_path", [0, 0, 255, 255]);

        match gen.generate_animation(&still, animation_req("attack", 2, 24)).wait() {
            JobStatus::Success(_) => {}
            other => panic!("expected Success, got {other:?}"),
        }

        let expected = clip_video_out_path(&still, "attack");
        assert_eq!(
            captured_video_out.lock().unwrap().clone(),
            Some(expected.clone()),
            "extraction must run on the dedicated video-output path"
        );
        assert_ne!(
            expected,
            clip_raw_frames_dir(&still, "attack").join("anim.png"),
            "the video path must be distinct from the frames directory materialize_clip reads"
        );
    }
}
