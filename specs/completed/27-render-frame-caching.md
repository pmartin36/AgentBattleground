# Render Frame Caching

> # ✅ DONE! — Completed 2026-07-03
> Status: implemented. `AnimatedSprite` (`crates/render/src/anim.rs`) caches rasterized `DotBuffer` output per instance, keyed by animation-frame index + dot dims (plain path, `dots_at`) or + rotation/scale (transform path, `rasterize_at`; `translate` excluded from the key). Wired into `battle_viewer.rs::piece_dots` and `roster_manager.rs::render_group`. Caching is scoped strictly to the pre-tint rasterization step — tint is always recomputed fresh on top of the (possibly cached) buffer, so it is unaffected by this cache.
>
> Skip re-rasterizing a sprite (`sprite_to_dots`/`rasterize`, `13-rendering` / `16-world-space-and-camera`) when nothing that affects its output has changed since the last frame. Split out as new scope rather than added to `13-rendering`'s TBDs — `13` is already marked done.

## Purpose
Every sprite is currently fully re-rasterized every frame, unconditionally, even when its animation frame and transform haven't changed since the previous tick. At 30fps with a GIF frame duration of ~100ms, the same source image gets resized/re-rasterized ~3× before the animation frame index ever advances. This spec is about caching that redundant work.

## Scope
- Detect when a sprite's rasterized output would be identical to last frame (same animation-frame index + same `Transform`) and reuse the cached `DotBuffer` instead of recomputing.
- Applies to both `sprite_to_dots` (plain image → dots) and `transform.rs::rasterize` (scale/rotate → dots).

Out of scope:
- The crowd-level "pre-rendered frames per band, reused across instances" caching gestured at in `13-rendering`'s Crowd/Battlefield Compositing section — that's sharing one rasterized frame across many sprite *instances*, a different (and larger) scope than one sprite's own frame-to-frame redundancy.
- Compositing/threshold caching (`composite_dots`, `dots_to_grid`) — unexplored, not this spec's scope.

## Open Questions / TBDs
- Cache key: sprite identity + animation-frame index + `Transform`. `rasterize` already ignores `translate` (translation is applied later, at `place()`, not baked into the buffer) — so only rotation/scale need to invalidate the cache, not the full `Transform`.
- Where the cache lives: per-`AnimatedSprite` instance state vs. a shared cache keyed by content hash.
- Eviction/lifetime — fixed-size LRU vs. cleared on scene exit vs. left unbounded (likely fine — bounded by number of distinct creatures/sprites on screen).
- Whether this is worth building before a real perf ceiling is hit (see `13-rendering`'s existing "performance ceiling" TBD) — may be premature without a profiled hotspot.

## Dependencies
- `13-rendering` — the `sprite_to_dots`/`dots_to_grid` pipeline this caches around.
- `16-world-space-and-camera` — `Transform`/`rasterize`, the other rasterization entry point this applies to.
