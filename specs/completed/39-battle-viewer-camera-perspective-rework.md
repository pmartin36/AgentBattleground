> # ⚠️ MAJOR ISSUES — flagged 2026-07-08
> Implementation shipped and its own gate is green, but the project owner has identified major issues with the result, to be discussed and addressed in a follow-up session. Do not treat this spec as complete or as a safe foundation to build on until that discussion resolves it.

# Battle Viewer — Camera, Perspective & Piece Rendering Rework

## Purpose
`37-battle-viewer-dynamic-camera` shipped three camera modes, but the actual rendered result doesn't read as three distinct angles: Over-the-shoulder projects nearly identically to Top-Down (no vertical compression at all — its `project`/`depth_key` y-term is literally `pos.y * scale_dots`, the same formula `TopDownView` uses), and Sideline looks down the same axis Over-the-shoulder does (behind one team, toward the other) rather than broadside. Two more shipped-but-wrong symptoms turned out to share root causes with the angle problem, not to be independent bugs: the "invisible" dim grid lines (a color-contrast problem, not a positional one) and the "missing" bench piece (actually only broken under Over-the-shoulder, where its shear formula displaces the far team's entire row — bench included — roughly 2.5 cells sideways from its true column; Sideline and Top-Down already render the bench piece correctly).

This spec also folds in two follow-up asks the project owner scoped as ready regardless of the camera work: swapping the placeholder wizard sprite for real bundled creature art, and replacing per-team sprite tinting with a team-colored contact-shadow blob. Supersedes `specs/needs-research/battle-viewer-camera-and-piece-rendering.md` (retired once this spec lands).

**Perspective scope, resolved:** stays 2D world space — `16-world-space-and-camera`'s `WorldPos`/`Camera` model (a `project`/`depth_key` pair over continuous 2D positions, no 3D position, no camera orientation, no perspective divide) is unchanged. Full 3D was considered and explicitly rejected: it would mean rewriting `WorldPos`, the `Camera` trait signature, `Transform`, and the billboard/rasterize pipeline — a foundational change touching every spec built on `16` (18, 20, 26-33, 36, 37...), for a terminal renderer whose actual draw step is, and remains, painter's-algorithm compositing of pre-rendered 2D sprite billboards. That step doesn't change whether the projection feeding it is "real" 3D or a stylized 2D approximation. There's also no free-roaming camera here — exactly 3 fixed, designer-picked presets — which is precisely the case where 3D's main advantage (works correctly for *any* orientation automatically) is least valuable relative to its cost. The one thing true 3D would buy that this spec can't — a real height/altitude axis for flight, jump, or lobbed-projectile mechanics — isn't required by anything currently specced.

**What changes instead:** the three cameras stop being three independently hand-derived formulas (which is what produced `37`'s bug — each one had to be separately gotten right, and one wasn't). They become **one general, principled oblique-projection formula**, parameterized per preset by (elevation, which world axis is depth, anchor). Grid-line rendering and per-piece sprite scale both consume the *same* formula/parameters the piece-position projection uses, so a future ground-plane visual (an AOE ring, a decal, another shadow-like effect) gets correct camera-relative squash for free by calling the same function — not by inventing a new per-effect constant.

## Scope
- One shared `ObliqueCamera` projection (`crates/engine/render/src/camera.rs`) with three named preset constructors, used by all three `BattleCamera` variants — replacing `TopDownView` and `OverShoulderView` entirely. `SideView` (a separate, simpler camera type) is *not* replaced or touched — see Decision 1 for why.
- `draw_board_lines` (`crates/game/src/scenes/battle_viewer.rs:83-111`) projects grid-cell corners through the same `camera.project()` pieces use, instead of independent flat screen-space math — this is what keeps the grid and the pieces standing on it visually consistent under every camera, not just Top-Down.
- **Converging grid lines.** Grid spacing doesn't just compress with depth, it visually tapers toward a vanishing point (like a road narrowing into the distance), reusing the same elevation-gated math as everything else in Decision 1 — one more multiplier, not a new architecture.
- Per-piece sprite scale varies with camera-relative depth (nearer = larger, farther = smaller), reusing the same depth/anchor terms the position projection already computes.
- A new, live-editable **`BattleViewerTuning`** config (Decision 1) holding every visual-tuning constant this spec introduces (grid dim alpha, depth-scale falloff/floor, grid-taper falloff/floor, shadow fade duration) — exposed through the project's existing `Inspectable`/inspector mechanism (the same one already used for `Piece`/`Transform` fields in this file) so these can be tried and changed live, without a rebuild, instead of being buried as hardcoded constants.
- Alpha-aware dot compositing: `Rgba` already carries an `a` channel (`crates/engine/core/src/color.rs`) that the render pipeline currently ignores everywhere (`Dot::Lit` is always treated as fully opaque). `composite_dots` (`crates/engine/render/src/composite.rs`) and the dot-buffer-to-terminal blit (`draw_grid`, `crates/engine/render/src/grid.rs`) start honoring it.
- Dim grid lines (Sideline/Over-the-shoulder) become translucent instead of a separate dark, low-contrast color constant.
- A team-colored, soft-edged "contact shadow" blob under each piece, replacing today's multiply-blend sprite tint, with its squash ratio derived from the active camera's elevation (same reuse principle as the grid/sprite-scale changes).
- The 8 hardcoded demo pieces render as 8 distinct bundled creatures (`crates/game/src/creatures.rs`) instead of one shared wizard placeholder.
- An explicit, enforced billboarding invariant: nothing this spec adds may apply camera-derived rotation/shear to a sprite's own rasterized pixels — only to its screen *position* and *uniform* size.

Out of scope:
- Full 3D / true perspective-divide projection, and a world-space height/altitude axis (resolved above).
- Wiring the battle viewer to the player's actual roster/squad selection — the demo layout stays hand-authored/hardcoded (`pieces()`/`demo_events()`); creature art is reassigned within that existing hardcoded layout only.
- Camera persistence across sessions, and mouse/UI camera controls — both still out of scope per `37`.
- A lateral "shoulder offset" for Over-the-shoulder (a fixed sideways camera offset independent of depth). A cheap future addition (another parameter into the same formula); dropped here to keep the fix scoped to what was actually broken.
- Making the core camera geometry parameters (`elevation_deg`, `camera_depth`, which axis is depth) live-tunable — only the visual-polish constants (Decision 1's `BattleViewerTuning`) are exposed that way. The three camera presets themselves are structural, not polish.

## Decisions (v1)

### 1. One unified oblique-projection camera
**Engine crate** (`crates/engine/render/src/camera.rs`) — this is a general projection primitive, not battle-viewer-specific, so it belongs under `crates/engine/` per this project's engine/game boundary rule (CLAUDE.md): any hypothetical future game built on this engine could reuse an oblique camera. New shared type implementing `Camera`, replacing `TopDownView`/`OverShoulderView`:

**`SideView` is kept, unmodified, exactly as it exists today.** It is *not* one of the three `ObliqueCamera` presets below — grepping the codebase confirms `SideView` is used directly (not through `BattleCamera`) by two demo/reference programs that exercise the engine's render pipeline in isolation, `crates/game/examples/render_transform.rs` and `render_movement.rs` (these are physically under `crates/game/`, since Cargo requires examples to live in the crate that owns the binary target, but they're standalone reference programs for `engine_render`'s API — not part of the battle-viewer scene or any other game content), and by camera-agnostic tests in `crates/engine/render/src/transform.rs` and `composite.rs` that just need *some* simple camera fixture, unrelated to `SideView`'s specific formula. `TopDownView` and `OverShoulderView` have no such outside users (confirmed by the same search — every reference is either their own definition/tests in `camera.rs`, or `crates/game/src/scenes/battle_viewer.rs`), so those two are safe to delete outright.

```rust
/// Which world axis is "depth" (compressed by elevation, into the screen);
/// the other axis becomes screen-x unchanged.
pub enum DepthAxis { Row, Col }

pub struct ObliqueCamera {
    pub scale_dots: f32,
    pub depth_axis: DepthAxis,
    pub elevation_deg: f32,       // 0 = level with the ground, 90 = straight down
    pub camera_depth: f32,        // world-space depth coordinate the camera anchors on (its own "near" position)
    pub taper_per_world_unit: f32, // copied in from BattleViewerTuning when the camera is rebuilt each frame
    pub taper_min: f32,            // same
}
```
The last two fields carry the live-tunable taper constants (Decision 1 below) *into* the camera itself, rather than changing the `Camera` trait's `project(&self, pos)` signature (per `16-world-space-and-camera`, unmodified by this spec) to take a tuning parameter. They're populated from the scene's `BattleViewerTuning` every frame, the same way `scale_dots` is already rebuilt from `BoardGeometry` every frame today (`BattleCamera::with_scale_dots`) — not stored once at construction and left stale.

**Why an axis enum, not a continuous rotation angle:** only two axis assignments are ever actually needed (Top-Down/Over-the-shoulder look along the row axis; Sideline looks along the column axis) — there is no free-roaming camera here to justify a general rotation formula, and an earlier draft of this spec tried exactly that (a `sin`/`cos`-based `azimuth` formula) and got the sign wrong for one of the two cases, which would have rendered the whole board off-screen. `DepthAxis` is deliberately not "more general" than what's used and verified below, in both directions.

`project`/`depth_key` are one formula, not three, and both keep the exact `Camera` trait signature from `16-world-space-and-camera` (`project(&self, pos: WorldPos)`, no extra parameters) since the taper constants now live on `self`. `project` also applies a **convergence taper**: the "spread" axis (screen-x) shrinks toward the camera-depth anchor's own position as a point gets farther away, which is what makes grid lines and piece positions actually taper toward a vanishing point instead of just staying parallel with compressed spacing:
```rust
fn axis_values(depth_axis: DepthAxis, pos: WorldPos) -> (f32 /* depth */, f32 /* spread */) {
    match depth_axis {
        DepthAxis::Row => (pos.y, pos.x), // depth = row (team-separation axis); spread = column
        DepthAxis::Col => (pos.x, pos.y), // depth = column (within-row axis); spread = row
    }
}

fn taper_factor(&self, pos: WorldPos) -> f32 {
    let (depth, _) = axis_values(self.depth_axis, pos);
    let dist = (depth - self.camera_depth).abs();
    let k = self.elevation_deg.to_radians().sin();
    (1.0 - (1.0 - k) * dist * self.taper_per_world_unit).max(self.taper_min)
}

fn project(&self, pos: WorldPos) -> (i32, i32) {
    let (depth, spread) = axis_values(self.depth_axis, pos);
    let k = self.elevation_deg.to_radians().sin();
    let screen_x = spread * self.scale_dots * self.taper_factor(pos);
    let screen_y = (self.camera_depth * (1.0 - k) + depth * k) * self.scale_dots;
    (screen_x.round() as i32, screen_y.round() as i32)
}

fn depth_key(&self, pos: WorldPos) -> i32 {
    let (depth, _) = axis_values(self.depth_axis, pos);
    (-(depth - self.camera_depth).abs() * self.scale_dots).round() as i32
}
```
At `elevation = 90°` (`k = 1`), `taper_factor` is always exactly `1.0` (no convergence — verified: at `k=1`, `(1.0 - k) = 0`, so the whole taper term vanishes regardless of `dist`) and `screen_y` reduces to `depth * scale_dots` regardless of `camera_depth` — both anchor-dependent terms are exactly cancelled at Top-Down's elevation. Lowering elevation compresses every other depth value's spread toward the anchor's own screen position, and now also shrinks its screen-x spread toward the anchor's own screen-x. `camera_depth` represents "where the camera itself sits along the depth axis" — near-camera content barely moves or tapers as elevation changes; far content swings toward it and narrows. This is what "tilting the camera to a shallower angle, with converging perspective" means in this model. Verified numerically (not just by hand): under Over-the-shoulder, two same-row pieces 2 world-units apart in column spread 56 dots apart in screen-x when on the near row, but only 46 dots apart when on the far row — a real, working convergence effect, confirmed to be exactly `1.0`× (no effect at all) under Top-Down.

Three presets (`crates/game/src/scenes/battle_viewer.rs`, replacing today's `BattleCamera::with_scale_dots`/`handle_input` construction sites — the `BattleCamera` enum's three named variants are unchanged, each now wraps `ObliqueCamera`):

| Preset | `depth_axis` | `elevation_deg` | `camera_depth` | Effect |
|---|---|---|---|---|
| **Top-Down** (unchanged behavior) | `Row` | `90.0` | `1000.0` (sentinel — see below) | Exactly reproduces today's `TopDownView`: `screen_x = x·s`, `screen_y = y·s`, ascending depth-order by row. Verified: `axis_values(Row, pos) = (pos.y, pos.x)`, so `screen_x = pos.x·s`, and at `k=1`, `screen_y = pos.y·s`. |
| **Over-the-shoulder** | `Row` (same depth axis as Top-Down: world `y`, the team-separation axis) | `30.0` | `OVER_SHOULDER_ROW` (`6.5`, unchanged constant) | Rows compress toward the near (Team B) row; `screen_x = x·s` with **no shear term** — this removes the bug that displaced the far team ~2.5 cells sideways. |
| **Sideline (broadside)** | `Col` (depth axis is world `x`, the within-row column axis; `y`, the team-separation axis, becomes `screen_x`) | `10.0` | `BOARD_CENTER_COL` (`BOARD_COLS as f32 / 2.0 = 3.5`) | Teams separate left/right on screen (fixing "looks down the same axis Over-the-shoulder does"); each row's 3 pieces get a shallow depth stagger instead of a flat horizontal line — the intended read at a near-eye-level broadside angle. Verified: `axis_values(Col, pos) = (pos.x, pos.y)`, so `screen_x = pos.y·s` (team axis → screen-x) and `screen_y` compresses around column `3.5`. |

Top-Down's `camera_depth = 1000.0` is a sentinel, not a real board position: it's irrelevant to `project()` (cancelled by `k = 1`) but required for `depth_key()` to preserve today's "larger row = nearer = drawn on top" ordering (`depth_key` has no `(1 - k)` gate, so it needs an anchor placed beyond the far edge of the occupied depth range on the correct side — any large sentinel works; the exact value is not load-bearing).

**Grid lines.** `draw_board_lines` currently computes grid-cell-boundary dot positions with flat screen math, entirely independent of the active camera — this is *why* the grid stayed a perfect rectangle while corrected piece positions would otherwise drift away from it. It's rewritten to project each grid line's world-space points through `geom.camera.project()` and rasterize dot segments connecting them (a generic line rasterizer, not the current "light one fixed dot-column/row" loop).

With the taper term now included, a world-space grid line does not always project to a single straight screen segment: the taper's `(depth - camera_depth).abs()` term has a kink exactly at `camera_depth` (verified numerically — the outermost grid boundary line, which spans past `OVER_SHOULDER_ROW = 6.5` out to the board edge at world `7.0`, shows a very slight bend there; every other grid line in the actually-occupied board area sits entirely on one side of `camera_depth` and remains perfectly straight). Under Top-Down, taper is always `1.0`, so every line is straight and axis-aligned exactly as today. Given this, `draw_board_lines` should project **several sample points along each line** (not just its two endpoints) and connect them segment-by-segment, rather than assuming a single straight line always works — correct in general, and indistinguishable from a single straight segment everywhere except that one boundary-past-the-board-edge case.

**Live-tunable visual config.** Every visual-tuning constant this spec introduces — as opposed to the three cameras' *structural* parameters (`depth_axis`, `elevation_deg`, `camera_depth`, fixed per preset) — lives in one new **`BattleViewerTuning`** struct, deriving `Inspectable` exactly the way `Piece`/`Transform`/`BattleViewer` itself already do in this file, so every value below is editable live through the project's existing inspector tool while the game runs, per the project owner's explicit ask ("ideally it's a scene config we can tamper with") — not hardcoded `const`s requiring a rebuild to try a different number:
```rust
#[derive(Clone, Copy, PartialEq, Debug, Inspectable)]
pub struct BattleViewerTuning {
    pub grid_dim_alpha: u8,               // 0x60 default (~38%) — Decision 3
    pub depth_scale_per_world_unit: f32,  // 0.05 default — below
    pub depth_scale_min: f32,             // 0.6 default — below
    pub grid_taper_per_world_unit: f32,   // 0.06 default — above
    pub grid_taper_min: f32,              // 0.4 default — above
    pub shadow_fade_ms: u32,              // 150 default — Decision 4
}
```
Held as a non-hidden field on `BattleViewer` (e.g. `pub tuning: BattleViewerTuning`, no `#[inspect(hidden)]`, unlike `sprite`/`events`), defaulting to the values above in `Default for BattleViewer`, so it shows up and is editable in the inspector the same way `Piece` fields already do. `ObliqueCamera`'s own `taper_per_world_unit`/`taper_min` fields (above) are copied in from this config whenever the camera is rebuilt each frame — the same existing per-frame-rebuild path `scale_dots` already goes through.

**Per-piece sprite scale by depth.** Nearer pieces render larger, farther pieces smaller, reusing the same `depth`/`camera_depth`/`k` terms and the same `BattleViewerTuning` values:
```rust
fn depth_scale_factor(camera: &ObliqueCamera, tuning: &BattleViewerTuning, pos: WorldPos) -> f32 {
    let (depth, _) = axis_values(camera.depth_axis, pos);
    let dist = (depth - camera.camera_depth).abs();
    let k = camera.elevation_deg.to_radians().sin();
    (1.0 - (1.0 - k) * dist * tuning.depth_scale_per_world_unit).max(tuning.depth_scale_min)
}
```
At `k = 1` (Top-Down), the factor is always exactly `1.0` — no size change, matching "Top-Down confirmed fine, don't touch it." This multiplies into the piece's existing `Transform.scale` at render time (`scale.x = team.scale_x() * depth_scale_factor(...)`, `scale.y = 1.0 * depth_scale_factor(...)`) — composes automatically with the existing team-mirror sign and the Die-event's own shrink-to-zero tween, no changes needed to either. Because this must be recomputed every frame (it depends on the currently-active camera and the live-tunable `tuning`, either of which can change independent of a piece's own state), it's computed in `Scene::render()`'s per-piece draw loop, not stored on `Piece` itself; `SpriteDraw`'s `transform` may need to move from a borrowed `&Transform` to an owned `Transform` (or draw from per-frame scratch storage) to make a freshly-computed scaled copy constructible per piece per frame — exact mechanism is an implementation call, no visible behavior difference either way.

**Billboarding invariant (hard requirement, enforced by construction):** everything above only ever changes a piece's *position* (`project`) or *uniform* size (`depth_scale_factor`, applied equally to both scale axes' magnitude, mirror sign aside). Nothing here touches `Transform.rotation` or applies a *non-uniform* per-axis shear to a sprite's own pixels during `rasterize` (`crates/engine/render/src/transform.rs:70`). This must remain true for any future camera work too, including a future taper term: camera angle/depth is expressed purely through `project`/`depth_key`/uniform scale, never through a piece's own rasterized shape.

### 2. Alpha-aware compositing
**Engine crate.** General compositor capability, not battle-viewer-specific — belongs under `crates/engine/`. `Rgba` (`crates/engine/core/src/color.rs:5-9`) already has an `a: u8` field; `Rgba::rgb()` sets it to `0xFF` and every render path today (`composite_dots`, `dots_to_grid`/`dots_to_grid_tinted`, `draw_grid`) ignores it, treating any `Dot::Lit` as fully, unconditionally opaque. No change to the `Dot`/`DotBuffer` shape is needed — each dot already carries its own independent `Rgba`.

- `composite_dots` (`crates/engine/render/src/composite.rs:32-55`): when placing a `Lit(Rgba)` dot whose `a < 0xFF`, blend it against whatever `Dot` already occupies that destination cell (`out = lerp(dest, src, src.a / 255.0)` per channel) instead of overwriting. `a == 0xFF` (the default, and every existing call site) must produce byte-identical output to today's hard overwrite — this is strictly additive.
- The dot-buffer→terminal blit (`draw_grid`, `crates/engine/render/src/grid.rs`): same blend, but against the ratatui `Buffer` cell's current color at that position (read-modify-write the cell's `fg`), since board chrome is drawn directly into the frame buffer rather than through `composite_dots`. Same `a == 0xFF` backward-compatibility requirement.

### 3. Dim grid lines use alpha, not a separate dark color
**Game crate.** Battle-viewer-specific board chrome — all in `crates/game/src/scenes/battle_viewer.rs`; only *consumes* Decision 2's engine-level alpha support. Replaces `GRID_LINE_COLOR_DIM` (`crates/game/src/scenes/battle_viewer.rs:377`, `Rgba::rgb(0x2a,0x2a,0x2a)` — confirmed too close to the background to read as visible, not a positional bug) with the same base color rendered translucent, per the project owner's direct suggestion once shown the actual screen. `BattleCamera::grid_line_color()` (`battle_viewer.rs:158-164`) keeps its exact signature/dispatch shape, only the returned `Rgba` values change:
- Top-Down: unchanged, `GRID_LINE_COLOR` (`0x55,0x55,0x55`, `a = 0xFF`) — this one was never reported broken.
- Sideline / Over-the-shoulder: white at partial alpha, `Rgba::new(0xFF, 0xFF, 0xFF, tuning.grid_dim_alpha)` (`tuning.grid_dim_alpha` defaults to `0x60`, ~38% — confirmed by the project owner as the right starting strength, live-tunable per Decision 1's `BattleViewerTuning`) — blends against whatever background is actually behind the board at render time (via Decision 2), so it stays legible regardless of the exact background color rather than depending on a hand-guessed dark RGB triple.

### 4. Contact shadow replaces sprite tint
**Split across both crates — the generic shape primitive is engine-level, everything about how/when/what-color the battle viewer uses it is game-level:**
- **Engine** (`crates/engine/render/`, new function, e.g. alongside `dots.rs`/`composite.rs`): a shape rasterizer, structured as an extension point rather than a single hardcoded shadow-shape function — the same "one real variant today, room for more later without touching call sites" convention this codebase already uses for `SpriteContent` (`Animated` today, `Prerasterized` documented but not built) and `AnimationKind` (`Idle` today, `Attack`/`Hurt`/`Death` documented but not built):
  ```rust
  /// The kind of soft-edged shape to rasterize. Today only `Ellipse` is
  /// implemented (a circle when width == height dots) — extension point for
  /// future shapes (e.g. a ring/annulus for an AOE indicator, a rectangle for
  /// a decal) without changing existing call sites.
  pub enum ShapeKind {
      Ellipse,
  }

  pub fn rasterize_shape(kind: ShapeKind, width_dots: usize, height_dots: usize, color: Rgba) -> DotBuffer {
      match kind {
          ShapeKind::Ellipse => { /* radial alpha falloff from center to edge */ }
      }
  }
  ```
  Not speculative machinery — the function body only implements what Decision 4's shadow actually needs (one shape, one falloff curve), and `ShapeKind` has exactly one variant, matching what's true today. What makes it an "extension point" is purely the naming/shape of the API (a `match` with room for more arms, a function named for the general operation rather than `rasterize_shadow`), not unused generality. It belongs under `crates/engine/` for the same reason as Decision 2: the function itself contains zero pieces/teams/board knowledge.
- **Game** (`crates/game/src/scenes/battle_viewer.rs`): everything about *this* shadow is battle-viewer content, not engine capability — that it's sized to a board cell, colored by `Team::tint_color()`, squashed by the active camera's `k`, and gated on/off by move-tween state, all belong in `battle_viewer.rs`, calling the engine primitive above.

`SpriteDraw.tint` (`crates/engine/render/src/composite.rs`) stops being used for team color on the piece's own sprite — once pieces render as real creature art (Decision 5), multiply-tinting it pale-gold/mint would muddy the art. `Piece.color` / `Team::tint_color()` (`battle_viewer.rs:365-386`) are kept, but now feed a separate shadow draw instead:

- The engine-level blob primitive is invoked sized to roughly the piece's own board cell (`geom.cell_width_cols * 2` dots wide), alpha highest at center and fading to `0` at the shape's edge, colored with the piece's `Team::tint_color()`. **Vertical squash reuses the same `k = sin(elevation)` term from Decision 1** rather than an independently-tuned ratio — a round-ish shadow under Top-Down (`k = 1`), flattening into a thin ellipse under Sideline (`k ≈ 0.17`), consistent with however squashed the ground plane already reads under the active camera. This is the concrete payoff of the unified formula for "future ground-plane effects": any later shadow-like or decal-like visual reuses the same `k` rather than inventing its own per-shape flatten constant.
- Composited at the same `translate` (world position) as the piece's own sprite draw, inserted **immediately before** the piece's own `SpriteDraw` in the per-frame `draws` vec built by `Scene::render()` (`battle_viewer.rs:625-651`). Since both entries share the same world position and therefore the same `depth_key`, `composite_dots`' stable ascending sort preserves input order for ties — the shadow paints first, the piece's own (now-untinted) sprite paints over it. No new depth-key math needed.
- Visibility rule, fully decided (not tied to move speed at all — confirmed with the project owner): the instant a `Move` event's window opens for a piece, its shadow starts fading out over a fixed `tuning.shadow_fade_ms` (default `150`ms), regardless of how long the move itself takes. If the move is longer than the fade, the shadow simply stays at `0` alpha for the rest of the move. The instant the move settles (the event's window closes, landing at the destination cell), the shadow starts fading back in over that same fixed `tuning.shadow_fade_ms`, at the new position. Both transitions are computable directly from data `drive_events()` already has, no new bookkeeping required: for a piece with an active/recently-finished `Move` event, let `t_start = event.start_time`, `t_end = event.start_time + event.duration`, `fade = tuning.shadow_fade_ms / 1000.0`; shadow alpha is `lerp(full, 0, (elapsed - t_start) / fade)` while `elapsed < t_start + fade`, `0` while `t_start + fade <= elapsed < t_end`, `lerp(0, full, (elapsed - t_end) / fade)` while `t_end <= elapsed < t_end + fade`, and `full` otherwise (including "no relevant event at all," i.e. a piece that's simply standing still). A `Die`-ing piece fades out the same way on the event's window opening, but never needs a fade-in — once `alive` becomes `false` the piece (and therefore its shadow) stops rendering entirely.

### 5. Real creature art replaces the wizard placeholder
**Game crate**, entirely — bundled creature art and the demo battle layout are Agent-Battleground-specific content, never engine-generic (per CLAUDE.md's hard invariant: no `crates/engine/` crate may bundle game art). All in `crates/game/src/scenes/battle_viewer.rs` and `crates/game/src/creatures.rs`. `BattleViewer` (`battle_viewer.rs:456-515`) drops its single shared `sprite: AnimatedSprite` (`include_bytes!("assets/wizard.gif")`) in favor of holding all 8 bundled creatures' idle animations (`crate::creatures::all()` — 6 existing plus 2 new ones added via `experiments/creature_lab`, for exactly 8, one per demo piece). Each `Piece`'s index (`0..8`, stable per `pieces()` — `battle_viewer.rs:303-318`) maps 1:1 to a creature; `Scene::render()`'s per-piece `SpriteContent::Animated` draw (`battle_viewer.rs:635-641`) sources its `sprite` from `creature.animation(AnimationKind::Idle)` instead of `&self.sprite`, per creature. Team-based horizontal mirroring (`Team::scale_x()`) and idle-frame phase-stagger (`piece_elapsed`) continue to apply per-piece exactly as today, now on top of each piece's own distinct creature animation instead of a shared one.

Does not reflect the player's actual squad — still the existing hardcoded 8-piece demo layout, just with 8 distinct creatures assigned to its 8 fixed slots instead of one repeated wizard.

## Open Questions / TBDs
All substantive open items from earlier drafts of this spec have been resolved directly with the project owner (grid dim alpha, depth-scale/taper falloff exposed as live-tunable config rather than fixed values, shadow fade behavior, and `SideView`/`TopDownView`/`OverShoulderView`'s disposition, confirmed by grep). What remains is purely mechanical, with no visible-behavior consequence either way:
- Whether `SpriteDraw.transform` becomes an owned `Transform` or draws from per-frame scratch storage to carry the depth-scaled copy (Decision 1) — a Rust ownership/lifetime detail, not a design decision.

## Where the code lives
Per this project's engine/game boundary rule (CLAUDE.md): `crates/engine/` is reusable by any hypothetical future game built on this engine; `crates/game/` is Agent-Battleground-specific content. This spec touches both — summary, decision-by-decision (see each Decision above for the full reasoning):

| Decision | Crate | Files |
|---|---|---|
| 1. `ObliqueCamera`/`DepthAxis` (generic projection primitive) | **Engine** | `crates/engine/render/src/camera.rs` |
| 1. `BattleCamera` presets, `BattleViewerTuning`, grid-line projection, per-piece depth scale | **Game** | `crates/game/src/scenes/battle_viewer.rs` |
| 2. Alpha-aware `composite_dots`/`draw_grid` | **Engine** | `crates/engine/render/src/composite.rs`, `crates/engine/render/src/grid.rs` |
| 3. Dim grid lines via alpha | **Game** | `crates/game/src/scenes/battle_viewer.rs` |
| 4. `ShapeKind`/`rasterize_shape` (shape rasterizer, `Ellipse`-only today) | **Engine** | `crates/engine/render/` (new function) |
| 4. Contact shadow's color/size/camera-squash/move-tween-gating (uses the primitive above) | **Game** | `crates/game/src/scenes/battle_viewer.rs` |
| 5. Creature art swap | **Game** | `crates/game/src/scenes/battle_viewer.rs`, `crates/game/src/creatures.rs` |

No change anywhere touches `crates/engine`-forbidden content (no `include_bytes!`-bundled game art, no closed enum of concrete scenes/creatures, no path dependency on `crates/game`, per CLAUDE.md's hard invariants) — the two Engine rows above are pure, content-free primitives (a projection formula; a compositor blend; a shape rasterizer), and every piece of actual battle-viewer content (creature assignment, team colors, board layout, tuning defaults) stays in `crates/game`.

## Dependencies
- `16-world-space-and-camera` ✅ — the 2D `WorldPos`/`Camera` model this spec's unified formula stays within; not revised.
- `37-battle-viewer-dynamic-camera` — the shipped-but-wrong feature this spec corrects; `BattleCamera` enum, key 1/2/3 dispatch, and grid-line-prominence mechanism are all reused as-is, only their inputs/formulas change.
- `36-battle-viewer-squad-layout` — the 7×7/3-active+1-bench-per-side geometry these cameras render; unchanged.
- `13-rendering` ✅ / `33-scene-composite-primitive` ✅ — own `composite_dots`/`draw_grid`, extended (not replaced) by Decision 2's alpha support.
- `29-tint-shape-invariance` ✅ — the existing multiply-blend tint mechanism Decision 4 stops using for per-piece team color (shape/tint separation itself is unaffected; `tint` becomes unused by `BattleViewer` specifically, not removed from the engine).
- `23-piece-identity-data-model` ✅ / `crates/game/src/creatures.rs` — the bundled creature art Decision 5 reuses; this is its first use outside the roster screen.
- `15-debug-inspector` ✅ — the `Inspectable`/inspector mechanism `BattleViewerTuning` (Decision 1) is exposed through; no change to the inspector itself, just a new inspectable struct on an existing scene.
- Retires `specs/needs-research/battle-viewer-camera-and-piece-rendering.md` — its open question (2.5D vs. full 3D) is resolved here (2D, unified oblique projection); its action items 1, 2, 3, 5, 6, 7 are this spec's Decisions 1-5; its action item 4 is Decision 5.
