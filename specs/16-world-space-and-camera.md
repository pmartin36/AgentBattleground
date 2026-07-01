> # ✅ DONE! — Completed 2026-07-01

# World Space & Camera

## Purpose
The general, board-agnostic spatial foundation beneath everything visual. Defines the continuous coordinate space every renderable object lives in, and the **camera** that maps a world position to a screen position and a draw-order depth. The renderer (`13-rendering`) consumes this; any gameplay layout (e.g. the battlefield grid, `05`) sits *on top* of it. Deliberately independent of any board/grid concept so the engine is never locked to square-tile games.

## Scope
- World space: the continuous coordinate system objects are positioned in
- The camera: world → screen projection and world → depth
- Dot-resolution screen mapping (sub-cell placement)
- Decoupling of world space from any board/grid
- Movement / interpolation of world positions over time (the render-side truth)
- The rendering-pipeline consequence: dot-granularity compositing

Out of scope: braille conversion + compositing internals (`13`), the battlefield's grid and gameplay rules (`05`), the battle sim that drives movement (`10`).

## Decisions (v1)
- **World space is continuous 2D, board-agnostic.** Every renderable carries a `world_pos: (f32, f32)`. Units are game-defined; a grid game may treat 1 cell = 1.0 world unit, but world space itself knows nothing about cells.
- **World position is the single spatial input to rendering.** The renderer reads `world_pos` only. It is NOT derived from a board position in the render path — any board→world mapping is a gameplay-layer concern.
- **The camera is two pure functions of world position:** `project(world_pos) → screen_dot` and `depth_key(world_pos) → depth`. The camera encodes the view angle; swapping angles = swapping these two functions over unchanged world positions.
- **Screen space is dot-resolution** — 2 dots wide × 4 tall per terminal cell. The camera projects to dots, giving sub-cell (dot-granularity) placement.
- **Depth is per-sprite (painter's), computed by the camera from world position.** No per-dot/per-cell z-buffer.
- **Compositing is dot-level** to honor dot-granularity (see *Rendering Consequence*).
- **Sprites are positioned via a `Transform`** — `translate` (world position) + `rotation` (in **degrees**; radians are an internal detail) + per-axis `scale` (negative mirrors, so "flip" is just `scale.x < 0`). Rotation/scale are applied by an affine rasterize at image resolution. T/R/S animate via small `lerp`/easing helpers.

## Key Details

### World Space
- A continuous 2D coordinate system. Every renderable object (sprite, effect, crowd member) carries a `world_pos`.
- **Board-agnostic and unit-agnostic:** the game defines what a unit means. A grid game maps cell `(col,row) → world (col·s, row·s)` for some scale `s` (commonly 1); a free-form game sets `world_pos` directly. World space does not know which — and must not.
- This is the **only** spatial fact the renderer needs. Render position is a function of world position, full stop.

### Decoupling from the Board
There is **no `board → world → render` pipeline.** Two separate representations, owned by different layers:

| | Owner | Form | Role |
|---|---|---|---|
| **World position** | presentation / render | continuous `(x,y)` | the render truth — always present |
| **Board cell** | gameplay (optional) | discrete `(col,row)` | gameplay truth — only if the game uses a grid |

- The dependency is one-way and lives entirely in the **gameplay** layer: when a grid game resolves a move, it may set a world-space target and animate `world_pos` toward it. The world-space / camera / render system never references the board.
- A game with **no** grid drives `world_pos` directly. The engine supports both without change — this is precisely what keeps it from being locked to square-tile games.

### The Camera
Two pure functions of world position — nothing else:
- **`project(world_pos) → screen_dot`** — where on screen (in dots), including any pan/zoom the camera applies.
- **`depth_key(world_pos) → depth`** — the back-to-front sort scalar (larger = nearer).

The camera **is** the view angle. Examples, over world position:
- side view → `depth_key = world_y`
- isometric → `depth_key = world_x + world_y`
- 3/4 top-down → `depth_key = world_y`

Supporting several angles = supplying several `(project, depth_key)` pairs over the same world positions. The renderer and compositor are angle-agnostic — they only ever see the resulting screen-dot position and depth scalar.

### Dot-Resolution Screen
- The terminal renders braille: 2×4 dots per cell. Screen space is therefore a **dot grid** — `2·cols` wide, `4·rows` tall.
- The camera projects world positions to **dot coordinates**, so a moving object advances in dot steps (½ cell horizontally, ¼ cell vertically) rather than snapping a whole cell. This is the finest smoothness the medium allows; true pixel-smoothness is not achievable in a terminal.

### Movement / Interpolation
- `world_pos` is continuous and **animated over time** (lerp, easing, arcs) by the presentation/gameplay layer — e.g. gliding a piece from one cell's world location to another over a turn's animation.
- The renderer reads the **current** `world_pos` each frame; interpolation is not the renderer's concern.
- The lerp is **cosmetic**: gameplay truth (the board cell, if any) commits instantly when a move resolves; the world position slides to catch up. Gameplay logic never reads the in-flight world position — couple them and you get bugs.

### Rendering Consequence: Dot-Level Compositing
Dot-granularity placement changes the compositing pipeline. A sprite pre-baked to per-cell braille glyphs can only sit at whole-cell offsets (a glyph's 2×4 dots are locked to one cell). To place at a **dot** offset, the compositor must work at the dot level:

1. Convert each source frame to a **dot buffer** (per dot: lit? + color), not directly to braille glyphs.
2. Composite dot buffers **at their dot offsets**, back-to-front by camera depth (painter's): an opaque dot overwrites, a transparent dot reveals.
3. Emit braille glyphs from the final dot buffer at the end (each cell's 8 dots → one glyph + one color).

This **generalizes** the cell-level compositor validated during the renderer tiers (whole-cell placement is the special case where the dot offset is a multiple of the cell). It remains **binary alpha** (per-dot cutout) — translucent RGBA blending stays a separate, deferred capability (`13` Decisions). Depth is still per-sprite painter's; the dot buffer holds no per-dot depth.

### Sprite Transform & Placement
A renderable is positioned with a **`Transform`**, not raw dot math:

```
Transform { translate: WorldPos, rotation: f32 /* degrees */, scale: Vec2 /* per-axis; negative mirrors */ }
```

- **Translate** → screen via `camera.project(translate)`; the sprite's **pivot** (default: center) anchors there.
- **Rotation** is in **degrees**, applied on the 2D screen plane about the pivot.
- **Scale** is per-axis; a negative component mirrors on that axis (flip is `scale.x < 0`, not a separate flag).

**Rasterize is an affine warp, not a plain resize.** To honor rotation, `rasterize(image, transform, base_size) → DotBuffer` inverse-maps each output dot through `(rotate ∘ scale)` to a source pixel and samples (alpha cutout + color) at image resolution — as clean as the dot grid allows. The output bbox grows with rotation. `scale = (1,1)`, `rotation = 0` reduces to the plain resize. Depth is unaffected (still per-sprite from world position). Arbitrary-angle rotation resamples (mild dot shimmer in motion — validated as acceptable in the braille aesthetic); 90°/180°/270° are lattice-exact.

**Placement** then anchors the rasterized buffer's pivot at `camera.project(translate)` and tags it with `camera.depth_key(translate)`, producing a `DotPlacement` — collapsing the per-sprite `rasterize → project → placement` boilerplate.

### Animating a Transform
Any T/R/S component animates over time with small helpers rather than hand-rolled math: `lerp(a, b, t)`, a few easing curves (linear, ease-in-out), and a `Tween { from, to, duration }` yielding the eased value at an elapsed time. A minimal utility — not a timeline/animation-graph system. Frame animation (`AnimatedSprite`) and transform animation are independent and compose.

## Relationship to Other Specs
- `13-rendering` — consumes the camera's output (positioned, depth-tagged sprites) and owns braille conversion + the dot-level compositor. The camera / world-space / depth-key model is **owned here**; `13`'s "Depth & Draw Order" references this spec.
- `05-battle-viewer` / battlefield — one **consumer**: it may impose a grid whose cells map to world positions, and lerp world positions on moves. It builds on this spec; this spec does not depend on it.
- `10-battle-simulation-engine` — emits discrete moves; the viewer animates world positions between them.

## Current Implementation State
Built: `WorldPos`, the `Camera` trait + `SideView`, the dot pipeline (`sprite_to_dots` / `dots_to_grid`) and the dot-level compositor (`composite_dots`), dogfooded by the wandering-wizards demo (`render_movement`). Not yet built: the **`Transform`** (translate + degrees-rotation + per-axis scale), the affine **`rasterize`** (rotation/scale), the pivot-aware **placement helper**, and the **lerp/easing/tween** utility — arbitrary rotation has been validated as acceptable in braille (static + spinning).

## Open Questions / TBDs
- The world unit for the battle game (ties to the grid-vs-free-form decision in `05`).
- A height / 3rd axis (jumping, flying) — a vertical visual offset distinct from depth; deferred.
- Camera angle for the battle game (side / iso / 3-4) — configuration, not yet chosen.
- Sub-cell **RGBA / translucency** blending remains separate and deferred (this spec's dot compositing is binary cutout).

## Dependencies
- Consumed by `13-rendering` (which owns the compositor implementing dot-level composite) and any scene that positions sprites.
- `05-battle-viewer` and the battlefield build on it.
