//! RED test for b3-t2: `RosterManager` must route `DOT_FILLED`/`DOT_UNFILLED`
//! through the SHARED `engine_render::asset_cache` rasterize cache (b1-t2)
//! every render, instead of decoding them once into its own
//! `dot_filled`/`dot_unfilled` fields and re-rasterizing them itself
//! per-slot, per-frame.
//!
//! Why not a cross-crate probe (rejected after this file's first iteration
//! caused a false RED that could never go green): `DOT_FILLED`/`DOT_UNFILLED`
//! are `pub const` `&'static [u8]` (assets.rs), not `pub static`. Rust
//! re-materializes a `const`'s value at every reference site, so naming
//! `game::assets::DOT_FILLED` from THIS separate integration-test crate
//! yields different backing bytes (and thus a different `.as_ptr()`, the
//! cache's key) than the copy `roster_manager.rs` uses from inside the
//! `game` crate itself. A probe from here can never observe the SAME cache
//! entry `render_group` populated, no matter how correctly `RosterManager`
//! routes through the cache — so it is not a valid technique.
//!
//! Why not the sibling b3-t1 (`main_hub_shared_cache.rs`) "render twice,
//! second render is a pure hit" shape either: `RosterManager::render_group`
//! also renders an `AnimatedSprite` (creature idle frame) every call, and
//! `AnimatedSprite` already shares this SAME process-global counter
//! (b2-t1, landed) regardless of whether the dots are cached. So (a) "first
//! render performs >= 1 rasterization" would pass today from the sprite
//! alone, and (b) "second render at the same area adds 0" is ALSO already
//! true today even though the dots never touch the counter at all (a path
//! that never calls into `asset_cache` trivially contributes a 0 delta,
//! indistinguishable from a real cache hit). Neither shape discriminates.
//!
//! The technique that DOES discriminate: this file is its own test binary
//! (own process, own copy of the lib's process-global cache/counter,
//! guaranteed cold at start — no other test in this file or process can have
//! touched it yet) and there is exactly ONE cold path through
//! `RosterManager::render()` per render: `render_group` (one `AnimatedSprite`
//! frame plus, for the 6 dot slots — all identically sized per `dot_slots`,
//! so they collapse to at most one cache entry per image — at most
//! `DOT_FILLED` and `DOT_UNFILLED` once each) plus its three
//! `engine_render::Button`s (`left`/`right`/`home`), which as of b4-t1 also
//! route their panel+icon rasterization through this SAME shared cache. The
//! two arrow buttons share dims `(ARROW_W, ARROW_H)` with each other, but as
//! of spec 38's arrow-shrink correction those dims `(4, 2)` are DISTINCT from
//! the home button's `(HOME_W, HOME_H)` = `(6, 3)`, so `BUTTON_PANEL` yields
//! TWO cache entries (one at arrow dims, one at home dims), and
//! `ICON_ARROW_LEFT`/`ICON_ARROW_RIGHT`/`ICON_HOME` are three more
//! distinct-bytes entries. So a single cold render's EXACT recompute delta
//! is discriminating: routed through the shared cache (dots fixed by this
//! test's own task, buttons fixed by b4-t1) it is 8 (sprite + DOT_FILLED +
//! DOT_UNFILLED + 2 panel entries (arrow dims + home dims) + 3 icon entries,
//! each a first-ever miss); with the dots NOT routed through the cache it
//! would be 6 (sprite + the 2 panel + 3 icon entries only, since the dots
//! never touch `asset_cache` at all on that path). A plain "delta > 0" or
//! "second render adds 0" check cannot tell these apart; the exact value 8
//! vs. 6 can.

use std::sync::Mutex;

use engine_core::scene::Scene;
use engine_render::asset_cache;
use game::scenes::RosterManager;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

/// Serializes this file's (single) test's full before/render/after sampling
/// window against itself, mirroring `main_hub_shared_cache.rs`'s convention —
/// defensive against a future sibling test being added to this file.
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

/// A single cold `RosterManager` render (fresh scene, fresh process — no
/// prior renders in this binary) must perform EXACTLY 7 rasterizations:
/// creature idle sprite (1) + `DOT_FILLED` (1) + `DOT_UNFILLED` (1) +
/// `BUTTON_PANEL` at arrow dims (1) + `ICON_ARROW_LEFT` (1) +
/// `ICON_ARROW_RIGHT` (1) + `ICON_HOME` (1), each a first-ever miss. The home
/// button draws a procedural gold `circle` (no `BUTTON_PANEL` rasterization),
/// but still rasterizes `ICON_HOME` for its icon. A delta of 5 means the two
/// dot sprites never touched the shared cache (`RosterManager` still routes
/// them through its own decoded field instead of `engine_render::asset_cache`)
/// — a clean, deterministic RED against that code.
#[test]
fn roster_manager_render_routes_dots_through_shared_cache() {
    let _guard = test_lock();
    let scene = RosterManager::new();

    let before = asset_cache::rasterize_recompute_count();
    let _ = render_to_buffer(&scene, 40, 20);
    let after = asset_cache::rasterize_recompute_count();

    assert_eq!(
        after - before,
        7,
        "a single cold RosterManager render must perform exactly 7 \
         rasterizations (creature sprite + DOT_FILLED + DOT_UNFILLED + \
         BUTTON_PANEL at arrow dims + 3 distinct icon entries, each once — the \
         home button draws a procedural gold circle, not BUTTON_PANEL, but \
         still rasterizes ICON_HOME) — a delta of 5 means the dots never \
         touched the shared cache (RosterManager still routes them through its \
         own decoded field instead of engine_render::asset_cache); \
         before={before}, after={after}"
    );
}
