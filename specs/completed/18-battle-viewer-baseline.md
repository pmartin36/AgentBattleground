> # ✅ DONE! — Completed 2026-07-01

# Battle Viewer — Baseline

> **Status: implemented.** Stage 1 of `05-battle-viewer`'s staged build approach: the static battlefield — an 8×8 board with a fixed 6v6 layout of idle-animating placeholder pieces, camera-framed, built and validated before any battle-simulation rules exist. Stages 2 (replay-driven playback) and 3 (live sim) remain pending in `05`.

## Purpose
The presentation-only foundation the rest of the Battle Viewer builds on: a board that actually renders, with pieces actually placed on it and actually animating, using the real render pipeline (world space, camera, dot compositor) rather than a placeholder fill.

## Scope
- Board geometry — an 8×8 grid computed once per frame from the scene's real render area (not a hardcoded terminal size), shared identically by board-line drawing and piece placement
- Board grid lines — bordered, empty-interior cells (no checkerboard fill)
- Fixed 6v6 static layout — two facing rows, centered columns
- Per-piece owned render state — each piece stores its own `Transform` (world position, rotation, mirror) and `color`, seeded at construction, not re-derived every frame
- Placeholder art reuse — one bundled sprite for all 12 pieces, differentiated by team tint + mirror
- Idle animation — phase-staggered GIF loop, no independent movement yet

Out of scope: turn structure, replay/live modes, playback controls, tutorial overlay — all still pending in `05-battle-viewer`.

## Decisions (v1)
- **World position is the cell CENTER**, not the corner: a piece resting in board cell `(col, row)` has world position `(col + 0.5, row + 0.5)`, per `16-world-space-and-camera`'s cell-center movement model.
- **One shared `BoardGeometry`** (per-cell terminal size, the board's centered `Rect` within the render area, and the `SideView` camera derived from it) is computed once per frame and consumed identically by the board-line renderer and the piece-placement code — there is no second, independent formula for "where is cell (col, row) on screen."
- **The board is not a depth-sorted sprite.** Board lines are drawn directly into the terminal buffer first (a background layer with no `Transform`/`depth_key` of its own); pieces composite on top afterward, and the dot-compositor's transparency rules let the board lines show through the gaps around each sprite. Painter's depth (`depth_key = world_y`) is still used when compositing pieces, but is not independently observable in this non-overlapping two-row layout.
- **Layout**: Team A occupies row `0`, Team B occupies row `BOARD_ROWS - 1`, both on columns `1..BOARD_COLS-1` (columns `0` and `BOARD_COLS-1` stay empty on both rows).
- **Placeholder art, not final art.** No unique per-piece art exists yet (`17-creature-art-asset-pipeline` not started). All 12 pieces reuse one bundled wizard sprite/animation, differentiated only by:
  - a per-piece owned `color`, seeded from a team default and applied via **multiply-blend** tint (`out = src × color ÷ 255`, per channel) — this preserves the sprite's own shading instead of flattening it to a silhouette. Team defaults are light pastels (pale gold `(255,232,176)` for Team A, pale mint `(176,255,224)` for Team B) — saturated colors darken too much under multiply blend to stay readable.
  - a per-piece owned `transform.scale.x` mirror (Team B negative, Team A positive).
  
  Both are placeholder differentiators, not gameplay signal.
- **Pieces own real state, not derived values.** `Piece` stores `transform: Transform` and `color: Rgba` as actual struct fields, seeded once at construction from `(col, row, team)`. `BattleViewer.pieces: Vec<Piece>` is built once and stored, not reconstructed every `render()` call. `render()` reads each piece's own fields directly — mutating a stored piece's `color`/`transform` changes what the very next frame draws. This matters because it's the data shape a future schema-driven field editor (`15-debug-inspector`) needs to exist against.
- **Idle animation is just the sprite's native frame loop**, phase-staggered per piece index so the 12 don't animate in lockstep. No independent per-piece transform animation (bob/pulse/movement) — pieces are stationary at their cell's center.
- **No interactivity.** No playback controls, no camera controls, no scene-specific input handling beyond the engine's existing global scene-switch keys.

## Dependencies
- `13-rendering` ✅ — the dot/braille pipeline this renders through.
- `16-world-space-and-camera` ✅ — `WorldPos`, `Camera`/`SideView`, `Transform`, and the cell-center convention this follows.
- `17-creature-art-asset-pipeline` — will eventually replace the placeholder wizard sprite + flat team tint with real per-piece art.
- Feeds `05-battle-viewer` — stages 2 and 3 build directly on this.
- Feeds `15-debug-inspector` — the owned per-piece `transform`/`color` fields are the data model that spec's schema/field-editor work targets first.
