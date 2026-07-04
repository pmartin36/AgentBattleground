# Static Asset Rasterization Caching

> **Status: draft (not started).** Closes a real gap between `27-render-frame-caching` and `30-asset-decode-caching`, and consolidates both into one coherent, fully-automatic, engine-owned mechanism. Confirmed via direct re-measurement during `30`'s verification: `MainHub::render()` still averages ~161ms/frame in debug after `30`'s decode-once fix landed — ~149ms of that is `render::convert`/`sprite_to_dots` re-resizing the 1212×481 logo every frame. In release this "only" costs ~2.6ms/frame, but it's fully redundant work every frame regardless of profile.

## Purpose
Every image this game ever rasterizes — creature GIFs, the logo, button panels, icons — originates from `'static` bundled bytes (`include_bytes!`). That fact makes a single, fully-automatic, engine-level cache both possible and safe: bake caching directly into the engine's shared rendering primitives so **any caller gets it for free**, with zero scene-side caching code, present or future. This is a stronger requirement than "cache the sprites we know about today" — a hypothetical new scene (e.g. a `ScoreScreen`) rendering duplicate sprites must get correct caching automatically, without its author writing or even knowing about any caching code.

## Root Cause / Current State
- `27-render-frame-caching`'s cache is a `RefCell<RasterCache>` field *inside the `AnimatedSprite` struct* — it only helps callers that hold an `AnimatedSprite` instance (`battle_viewer.rs`'s pieces, `roster_manager.rs`'s carousel).
- `30-asset-decode-caching`'s fix is a manually-added, decode-once `DynamicImage` field on specific structs (`MainHub`, `RosterManager`) — it requires each struct's author to remember to add the field and read from it instead of calling `image::load_from_memory`/`sprite_to_dots` the "obvious" way.
- Neither mechanism is automatic. Both require a scene author to opt in by writing caching-aware code instead of just calling the normal rendering entry points (`sprite_to_dots`, `render::convert`, `render::draw_asset`, `transform::rasterize`). Concretely, this leaves real, uncached per-frame rasterization in `Button`/`FrameButton`'s `render_tinted` (calls `sprite_to_dots` on the panel and icon every render, for every button in every scene) — arguably the largest real cost of any of these, since buttons render constantly.
- Compounding the gap: even where `30`'s per-struct caching exists, it's per-*instance*, not per-*content* — `MainHub` alone constructs 3 separate `FrameButton`s that all pass the identical `assets::FRAME_PANEL` bytes at the identical rect size. A naive per-instance fix would still rasterize the same panel 3 times instead of once.

## The Design
One mechanism, living entirely in `crates/engine/render` (per `31-engine-game-crate-split`'s boundary — this is 100% generic, asset-agnostic machinery, zero game-domain knowledge), with two layers:

1. **Decode cache**: keyed on the *source bytes' pointer* (`bytes.as_ptr()` for a `'static &[u8]` bundled asset) → the decoded image content. Decoded once per unique bytes, ever, kept alive for the process's lifetime.
2. **Rasterize cache**: keyed on `(source bytes pointer, [frame index, for animated content], target dims or transform)` → `DotBuffer`. Same lifetime/scope as the decode cache.

**Critical correctness constraint — read before implementing:** the cache key MUST be the *original `'static` source bytes' pointer*, never the *decoded image's* own memory address. A `'static` byte slice lives in the binary's rodata for the entire process lifetime and is never freed, so keying on it has no reuse hazard. Keying on a decoded image's address is unsound: once whatever owned that decoded image is dropped (e.g. a scene is torn down on switch), its heap allocation is freed and Rust's allocator is free to reuse that exact address for a completely unrelated later allocation — a cache still keyed on the old address would then serve stale, wrong data to something unrelated. Do not key on `img.as_bytes().as_ptr()` or any other property of the *decoded* value; only the original `&'static [u8]` bytes reference is safe to use as an identity.

This single mechanism replaces (not supplements) the ad-hoc approaches from `27` and `30`:
- **`27`'s `AnimatedSprite` cache** gets consolidated into this shared mechanism. `AnimatedSprite`'s public API (`dots_at`, `rasterize_at`) stays the same — same signatures, same tests, same observable behavior — but internally becomes a thin wrapper over the shared engine-level cache instead of its own private per-instance `HashMap`. This requires `AnimatedSprite` to retain a reference to its own source GIF bytes (today `from_gif` decodes once and discards the original bytes) so it can key into the shared cache. `27`'s existing regression tests (proving cache-hit/miss behavior, byte-exact output) must all still pass unmodified — this is an internal consolidation of `27`'s mechanism, not a change to its contract.
- **`30`'s manually-added `DynamicImage` fields** (`MainHub.logo`/`title_frame`, `RosterManager.dot_filled`/`dot_unfilled`) become unnecessary and should be removed — those scenes call the new engine-level cached entry point directly (e.g. passing `assets::LOGO` bytes each render) instead of storing and reading their own decoded copy. This is a net simplification of scene code, not just a performance fix: less state to own, nothing scene-specific to get wrong.
- **`Button`/`FrameButton`'s `render_tinted`** calls the same engine-level cached entry point for its panel/icon rasterization instead of calling `sprite_to_dots` directly — this alone gets full cross-instance sharing (3 `MainHub` `FrameButton`s sharing one rasterization) automatically, with no button-specific cache code, because the cache lives beneath `sprite_to_dots`, not beside it.
- **`29`-tint-shape-invariance is untouched.** Tint remains strictly downstream of whatever the cache returns — cached content is always the pre-tint buffer, tint is always recomputed fresh every render, exactly as `27`/`29` already established. Verify by confirming `Button`/`FrameButton`'s and `battle_viewer.rs`'s existing glyph-mask-invariance regression tests (`29`'s deliverable) still pass unmodified.

## Scope
- Design and implement the decode-cache + rasterize-cache pair described above, living in `engine-render`, with a small set of public entry points any caller (present or future, in `engine-render` or `game`) uses instead of calling `sprite_to_dots`/`rasterize`/`image::load_from_memory` directly for anything backed by `'static` bytes. Exact function names/shapes are for research to design against the actual current code, but the *effect* must be: any caller passing the same `'static` bytes + same dims/frame gets a cache hit, automatically, no caller-side cache bookkeeping.
- Consolidate `AnimatedSprite`'s existing cache (`27`) into this shared mechanism, preserving its public API and all existing tests.
- Remove `MainHub`/`RosterManager`'s manually-added decoded-image fields (`30`) in favor of calling the new shared entry point directly.
- Route `Button`/`FrameButton`'s `render_tinted` panel/icon rasterization through the same shared entry point.
- Audit the whole workspace (grep, don't trust a stale count) for any other direct `sprite_to_dots`/`rasterize`/`render::convert`/`render::draw_asset`/`image::load_from_memory` call site backed by `'static` bytes and route it through the same mechanism, or record explicitly why it's out of scope (e.g. a genuinely one-off, non-per-frame call, or something backed by non-`'static` content — confirm no such case currently exists in the render path before assuming it doesn't).
- Re-measure `MainHub::render()` in debug after the fix (the same measurement `30` used) and confirm the ~149ms of logo rasterization is gone (near-zero after the first render) — the concrete proof the gap `30` left open is actually closed.

Out of scope:
- Any change to the M1/M2 wire protocol, `Scene`/`SceneManager`, or anything outside the rendering pipeline — this is purely a rendering-internals consolidation.
- Non-`'static`, runtime-supplied images (e.g. a hypothetical user-uploaded avatar) — nothing in the current codebase renders anything but bundled, `'static` assets; if this ever changes, a content-hash-based key (rather than a pointer-based one) would be needed for that specific case, but that's not a today problem.
- Eviction of cache entries — per `27`/`30`'s established precedent, the key space here is bounded by "how many distinct bundled assets + creature GIFs this game ships with," which is small and fixed; a cache that's never evicted and lives for the process's lifetime is fine, not a leak.

## Decisions (v1)
- **Cache key is always derived from the original `'static` source bytes' pointer, never from a decoded image's address.** This is the load-bearing safety property of the whole design (see Root Cause / Current State above) — a regression test must exist proving this isn't accidentally violated (e.g. constructing two independently-decoded images from the same bytes at different addresses and confirming they still share one cache entry, keyed correctly).
- **One shared cache, not per-scene or per-type caches.** Every current caller (`AnimatedSprite`, `MainHub`, `RosterManager`, `Button`, `FrameButton`) and every future one routes through the same underlying mechanism; there is no "does this specific caller need caching" decision left for a scene author to make.
- **`AnimatedSprite` needs to retain its own source bytes** (currently discarded after `from_gif` decodes them) so it can key into the shared cache — a small, necessary API-internal change, not a public API change.
- **Verification requirements**:
  - Re-measure debug-build `MainHub::render()` timing after the fix and record the actual number (per `27`/`30`'s established convention — not "done" until re-measured).
  - A test proving cross-instance sharing actually works: construct two separate `Button`/`FrameButton` instances with identical bundled bytes at identical dims, render both, and confirm only one real rasterization occurred (an instrumented recompute counter, not just "both render correctly").
  - A test proving the pointer-reuse safety property described above isn't violated.
  - Confirm `27`'s and `29`'s existing test suites pass unmodified after this consolidation — this spec changes internals, not observable behavior, for either.

## Dependencies
- `27-render-frame-caching` ✅ — the mechanism and principle this spec generalizes and consolidates into the engine-level cache.
- `30-asset-decode-caching` ✅ — the decode-once insight this spec generalizes from "once per struct" to "once per unique bytes, globally"; this spec's manual per-struct fields become unnecessary.
- `29-tint-shape-invariance` ✅ — the `dots_to_grid_tinted` mechanism this spec's caching must not disturb (cached content is always pre-tint).
- `31-engine-game-crate-split` ✅ — this entire mechanism lives in `engine-render`, zero game-domain knowledge, consistent with the engine/game boundary.
- `22-braille-ui-chrome` ✅ — `Button`/`FrameButton`'s `render_tinted`, a primary fix target here.
- `25-main-hub-navigation` ✅ — `MainHub`'s logo/title-frame, the originally-reported symptom's actual root cause.
