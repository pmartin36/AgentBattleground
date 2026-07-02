> # ✅ DONE! — Completed 2026-07-01

# Rendering

> **Status: implemented.** The braille renderer — image/GIF → colored braille, animation, dot-level depth compositing — is built and validated (see *Validation*). World space + camera + the sprite transform live in `16-world-space-and-camera`; creature-art generation/import lives in `17-creature-art-asset-pipeline`.

## Purpose
The shared visual layer that turns image/sprite assets into terminal output. Every scene that shows a piece, a portrait, or the battlefield draws through this system. It defines the game's visual identity. The Battle Viewer (spec 05) is its most demanding consumer.

## Scope
- Image/GIF → terminal conversion
- Colored sprite rendering with transparency
- Multi-sprite compositing for crowds and battlefields
- The renderer contract that scenes build against

## Decisions (v1)
- **Braille is universal except text.** Every non-text visual element — sprites, battlefield/board chrome (grid lines, borders, panels), effects — renders through this braille dot pipeline (image/procedural content → `DotBuffer` → composite → braille glyph), never drawn directly with other Unicode/ASCII characters. The sole exception is text (scene labels, menus, HUD copy): braille cannot render legible Latin glyphs at this resolution, so text stays plain ratatui characters/spans. No render pass may bypass the dot pipeline for non-text content — a "just draw some box-drawing characters for this UI element" shortcut is not permitted, no matter how minor the element.
- **No synthetic grain.** Sprites render as clean colored braille; conversion is deterministic. (Resolves the earlier grain/no-grain contradiction in favor of clean.)
- **Dot-level compositing, binary alpha.** Sprites composite at dot granularity (image → dot buffer → composite at dot offsets → braille), per `16-world-space-and-camera`; the cell-level compositor built during renderer validation is the whole-cell special case. Alpha is a per-dot cutout (lit or transparent), not translucency; sub-cell RGBA blending between overlapping translucent sprites is a future refinement.
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
Compositing is a back-to-front **painter's algorithm** over a depth-sorted sprite list — no z-buffer. The world-space + camera model that produces each sprite's screen position and depth scalar is specified in `16-world-space-and-camera`; this section covers only how the compositor consumes them.

- Each sprite arrives with a screen position and a **depth** scalar from the camera (`depth_key(world_pos)`, per `16`) plus a **band**. The compositor is angle-agnostic — it never interprets depth, only sorts by it.
- The global draw key is **hierarchical**: `(band, depth)`. Bands order coarse layers — background parallax (far) → battlefield → foreground / UI; `depth` orders within a band.
- **Tall / multi-cell sprites sort on their footprint (anchor) position, not per cell** — this bounds the sprite-overlap mis-sort case without splitting sprites.

This is the standard 2D/2.5D sorting model (cf. Unity sorting-layer + order-in-layer + transparency-sort-axis, Godot Y-sort, GameMaker `depth`).

### Animation
- An animated sprite is a **sequence of frames** advanced by **elapsed wall-clock time**, decoupled from the render framerate. Playback rate = a per-sprite **base frame duration** scaled by a runtime **speed multiplier** (the game-engine convention — Unity `Animator.speed`, Godot `speed_scale`, Unreal `PlayRate`): `1.0` natural, `2.0` twice as fast, `0` holds, negative plays in reverse. The renderer iterates a frame list — it is not a format-specific player.
- **Source formats are importers** that decompose into that frame list: GIF decode, a texture-atlas slice, or a PNG sequence all yield `frames: Vec<…>`. The renderer is source-agnostic; per-frame source delays (e.g. a GIF's variable timing) are **not** honored in v1 — frames advance at the sprite's uniform rate.
- Conversion is **deterministic**: the same source frame at the same size always yields the same braille grid — no synthetic grain or per-display-frame randomness. A grid changes only when the active frame or sprite position changes.

### Renderer Contract
Scenes do not implement braille conversion themselves. The renderer exposes:
- Convert an image/frame to a grid of colored braille cells, fitting within a target area (cols×rows) while preserving source aspect, centered (source aspect drives the dot-grid dimensions, not the terminal cell ratio).
- Composite positioned sprite grids into a screen buffer: a back-to-front painter's sort by `(band, camera.depth_key(position))`, honoring transparency. The camera supplies the projection and `depth_key` (see *Depth & Draw Order*).
- Emit the buffer as ratatui drawable lines/spans.

The current art-style decision excludes: posterization, sprite outlining, and interior noise/grain. Sprites render as clean colored braille.

## Reference Prototype
`experiments/ascii_test/` contains working prototypes used to validate the style:
- `src/main.rs` — single-image fidelity (ASCII vs. half-block vs. braille, face crop)
- `src/anim.rs` — animated GIF playback in braille with transparency
- `src/flow.rs` — multi-sprite depth-layer crowd ("tidal wave") compositing
- `src/downrez.rs` — standalone CLI: image → colored braille (the down-rezzer both front-ends use)
- `src/playframes.rs` — braille frame-sequence player: PNG-dir or GIF → animated braille (chroma-key + ping-pong loop)

`experiments/creature_lab/` is the generation rig (stable-diffusion.cpp CLI):
- `generate.sh` — high-detail creature + img2img-simplified battlefield form → braille preview of both
- `animate.sh` — Wan 2.2 I2V animates a low-detail sprite → braille loop (low detail downrezzes cleanly *and* the video model drifts less)

## Validation
The renderer is validated in three tiers of increasing difficulty. Each tier pairs an automated **golden test** (the gate) with an **example that renders through the real engine loop** (the eyeball check). The conversion algorithm is ported from the prototypes in `experiments/ascii_test/`; correctness is pinned by **hand-derived expectations** for known inputs (independent of any implementation), with the prototypes as the visual reference.

| Tier | Exercises | Golden test (gate) | Example |
|---|---|---|---|
| 1 · static sprite | `convert`: alpha cutout, luma threshold, color, fit-to-area aspect | known-input cells match hand-derived expectations (single-cell glyph+fg, alpha cutout, non-square aspect/centering); a committed snapshot guards regressions | `render_tier1` |
| 2 · animated sprite | + frame-sequence model, uniform-rate playback by elapsed time, GIF import | frame-index selection is correct (t=0→f0, wraps at N·dur); decoded frame count + a chosen frame's cells match hand-derived expectations | `render_tier2` |
| 3 · overlapping animating sprites, alpha 0/1 | + `composite`, depth/occlusion, binary-cutout transparency | near sprite occludes far at overlap; draw order matches camera `depth_key` | `render_tier3` |

- **Golden tests** are headless `render`-crate tests (no terminal/display) — the CI gate. Expectations are hand-derived from the documented algorithm, so a faithful-port error is caught — they are not a snapshot of the implementation under test.
- **Examples** are `game`-crate examples (`cargo run -p game --example render_tierN`) that boot the real engine loop (`game::run`) with an inline validation scene — visualizing through the genuine render path **without** adding to the shipped scene catalog and **without** any new CLI flag or cargo feature. They are dev-only artifacts, not game content.
- Tier 3 uses **binary alpha (0/1)** only — translucent blending is out of v1 scope (see *Decisions (v1)*); it is the integration test for what v1 actually ships.
- Fixtures (a small PNG-with-alpha, a short GIF) are committed under the `render` crate's test assets so the golden tests are self-contained; examples embed them via `include_bytes!`.

## Open Questions / TBDs (future refinements — renderer is shipped)
- Performance ceiling: max sprites on screen at target framerate.
- Color treatment for team/faction identification in a crowd.
- Sub-cell RGBA / translucency blending (deferred; current compositing is binary cutout).

(Moved out: creature fidelity + upgrade visuals → `17`; camera angle → `16`; battlefield representation → `05`.)

## Dependencies
- Consumed by `05-battle-viewer` (primary), and any scene showing pieces (`02`, `03`, `06`, `07`).
- `16-world-space-and-camera` — provides world position, the camera (projection + `depth_key`), and the sprite `Transform` the renderer places by.
- `17-creature-art-asset-pipeline` — produces the background-removed source images the renderer converts.
- `12-data-model-sync` — sprite/asset references in the data model.
