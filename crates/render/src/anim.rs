//! Animated sprite: a sequence of frames played at a base rate scaled by a
//! runtime speed multiplier.
//!
//! Callers accumulate wall-clock elapsed time externally and call
//! [`AnimatedSprite::frame_index_at`] or [`AnimatedSprite::frame_at`] to
//! retrieve the active frame. No clock reads or mutation occur inside this
//! type.

use crate::dots::{sprite_to_dots, DotBuffer};
use crate::transform::{rasterize, Transform};
use image::codecs::gif::GifDecoder;
use image::AnimationDecoder;
use image::DynamicImage;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Cursor;
use std::time::Duration;

/// Cache key for the plain rasterization path — mirrors
/// `sprite_to_dots(img, dot_cols, dot_rows)`'s parameters.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct PlainKey {
    frame_index: usize,
    dot_cols: u32,
    dot_rows: u32,
}

impl PlainKey {
    fn new(frame_index: usize, dot_cols: u32, dot_rows: u32) -> Self {
        Self { frame_index, dot_cols, dot_rows }
    }
}

/// Cache key for the transform rasterization path — mirrors
/// `transform::rasterize(img, transform, base_dot_rows)`'s parameters,
/// EXCLUDING `transform.translate` (applied later by `place()`, never baked
/// into the buffer). `f32` fields are stored as raw bit patterns (`to_bits()`)
/// so the key derives `Eq`/`Hash` with total, exact equality. `base_dot_rows`
/// is included because `rasterize`'s output size depends on it as well as on
/// `scale` (`transform.rs:77`) — a key omitting it would serve a stale,
/// wrong-sized buffer after e.g. a terminal resize changes `base_dot_rows`
/// while `(frame_index, rotation, scale)` stay the same.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct TransformKey {
    frame_index: usize,
    base_dot_rows: u32,
    rotation_bits: u32,
    scale_x_bits: u32,
    scale_y_bits: u32,
}

impl TransformKey {
    fn new(frame_index: usize, transform: &Transform, base_dot_rows: u32) -> Self {
        Self {
            frame_index,
            base_dot_rows,
            rotation_bits: transform.rotation.to_bits(),
            scale_x_bits: transform.scale.x.to_bits(),
            scale_y_bits: transform.scale.y.to_bits(),
        }
    }
}

/// Per-instance rasterization cache. Two separate maps keep the plain and
/// transform entry points from ever colliding on a shared key space.
#[derive(Default)]
struct RasterCache {
    plain: HashMap<PlainKey, DotBuffer>,
    transform: HashMap<TransformKey, DotBuffer>,
    /// Count of real `sprite_to_dots` recomputes (cache misses) this instance
    /// has performed. Test-only observability, exposed via
    /// `AnimatedSprite::recompute_count`.
    recomputes: u64,
}

/// A sequence of [`DynamicImage`] frames played in order, wrapping continuously.
///
/// Playback rate is a **base per-frame duration** (`frame_dur`, the rate at
/// `speed == 1.0`) scaled by a runtime **speed multiplier**, mirroring how game
/// engines expose animation speed — Unity `Animator.speed`, Godot `speed_scale`,
/// Unreal `PlayRate`: `1.0` natural, `2.0` twice as fast, `0.5` half, a negative
/// value plays in reverse, `0.0` holds frame 0.
///
/// At the default `speed == 1.0`, frame selection is exact integer nanosecond
/// division.
pub struct AnimatedSprite {
    frames: Vec<DynamicImage>,
    /// Base per-frame duration at `speed == 1.0`.
    frame_dur: Duration,
    /// Playback multiplier; effective rate = base × `speed`. See [`Self::set_speed`].
    speed: f32,
    /// Per-instance rasterization cache, keyed by frame/dot-size or
    /// frame/transform. Interior-mutable so read-only render calls can
    /// populate it without requiring `&mut self`.
    cache: RefCell<RasterCache>,
}

impl AnimatedSprite {
    /// Construct from a frame list and a uniform per-frame duration.
    ///
    /// `frame_dur` is the caller-supplied display time per frame — it is NOT
    /// derived from GIF metadata or any external source.
    pub fn new(frames: Vec<DynamicImage>, frame_dur: Duration) -> Self {
        Self {
            frames,
            frame_dur,
            speed: 1.0,
            cache: RefCell::new(RasterCache::default()),
        }
    }

    /// Set the playback speed multiplier and return self (builder style).
    pub fn with_speed(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }

    /// Set the playback speed multiplier in place.
    ///
    /// Mirrors Unity's `Animator.speed` / Godot's `speed_scale`: `1.0` is the
    /// natural rate, `2.0` doubles it, `0.5` halves it, a negative value plays in
    /// reverse, and `0.0` holds frame 0. Effective per-frame duration = `frame_dur / speed`.
    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
    }

    /// The current playback speed multiplier (default `1.0`).
    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// Number of frames in the sprite.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// The uniform per-frame duration supplied at construction.
    pub fn frame_dur(&self) -> Duration {
        self.frame_dur
    }

    /// The frame index active at the given `elapsed` wall-clock time, honoring
    /// the [`speed`](Self::speed) multiplier.
    ///
    /// Frames advanced = `elapsed × speed / frame_dur`, floored and wrapped into
    /// `0..frame_count` (euclidean, so a negative `speed` plays in reverse). At
    /// the default `speed == 1.0` this is exact integer nanosecond division.
    /// Returns 0 when `frame_count <= 1` or `frame_dur` is zero (no panic).
    pub fn frame_index_at(&self, elapsed: Duration) -> usize {
        let n = self.frames.len();
        if n <= 1 {
            return 0;
        }
        let base = self.frame_dur.as_nanos();
        if base == 0 {
            return 0;
        }
        // Signed frame position; speed < 0 walks the index backward.
        let pos = (elapsed.as_nanos() as f64) * (self.speed as f64) / (base as f64);
        (pos.floor() as i128).rem_euclid(n as i128) as usize
    }

    /// The frame active at `elapsed`.
    ///
    /// Equivalent to `&frames[frame_index_at(elapsed)]`.
    pub fn frame_at(&self, elapsed: Duration) -> &DynamicImage {
        &self.frames[self.frame_index_at(elapsed)]
    }

    /// Decode a GIF from `bytes` into an animated sprite played at the uniform
    /// `frame_dur`. The GIF's own per-frame delays are intentionally ignored.
    /// Returns `Err` (never panics) on malformed input.
    pub fn from_gif(bytes: &[u8], frame_dur: Duration) -> Result<Self, image::ImageError> {
        let decoder = GifDecoder::new(Cursor::new(bytes))?;
        let frames = decoder
            .into_frames()
            .collect_frames()?
            .into_iter()
            .map(|f| DynamicImage::ImageRgba8(f.into_buffer()))
            .collect();
        Ok(Self::new(frames, frame_dur))
    }

    /// Rasterize the frame active at `elapsed` to `dot_cols × dot_rows` dots,
    /// reusing a cached `DotBuffer` when the same `(frame_index, dot_cols,
    /// dot_rows)` was already computed on this instance. Output is
    /// byte-identical to `sprite_to_dots(self.frame_at(elapsed), dot_cols,
    /// dot_rows)`.
    pub fn dots_at(&self, elapsed: Duration, dot_cols: u32, dot_rows: u32) -> DotBuffer {
        let frame_index = self.frame_index_at(elapsed);
        let key = PlainKey::new(frame_index, dot_cols, dot_rows);
        let mut cache = self.cache.borrow_mut();
        if let Some(buf) = cache.plain.get(&key) {
            return buf.clone();
        }
        let buf = sprite_to_dots(&self.frames[frame_index], dot_cols, dot_rows);
        cache.recomputes += 1;
        cache.plain.insert(key, buf.clone());
        buf
    }

    /// Rasterize the frame active at `elapsed` under `transform` at
    /// `base_dot_rows`, reusing a cached `DotBuffer` when the same
    /// `(frame_index, base_dot_rows, rotation, scale)` was already computed on
    /// this instance (`translate` excluded — applied later by `place()`).
    /// Output is byte-identical to `transform::rasterize(self.frame_at(elapsed),
    /// transform, base_dot_rows)`.
    pub fn rasterize_at(&self, elapsed: Duration, transform: &Transform, base_dot_rows: u32) -> DotBuffer {
        let frame_index = self.frame_index_at(elapsed);
        let key = TransformKey::new(frame_index, transform, base_dot_rows);
        let mut cache = self.cache.borrow_mut();
        if let Some(buf) = cache.transform.get(&key) {
            return buf.clone();
        }
        let buf = rasterize(&self.frames[frame_index], transform, base_dot_rows);
        cache.recomputes += 1;
        cache.transform.insert(key, buf.clone());
        buf
    }

    /// Test-only: number of real `sprite_to_dots` recomputes (cache misses)
    /// this instance has performed. Used to prove cache hits do not recompute.
    #[cfg(test)]
    fn recompute_count(&self) -> u64 {
        self.cache.borrow().recomputes
    }
}

#[cfg(test)]
mod tests {
    use super::{AnimatedSprite, PlainKey, TransformKey};
    use crate::camera::WorldPos;
    use crate::dots::sprite_to_dots;
    use crate::transform::{rasterize, Transform, Vec2};
    use image::{DynamicImage, GenericImageView, Rgba as PixelRgba, RgbaImage};
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::Duration;

    /// Hash a value via the standard `Hash` trait, for cross-checking `PartialEq`
    /// against `Hash` on cache keys (both must agree for correct `HashMap` use).
    fn hash_of<T: Hash>(v: &T) -> u64 {
        let mut h = DefaultHasher::new();
        v.hash(&mut h);
        h.finish()
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    /// Build a 1×1 solid-color fully-opaque RGBA image.
    fn px(r: u8, g: u8, b: u8) -> DynamicImage {
        let mut img = RgbaImage::new(1, 1);
        img.put_pixel(0, 0, PixelRgba([r, g, b, 255]));
        DynamicImage::from(img)
    }

    /// Retrieve the RGBA bytes of pixel (0,0) from a ≥1×1 image.
    fn pixel0(img: &DynamicImage) -> [u8; 4] {
        img.get_pixel(0, 0).0
    }

    /// 3-frame sprite with frame_dur = 100 ms.
    /// Frame 0 = red (255,0,0), Frame 1 = green (0,255,0), Frame 2 = blue (0,0,255).
    fn make_sprite() -> AnimatedSprite {
        let frames = vec![px(255, 0, 0), px(0, 255, 0), px(0, 0, 255)];
        AnimatedSprite::new(frames, Duration::from_millis(100))
    }

    // ── frame_index_at: boundary cases ────────────────────────────────────────

    /// t = 0 → frame 0.
    #[test]
    fn index_at_zero() {
        let s = make_sprite();
        assert_eq!(s.frame_index_at(Duration::ZERO), 0);
    }

    /// t = frame_dur − 1 ns → still frame 0 (last nanosecond of first window).
    #[test]
    fn index_just_before_first_boundary() {
        let s = make_sprite();
        let t = Duration::from_millis(100) - Duration::from_nanos(1);
        assert_eq!(s.frame_index_at(t), 0, "one ns before boundary must still be frame 0");
    }

    /// t = frame_dur (exact boundary) → frame 1.
    ///
    /// This is the classical off-by-one trap: the new frame must start at
    /// exactly `frame_dur`, not at `frame_dur + 1`.
    #[test]
    fn index_at_first_exact_boundary() {
        let s = make_sprite();
        assert_eq!(
            s.frame_index_at(Duration::from_millis(100)),
            1,
            "exact frame_dur boundary must yield index 1"
        );
    }

    /// t = frame_count × frame_dur → frame 0 (wraps back to start of next cycle).
    #[test]
    fn index_wraps_at_full_cycle() {
        let s = make_sprite();
        // 3 frames × 100 ms = 300 ms → (3 / 3) = 1 cycle → index 0.
        assert_eq!(
            s.frame_index_at(Duration::from_millis(300)),
            0,
            "one full cycle must wrap to frame 0"
        );
    }

    /// t = (frame_count + 1) × frame_dur + frame_dur/2 → frame 1.
    ///
    /// Lands in the second frame of the second cycle (past a full wrap).
    #[test]
    fn index_mid_frame_second_cycle() {
        let s = make_sprite();
        // 4 × 100 ms + 50 ms = 450 ms → floor(450/100) = 4 → 4 % 3 = 1.
        let t = Duration::from_millis(450);
        assert_eq!(
            s.frame_index_at(t),
            1,
            "450 ms with 3-frame 100ms sprite must yield index 1"
        );
    }

    // ── single-frame sprite: always index 0 ───────────────────────────────────

    /// A single-frame sprite must return 0 for any elapsed time, including
    /// very large values. The modulo identity (n % 1 == 0) must hold.
    #[test]
    fn single_frame_always_zero() {
        let s = AnimatedSprite::new(vec![px(128, 128, 128)], Duration::from_millis(100));
        assert_eq!(s.frame_index_at(Duration::ZERO), 0);
        assert_eq!(s.frame_index_at(Duration::from_millis(100)), 0, "past frame_dur still 0");
        assert_eq!(s.frame_index_at(Duration::from_millis(99)), 0);
        assert_eq!(
            s.frame_index_at(Duration::from_secs(1_000_000)),
            0,
            "very large elapsed must still be 0 for a single-frame sprite"
        );
    }

    // ── zero frame_dur: no panic, returns 0 ──────────────────────────────────

    /// Zero `frame_dur` must NOT panic and must return 0 (guards div-by-zero).
    #[test]
    fn zero_frame_dur_no_panic() {
        let s = AnimatedSprite::new(vec![px(1, 2, 3), px(4, 5, 6)], Duration::ZERO);
        assert_eq!(
            s.frame_index_at(Duration::ZERO),
            0,
            "zero frame_dur at t=0 must not panic and return 0"
        );
        assert_eq!(
            s.frame_index_at(Duration::from_secs(99)),
            0,
            "zero frame_dur at large t must not panic and return 0"
        );
    }

    // ── frame_at: pixel identity ──────────────────────────────────────────────

    /// `frame_at` must return a reference to the correct source frame,
    /// verified by the distinct pixel color of each 1×1 frame image.
    #[test]
    fn frame_at_returns_correct_frame_by_pixel() {
        let s = make_sprite();
        // t = 0 ms → frame 0 → red
        assert_eq!(
            pixel0(s.frame_at(Duration::ZERO)),
            [255, 0, 0, 255],
            "t=0 must return the red frame"
        );
        // t = 100 ms → frame 1 → green
        assert_eq!(
            pixel0(s.frame_at(Duration::from_millis(100))),
            [0, 255, 0, 255],
            "t=100ms must return the green frame"
        );
        // t = 200 ms → frame 2 → blue
        assert_eq!(
            pixel0(s.frame_at(Duration::from_millis(200))),
            [0, 0, 255, 255],
            "t=200ms must return the blue frame"
        );
        // t = 300 ms → frame 0 (wrap) → red
        assert_eq!(
            pixel0(s.frame_at(Duration::from_millis(300))),
            [255, 0, 0, 255],
            "t=300ms (wrap) must return the red frame again"
        );
    }

    // ── accessors ─────────────────────────────────────────────────────────────

    /// `frame_count()` and `frame_dur()` must return exactly what was passed
    /// to `new`.
    #[test]
    fn accessors_round_trip() {
        let dur = Duration::from_millis(42);
        let s = AnimatedSprite::new(vec![px(1, 2, 3), px(4, 5, 6)], dur);
        assert_eq!(s.frame_count(), 2, "frame_count must match frame Vec length");
        assert_eq!(s.frame_dur(), dur, "frame_dur must match constructor arg");
    }

    // ── speed multiplier (Unity Animator.speed / Godot speed_scale) ───────────

    /// Default speed is the natural rate, 1.0.
    #[test]
    fn default_speed_is_one() {
        assert!((make_sprite().speed() - 1.0).abs() < f32::EPSILON);
    }

    /// `set_speed` mutates in place and round-trips via `speed()`.
    #[test]
    fn set_speed_round_trip() {
        let mut s = make_sprite();
        s.set_speed(3.5);
        assert!((s.speed() - 3.5).abs() < f32::EPSILON);
    }

    /// speed = 2.0 advances twice as fast (effective 50 ms on a 100 ms base).
    #[test]
    fn speed_2x_doubles_rate() {
        let s = make_sprite().with_speed(2.0);
        assert_eq!(s.frame_index_at(Duration::ZERO), 0);
        assert_eq!(s.frame_index_at(Duration::from_millis(50)), 1, "2x: 50ms → frame 1");
        assert_eq!(s.frame_index_at(Duration::from_millis(100)), 2, "2x: 100ms → frame 2");
    }

    /// speed = 0.5 advances half as fast (effective 200 ms).
    #[test]
    fn speed_half_slows_rate() {
        let s = make_sprite().with_speed(0.5);
        assert_eq!(s.frame_index_at(Duration::from_millis(100)), 0, "0.5x: 100ms still frame 0");
        assert_eq!(s.frame_index_at(Duration::from_millis(200)), 1, "0.5x: 200ms → frame 1");
    }

    /// Negative speed plays in reverse, wrapping euclidean into 0..n.
    /// 3-frame, 100 ms base: t=100ms → frame 2, t=200ms → frame 1.
    #[test]
    fn negative_speed_reverses() {
        let s = make_sprite().with_speed(-1.0);
        assert_eq!(s.frame_index_at(Duration::ZERO), 0, "reverse at t=0 → frame 0");
        assert_eq!(s.frame_index_at(Duration::from_millis(100)), 2, "reverse 100ms → frame 2");
        assert_eq!(s.frame_index_at(Duration::from_millis(200)), 1, "reverse 200ms → frame 1");
    }

    /// speed = 0 holds frame 0 (paused).
    #[test]
    fn zero_speed_holds_frame_zero() {
        let s = make_sprite().with_speed(0.0);
        assert_eq!(s.frame_index_at(Duration::from_millis(500)), 0, "speed 0 holds frame 0");
    }

    // ── PlainKey: cache key for sprite_to_dots(frame, dot_cols, dot_rows) ────

    /// Two keys built from identical inputs must compare AND hash equal
    /// (both are required for correct `HashMap` behavior).
    #[test]
    fn plain_key_identical_inputs_are_equal_and_hash_equal() {
        let a = PlainKey::new(2, 40, 20);
        let b = PlainKey::new(2, 40, 20);
        assert_eq!(a, b, "identical (frame_index, dot_cols, dot_rows) must produce equal keys");
        assert_eq!(hash_of(&a), hash_of(&b), "equal keys must hash equal");
    }

    /// Keys differing only in `dot_cols` must compare unequal.
    #[test]
    fn plain_key_differs_by_dot_cols_is_unequal() {
        let a = PlainKey::new(2, 40, 20);
        let b = PlainKey::new(2, 41, 20);
        assert_ne!(a, b, "differing dot_cols must not produce equal keys");
    }

    /// Keys differing only in `dot_rows` must compare unequal.
    #[test]
    fn plain_key_differs_by_dot_rows_is_unequal() {
        let a = PlainKey::new(2, 40, 20);
        let b = PlainKey::new(2, 40, 21);
        assert_ne!(a, b, "differing dot_rows must not produce equal keys");
    }

    /// Keys differing only in `frame_index` must compare unequal.
    #[test]
    fn plain_key_differs_by_frame_index_is_unequal() {
        let a = PlainKey::new(2, 40, 20);
        let b = PlainKey::new(3, 40, 20);
        assert_ne!(a, b, "differing frame_index must not produce equal keys");
    }

    // ── TransformKey: cache key for rasterize(frame, transform, base_dot_rows) ─

    /// Shared `base_dot_rows` for TransformKey/rasterize_at tests below.
    const BR: u32 = 16;

    /// Two keys built from the same (frame_index, rotation, scale, base_dot_rows)
    /// must compare AND hash equal.
    #[test]
    fn transform_key_identical_inputs_are_equal_and_hash_equal() {
        let t = Transform::new(WorldPos::new(1.0, 2.0), 45.0, Vec2::new(1.5, -1.0));
        let a = TransformKey::new(2, &t, BR);
        let b = TransformKey::new(2, &t, BR);
        assert_eq!(a, b, "identical (frame_index, rotation, scale, base_dot_rows) must produce equal keys");
        assert_eq!(hash_of(&a), hash_of(&b), "equal keys must hash equal");
    }

    /// Load-bearing contract: two `Transform`s differing ONLY in `translate`
    /// must produce EQUAL `TransformKey`s. `translate` is applied later by
    /// `place()` and must never invalidate/participate in this cache key.
    #[test]
    fn transform_key_ignores_translate() {
        let t1 = Transform::default();
        let t2 = Transform { translate: WorldPos::new(500.0, -500.0), ..Transform::default() };
        assert_eq!(
            TransformKey::new(0, &t1, BR),
            TransformKey::new(0, &t2, BR),
            "translate-only difference must not change the transform cache key"
        );
    }

    /// Keys differing only in `rotation` must compare unequal.
    #[test]
    fn transform_key_differs_by_rotation_is_unequal() {
        let t1 = Transform::default();
        let t2 = Transform { rotation: 90.0, ..Transform::default() };
        assert_ne!(TransformKey::new(0, &t1, BR), TransformKey::new(0, &t2, BR));
    }

    /// Keys differing only in `scale.x` must compare unequal.
    #[test]
    fn transform_key_differs_by_scale_x_is_unequal() {
        let t1 = Transform::default();
        let t2 = Transform { scale: Vec2::new(2.0, 1.0), ..Transform::default() };
        assert_ne!(TransformKey::new(0, &t1, BR), TransformKey::new(0, &t2, BR));
    }

    /// Keys differing only in `scale.y` must compare unequal.
    #[test]
    fn transform_key_differs_by_scale_y_is_unequal() {
        let t1 = Transform::default();
        let t2 = Transform { scale: Vec2::new(1.0, 2.0), ..Transform::default() };
        assert_ne!(TransformKey::new(0, &t1, BR), TransformKey::new(0, &t2, BR));
    }

    /// Keys differing only in `frame_index` must compare unequal.
    #[test]
    fn transform_key_differs_by_frame_index_is_unequal() {
        let t = Transform::default();
        assert_ne!(TransformKey::new(0, &t, BR), TransformKey::new(1, &t, BR));
    }

    /// Keys differing only in `base_dot_rows` must compare unequal — this is the
    /// resize-staleness bug the b1-t3 blueprint fixes: `rasterize`'s output size
    /// depends on `base_dot_rows` as well as `scale` (`transform.rs:77`), so a key
    /// omitting it would serve a stale, wrong-sized buffer after a resize.
    #[test]
    fn transform_key_differs_by_base_dot_rows_is_unequal() {
        let t = Transform::default();
        assert_ne!(
            TransformKey::new(0, &t, BR),
            TransformKey::new(0, &t, BR + 1),
            "differing base_dot_rows must not produce equal keys"
        );
    }

    /// `+0.0` and `-0.0` rotation must produce UNEQUAL keys — proves the key
    /// hashes/compares raw bit patterns (`to_bits()`), not `==` on the float
    /// (where `+0.0 == -0.0`).
    #[test]
    fn transform_key_distinguishes_signed_zero_rotation() {
        let t_pos = Transform { rotation: 0.0, ..Transform::default() };
        let t_neg = Transform { rotation: -0.0, ..Transform::default() };
        assert_ne!(
            TransformKey::new(0, &t_pos, BR),
            TransformKey::new(0, &t_neg, BR),
            "+0.0 and -0.0 rotation must hash to distinct bit patterns"
        );
    }

    // ── rasterize_at: cached transform-path accessor (b1-t3) ─────────────────

    /// Two calls with the same resolved `(frame_index, base_dot_rows, rotation,
    /// scale)` must perform exactly ONE real recompute (the second is a cache
    /// hit), and both calls must return equal buffers.
    #[test]
    fn rasterize_at_second_identical_call_is_cache_hit() {
        let s = make_sprite();
        let tf = Transform::default();
        let first = s.rasterize_at(Duration::ZERO, &tf, BR);
        let second = s.rasterize_at(Duration::ZERO, &tf, BR);
        assert_eq!(first, second, "identical calls must return equal buffers");
        assert_eq!(
            s.recompute_count(),
            1,
            "second call with identical (frame_index, base_dot_rows, rotation, scale) must be a cache hit"
        );
    }

    /// `rasterize_at` output must be byte-identical to calling
    /// `transform::rasterize` directly on the frame resolved by `frame_at` for
    /// the same elapsed time.
    #[test]
    fn rasterize_at_matches_rasterize_directly() {
        let s = make_sprite();
        let transforms = [
            Transform::default(),
            Transform { rotation: 45.0, scale: Vec2::new(1.5, -1.0), ..Transform::default() },
        ];
        for t in [Duration::ZERO, Duration::from_millis(100), Duration::from_millis(200)] {
            for tf in &transforms {
                let via_cache = s.rasterize_at(t, tf, BR);
                let direct = rasterize(s.frame_at(t), tf, BR);
                assert_eq!(via_cache, direct, "rasterize_at must match rasterize directly for t={t:?}");
            }
        }
    }

    /// Load-bearing: two `Transform`s differing ONLY in `translate`, called at
    /// the same `elapsed`/`base_dot_rows`, must be a cache HIT on the second
    /// call (no recompute) — `translate` is applied later by `place()` and must
    /// never invalidate the transform-path cache.
    #[test]
    fn rasterize_at_translate_only_change_is_cache_hit() {
        let s = make_sprite();
        let t1 = Transform::default();
        let t2 = Transform { translate: WorldPos::new(500.0, -500.0), ..Transform::default() };
        let first = s.rasterize_at(Duration::ZERO, &t1, BR);
        assert_eq!(s.recompute_count(), 1);
        let second = s.rasterize_at(Duration::ZERO, &t2, BR);
        assert_eq!(
            s.recompute_count(),
            1,
            "a translate-only difference must not trigger a recompute"
        );
        assert_eq!(first, second, "translate-only difference must return the same cached buffer");
    }

    /// A changed `rotation` at the same `elapsed`/`base_dot_rows` must force a
    /// fresh recompute.
    #[test]
    fn rasterize_at_rotation_change_recomputes() {
        let s = make_sprite();
        let t1 = Transform::default();
        let t2 = Transform { rotation: 90.0, ..Transform::default() };
        s.rasterize_at(Duration::ZERO, &t1, BR);
        assert_eq!(s.recompute_count(), 1);
        s.rasterize_at(Duration::ZERO, &t2, BR);
        assert_eq!(s.recompute_count(), 2, "changed rotation must trigger a recompute");
    }

    /// A changed `scale` at the same `elapsed`/`base_dot_rows` must force a
    /// fresh recompute.
    #[test]
    fn rasterize_at_scale_change_recomputes() {
        let s = make_sprite();
        let t1 = Transform::default();
        let t2 = Transform { scale: Vec2::new(2.0, 1.0), ..Transform::default() };
        s.rasterize_at(Duration::ZERO, &t1, BR);
        assert_eq!(s.recompute_count(), 1);
        s.rasterize_at(Duration::ZERO, &t2, BR);
        assert_eq!(s.recompute_count(), 2, "changed scale must trigger a recompute");
    }

    /// A changed `base_dot_rows` at the same `elapsed`/transform must force a
    /// fresh recompute AND produce a differently-sized buffer — guards the
    /// resize-staleness bug (a key omitting `base_dot_rows` would silently
    /// serve a stale, wrong-sized buffer after a terminal resize).
    #[test]
    fn rasterize_at_base_dot_rows_change_recomputes() {
        let s = make_sprite();
        let tf = Transform::default();
        let first = s.rasterize_at(Duration::ZERO, &tf, BR);
        assert_eq!(s.recompute_count(), 1);
        let second = s.rasterize_at(Duration::ZERO, &tf, BR * 2);
        assert_eq!(s.recompute_count(), 2, "changed base_dot_rows must trigger a recompute");
        assert_ne!(first, second, "changed base_dot_rows must produce a differently-sized buffer");
    }

    /// An `elapsed` that resolves to a different `frame_index_at` must force a
    /// fresh recompute, and the two buffers must differ (no cross-frame cache
    /// bleed).
    #[test]
    fn rasterize_at_frame_change_recomputes() {
        let s = make_sprite();
        let tf = Transform::default();
        let frame0 = s.rasterize_at(Duration::ZERO, &tf, BR);
        assert_eq!(s.recompute_count(), 1);
        let frame1 = s.rasterize_at(Duration::from_millis(100), &tf, BR);
        assert_eq!(s.recompute_count(), 2, "a different resolved frame_index must trigger a recompute");
        assert_ne!(frame0, frame1, "distinct source frames (red vs green) must produce distinct buffers");
    }

    // ── dots_at: cached plain-path accessor (b1-t2) ──────────────────────────

    const DC: u32 = 4;
    const DR: u32 = 8;

    /// Two calls with the same resolved `(frame_index, dot_cols, dot_rows)`
    /// must perform exactly ONE real recompute (the second is a cache hit),
    /// and both calls must return equal buffers.
    #[test]
    fn dots_at_second_identical_call_is_cache_hit() {
        let s = make_sprite();
        let first = s.dots_at(Duration::ZERO, DC, DR);
        let second = s.dots_at(Duration::ZERO, DC, DR);
        assert_eq!(first, second, "identical calls must return equal buffers");
        assert_eq!(
            s.recompute_count(),
            1,
            "second call with identical (frame_index, dot_cols, dot_rows) must be a cache hit, not a recompute"
        );
    }

    /// `dots_at` output must be byte-identical to calling `sprite_to_dots`
    /// directly on the frame resolved by `frame_at` for the same elapsed time.
    #[test]
    fn dots_at_matches_sprite_to_dots_directly() {
        let s = make_sprite();
        for t in [Duration::ZERO, Duration::from_millis(100), Duration::from_millis(200)] {
            let via_cache = s.dots_at(t, DC, DR);
            let direct = sprite_to_dots(s.frame_at(t), DC, DR);
            assert_eq!(via_cache, direct, "dots_at must match sprite_to_dots directly for t={t:?}");
        }
    }

    /// A changed `dot_cols` or `dot_rows` at the same elapsed time must force
    /// a fresh recompute each time (no false cache hit across distinct dims).
    #[test]
    fn dots_at_different_dims_recompute() {
        let s = make_sprite();
        s.dots_at(Duration::ZERO, DC, DR);
        assert_eq!(s.recompute_count(), 1);
        s.dots_at(Duration::ZERO, DC + 1, DR);
        assert_eq!(s.recompute_count(), 2, "changed dot_cols must trigger a recompute");
        s.dots_at(Duration::ZERO, DC + 1, DR + 1);
        assert_eq!(s.recompute_count(), 3, "changed dot_rows must trigger a recompute");
    }

    /// An `elapsed` that resolves to a different `frame_index_at` must force a
    /// fresh recompute, and the two buffers must differ (no cross-frame
    /// cache bleed between distinct source frames).
    #[test]
    fn dots_at_different_frame_recompute() {
        let s = make_sprite();
        let frame0 = s.dots_at(Duration::ZERO, DC, DR);
        assert_eq!(s.recompute_count(), 1);
        let frame1 = s.dots_at(Duration::from_millis(100), DC, DR);
        assert_eq!(s.recompute_count(), 2, "a different resolved frame_index must trigger a recompute");
        assert_ne!(frame0, frame1, "distinct source frames (red vs green) must produce distinct buffers");
    }
}
