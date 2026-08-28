# Creature Art & Asset Pipeline

> **Status: draft (not started).** Where creature art comes from — the generation/import front-ends that produce the source images the braille renderer (`13-rendering`) turns into sprites. Split out of `13` so the renderer can be marked done while this stays pending.

## Purpose
Defines how a creature's source images are produced and prepared, *before* they reach the renderer. The renderer (`13`) consumes finished, background-removed images; this spec covers everything upstream of that — generation, import, simplification, and the two-fidelity model.

## Scope
- Two source images per creature (viewer + battlefield fidelity)
- In-app creature generation (local diffusion, GPU-gated)
- Drag-n-drop import (universal, no GPU)
- Background removal (transparent cutout)

Out of scope: braille conversion / compositing / animation (`13-rendering`); how asset references are stored/synced (`12-data-model-sync`).

## Two Fidelities Per Creature
Each creature has **two source images**, both rendered through the braille converter at different sizes:
- **Creature-viewer image** — high detail. Rendered as a large braille sprite for detail/roster views.
- **Battlefield image** — low detail, simplified. Rendered as a small braille sprite for the battlefield.

These are two distinct images, not one image at two zoom levels. A detailed image shrunk to battlefield size reads as mush; a purpose-simplified image reads as a clean small sprite. The battlefield image is **derived from the creature-viewer image** (image-to-image with low denoise + a simplification prompt) so the two share identity — same creature, two detail levels.

## Asset Generation Pipeline
Creature art enters through two front-ends that converge on the same converter:
- **In-app generation** (GPU-gated, optional) — local diffusion produces the high-detail image, then an img2img-simplify pass derives the battlefield image.
- **Drag-n-drop import** (universal) — the player supplies the high-detail image from any source; the simplify + convert steps run locally.

Both paths: source image → background removal (transparent cutout) → braille convert. Background removal is required for clean compositing (transparent pixels become empty cells).

Generation engine: **stable-diffusion.cpp** (pure C/C++, GGUF-quantized, ggml family — same ecosystem as the text model). Invoked via its CLI as a sandboxed subprocess, or via Rust bindings (`diffusion-rs`) in-process. Candidate models: Z-Image Turbo (fast, low VRAM) and FLUX.2 klein (higher quality). This keeps generation a local, optional, GPU-gated capability; the down-rezzer and import path require no GPU, so the minimum player spec stays "a terminal."

## Visual Style Guidance
Generation prompts (both fidelities) should steer toward a **cartoony, mobile-game style** rather than realistic/painterly rendering, and should **avoid overusing dark colors**. This is a legibility requirement, not a purely aesthetic one: the renderer's output is braille dots in a terminal, where dark, low-contrast, or heavily-shaded regions read poorly compared to flat, saturated color fields with clear silhouette edges. This guidance applies to in-app generation prompts; imported images are the player's own and aren't constrained by it.

## Reference Prototype
`experiments/creature_lab/` — CLI bake-off rig: generate (high-res) → img2img-simplify (battlefield) → rembg → braille preview, on stable-diffusion.cpp.

## Open Questions / TBDs
- Per-piece upgrade visuals — how do sprites evolve across the two fidelities?
- Which generation model ships as the default (quality vs. VRAM).
- Background-removal approach (rembg subprocess vs. in-process).

## Dependencies
- Backend of `66-asset-generation-api` — implements its `generate_image` operation (generation + import).
- Feeds `13-rendering` — produces the background-removed source images the renderer converts to braille.
- `12-data-model-sync` — how a creature's two images are stored/referenced.
- `01-onboarding-first-run` / `03-army-skill-editing` — where generation/import are triggered in the UX.
