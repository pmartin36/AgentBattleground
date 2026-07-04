# Scene Composite Primitive

> **Status: draft (not started).** Moves `BattleViewer`'s hand-rolled depth-sorted sprite compositing into `crates/engine/render` as a reusable primitive, so any future scene with the same need (N sprites, depth-sorted, tint-invariant) inherits it automatically instead of hand-rolling its own copy. Research substantially done — see `specs/needs-research/scene-rendering-engine-side.md` (superseded by this spec once accepted) for the investigation that produced this.

## Purpose
`BattleViewer::render()` currently hand-orchestrates rasterize → tint → place → depth-composite (twice, for spec 29's tint-shape-invariance) → grid-convert, inline in game code, every frame. That exact shape — "N depth-sorted sprites composited into one `Grid`" — is the single most common rendering need any future scene, or future game built on this engine, will have. Per the project's engine/game boundary rule, a cross-cutting rendering mechanism like this belongs in `crates/engine/render`, built so any caller inherits it by default, not hand-rolled per scene.

## Root Cause / Current State
`BattleViewer::render()` (`crates/game/src/scenes/battle_viewer.rs`) does, every frame:
1. For each alive piece, calls `piece_shape_and_color` — rasterizes the piece's current animation frame via `AnimatedSprite::rasterize_at`, then `tint()`s it with the piece's team color — producing an untinted `(shape)` and tinted `(color)` `DotBuffer` pair.
2. Calls `place_piece` on each pair's `.0` and `.1` separately (`place_piece` is a one-line wrapper over `engine_render::transform::place`), producing two parallel `Vec<DotPlacement>` — one "shape" set, one "color" set.
3. Calls `composite_dots` twice — once over the shape set, once over the color set — because spec 29's tint-shape-invariance requires the glyph mask to be decided from untinted data while color comes from a separately-tinted parallel composite.
4. Calls `dots_to_grid_tinted(&shape, &color)` once to produce the final `Grid`.

This is ~25 lines of orchestration logic that is entirely generic — it does not depend on `Piece`, `Team`, or anything else that is genuinely game-specific. It only depends on primitives that already exist unchanged in `engine-render`: `Camera`/`SideView` (spec 16), `AnimatedSprite` (spec 27, consolidated onto spec 32's shared asset cache), `composite_dots` (spec 13), `dots_to_grid_tinted` (spec 29).

Two other scenes with `render()` bodies (`MainHub`, `RosterManager`) were checked and are **not** additional instances of this problem:
- `MainHub` calls `.render(buf)` on `Button`/`FrameButton` widgets (which already internally encapsulate their own compositing/tint/caching) and does single-image `asset_cache::convert` + `draw_grid` calls for its logo/cursor-arrow — no multi-sprite depth compositing at all.
- `RosterManager::render_group` is a bespoke "render to a scratch buffer, then blit non-space cells shifted by a screen-space column offset" slide-transition effect. It doesn't do world-space placement, depth sorting, or tinting — forcing it through a sprite-compositing API would be a bad fit for what it actually does, not a real win.

## Scope
- Add one new function to `engine-render`: `composite_scene`, taking a camera and a slice of sprite-draw requests, returning a finished `Grid` — collapsing steps 1-4 above into a single engine-owned call.
- Migrate `BattleViewer::render()` onto this new function. `draw_board_lines` (procedural grid-line drawing, not a sprite) stays exactly as its own separate pass — it never belonged in a sprite-compositing API.
- **Hard done-criterion: byte-identical output.** `BattleViewer`'s existing render-output regression tests — `battle_viewer_scene_wiring_tests`, `render_glyph_mask_invariant_to_tint`, `render_reflects_mutated_stored_piece_color`, `render_reflects_mutated_stored_piece_transform_translate`, `render_excludes_dead_piece_keeps_alive_sibling`, `render_reincludes_piece_when_alive_flipped_back_true`, plus the 7 tests in `piece_render_tests` that do **not** call `piece_dots`/`piece_shape_and_color` (`team_colors_are_pale_gold_and_pale_mint`, `piece_transform_scale_x_mirrors_team_b_only`, `piece_new_seeds_transform_from_layout_math`, `piece_new_seeds_color_from_team_default`, `piece_new_defaults_alive_true`, `piece_elapsed_desyncs_frame_selection_across_indices`, `sprite_base_dot_rows_matches_ratio_constant`) — must pass **unmodified**, asserting on `render()`'s output buffer (or on `Piece`/`piece_elapsed`/`sprite_base_dot_rows`, none of which this spec touches) exactly as they do today.
- **Explicitly NOT "pass unmodified"**: the other 5 tests in `piece_render_tests` (`piece_dots_tints_each_team_distinctly_via_multiply_blend`, `piece_dots_reads_piece_color_field_not_team_default`, `piece_shape_and_color_untinted_carries_raw_source_rgb`, `piece_shape_and_color_topology_parity`, `piece_shape_and_color_tinted_matches_piece_dots`) unit-test `piece_dots`/`piece_shape_and_color` directly — functions this spec deletes (see Decisions below). These tests cannot survive unmodified; the refactor is not done until their *coverage* (per-team multiply-tint correctness against a synthetic uniform-gray opaque source; raw/tinted Lit/Transparent topology parity; a piece's own `color` field, not the team default, driving the tint) is reproduced as `composite_scene`-level tests in `engine-render` instead (see Verification Requirements). Deleting them without porting their coverage is not an acceptable resolution.

Out of scope:
- `MainHub` / `RosterManager` — confirmed above to not have the problem this spec solves. Not migrated onto `composite_scene`.
- Any new `SpriteContent` variant beyond what `BattleViewer` actually uses today (see "Deferred: non-animated content" below).
- Any change to `composite_dots`, `dots_to_grid_tinted`, `place`, `AnimatedSprite`, or the asset cache (specs 13/27/29/32) — this spec is pure orchestration consolidation, built entirely on existing, unchanged primitives.
- UI/HUD elements composited *among* pieces at the sub-cell level (e.g. a status icon depth-sorted against specific pieces). Nothing in this codebase or in `05-battle-viewer`'s planned scope (tutorial overlay, playback controls — both still un-designed/draft) needs this today; whole-board overlays (panels, buttons, modals) already work with zero new mechanism via plain sequential `draw_grid`/`Button::render` calls on top of the composited board, since ratatui `Buffer` cells are opaque and later writes simply overwrite earlier ones. See "Extension point" below for how this would be added *if* a concrete need appears later.

## Decisions (v1)
- **New types and function, `crates/engine/render/src/composite.rs`** (alongside the existing `DotPlacement`/`composite_dots`):

  ```rust
  pub enum SpriteContent<'a> {
      /// A piece of `AnimatedSprite` playback — mirrors `piece_shape_and_color`'s
      /// inputs exactly: sprite, elapsed time, transform, and the base dot-row
      /// count sizing the rasterization.
      Animated {
          sprite: &'a AnimatedSprite,
          elapsed: Duration,
          transform: &'a Transform,
          base_dot_rows: u32,
      },
  }

  pub struct SpriteDraw<'a> {
      pub content: SpriteContent<'a>,
      /// World position the sprite's rasterized center is placed at (fed to
      /// `camera.project`/`camera.depth_key`, same as `place_piece` today).
      pub translate: WorldPos,
      /// Team/instance tint. `None` composites only into the shape set (no
      /// separate color pass needed for that draw); `Some(c)` composites into
      /// both the shape and color sets per spec 29's tint-invariance rule.
      pub tint: Option<Rgba>,
  }

  /// Rasterizes, places, and depth-composites every draw in `draws` into one
  /// `dot_cols` x `dot_rows` Grid, tint-invariant per spec 29. Order in `draws`
  /// does not matter — depth is derived per-draw from `camera.depth_key`.
  pub fn composite_scene<C: Camera>(
      dot_cols: usize,
      dot_rows: usize,
      camera: &C,
      draws: &[SpriteDraw],
  ) -> Grid
  ```

  Internally: for each draw, rasterize per `content` (today, always the `Animated` arm — calls `sprite.rasterize_at(elapsed, transform, base_dot_rows)`), tint if `tint.is_some()` (else reuse the raw buffer for both shape and color positions, matching `piece_shape_and_color`'s existing behavior when no tint is needed), `place` at `translate` via `camera`, accumulate into parallel shape/color `DotPlacement` vecs, then run today's exact two-composite + `dots_to_grid_tinted` sequence once at the end.

- **`BattleViewer::render()` collapses to:** build a `Vec<SpriteDraw>` from `self.pieces` (one per alive piece, `content: Animated { sprite: &self.sprite, elapsed: piece_elapsed(elapsed, p.index), transform: &p.transform, base_dot_rows: sprite_base_dot_rows(&geom.camera) }`, `translate: p.transform.translate`, `tint: Some(p.color)`), call `composite_scene(w, h, &geom.camera, &draws)`, then `draw_grid`. `piece_shape_and_color`, `piece_dots` (a thin delegate over `piece_shape_and_color(...).1` with no production caller once `render()` no longer needs it), `place_piece`, and the manual double-`composite_dots` call are all deleted from `battle_viewer.rs` — their logic now lives only in `composite_scene`. This is a genuine deletion, not a deprecation: confirm via grep that `piece_dots`/`piece_shape_and_color`/`place_piece` have no remaining callers (production or test) before removing them, per the test-porting requirement above.
- **Depth still derives from `camera.depth_key(translate)`**, exactly as `place`/`place_piece` do today — not made an explicit field on `SpriteDraw`. No behavior change; `SideView::depth_key` is already a pure function of world position.
- **Extension point (documented, not built):** if a future concrete need requires compositing non-animated content into the same call — e.g. a static image, or a procedurally-generated `DotBuffer` for a HUD element depth-composited among pieces — it is added as one more `SpriteContent` variant (e.g. `Prerasterized(&'a DotBuffer)`). This is additive: existing `SpriteDraw`/`Animated` construction sites do not change when a new variant is added. Not built now because no current caller needs it and no confirmed design exists yet for what such a HUD element would look like (`05-battle-viewer`'s tutorial overlay / playback controls are still draft-not-started; its cutaway idea is explicitly flagged unconfirmed).

## Verification Requirements
- All `BattleViewer` render-output tests (and the 7 unaffected `piece_render_tests` tests) listed under Scope above pass **unmodified** — same assertions, same expected output, proving the refactor is behavior-preserving, not just "compiles."
- A new `composite_scene`-level unit test suite in `engine-render` that both (a) mirrors `composite_dots`'s existing depth-ordering/transparency/out-of-bounds tests at the `composite_scene` level, and (b) reproduces, as `composite_scene`/`SpriteContent::Animated` tests, the coverage of the 5 `piece_render_tests` tests being deleted per Scope above:
  - Multiple draws at distinct depths composite back-to-front correctly (mirrors `composite_dots`'s existing depth tests, at the `composite_scene` level).
  - A `tint: Some(c)` draw's glyph mask is invariant to `c` while color varies, using a synthetic uniform-gray opaque source and hand-derived per-tint multiply-blend values (ports `piece_dots_tints_each_team_distinctly_via_multiply_blend` / `piece_shape_and_color_untinted_carries_raw_source_rgb`'s expected-value math).
  - A `tint: Some(c)` draw's shape/color pair share identical Lit/Transparent topology at every dot (ports `piece_shape_and_color_topology_parity`).
  - Two draws with the same content but different `tint` values still produce the correct, independently-tinted output — i.e. tint is read from the per-draw `tint` field, not some shared/cached default (ports `piece_dots_reads_piece_color_field_not_team_default`'s intent, adapted to `composite_scene`'s API since there is no `Piece`/`piece.color` at this layer).
  - A `tint: None` draw composites the same buffer into both the shape and color positions (ports `piece_shape_and_color_tinted_matches_piece_dots`'s delegation-pin intent: no separate/divergent computation for the untinted case).
- Confirm via `cargo test --workspace` that specs 27/29/32's existing regression suites (rasterization caching, tint-shape-invariance, asset-rasterization caching) all pass unmodified — this spec touches none of their internals but composes them differently.
- Confirm via grep that `piece_dots`, `piece_shape_and_color`, and `place_piece` have zero remaining references anywhere in the workspace after deletion.

## Dependencies
- `13-rendering` ✅ — `composite_dots`, `dots_to_grid`, `Grid`, `Cell`.
- `16-world-space-and-camera` ✅ — `Camera`/`SideView`, `place`.
- `27-render-frame-caching` ✅ / `32-static-asset-rasterization-caching` ✅ — `AnimatedSprite::rasterize_at`, unchanged, called from inside `composite_scene` instead of from `battle_viewer.rs` directly.
- `29-tint-shape-invariance` ✅ — `dots_to_grid_tinted`'s shape/color contract, which `composite_scene` must preserve exactly.
- `18-battle-viewer-baseline` ✅ / `20-battle-viewer-event-playback` ✅ — `BattleViewer`'s existing render-output tests, reused unmodified as this spec's verification harness.
- `31-engine-game-crate-split` ✅ — `composite_scene` lives entirely in `engine-render`; contains no `Piece`/`Team`/game-domain knowledge.
