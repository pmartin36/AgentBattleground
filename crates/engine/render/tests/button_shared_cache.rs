//! RED test for b4-t1: `Button`/`FrameButton`'s `render_tinted` must route its
//! panel/icon rasterization through the shared b1-t2 rasterize cache
//! (`asset_cache::sprite_to_dots`), so two SEPARATE button instances built
//! from the SAME `'static` source bytes share one rasterization instead of
//! each recomputing independently.
//!
//! Kept in its OWN test binary (not alongside `golden.rs`/`anim_golden.rs`)
//! so the `asset_cache::rasterize_recompute_count()` before/after delta this
//! file samples is race-free: each `tests/*.rs` file compiles to an
//! independent binary with its own copy of the lib's process-global statics,
//! mirroring `anim_shared_cache.rs` (the b2-t1 precedent).

use engine_render::{asset_cache, Button, ButtonState, FrameButton};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::sync::Mutex;

/// Serializes each test's full before/render/after sampling window. Same
/// convention as `asset_cache.rs`'s own `TEST_LOCK` / `anim_shared_cache.rs`'s
/// `TEST_LOCK`: `rasterize_recompute_count()` is one process-global counter
/// shared by every `#[test]` fn in this binary, which libtest runs across
/// multiple OS threads by default.
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn encode_png(img: image::RgbaImage) -> Vec<u8> {
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("synthetic test fixture must encode to PNG");
    buf
}

/// Opaque-white rounded-rect on a transparent ground (mirrors `button.rs`'s
/// own `rounded_rect_png` test fixture — this file cannot see that private
/// `#[cfg(test)]` helper, so it rebuilds an equivalent one from public APIs
/// only). Leaked to `'static` so distinct calls yield distinct, stable
/// pointers for cache-key isolation between test cases.
fn leaked_rounded_rect(w: u32, h: u32, r: i32) -> &'static [u8] {
    let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 0]));
    let x_lo = r;
    let x_hi = w as i32 - 1 - r;
    let y_lo = r;
    let y_hi = h as i32 - 1 - r;
    for y in 0..h {
        for x in 0..w {
            let (xi, yi) = (x as i32, y as i32);
            let cx = xi.clamp(x_lo, x_hi);
            let cy = yi.clamp(y_lo, y_hi);
            let (dx, dy) = (xi - cx, yi - cy);
            if dx * dx + dy * dy <= r * r {
                img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            }
        }
    }
    Box::leak(encode_png(img).into_boxed_slice())
}

/// Opaque-white silhouette blob inset from every edge, on a transparent
/// ground — stand-in for a bundled icon.
fn leaked_inset_blob(w: u32, h: u32, inset: u32) -> &'static [u8] {
    let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 0]));
    for y in inset..(h - inset) {
        for x in inset..(w - inset) {
            img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
    }
    Box::leak(encode_png(img).into_boxed_slice())
}

fn make_buf(w: u16, h: u16) -> Buffer {
    Buffer::empty(Rect::new(0, 0, w, h))
}

fn moved(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Moved,
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

/// Two `FrameButton`s built from the SAME `'static` frame bytes at the same
/// rect must perform exactly ONE real rasterization TOTAL across both
/// instances' renders (shared-cache hit on the second), not one each via
/// independent per-instance decoding.
#[test]
fn two_frame_buttons_same_bytes_share_one_rasterization() {
    let _guard = test_lock();
    let frame = leaked_rounded_rect(64, 32, 8);
    let rect = Rect::new(2, 1, 8, 4);

    let b1 = FrameButton::new(rect, frame, "Go");
    let b2 = FrameButton::new(rect, frame, "Go");

    let before = asset_cache::rasterize_recompute_count();
    let mut buf1 = make_buf(16, 8);
    b1.render(&mut buf1);
    let mut buf2 = make_buf(16, 8);
    b2.render(&mut buf2);
    let delta = asset_cache::rasterize_recompute_count() - before;

    assert_eq!(
        delta, 1,
        "two FrameButtons built from the SAME 'static bytes at the same rect \
         must share ONE rasterization total via the shared cache, not one \
         recompute per instance"
    );
}

/// A SECOND `Button` instance built from the SAME panel+icon bytes at the
/// same rect, after the first instance has already been rendered once, must
/// be a pure cache hit: rendering it performs ZERO additional
/// rasterizations (both the panel and the icon's pre-tint raster are
/// already cached).
#[test]
fn second_button_instance_same_bytes_is_a_pure_cache_hit() {
    let _guard = test_lock();
    let panel = leaked_rounded_rect(64, 32, 8);
    let icon = leaked_inset_blob(48, 48, 12);
    let rect = Rect::new(2, 1, 8, 4);

    let first = Button::new(rect, panel).icon(icon);
    let mut warm_buf = make_buf(16, 8);
    first.render(&mut warm_buf); // warms the shared cache for (panel, rect) and (icon, fitted dims)

    let second = Button::new(rect, panel).icon(icon);
    let before = asset_cache::rasterize_recompute_count();
    let mut buf = make_buf(16, 8);
    second.render(&mut buf);
    let delta = asset_cache::rasterize_recompute_count() - before;

    assert_eq!(
        delta, 0,
        "a second Button built from bytes already rasterized by another \
         instance at the same rect must not trigger any new rasterization"
    );
}

/// Distinct source bytes, or the same bytes at distinct rect dimensions,
/// must each force an independent rasterization — no key conflation.
#[test]
fn distinct_bytes_or_dims_force_independent_rasterizations() {
    let _guard = test_lock();

    // Two different frame images at the same rect -> 2 recomputes.
    let frame_a = leaked_rounded_rect(64, 32, 8);
    let frame_b = leaked_rounded_rect(64, 32, 10);
    let rect = Rect::new(2, 1, 8, 4);

    let before = asset_cache::rasterize_recompute_count();
    let ba = FrameButton::new(rect, frame_a, "Go");
    let bb = FrameButton::new(rect, frame_b, "Go");
    let mut buf_a = make_buf(16, 8);
    ba.render(&mut buf_a);
    let mut buf_b = make_buf(16, 8);
    bb.render(&mut buf_b);
    let delta = asset_cache::rasterize_recompute_count() - before;
    assert_eq!(
        delta, 2,
        "two distinct source images at the same rect must each recompute independently"
    );

    // The SAME frame image rendered at two differently-sized rects -> 2 recomputes.
    let frame_c = leaked_rounded_rect(64, 32, 8);
    let rect_small = Rect::new(2, 1, 8, 4);
    let rect_large = Rect::new(2, 1, 10, 5);

    let before2 = asset_cache::rasterize_recompute_count();
    let small = FrameButton::new(rect_small, frame_c, "Go");
    let large = FrameButton::new(rect_large, frame_c, "Go");
    let mut buf_small = make_buf(16, 8);
    small.render(&mut buf_small);
    let mut buf_large = make_buf(16, 8);
    large.render(&mut buf_large);
    let delta2 = asset_cache::rasterize_recompute_count() - before2;
    assert_eq!(
        delta2, 2,
        "the same source image rendered at two distinct rect sizes must each \
         recompute independently"
    );
}

/// A `ButtonState` change between two renders of the SAME instance must NOT
/// force a second pre-tint rasterization — tint is strictly downstream of
/// the cached raster. Across an Idle render then a Hover render, the total
/// delta must be exactly 1 (the first render's miss only), while the two
/// renders still paint visibly different colors (proving the tint itself
/// did take effect).
#[test]
fn state_change_between_renders_does_not_force_extra_rasterization() {
    let _guard = test_lock();
    let frame = leaked_rounded_rect(64, 32, 8);
    let rect = Rect::new(2, 1, 8, 4);
    let mut b = FrameButton::new(rect, frame, "Go");

    let before = asset_cache::rasterize_recompute_count();
    let mut buf_idle = make_buf(16, 8);
    b.render(&mut buf_idle);
    assert_eq!(b.state(), ButtonState::Idle);

    b.handle_mouse(&moved(rect.x + 1, rect.y));
    assert_eq!(b.state(), ButtonState::Hover);
    let mut buf_hover = make_buf(16, 8);
    b.render(&mut buf_hover);

    let delta = asset_cache::rasterize_recompute_count() - before;
    assert_eq!(
        delta, 1,
        "a ButtonState change between two renders of the same instance must \
         not trigger a second pre-tint rasterization"
    );

    let idle_fg = buf_idle
        .cell((rect.x + rect.width / 2, rect.y))
        .expect("top-border cell must exist")
        .fg;
    let hover_fg = buf_hover
        .cell((rect.x + rect.width / 2, rect.y))
        .expect("top-border cell must exist")
        .fg;
    assert_ne!(
        idle_fg, hover_fg,
        "Idle and Hover renders must still paint visibly different colors"
    );
}
