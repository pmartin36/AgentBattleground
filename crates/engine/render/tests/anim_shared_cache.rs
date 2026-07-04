//! RED test for b2-t1: `AnimatedSprite`'s bytes-known (`from_gif`) path must
//! route `dots_at`/`rasterize_at` through the shared b1-t2 rasterize cache
//! (`asset_cache::plain_cached`/`transform_cached`), so two SEPARATE sprites
//! built from the SAME `'static` GIF bytes share one rasterization instead of
//! each maintaining its own private per-instance cache.
//!
//! Kept in its OWN test binary (not alongside `from_gif_importer.rs` or
//! `anim_golden.rs`) so the `asset_cache::rasterize_recompute_count()`
//! before/after delta this test samples is race-free: this file is the SOLE
//! mutator of the shared rasterize-recompute counter in its own test binary
//! (each `tests/*.rs` file compiles to an independent binary with its own
//! copy of the lib's process-global statics).

use engine_render::{asset_cache, AnimatedSprite};
use image::codecs::gif::{GifEncoder, Repeat};
use image::{Frame, Rgba as PixelRgba, RgbaImage};
use std::sync::Mutex;
use std::time::Duration;

/// Serializes each test's full before/rasterize/after sampling window.
/// `asset_cache::rasterize_recompute_count()` is a single process-global
/// counter shared by BOTH `#[test]` fns in this file; libtest runs them on
/// separate OS threads by default, so without this guard one test's
/// `dots_at`/`rasterize_at` call can land inside the other's `[before,
/// after]` window and inflate its observed delta from 1 to 2 (this is the
/// exact flake diagnosed by the validator on the prior iteration — NOT a
/// code defect: `--test-threads=1` and isolated single-test runs always
/// showed the correct delta of 1). Mirrors `asset_cache.rs`'s own
/// `TEST_LOCK` convention. Held for each test's entire before/after
/// measurement window makes the delta race-free regardless of thread
/// scheduling.
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Encode a small `frame_count`-frame GIF and leak it to `'static` so two
/// `AnimatedSprite`s can be constructed from the identical byte pointer.
fn make_gif_static(frame_count: u32) -> &'static [u8] {
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
    Box::leak(buf.into_boxed_slice())
}

/// Two `AnimatedSprite`s constructed via `from_gif` on the SAME `'static`
/// bytes constant, rasterized at the same frame/dims, must perform exactly
/// ONE real rasterization TOTAL across both instances (shared-cache hit on
/// the second sprite's call), per b2-t1's cross-instance-sharing contract.
#[test]
fn two_from_gif_sprites_same_bytes_share_one_rasterization_via_dots_at() {
    let _guard = test_lock();
    let bytes = make_gif_static(2);
    let s1 = AnimatedSprite::from_gif(bytes, Duration::from_millis(100))
        .expect("valid GIF must decode to Ok");
    let s2 = AnimatedSprite::from_gif(bytes, Duration::from_millis(100))
        .expect("valid GIF must decode to Ok");

    let before = asset_cache::rasterize_recompute_count();
    let a = s1.dots_at(Duration::ZERO, 8, 16);
    let b = s2.dots_at(Duration::ZERO, 8, 16);
    let delta = asset_cache::rasterize_recompute_count() - before;

    assert_eq!(
        delta, 1,
        "two from_gif sprites built from the SAME 'static bytes must share ONE \
         rasterization total via the shared cache, not one each via private \
         per-instance caches"
    );
    assert_eq!(
        a, b,
        "the shared cache must serve a byte-identical buffer to both sprites"
    );
}

/// Same cross-instance-sharing contract via the transform path
/// (`rasterize_at` -> `asset_cache::transform_cached`).
#[test]
fn two_from_gif_sprites_same_bytes_share_one_rasterization_via_rasterize_at() {
    let _guard = test_lock();
    let bytes = make_gif_static(2);
    let s1 = AnimatedSprite::from_gif(bytes, Duration::from_millis(100))
        .expect("valid GIF must decode to Ok");
    let s2 = AnimatedSprite::from_gif(bytes, Duration::from_millis(100))
        .expect("valid GIF must decode to Ok");

    let transform = engine_render::transform::Transform::new(
        engine_render::camera::WorldPos::new(0.0, 0.0),
        0.0,
        engine_render::transform::Vec2::new(1.0, 1.0),
    );

    let before = asset_cache::rasterize_recompute_count();
    let a = s1.rasterize_at(Duration::ZERO, &transform, 16);
    let b = s2.rasterize_at(Duration::ZERO, &transform, 16);
    let delta = asset_cache::rasterize_recompute_count() - before;

    assert_eq!(
        delta, 1,
        "two from_gif sprites built from the SAME 'static bytes must share ONE \
         transform rasterization total via the shared cache"
    );
    assert_eq!(
        a, b,
        "the shared cache must serve a byte-identical transform buffer to both sprites"
    );
}
