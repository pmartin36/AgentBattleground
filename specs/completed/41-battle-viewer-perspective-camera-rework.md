> # ✅ DONE! — Completed 2026-07-09
> Status: implemented via the `tdd-pipeline`, all tasks GREEN, full workspace gate clean. Manually verified beyond the automated gate: rendered every preset and caught two real bugs the pipeline missed (a vertical near/far inversion, and Over-the-shoulder facing the wrong team), both fixed and re-verified against live rendered output. Supersedes `39-battle-viewer-camera-perspective-rework` (moved to `completed/` alongside this spec, flagged as superseded rather than shipped-correct).

# Battle Viewer — Real Perspective Camera Rework

## Purpose
`39-battle-viewer-camera-perspective-rework` shipped (gate green) but the rendered result was badly broken: Sideline's grid lines converged sideways instead of receding, both oblique views used roughly a quarter of the available screen, creature sprites floated centered in their cells instead of standing in them, the "dim" grid lines were numerically indistinguishable from the opaque Top-Down ones, and the bench piece was still not visibly readable. Root cause, confirmed by inspection, not superficial: `39`'s `ObliqueCamera` used two different, mutually inconsistent projection models on its two axes — a plain linear (non-converging) map for depth/screen-y, with an ad hoc, independently-invented convergence term (`taper_factor`) bolted onto only the spread/screen-x axis. Real axonometric/oblique projections (the standard cheap-3D technique for tile games — no vanishing point, parallel lines stay parallel) and real perspective projections (one real camera, everything converges consistently to one vanishing point) are each internally coherent; `39`'s hybrid was neither, which is exactly why the convergence looked wrong on inspection. Sideline's `camera_depth` also sat at the *middle* of the occupied depth range rather than past its edge, i.e. modeled a camera floating at the board's center receding in both directions at once — not a real vantage point.

This spec replaces `ObliqueCamera`'s hand-rolled formulas with a real, minimal pinhole-camera projection (position + pitch + field of view, still projecting a 2D `WorldPos` ground-plane point — `16-world-space-and-camera`'s 2D world model is *not* revised) for Over-the-shoulder and Sideline, fixes the viewport-sizing and sprite-anchoring bugs that were independent of the projection model, and puts a render-and-inspect verification step in front of "done" for every camera preset — the process gap that let `39` ship broken with a green gate. Supersedes `specs/39-battle-viewer-camera-perspective-rework.md` (retired once this spec lands, same as `39` retired the needs-research doc it replaced).

**Scope check with the project owner:** the shot roster stays exactly what it is today — Top-Down, Over-the-shoulder, Sideline, same three keybindings (`1`/`2`/`3`). No new shots (flipped over-the-shoulder, single-team framed sideline, etc.) ship in this spec. The point of the real-camera architecture is that adding those later is new *data* (a position/aim/FOV triple), not new derivation math — but that's future scope, not this spec's.

## Scope
- A real pinhole-camera projection, `PerspectiveCamera` (`crates/engine/render/src/camera.rs`), replacing `ObliqueCamera` for the **Over-the-shoulder** and **Sideline** presets only. **Top-Down is NOT touched** — it stays the existing flat orthographic formula (`screen_x = x·s`, `screen_y = y·s`), unchanged, because it was the one view confirmed correct in both `37` and `39`'s postmortems and a real-camera rewrite of it would be pure regression risk for zero benefit.
- `BattleCamera` gains a variant split to match: `TopDown` keeps wrapping the old flat/orthographic formula; `Sideline`/`OverShoulder` wrap `PerspectiveCamera`. (Exact type shape is an implementation call — could be two wrapped types, or one enum inside `ObliqueCamera`'s old slot; what must NOT happen is Top-Down's formula changing by so much as a rounding rule.)
- **Fit-to-viewport sizing.** `board_geometry` stops sizing the board box from raw `BOARD_COLS`/`BOARD_ROWS` cell counts (today's bug: sized as if the board renders flat, then the projection shrinks the actual content well inside that box — the "quarter of the screen" symptom). It instead projects the board's world-space corners through the *active* camera, takes the resulting screen-space bounding box, and solves for the `scale_dots` that fits that bounding box to the available area. `cell_width_cols`/`cell_height_rows` become derived from that fitted scale, not the other way around.
- **Feet-anchored billboard placement.** `place()` (`crates/engine/render/src/transform.rs`) gains a vertical anchor: `Bottom` (sprite's own bottom row of dots lands at the projected ground point, sprite grows upward from there) vs. today's implicit `Center` (unchanged, kept for Top-Down, where there's no verticality to anchor). Threaded through `SpriteDraw` so `composite_scene` can pass it per-draw.
- **Depth-based sprite scale reuses the camera's own perspective-divide factor** instead of `39`'s separately-invented `depth_scale_factor` formula — one real distance-from-camera term drives both position and size, not two hand-tuned approximations of the same thing.
- **Camera anchor must sit outside the board's occupied range, for every preset that uses one.** This is the fix for Sideline's "camera in the middle of the pitch" bug — `camera_depth` for a `PerspectiveCamera` is a literal 3D position the camera sits at, not a reference scalar that can legally sit mid-board the way `39` treated it.
- Recalibrate `grid_dim_alpha` — `39`'s `0x60` blended over the assumed-black fallback renders at ~`(96,96,96)`, nearly identical brightness to Top-Down's opaque `(85,85,85)`, which is why "we implemented alpha but it looks the same." New value chosen by actually rendering and looking, not computed on paper.
- Contact shadow switches from a filled-gradient `ShapeKind::Ellipse` to a new `ShapeKind::Ring` (annulus) primitive — the spec `39` already anticipated this exact extension point for a future AOE ring; it turns out the base shadow needs it now, not later.
- Fix `SPRITE_DOT_RATIO` (`1.2`, i.e. deliberately 20% larger than the cell) causing Top-Down sprite overflow — retune to fit inside the cell.
- Explicit bench-piece visibility regression test for all three presets, using this project's own `decode_braille_cell`/dot-alignment verification convention (`crates/game/src/scenes/test_util.rs`) — not eyeballing, not assuming.
- **Hard process requirement carried by this spec, not optional:** every camera preset must be checked against actual rendered/decoded dot output before its task is considered done — passing unit tests on the projection formula in isolation is not suffient evidence, per this project's own verification standard. This is what `39` skipped.

Out of scope:
- Any new shot beyond the existing three (explicitly confirmed with the project owner this round — see Purpose).
- Camera persistence across sessions, mouse/UI camera control (still out per `37`/`39`).
- Any change to `WorldPos` (stays 2D), `Transform`, or the billboard/rasterize pipeline's shape — sprites are still uniform-scale billboards, never sheared/rotated by camera geometry. This is a narrower, contained reversal of one clause of `16-world-space-and-camera` ("no camera orientation, no perspective divide") — scoped strictly to `PerspectiveCamera`'s internal `project`/`depth_key` math; the `Camera` trait's signature is unchanged, so nothing outside `camera.rs` needs to know a real camera model exists underneath it.

## Decisions (v1)

### 1. `PerspectiveCamera` — real pinhole projection for Sideline/Over-the-shoulder
**Engine crate** (`crates/engine/render/src/camera.rs`). Restricted to the same "no yaw, only pitch" constraint `39`'s `DepthAxis` already established (only two axis assignments are ever needed — no free-roaming camera exists here), which keeps the general 3D lookAt math collapsed to a closed form instead of needing full matrix/quaternion machinery:

```rust
pub struct PerspectiveCamera {
    pub depth_axis: DepthAxis,      // unchanged from 39 — which world axis is depth
    pub elevation_deg: f32,         // pitch below horizontal: 0 = along the ground, 90 = straight down
    pub camera_depth: f32,          // world depth-axis position the camera SITS at — must be
                                     // outside [0, BOARD_*] on the correct side, never mid-range
    pub camera_height: f32,         // world units above the ground plane the camera sits at
    pub spread_center: f32,         // world spread-axis coordinate the camera aims at (no yaw)
    pub fov_deg: f32,               // vertical field of view
    pub scale_dots: f32,            // NDC → dots scale; fit-derived every frame (Decision 3)
}

fn project(&self, pos: WorldPos) -> (i32, i32) {
    let (depth, spread) = axis_values(self.depth_axis, pos);
    let elev = self.elevation_deg.to_radians();
    let dz = depth - self.camera_depth;              // signed distance along depth axis
    let dy = -self.camera_height;                     // ground (z=0) relative to camera height
    let cam_forward = dz * elev.cos() - dy * elev.sin();   // must stay > 0 for on-screen content
    let cam_vertical = dz * elev.sin() + dy * elev.cos();
    let cam_right = spread - self.spread_center;

    let half_fov_tan = (self.fov_deg.to_radians() / 2.0).tan();
    let denom = cam_forward.max(NEAR_EPS) * half_fov_tan;
    let screen_x = (cam_right / denom * self.scale_dots).round() as i32;
    let screen_y = (cam_vertical / denom * self.scale_dots).round() as i32;
    (screen_x, screen_y)
}

fn depth_key(&self, pos: WorldPos) -> i32 {
    let (depth, _) = axis_values(self.depth_axis, pos);
    let elev = self.elevation_deg.to_radians();
    let dz = depth - self.camera_depth;
    let cam_forward = dz * elev.cos() + self.camera_height * elev.sin();
    (-cam_forward * self.scale_dots).round() as i32   // nearer (smaller cam_forward) sorts on top
}
```
Both `screen_x` and `screen_y` divide by the *same* `cam_forward` term — this is the fix for `39`'s root bug: one real perspective-divide, applied identically to both axes, instead of a linear map on one and an independently-tuned taper on the other. Convergence (if any is visually present) now happens consistently in both directions from one real camera position, because it's genuinely one camera, not two different fake-depth heuristics glued together.

`camera_depth`, `camera_height`, `fov_deg`, `spread_center` are structural per-preset constants (not live-tunable, matching `39`'s existing distinction between structural camera params and `BattleViewerTuning`'s visual-polish constants). **Their starting values are not pinned by this spec** — unlike `39`, which computed "verified" numbers on paper that turned out to look wrong when actually rendered, this spec requires each preset's constants to be chosen by rendering the view and adjusting until it reads as the intended angle (over-the-shoulder actually reading as ~30° behind the team, sideline actually reading as broadside), not by trusting the formula in isolation.

**Top-Down is untouched.** `BattleCamera::TopDown` keeps wrapping the existing flat orthographic formula (`screen_x = x·s`, `screen_y = y·s`) byte-for-byte — no `PerspectiveCamera`, no pitch, no FOV. It was correct before and stays correct by not being part of this rewrite.

### 2. Fit-to-viewport board sizing
**Game crate** (`crates/game/src/scenes/battle_viewer.rs`), `board_geometry`. Today's algorithm picks `cell_height_rows` from `area`/`BOARD_COLS`/`BOARD_ROWS` alone, as if the board always renders as a flat, unprojected `BOARD_COLS × BOARD_ROWS` rectangle — true for Top-Down, false for any camera that compresses its projection, which is why Over-the-shoulder/Sideline only filled a quarter of the screen. New flow:
1. Build the active preset's camera at a reference `scale_dots` (structural params only).
2. Project the board's four world-space corners `(0,0)`–`(BOARD_COLS, BOARD_ROWS)` through it; take the screen-space bounding box.
3. Solve for the `scale_dots` that fits that bounding box to `area`'s dot dimensions (`area.width*2 × area.height*4`).
4. Rebuild the camera at the solved scale; derive `cell_width_cols`/`cell_height_rows`/`board_rect` from it.
Works identically for Top-Down (whose bbox is already exactly the flat rectangle, so this reduces to today's behavior) and for both perspective presets (whose bbox is now the *actual* projected footprint, so the fitted scale genuinely fills the available area).

### 3. Feet-anchored billboard placement
**Engine crate** (`crates/engine/render/src/transform.rs`). `place()` gains an anchor:
```rust
pub enum VerticalAnchor { Center, Bottom }

pub fn place<'a, C: Camera>(dots: &'a DotBuffer, translate: WorldPos, camera: &C, anchor: VerticalAnchor) -> DotPlacement<'a> {
    let (px, py) = camera.project(translate);
    let dot_y = match anchor {
        VerticalAnchor::Center => py - (dots.rows() / 2) as i32,
        VerticalAnchor::Bottom => py - dots.rows() as i32,
    };
    DotPlacement { dots, dot_x: px - (dots.cols() / 2) as i32, dot_y, depth: camera.depth_key(translate) }
}
```
`SpriteDraw` gains a `vertical_anchor: VerticalAnchor` field (`Center` default, so `render_transform.rs`/`render_movement.rs` and the camera-agnostic `composite.rs`/`transform.rs` tests keep compiling unchanged). Battle-viewer content sets `Bottom` for Sideline/Over-the-shoulder pieces and shadows, `Center` for Top-Down — this fixes "creature's feet aren't in the box."

### 4. Depth-scale reuses the camera's own perspective-divide term
**Game crate.** `39`'s `depth_scale_factor` was a second, independently-tuned approximation of "farther = smaller" alongside `taper_factor`. With a real camera, the same `cam_forward` term already computed inside `project` *is* the correct distance-from-camera measure — expose it (e.g. `PerspectiveCamera::forward_distance(pos) -> f32`) and drive sprite scale directly from `1 / forward_distance` (normalized against the camera's own near reference), not a separately-tuned linear falloff with its own `tuning.depth_scale_per_world_unit`/`depth_scale_min` constants. Top-Down keeps no depth scale at all (unchanged — `k=1` gave exactly `1.0` before, and Top-Down isn't going through `PerspectiveCamera`, so there's no term to derive it from).

### 5. Grid dim alpha, shadow shape, sprite overflow — independent tuning fixes
**Game crate**, none of these depend on the camera rewrite above and should not wait on it:
- `BattleViewerTuning::grid_dim_alpha`: `39`'s `0x60` blended over assumed-black renders indistinguishably from Top-Down's opaque grid. Re-tune by rendering both views side by side and choosing a value that reads as visibly dimmer, not by computing an "acceptable" alpha on paper.
- Contact shadow: add `ShapeKind::Ring` (`crates/engine/render/src/shapes.rs`) — alpha zero at center and at the outer edge, peaking in a band between an inner and outer radius — and use it in place of `ShapeKind::Ellipse` for the battle-viewer's per-piece shadow. `Ellipse` stays (still a valid general primitive; `Ring` is additive, matching the "one real variant today, extension point for more" convention `39` already used for `ShapeKind`).
- `SPRITE_DOT_RATIO` (`1.2`): retune to a value that keeps Top-Down sprites inside their cell (verify by rendering, not by picking a new constant on paper).

### 6. Bench visibility — verify, don't assume
**Game/test.** Add a regression test per camera preset that renders the demo `pieces()` layout and asserts (via `decode_braille_cell`/this project's existing dot-alignment helpers in `crates/game/src/scenes/test_util.rs`) that both bench pieces (`TEAM_A_BENCH_ROW`/`TEAM_B_BENCH_ROW`) produce at least one lit dot distinguishable from the background, for all three presets. This is the check `39` never actually ran — "the bench isn't visible" was reported after the fact, not caught by any test, because no test looked at rendered dots at all.

## Open Questions / TBDs
- Exact `camera_depth`/`camera_height`/`fov_deg`/`spread_center` starting values for Sideline and Over-the-shoulder — deliberately not pinned here (see Decision 1); chosen during implementation by rendering and iterating, then written back into this spec as the shipped values once verified.
- Exact `PerspectiveCamera`/`BattleCamera` type shape (does `TopDown` wrap a distinct small orthographic type, or does `BattleCamera` stop being three variants of one wrapped type and become a true per-variant enum) — mechanical, no visible-behavior consequence either way.
- `ShapeKind::Ring`'s inner/outer radius ratio and alpha curve — a rendering-and-iterating detail, not a design decision.

## Where the code lives
| Decision | Crate | Files |
|---|---|---|
| 1. `PerspectiveCamera` (generic projection primitive) | **Engine** | `crates/engine/render/src/camera.rs` |
| 1. `BattleCamera` presets, Top-Down untouched | **Game** | `crates/game/src/scenes/battle_viewer.rs` |
| 2. Fit-to-viewport `board_geometry` | **Game** | `crates/game/src/scenes/battle_viewer.rs` |
| 3. `VerticalAnchor`/`place()` | **Engine** | `crates/engine/render/src/transform.rs`, `composite.rs` |
| 3. Per-preset anchor choice | **Game** | `crates/game/src/scenes/battle_viewer.rs` |
| 4. Depth-scale via camera's own divide term | **Game** (reads an **Engine** accessor) | `crates/game/src/scenes/battle_viewer.rs`, `crates/engine/render/src/camera.rs` |
| 5. `ShapeKind::Ring` | **Engine** | `crates/engine/render/src/shapes.rs` |
| 5. Grid alpha / shadow-shape choice / sprite-ratio tuning | **Game** | `crates/game/src/scenes/battle_viewer.rs` |
| 6. Bench visibility regression test | **Game** | `crates/game/src/scenes/test_util.rs`, `battle_viewer.rs` |

## Dependencies
- `16-world-space-and-camera` ✅ — 2D `WorldPos`/`Camera` trait signature, unrevised; this spec narrows (not reopens) its "no camera orientation, no perspective divide" clause to `PerspectiveCamera`'s internals only.
- Supersedes and retires `specs/39-battle-viewer-camera-perspective-rework.md` — its `ObliqueCamera`/`taper_factor`/`depth_scale_factor` are replaced outright by Decisions 1 and 4 above; its Decisions 2 (alpha compositing), 4 (contact shadow primitive), 5 (creature art) are inherited as-is, not redone — only the shadow's *shape* (Decision 5 here) and the camera math change.
- `37-battle-viewer-dynamic-camera` — `BattleCamera` enum, key 1/2/3 dispatch, reused as-is (per the Purpose section's scope confirmation: no new shots this round).
- `36-battle-viewer-squad-layout` — the 7×7/bench-row geometry this spec's viewport-fit and bench-visibility test render against; unchanged.
- `13-rendering` ✅ / `33-scene-composite-primitive` ✅ — `place`/`composite_scene`, extended (not replaced) by Decision 3's anchor parameter.
