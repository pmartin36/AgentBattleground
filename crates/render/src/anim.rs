//! Animated sprite: a sequence of frames played at a uniform rate.
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

/// A sequence of [`DynamicImage`] frames played in order at a uniform
/// per-frame duration, wrapping continuously.
///
/// The frame selection is exact: integer nanosecond division — no floats,
/// no rounding surprises at frame boundaries.
pub struct AnimatedSprite {
    frames: Vec<DynamicImage>,
    frame_dur: Duration,
}

impl AnimatedSprite {
    /// Construct from a frame list and a uniform per-frame duration.
    ///
    /// `frame_dur` is the caller-supplied display time per frame — it is NOT
    /// derived from GIF metadata or any external source.
    pub fn new(frames: Vec<DynamicImage>, frame_dur: Duration) -> Self {
        Self { frames, frame_dur }
    }

    /// Number of frames in the sprite.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// The uniform per-frame duration supplied at construction.
    pub fn frame_dur(&self) -> Duration {
        self.frame_dur
    }

    /// The frame index active at the given `elapsed` wall-clock time.
    ///
    /// `= (elapsed_nanos / frame_dur_nanos) % frame_count`, using exact
    /// integer division on nanoseconds. Returns 0 when `frame_count <= 1`
    /// or `frame_dur` is zero (no panic).
    pub fn frame_index_at(&self, elapsed: Duration) -> usize {
        let n = self.frames.len();
        if n <= 1 {
            return 0;
        }
        let denom = self.frame_dur.as_nanos();
        if denom == 0 {
            return 0;
        }
        ((elapsed.as_nanos() / denom) % n as u128) as usize
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
}
