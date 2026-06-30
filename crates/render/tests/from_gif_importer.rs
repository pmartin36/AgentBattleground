//! RED tests for `AnimatedSprite::from_gif` (b1-t2).
//!
//! These tests cover the observable contract of the GIF importer:
//! - Valid GIF bytes → `Ok`; frame count matches; frames are RGBA8; `frame_dur` round-trips.
//! - Malformed bytes → `Err`, no panic.
//! - Alpha channel preserved (transparent GIF pixels decode with alpha = 0).
//!
//! No `anim.gif` fixture is required here: tests build minimal GIFs inline via
//! `image`'s `GifEncoder`, per the DEPENDENCY NOTE in research.md.

use image::codecs::gif::{GifEncoder, Repeat};
use image::{DynamicImage, Frame, GenericImageView, Rgba as PixelRgba, RgbaImage};
use render::AnimatedSprite;
use std::time::Duration;

// ── helper: encode N distinct frames into a GIF byte buffer ──────────────────

/// Encode `frame_count` small (4×4) RGBA frames into a GIF.
/// Frame `i` has pixel (0,0) set to `(i * 80, 0, 0, 255)` (opaque, distinct
/// per frame) and all other pixels set to opaque black.
fn make_gif(frame_count: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut encoder = GifEncoder::new_with_speed(&mut buf, 10);
        encoder.set_repeat(Repeat::Infinite).unwrap();
        for i in 0..frame_count {
            let mut img = RgbaImage::new(4, 4);
            for p in img.pixels_mut() {
                *p = PixelRgba([0, 0, 0, 255]);
            }
            img.put_pixel(0, 0, PixelRgba([(i * 80) as u8, 0, 0, 255]));
            encoder.encode_frame(Frame::new(img)).unwrap();
        }
    } // encoder drops + flushes here
    buf
}

// ── from_gif on valid input ───────────────────────────────────────────────────

/// `from_gif` on a 3-frame GIF must return `Ok` with `frame_count() == 3`.
#[test]
fn from_gif_ok_frame_count() {
    let gif_bytes = make_gif(3);
    let dur = Duration::from_millis(100);
    let sprite = AnimatedSprite::from_gif(&gif_bytes, dur)
        .expect("valid GIF must decode to Ok");
    assert_eq!(sprite.frame_count(), 3, "from_gif must yield 3 frames for a 3-frame GIF");
}

/// The caller-supplied `frame_dur` must be stored verbatim; GIF's own per-frame
/// delays are not consulted (spec: uniform rate, delays ignored).
#[test]
fn from_gif_frame_dur_round_trips() {
    let gif_bytes = make_gif(2);
    let dur = Duration::from_millis(250);
    let sprite = AnimatedSprite::from_gif(&gif_bytes, dur)
        .expect("valid GIF must decode to Ok");
    assert_eq!(
        sprite.frame_dur(),
        dur,
        "from_gif must store the caller-supplied frame_dur, not derive it from GIF metadata"
    );
}

/// Decoded frames must be `DynamicImage::ImageRgba8` — the RGBA8 contract from
/// the blueprint (alpha preserved, not flattened to RGB).
#[test]
fn from_gif_frames_are_rgba8() {
    let gif_bytes = make_gif(2);
    let sprite = AnimatedSprite::from_gif(&gif_bytes, Duration::from_millis(100))
        .expect("valid GIF must decode to Ok");
    let frame = sprite.frame_at(Duration::ZERO);
    assert!(
        matches!(frame, DynamicImage::ImageRgba8(_)),
        "decoded GIF frame must be DynamicImage::ImageRgba8 (RGBA8), not another variant"
    );
}

// ── malformed input: must return Err, never panic ────────────────────────────

/// `from_gif` on garbage bytes must return `Err` — it must NOT panic or
/// `unwrap`. The no-panic contract is essential: opponent skill assets are
/// untrusted input (CLAUDE.md security requirement).
#[test]
fn from_gif_malformed_returns_err_no_panic() {
    let bad_bytes = b"not a gif at all \x00\x01\x02";
    let result = AnimatedSprite::from_gif(bad_bytes, Duration::from_millis(100));
    assert!(result.is_err(), "malformed bytes must return Err, got Ok");
}

/// Empty byte slice must return `Err`, not panic.
#[test]
fn from_gif_empty_bytes_returns_err() {
    let result = AnimatedSprite::from_gif(&[], Duration::from_millis(100));
    assert!(result.is_err(), "empty bytes must return Err, got Ok");
}

// ── alpha preservation ────────────────────────────────────────────────────────

/// GIF's binary transparency must be preserved: a pixel designated transparent
/// in the GIF palette decodes with alpha = 0. This verifies the decoder
/// produces RGBA8 with intact alpha channel rather than a flattened RGB.
#[test]
fn from_gif_alpha_preserved_for_transparent_pixels() {
    // Build a 1-frame GIF: pixel (1,0) is fully transparent (alpha=0),
    // pixel (0,0) is opaque red.
    let mut buf = Vec::new();
    {
        let mut encoder = GifEncoder::new_with_speed(&mut buf, 10);
        encoder.set_repeat(Repeat::Infinite).unwrap();
        let mut img = RgbaImage::new(2, 2);
        img.put_pixel(0, 0, PixelRgba([255, 0, 0, 255])); // opaque red
        img.put_pixel(1, 0, PixelRgba([0, 0, 0, 0]));     // fully transparent
        img.put_pixel(0, 1, PixelRgba([0, 0, 255, 255])); // opaque blue
        img.put_pixel(1, 1, PixelRgba([0, 0, 0, 0]));     // fully transparent
        encoder.encode_frame(Frame::new(img)).unwrap();
    }

    let sprite = AnimatedSprite::from_gif(&buf, Duration::from_millis(100))
        .expect("valid GIF must decode to Ok");
    let frame = sprite.frame_at(Duration::ZERO);
    let transparent_pixel = frame.get_pixel(1, 0);
    assert_eq!(
        transparent_pixel.0[3],
        0,
        "pixel (1,0) was fully transparent in the source; decoded alpha must be 0"
    );
}
