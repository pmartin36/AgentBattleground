//! RED test for b3-t1: `MainHub` must route `FRAME_PANEL`/`LOGO`/
//! `ICON_ARROW_RIGHT` through the SHARED `engine_render::asset_cache`
//! rasterize cache (b1-t2) every render, instead of decoding once into its
//! own fields and re-rasterizing them itself per frame.
//!
//! Kept in its OWN test binary (not inside `main_hub.rs`'s `#[cfg(test)]`
//! unit-test module, which shares a process with `creatures.rs`/
//! `battle_viewer.rs`'s `AnimatedSprite` usage — also an `asset_cache`
//! caller) so the `asset_cache::rasterize_recompute_count()` before/after
//! delta sampled here is race-free: each `tests/*.rs` file compiles to an
//! independent binary/process with its own copy of the lib's process-global
//! statics, and `TEST_LOCK` below serializes this file's own two tests
//! against each other. Mirrors `crates/engine/render/tests/anim_shared_cache.rs`'s
//! established pattern for this exact same counter.

use std::sync::Mutex;

use engine_core::scene::Scene;
use engine_render::asset_cache;
use game::scenes::MainHub;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::Terminal;

/// Serializes each test's full before/render/after sampling window. The only
/// two tests in this binary both hold it for their entire measurement
/// window, so no concurrent cache miss from a sibling test can land inside
/// either window.
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn render_to_buffer(scene: &dyn Scene, w: u16, h: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|f| {
            let area = f.area();
            scene.render(f, area);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn has_non_space(buf: &Buffer, rect: Rect) -> bool {
    (rect.top()..rect.bottom())
        .flat_map(|y| (rect.left()..rect.right()).map(move |x| (x, y)))
        .any(|(x, y)| buf.cell((x, y)).unwrap().symbol() != " ")
}

/// A second `MainHub` render at the SAME area must be pure cache hits (no
/// additional rasterizations) — proves `FRAME_PANEL`/`LOGO`/
/// `ICON_ARROW_RIGHT` are routed through the shared cache, not re-rasterized
/// per frame, and shared across renders (not a per-instance cache: both
/// renders use a fresh `MainHub::default()`).
#[test]
fn main_hub_second_render_reuses_cached_rasterization() {
    let _guard = test_lock();
    let (w, h) = (118u16, 48u16);

    let before_warm = asset_cache::rasterize_recompute_count();
    let _ = render_to_buffer(&MainHub::default(), w, h);
    let after_warm = asset_cache::rasterize_recompute_count();
    assert!(
        after_warm > before_warm,
        "first MainHub render must perform at least one real rasterization via \
         the shared cache (before={before_warm}, after={after_warm}) — MainHub \
         must route FRAME_PANEL/LOGO/ICON_ARROW_RIGHT through \
         engine_render::asset_cache, not decode+rasterize them itself"
    );

    let _ = render_to_buffer(&MainHub::default(), w, h);
    let after_second = asset_cache::rasterize_recompute_count();
    assert_eq!(
        after_second, after_warm,
        "a second MainHub render at the SAME area must be pure cache hits \
         (no additional rasterizations): after_warm={after_warm}, \
         after_second={after_second}"
    );
}

/// A resized area yields different fit dims for `FRAME_PANEL`/`LOGO`,
/// forcing a fresh rasterization (a new cache key) — and the new size must
/// still paint real content, not a stale/blank cached buffer at the wrong
/// dims. Uses dims disjoint from the sibling test's so the two tests' cache
/// keys never collide.
#[test]
fn main_hub_resize_recomputes_and_repaints() {
    let _guard = test_lock();
    let (w1, h1) = (130u16, 54u16);
    let (w2, h2) = (90u16, 38u16);

    let _ = render_to_buffer(&MainHub::default(), w1, h1); // warm this test's own key set
    let before = asset_cache::rasterize_recompute_count();

    let buf2 = render_to_buffer(&MainHub::default(), w2, h2);
    let after = asset_cache::rasterize_recompute_count();
    assert!(
        after > before,
        "rendering at a different area must force fresh rasterization for the \
         new fit dims (before={before}, after={after})"
    );

    let area2 = Rect::new(0, 0, w2, h2);
    assert!(
        has_non_space(&buf2, area2),
        "resized render at {area2:?} must still paint real content"
    );
}
