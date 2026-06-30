# Rendering

## Purpose
The shared visual layer that turns image/sprite assets into terminal output. Every scene that shows a piece, a portrait, or the battlefield draws through this system. It defines the game's visual identity. The Battle Viewer (spec 05) is its most demanding consumer.

## Scope
- Image/GIF → terminal conversion
- Colored sprite rendering with transparency
- Multi-sprite compositing for crowds and battlefields
- The renderer contract that scenes build against

## Key Details

### Braille Cell Technique
Pixels render as Unicode braille characters (U+2800–U+28FF). Each cell is a 2×4 dot grid — 8 dots per character — giving 2× horizontal and 4× vertical resolution over one glyph per cell. This is the chosen art style: a dot-matrix look that reads as deliberate terminal art, works at any font size, and needs no terminal graphics protocol.

Per cell:
- Each of the 8 dots maps to a source pixel.
- A dot is **transparent** when its source alpha is below threshold; transparent dots stay unlit so the terminal background shows through.
- Lit dots are chosen by comparing each pixel's luma against the cell's average luma (adaptive threshold).
- The cell's foreground color is the average color of its visible pixels — one color per cell.

Flat, solid color regions saturate to fully-lit cells (solid fill); dot-matrix texture appears at edges and gradients.

### Color & Transparency
- 24-bit RGB foreground per cell.
- Native alpha transparency from the source asset (e.g. GIF alpha). Sprites composite over each other and over the background with transparent gaps preserved.
- No background-color keying — transparency comes from the asset's alpha channel, not color matching.

### Aspect Correction
Braille dots are square; output is sized so the source aspect ratio is preserved (source width/height drives the dot grid dimensions, not the terminal cell ratio).

### Crowd / Battlefield Compositing
Many sprites render into a single cell buffer:
- **Depth layers (parallax):** sprites belong to layers that differ in size, brightness, and movement speed. Distant layers are smaller, dimmer, slower, and higher on screen; near layers are larger, brighter, faster, and lower. This produces a sense of depth and flow in a moving mass.
- Sprites composite back-to-front so nearer sprites occlude farther ones.
- Per-sprite animation phase is staggered so shared animations do not lock into unnatural unison.
- Frames are pre-rendered per animation frame at each layer's scale and reused across instances.

### Animation
- Animated assets (GIFs) decode to per-frame braille grids.
- Frame timing honors the source asset's per-frame delays.
- Any per-cell visual variation (grain/noise) is baked into the per-frame render so it is stable across wall-clock frames and only updates when the underlying animation frame or sprite position changes — never re-rolled per display frame.

### Renderer Contract
Scenes do not implement braille conversion themselves. The renderer exposes:
- Convert an image/frame to a grid of colored braille cells at a target size.
- Composite multiple positioned sprite grids (with depth ordering and transparency) into a screen buffer.
- Emit the buffer as ratatui drawable lines/spans.

The current art-style decision excludes: posterization, sprite outlining, and interior noise/grain. Sprites render as clean colored braille.

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

## Reference Prototype
`ascii_test/` contains working prototypes used to validate the style:
- `src/main.rs` — single-image fidelity (ASCII vs. half-block vs. braille, face crop)
- `src/anim.rs` — animated GIF playback in braille with transparency
- `src/flow.rs` — multi-sprite depth-layer crowd ("tidal wave") compositing
- `src/downrez.rs` — standalone CLI: image → colored braille (the down-rezzer both front-ends use)
- `creature_lab/` — CLI bake-off rig: generate (high-res) → img2img-simplify (battlefield) → rembg → braille preview, on stable-diffusion.cpp

## Open Questions / TBDs
- Per-piece upgrade visuals — how do sprites evolve across the two fidelities?
- Battlefield representation (grid vs. free-form) and how sprites map onto it — see spec 05.
- Performance ceiling: max sprites on screen at target framerate.
- Color treatment for team/faction identification in a crowd.

## Dependencies
- Consumed by `05-battle-viewer` (primary), and any scene showing pieces (`02`, `03`, `06`, `07`).
- `12-data-model-sync` — sprite/asset references in the data model.
