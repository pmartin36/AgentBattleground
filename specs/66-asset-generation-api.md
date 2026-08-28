# Asset Generation API

> **Status: draft (not started).** The single in-game API for producing creature art assets. Any scene or feature that needs a generated still, an animation for an existing still, or a still plus a set of animations calls this one API. Game-specific (`crates/game/`). Backed by two recipes: still-image generation (`17-creature-art-asset-pipeline`) and animation (`64-creature-animation-pipeline`).

## Purpose
Give every feature one uniform way to request generated art, so no scene re-wires generation for itself. The API owns the shared job lifecycle, caching, and GPU-gating; the two recipes behind it own *what model and what prompt* for images vs. animations. This is deliberately general (image + animations), not creature-specific — creature presets are sugar over the general call.

## Scope
- The three-operation API surface below and its request/response types.
- The shared async job lifecycle: submit, poll, success/error/timeout — never a silent stall.
- Result caching keyed per operation; generation is a one-time bake, not per view/frame/battle.
- GPU-gating and per-operation fallback.

Out of scope: the still-image recipe (`17`), the animation recipe (`64`), braille conversion/compositing (`13-rendering`), and how asset handles are stored/synced (`12-data-model-sync`).

## The API
Three operations, all returning a job handle that resolves to a cached-asset handle:

```
generate_image(image_request)                          -> ImageAsset
generate_animation(image, animation_request)           -> ClipAsset
generate_image_with_animations(image_request, [animation_request, ...])
                                                       -> ImageAsset + [ClipAsset]
```

- **`generate_image`** — produce a background-removed still from a prompt (or an imported file on the no-GPU path). Dispatches to `17`.
- **`generate_animation`** — animate an existing still (a handle from `generate_image`, or an imported still) per an action request. Dispatches to `64`.
- **`generate_image_with_animations`** — the general form: generate the still, then produce each requested animation for it, sharing the still's identity and cache. This is `generate_image` composed with a list of `generate_animation` calls; it is the primary entry point most callers use.

A `generate_creature(...)` convenience may wrap `generate_image_with_animations` with this game's default action set (idle / attack / hatch) and creature-style prompt conventions. It is sugar over the general call, not part of the core surface.

## Request / response shapes
- **ImageRequest** — prompt, fidelity/size, seed, background key color, and an optional import path (the import path is the no-GPU route and skips generation).
- **AnimationRequest** — a beat-by-beat action prompt (per `64`'s prompt convention) and any per-clip params.
- **ImageAsset / ClipAsset** — handles to the cached, background-removed still and the cached PNG frame-sequence clip respectively, in the shapes `13-rendering`'s player and the sprite path already consume. Handle storage/sync is `12-data-model-sync`'s concern.

## Uniform behavior
Every operation behaves the same way, which is the whole point of one API:
- **Async job lifecycle** — submit, then poll on a fixed interval, resolving to success, a real error, or an explicit timeout. A caller is never left with no signal. `generate_image_with_animations` reports per-sub-job progress so a partially-complete result is observable.
- **Cache by key** — images keyed by their request, clips keyed by `(image, action)`. A repeat request returns the cached asset rather than regenerating. One-time bake at definition time.
- **GPU-gating + fallback** — image generation requires a local GPU; its fallback is import (`17`). Animation requires a local GPU; its fallback is defined by the calling feature (there is no no-GPU animation path). The API reports capability so a caller can choose the fallback before submitting.

## Placement
`crates/game/`. This API, its lifecycle, and its cache are specific to Agent Battleground, not engine-level. It drives the native `sd-cli` subprocess (the same binary `17` and `64` use) via the sibling-binary pattern the game already uses for the inspector.

## Backends
- `generate_image` → `17-creature-art-asset-pipeline` (Z-Image via `sd-cli`, import, two fidelities, background removal).
- `generate_animation` → `64-creature-animation-pipeline` (MiniMax H3 via `sd-cli`, the reference config and prompt convention there).

Both recipes drive the same native `sd-cli` subprocess; this API owns the shared orchestration so the recipes only supply model choice and prompt construction.

## Consumers
`65-hatchery` (egg still + hatch/attack animation), the battle viewer (`05`, attack/movement animations), roster and detail views (stills), the debug inspector (`19`), and onboarding (`01`, first-run generation). All call this API rather than invoking generation directly.

## Open Questions / TBDs
- Concurrency: whether more than one generation job may run at once, given a single GPU and the ~14.6GB VRAM peak per job. Default assumption is a serial queue.
- How far `generate_image_with_animations` should parallelize vs. serialize its sub-jobs on one GPU.

## Dependencies
- Backed by `17-creature-art-asset-pipeline` (image recipe) and `64-creature-animation-pipeline` (animation recipe).
- Feeds `13-rendering` — produces the assets its player and sprite path consume.
- `12-data-model-sync` — how the returned asset handles are stored and referenced.
- Called by `65-hatchery`, the battle viewer, roster/detail, the inspector, and onboarding.
