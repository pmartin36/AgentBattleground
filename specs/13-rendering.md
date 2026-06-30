# Rendering

## Purpose
The shared visual layer that turns image/sprite assets into terminal output. Every scene that shows a piece, a portrait, or the battlefield draws through this system. It defines the game's visual identity. The Battle Viewer (spec 05) is its most demanding consumer.

## Scope
- Image/GIF → terminal conversion
- Colored sprite rendering with transparency
- Multi-sprite compositing for crowds and battlefields
- The renderer contract that scenes build against

## Decisions (v1)
- **No synthetic grain.** Sprites render as clean colored braille; conversion is deterministic. (Resolves the earlier grain/no-grain contradiction in favor of clean.)
- **Cell-level compositing, binary alpha.** Sprites are pre-rendered to braille grids and painted back-to-front; alpha is a per-dot cutout (lit or transparent), not translucency. Sub-cell RGBA blending between overlapping translucent sprites is a future refinement.
- **`convert` fits within a target area** (cols×rows), preserving source aspect, centered.
- **Truecolor only.** 24-bit RGB terminals are assumed; no 256-color fallback in v1.

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
- 24-bit RGB foreground per cell. **Truecolor terminals are assumed** (no 256-color fallback in v1).
- Native alpha transparency from the source asset (e.g. GIF alpha), applied as a **per-dot binary cutout** (a dot is lit or transparent, not translucent). Sprites composite over each other and over the background with transparent gaps preserved; translucent blending between overlapping sprites is deferred to a future sub-cell RGBA compositor.
- No background-color keying — transparency comes from the asset's alpha channel, not color matching.

### Aspect Correction
Braille dots are square; output is sized so the source aspect ratio is preserved (source width/height drives the dot grid dimensions, not the terminal cell ratio).

### Crowd / Battlefield Compositing
Many sprites render into one cell buffer — an application of the band + camera model in *Depth & Draw Order*, with crowd-specific treatments:
- **Parallax depth cue:** distance bands read as depth — farther bands smaller, dimmer, slower; nearer ones larger, brighter, faster. Size and screen placement follow from the camera projection; the brightness/speed gradient is crowd styling layered on top, not an engine rule.
- **Per-sprite animation phase** is staggered so shared animations do not lock into unnatural unison.
- **Pre-rendered frames** per animation frame at each band's scale, reused across instances (the cell-level caching from *Decisions (v1)*).

### Depth & Draw Order
Compositing is a back-to-front **painter's algorithm** over a depth-sorted sprite list — no z-buffer.

- Sprites carry a **logical position in a game-defined space** plus a **band**, not a stored `z`. The space is the game's choice — a discrete grid or continuous 2D — and the renderer assumes neither.
- The **camera** supplies two functions: a projection `position → screen_pos`, and a `depth_key(position) → scalar`. The sorter is generic, so the **camera angle (isometric / side / top-down) is configuration, not an engine assumption.** Supporting several angles means supplying several `(projection, depth_key)` pairs over the same sprite data.
- The global draw key is **hierarchical**: `(band, depth_key)`. Bands order coarse layers — background parallax (far) → battlefield → foreground / UI; `depth_key` orders within a band.
- `depth_key` by camera (battlefield, back-to-front):
  - **Isometric:** `row + col`
  - **Side with depth rows:** `row`
  - **3/4 "top-down" (upright billboards):** `row` (≈ screen-y)
  - **Pure orthographic top-down:** degenerate — sprites never overlap, ordering is moot
- **Tall / multi-cell sprites sort on their footprint (anchor) position, not per cell** — this bounds the sprite-overlap mis-sort case without splitting sprites.

This is the standard 2D/2.5D sorting model (cf. Unity sorting-layer + order-in-layer + transparency-sort-axis, Godot Y-sort, GameMaker `depth`) — the 2.5D analog of a 3D camera's view-projection producing depth, made explicit because there is no projection matrix.

### Animation
- Animated assets (GIFs) decode to per-frame braille grids.
- Frame timing honors the source asset's per-frame delays.
- Conversion is **deterministic**: the same source frame at the same size always yields the same braille grid — no synthetic grain or per-display-frame randomness. A grid changes only when the underlying animation frame or sprite position changes.

### Renderer Contract
Scenes do not implement braille conversion themselves. The renderer exposes:
- Convert an image/frame to a grid of colored braille cells, fitting within a target area (cols×rows) while preserving source aspect, centered (source aspect drives the dot-grid dimensions, not the terminal cell ratio).
- Composite positioned sprite grids into a screen buffer: a back-to-front painter's sort by `(band, camera.depth_key(position))`, honoring transparency. The camera supplies the projection and `depth_key` (see *Depth & Draw Order*).
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
`experiments/ascii_test/` contains working prototypes used to validate the style:
- `src/main.rs` — single-image fidelity (ASCII vs. half-block vs. braille, face crop)
- `src/anim.rs` — animated GIF playback in braille with transparency
- `src/flow.rs` — multi-sprite depth-layer crowd ("tidal wave") compositing
- `src/downrez.rs` — standalone CLI: image → colored braille (the down-rezzer both front-ends use)
- `experiments/creature_lab/` — CLI bake-off rig: generate (high-res) → img2img-simplify (battlefield) → rembg → braille preview, on stable-diffusion.cpp

## Current Implementation State
Only an M1 **placeholder** exists in the `render` crate: a solid-color braille `fill` plus a centered `label`, used to make scene switching visible (see `14-scene-architecture`). None of the conversion, compositing, depth-sort, or animation described above is built yet. The placeholder is marked as such in-crate and is not the rendering model to extend.

## Validation
The renderer is validated in three tiers of increasing difficulty. Each tier pairs an automated **golden test** (the gate) with an **example that renders through the real engine loop** (the eyeball check). The conversion algorithm is ported from the prototypes in `experiments/ascii_test/`; correctness is pinned by **hand-derived expectations** for known inputs (independent of any implementation), with the prototypes as the visual reference.

| Tier | Exercises | Golden test (gate) | Example |
|---|---|---|---|
| 1 · static sprite | `convert`: alpha cutout, luma threshold, color, fit-to-area aspect | known-input cells match hand-derived expectations (single-cell glyph+fg, alpha cutout, non-square aspect/centering); a committed snapshot guards regressions | `render_tier1` |
| 2 · animated sprite | + GIF decode, per-frame grids, frame timing | frame count + a chosen frame's cells match hand-derived expectations | `render_tier2` |
| 3 · overlapping animating sprites, alpha 0/1 | + `composite`, depth/occlusion, binary-cutout transparency | near sprite occludes far at overlap; draw order matches camera `depth_key` | `render_tier3` |

- **Golden tests** are headless `render`-crate tests (no terminal/display) — the CI gate. Expectations are hand-derived from the documented algorithm, so a faithful-port error is caught — they are not a snapshot of the implementation under test.
- **Examples** are `game`-crate examples (`cargo run -p game --example render_tierN`) that boot the real engine loop (`game::run`) with an inline validation scene — visualizing through the genuine render path **without** adding to the shipped scene catalog and **without** any new CLI flag or cargo feature. They are dev-only artifacts, not game content.
- Tier 3 uses **binary alpha (0/1)** only — translucent blending is out of v1 scope (see *Decisions (v1)*); it is the integration test for what v1 actually ships.
- Fixtures (a small PNG-with-alpha, a short GIF) are committed under the `render` crate's test assets so the golden tests are self-contained; examples embed them via `include_bytes!`.

## Open Questions / TBDs
- Per-piece upgrade visuals — how do sprites evolve across the two fidelities?
- Battlefield representation (grid vs. free-form) and how sprites map onto it — see spec 05.
- Camera angle (isometric / side / 3-4 top-down) is not chosen. It is decoupled from the depth sorter (the camera supplies projection + `depth_key`), so it does not block the renderer; the battlefield-representation decision above constrains the coordinate space the camera projects.
- Performance ceiling: max sprites on screen at target framerate.
- Color treatment for team/faction identification in a crowd.

## Dependencies
- Consumed by `05-battle-viewer` (primary), and any scene showing pieces (`02`, `03`, `06`, `07`).
- `12-data-model-sync` — sprite/asset references in the data model.
