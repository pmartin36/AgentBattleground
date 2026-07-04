# Move Scene-Rendering Compositing Into the Engine

> **Status: DRAFT — more investigation needed before this becomes a real spec.** Research is substantially done (see below) but no spec has been written or executed. Preserved across a reboot/session loss per the project owner's explicit request (2026-07-04).

## The original question
Project owner's framing: a scene shouldn't have to hand-orchestrate rasterize → place → composite → tint → draw for its own objects — it should be able to just "submit the things you want to render" and have the engine take care of the rest. Same underlying principle as the `CLAUDE.md` engine/game-boundary rule we already wrote: cross-cutting rendering mechanics belong in `crates/engine/`, automatic for any caller, not hand-rolled per scene.

## Research findings (a `fork` sub-agent investigated this, 2026-07-04 — summarized here)
Re-read all three scenes' `render()` bodies fresh before proposing anything. **The premise needed one correction**: `MainHub` and `RosterManager` are not actually good examples of duplicated compositing boilerplate.
- `MainHub`/`RosterManager` already just call `.render(buf)` on their `Button`/`FrameButton` widgets — compositing/tint/caching is already encapsulated there. `MainHub`'s logo/cursor-arrow draws are already minimal single-image `convert()` + `draw_grid()` calls with no compositing at all.
- `RosterManager`'s slide-transition trick (render to a scratch buffer, blit non-space cells shifted by a screen-space offset) is a genuinely bespoke effect that doesn't map onto "sprite compositing" at all. Forcing it through a generic submission API would be an awkward fit, not a real win — it was correctly identified as **out of scope** for this idea.

The one scene with real, reusable "N sprites, depth-sorted, tint-invariant compositing" logic is **`BattleViewer::render()`**. It currently hand-builds, every frame: per-piece raw/tinted `DotBuffer` pairs (`piece_shape_and_color`), places them via the shared camera (`place_piece`), depth-composites twice (`composite_dots` for both the untinted "shape" set and the tinted "color" set, per spec 29's tint-shape-invariance requirement), and calls `dots_to_grid_tinted`. That exact shape is close to the single most common rendering need any future scene — or future game built on this engine — will have.

## Proposed design (from the fork's research — not yet built, not yet reviewed in depth)
Add one new function to `engine-render`, built entirely on primitives that **already exist and need no changes**: `Camera`, the existing `AnimatedSprite`/`asset_cache` caches (spec 27/32), `composite_dots`, `dots_to_grid_tinted` (spec 29).

```rust
pub struct SpriteDraw<'a> {
    pub content: SpriteContent<'a>,   // Animated{sprite, elapsed, transform, base_dot_rows} | Static{bytes, dims}
    pub translate: WorldPos,
    pub tint: Option<Rgba>,
}

pub fn composite_scene<C: Camera>(w: usize, h: usize, camera: &C, draws: &[SpriteDraw]) -> Grid
```

`BattleViewer::render()` would collapse to: build a `Vec<SpriteDraw>` from `self.pieces`, call `composite_scene`, then `draw_grid`. The current manual `piece_shape_and_color`/`place_piece`/dual-`composite_dots` orchestration moves into this one engine-owned function, reusable by any future scene with the same need. `draw_board_lines` (procedural grid-line drawing, not a sprite at all) stays exactly as-is, as its own separate simple pass — it never belonged in a sprite-compositing API and shouldn't be forced into one. `MainHub`/`RosterManager` are **not** migrated onto this new API — they don't have the problem it solves, per the correction above.

## Real cost, flagged honestly (not glossed over)
This touches `BattleViewer` — the priority scene per `CLAUDE.md` ("Battle Viewer is the Priority... prioritize this scene"). It needs byte-identical-output verification against the existing test suite before it's trustworthy: `battle_viewer_scene_wiring_tests`, the glyph-mask-invariance regression tests (spec 29), and the team-tint-banding tests all need to keep passing unmodified. This is a real, `tdd-pipeline`-sized task — not a quick mechanical refactor — though it's a clean, well-scoped one since it slots in *above* spec 32's caching layer without needing to touch that layer at all.

## Next steps when resuming
1. Decide whether to proceed with this design as-is, or refine the `SpriteDraw`/`composite_scene` API shape first (e.g. does `SpriteContent` need more variants; does depth need to be explicit on `SpriteDraw` or derived from `camera.depth_key(translate)` as today).
2. If proceeding: write this up as a proper numbered spec (next available number) with the same rigor as specs 27–32 (root cause / scope / decisions / dependencies), specifically calling out the byte-identical-output verification requirement against `BattleViewer`'s existing test suite as a hard done-criterion, not optional polish.
3. Execute via the `tdd-pipeline` skill the same way as specs 27–32, watching closely given it touches the priority scene.
