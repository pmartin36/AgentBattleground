# Asset Decode Caching

> **Status: draft (not started).** Bundled raster assets (logos, panels, frames — anything drawn via `render::draw_asset` or similar ad-hoc `image::load_from_memory` calls) are decoded from raw PNG bytes fresh on *every single render call*, with no caching. Confirmed via direct timing in a debug build: `MainHub::render()` averages ~198ms/frame (vs. ~5ms/frame in release) — of which decoding the bundled logo alone costs ~29ms *per frame*, 30 times a second. This is very likely the root cause of user-reported "insane" button-click latency on the Main Hub menu (at ~200ms/frame, real framerate craters from the intended 30fps to ~5fps, so input can only be processed roughly every 200ms).

## Purpose
Decode a bundled asset's PNG bytes into a `DynamicImage` once, not once per frame.

## Root Cause
- `render::draw_asset` (`crates/render/src/lib.rs`) takes raw `bytes: &[u8]` and calls `image::load_from_memory(bytes)` *inside the function*, every time it's called. `MainHub::render()` calls this once per frame for the logo, and `MainHub::draw_title_frame` separately calls `image::load_from_memory` for the frame panel — both on every render, with nothing cached between frames.
- Contrast with `Button`/`FrameButton` (`crates/render/src/button.rs`), which already do this correctly: `Button::new`/`FrameButton::new` decode `BUTTON_PANEL`/`FRAME_PANEL`/the icon bytes *once*, at construction, storing the decoded `DynamicImage` as a struct field (`self.panel`, `self.icon`, `self.frame`). `render()` only ever reads the already-decoded field.
- `MainHub` has no equivalent stored field for the logo/title-frame images — it's a unit-ish struct without per-asset state for these two, and `render::draw_asset`'s free-function signature (`bytes: &[u8]` in, decode-and-draw in one call) has no natural place to cache a decoded image between calls at all.
- This is a distinct waste from `27-render-frame-caching` (which is about caching the *rasterized* `DotBuffer`/`sprite_to_dots` output, keyed on animation-frame-index + transform) — that spec's scope starts from an already-decoded `DynamicImage`; this spec is about the decode step itself, for assets that aren't part of an `AnimatedSprite` at all (static logos/panels, not creature GIFs).

## Scope
- `MainHub` gains stored, decoded-once fields for the logo and the title frame panel (mirroring `Button`'s existing pattern), populated in `Default::default()`/`new()`, read (not re-decoded) in `render()`.
- Audit every other `image::load_from_memory` call site across the codebase for the same anti-pattern (grep-confirm; known candidates: anywhere still calling `render::draw_asset` per-frame for a static, non-animated asset) and apply the same fix — decode once at scene/widget construction, store the decoded image, read it in `render()`.
- Re-measure the same debug-build timing test used to discover this (`MainHub::render()` averaged per frame) after the fix, and confirm it drops to a small fraction of the 33ms frame budget — the fix isn't "done" until that's actually re-measured, not just assumed from the diagnosis.

Out of scope:
- `AnimatedSprite`-based assets (creature GIFs) — these already decode once at construction (`AnimatedSprite::from_gif`) and only re-select a frame index per render; they don't have this problem.
- `27-render-frame-caching`'s rasterization-caching scope — a separate, deeper optimization this spec doesn't attempt.
- Whether debug-build performance in general is an acceptable target for "how the game feels during development" vs. "always test with `--release`" — a process/expectations question for the project owner, not something this spec resolves by itself. This spec fixes the specific, unambiguous waste (re-decoding unchanged bytes every frame) regardless of that broader question.

## Decisions (v1)
- **Decode once, store as a field, read in `render()`** — the exact pattern `Button`/`FrameButton` already establish. No new caching abstraction/library needed; this is a straightforward "don't do redundant work" fix, not a cache-invalidation problem (bundled asset bytes never change at runtime).
- **`render::draw_asset`'s free-function signature is likely still fine for genuinely one-off/rare draws** (if any exist) but should not be used for anything called every frame — implementer should identify every *actual* per-frame call site and move those specifically to a decode-once-and-store pattern, not necessarily delete `draw_asset` itself if it still has a legitimate rare-call use.

## Dependencies
- `22-braille-ui-chrome` ✅ — `Button`/`FrameButton`'s existing decode-once pattern is the template this spec applies elsewhere.
- `25-main-hub-navigation` ✅ — `MainHub` is the concrete scene this bug was found in and the primary fix target.
- Related but distinct: `27-render-frame-caching` (rasterization-result caching, different pipeline layer) and `29-tint-shape-invariance` (a correctness bug, not a performance one) — surfaced by the same investigation, not the same fix.
