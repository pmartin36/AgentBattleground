//! Animated sprite: a sequence of frames played at a base rate scaled by a
//! runtime speed multiplier.
//!
//! Callers accumulate wall-clock elapsed time externally and call
//! [`AnimatedSprite::frame_index_at`] or [`AnimatedSprite::frame_at`] to
//! retrieve the active frame. No clock reads or mutation occur inside this
//! type.

use image::codecs::gif::GifDecoder;
use image::AnimationDecoder;
use image::DynamicImage;
use std::io::Cursor;
use std::time::Duration;

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
}

#[cfg(test)]
mod tests {
    use super::AnimatedSprite;
    use image::{DynamicImage, GenericImageView, Rgba as PixelRgba, RgbaImage};
    use std::time::Duration;

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
}
