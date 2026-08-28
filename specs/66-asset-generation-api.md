# Asset Generation API

> **Status: draft; orchestration core built, generation backends pending.** The single in-game API and pipeline for producing creature art assets: still images, animations of a still, or a still plus a set of animations. One API any scene calls; it really generates (Z-Image for stills, MiniMax H3 for animation, both via the native `sd-cli` subprocess), removes backgrounds, and caches results. Game-specific (`crates/game/`). Supersedes the former split across `17-creature-art-asset-pipeline` and `64-creature-animation-pipeline`, whose content is folded in here.

## Build state (do not rebuild)
The orchestration core is already implemented and committed under `crates/game/src/asset_gen/` and MUST NOT be rebuilt or re-decomposed:
- the request/response types, the `RecipeBackend` trait, the async job lifecycle + `SdCliRunner` (spawns the sibling `sd-cli`), the keyed cache, and the capability query.

Remaining work this spec drives:
- the two `RecipeBackend` implementations — the Z-Image image backend and the MiniMax H3 animation backend;
- background removal (method under research — see below);
- the three operations wired onto the existing lifecycle/cache/capability, plus the `generate_creature` preset.

## Purpose
Give every feature one uniform way to request generated art, so no scene re-wires generation for itself. The API owns the job lifecycle, caching, GPU-gating, and the real generation (which model, which prompt, background removal). Deliberately general (image + animations), not creature-specific — creature presets are sugar over the general call.

## Scope
- The three-operation API surface and its request/response types.
- The real generation backends: Z-Image (image) and MiniMax H3 (animation), via `sd-cli`.
- Background removal.
- The async job lifecycle: submit, poll, success/error/timeout — never a silent stall.
- Result caching keyed per operation; a one-time bake, not per view/frame/battle.
- GPU-gating and per-operation fallback.

Out of scope: braille conversion/compositing (`13-rendering`) and how asset handles are stored/synced (`12-data-model-sync`).

## The API
Three operations, all returning a job handle that resolves to a cached-asset handle:

```
generate_image(image_request)                          -> ImageAsset
generate_animation(image, animation_request)           -> ClipAsset
generate_image_with_animations(image_request, [animation_request, ...])
                                                       -> ImageAsset + [ClipAsset]
```

- **`generate_image`** — produce a background-removed still: run Z-Image from the prompt (or take an imported file on the no-GPU path), then remove the background.
- **`generate_animation`** — animate an existing still (a handle from `generate_image`, or an imported still) per an action request, by running MiniMax H3.
- **`generate_image_with_animations`** — the general form: generate the still, then produce each requested animation for it, sharing the still's identity and cache. This is `generate_image` composed with a list of `generate_animation` calls; it is the primary entry point most callers use.

A `generate_creature(...)` convenience may wrap `generate_image_with_animations` with this game's default action set (idle / attack / hatch) and creature-style prompt conventions. Sugar over the general call, not part of the core surface.

## Request / response shapes
- **ImageRequest** — prompt, size, seed, background key color, and an optional import path (the no-GPU route, skips generation). One image per creature; no separate high/low fidelities for now.
- **AnimationRequest** — a beat-by-beat action prompt and any per-clip params.
- **ImageAsset / ClipAsset** — handles to the cached, background-removed still and the cached PNG frame-sequence clip respectively, in the shapes `13-rendering`'s player and the sprite path already consume. Handle storage/sync is `12-data-model-sync`'s concern.

## Uniform behavior
Every operation behaves the same way, which is the whole point of one API:
- **Async job lifecycle** — submit, then poll on a fixed interval, resolving to success, a real error, or an explicit timeout. A caller is never left with no signal. `generate_image_with_animations` reports per-sub-job progress so a partially-complete result is observable.
- **Cache by key** — images keyed by their request, clips keyed by `(image, action)`. A repeat request returns the cached asset rather than regenerating. One-time bake at definition time.
- **GPU-gating + fallback** — image generation requires a local GPU; its fallback is import (the `ImageRequest`'s import path). Animation requires a local GPU; its fallback is defined by the calling feature (there is no no-GPU animation path). The API reports capability so a caller can choose the fallback before submitting.

## Generation backends
Both are `RecipeBackend` implementations, part of this feature — the API really produces images and clips, it does not defer generation elsewhere. Both drive the native `sd-cli` subprocess.

### Image (Z-Image)
- Z-Image Turbo via `sd-cli` (text-to-image), GPU-gated. Drag-and-drop import is the universal no-GPU route.
- **Key color**: generate the subject on a solid flat chroma-key screen so the background separates cleanly. Default green; use magenta when the creature's own dominant color is in the green family, or keying cannot tell subject from background.
- **Style guidance** (legibility, not just aesthetics — the output is braille dots): steer prompts toward a cartoony, mobile-game style and avoid heavy dark colors; dark, low-contrast regions read poorly as braille versus flat, saturated fields with clear silhouette edges. Applies to generated prompts; imported images are the player's own and unconstrained.

### Animation (MiniMax H3)
- MiniMax H3 image-to-video via `sd-cli`, using the verified config in `experiments/creature_lab/findings/12-attack-animation-pipeline.md`: Turbo LoRA at 1.0, 8 steps, `--cfg-scale 1.0`, `--flow-shift 12`, `--strength 1.0`, a 512-class canvas, 56 frames, and the CPU-offload flags (`--offload-to-cpu --clip-on-cpu --vae-tiling --temporal-tiling`). ~3 min/clip on a 16GB GPU; VRAM peak ~14.6GB; an out-of-VRAM condition is recoverable (retry with graph-cut streaming or a smaller canvas).
- **Prompt convention (load-bearing, not stylistic):** style-preservation language (art style, outline weight, color/proportion consistency, static camera, flat background color) plus the action described as concrete physical beats — windup, the motion itself, impact/extension, brief follow-through. A vague verb ("attacks fiercely") does not reliably produce the intended motion; a beat-by-beat physical description does. Motion magnitude comes from the beats, not from motion-adjective padding. The style being preserved is whatever the still already has.

### Background removal
A required step of the image path and of preparing a still for animation. Z-Image and H3 are RGB models that cannot emit alpha, so output sits on the key-color screen and the moving silhouette must be separated per frame, after generation. The method is unresolved — see `needs-research/creature-background-removal.md`. The pipeline reserves the step; the research picks between per-frame matting (rembg), chroma-key with despill, and alternatives, judged on the real braille output.

## Pipeline (still → animation)
1. Generate (or import) the still on the key-color screen.
2. Clean the still's background/shadow to a uniform flat color before animation (H3 does not reliably reproduce a shadow's shape through motion, leaving bleed otherwise).
3. Animate via H3 with the prompt convention above.
4. Remove the background per frame and extract a PNG frame-sequence asset.
5. Cache, keyed by `(subject, action)`.

## Behavior notes
- A held prop (e.g. a weapon) stays attached to the creature throughout a generated motion, including fast committed attack swings.
- A pose whose limbs are load-bearing (e.g. a handstand) keeps those limbs planted by physical plausibility; whole-body dynamism comes from beat-by-beat action language.

## Placement
`crates/game/`. This API, its lifecycle, its cache, and its generation are specific to Agent Battleground, not engine-level. It drives the native `sd-cli` subprocess via the sibling-binary pattern the game already uses for the inspector.

## Consumers
`65-hatchery` (egg still + hatch/attack animation), the battle viewer (`05`, attack/movement animations), roster and detail views (stills), the debug inspector (`19`), and onboarding (`01`, first-run generation). All call this API rather than invoking generation directly.

## Open Questions / TBDs
- Background-removal method — see `needs-research/creature-background-removal.md`.
- Concurrency: whether more than one generation job may run at once, given a single GPU and the ~14.6GB VRAM peak per job. Default assumption is a serial queue.
- How far `generate_image_with_animations` should parallelize vs. serialize its sub-jobs on one GPU.

## Dependencies
- Reference config / prompt conventions for the animation backend: `experiments/creature_lab/findings/12-attack-animation-pipeline.md`.
- Background-removal method: `needs-research/creature-background-removal.md`.
- Feeds `13-rendering` — produces the assets its player and sprite path consume.
- `12-data-model-sync` — how the returned asset handles are stored and referenced.
- Called by `65-hatchery`, the battle viewer, roster/detail, the inspector, and onboarding.
