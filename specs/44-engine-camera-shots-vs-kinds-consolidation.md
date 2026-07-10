# Engine Camera: Shots vs. Kinds Consolidation

## Purpose
Spec 42 built an engine-level `AnyCamera` enum specifically to stop game code matching on preset identity — but it named the enum's variants after **shots** (`FreeRoam`), not **projection kinds**, and left a genuine duplicate (`SideView`) sitting alongside its own replacement. A camera *kind* is a projection formula (orthographic, pinhole/perspective). A *shot* is how that formula gets used — where it's positioned, whether its parameters are fixed once or driven continuously by input. Those are different axes, and only the first one belongs in the engine crate: a hypothetical different game built on this engine has no idea what a "free-roam camera" or a "side-view camera" *is* as a distinct kind of projection, because they aren't one — they're this game's names for a use of a perspective/orthographic camera.

Concretely, checked against the actual code (not assumed):
- `SideView` and `OrthographicCamera` are the exact same formula under two names (`project`/`depth_key`/`local_dots_per_world_unit` are identical, same `{ scale_dots: f32 }` shape). `SideView` predates the Oblique→Orthographic lineage (spec 16/13-era) and its only remaining callers are engine's own unit tests (`composite.rs`, `transform.rs`) and two stale example binaries — the real game (`battle_viewer.rs`) only ever uses `OrthographicCamera`. Pure leftover duplication.
- `PerspectiveCamera` and `FreeRoamCamera` are the same pinhole-camera formula (position, orientation, FOV, scale → screen). `PerspectiveCamera` restricts "which way it faces" to one of exactly two world axes (`DepthAxis::Row|Col` + `facing_sign` + `spread_center`) instead of a real yaw — a spec-41-era scope-down (only two named shots existed then), not a different *kind* of camera. `FreeRoamCamera` is the strictly more general version of the same math. There's no principled reason for two pinhole-camera types where one is a same-shaped subset of the other — the exact "match arm added per preset instead of per real kind" problem spec 42 set out to fix, just not carried all the way through.

This spec finishes that: engine ends up with exactly two `Camera`-implementing types (`OrthographicCamera`, `PerspectiveCamera` generalized to arbitrary yaw), and every named thing currently modeled as a camera *kind* — Sideline, Over-the-shoulder, Top-down, Free-roam — becomes purely a game-crate concept: a preset constructor picking fixed parameters, or (for free-roam) a per-keypress *behavior* applied to an ordinary `PerspectiveCamera` value, never a distinct engine type or `AnyCamera` variant.

**Amendment (post-implementation finding, confirmed with the project owner):** the first implementation attempt surfaced a real mathematical fact this spec's first draft missed. The *old* `PerspectiveCamera` (`DepthAxis` + `facing_sign`) wasn't a real camera orientation at all — `facing_sign` flips which way is "near" on the depth axis while leaving left/right completely untouched, which is a reflection (determinant −1), not something a real, single-orientation camera can physically do (a real camera can't face backwards in depth while its left/right stays fixed — that requires mirroring it, not rotating it). `FreeRoamCamera`'s single-`yaw_deg` rotation (determinant +1) is the physically-correct model — the same one real engines use — and a rotation can never reproduce a reflection for any choice of yaw, verified numerically for both Sideline and Over-the-shoulder. So "fold `FreeRoamCamera`'s formula in verbatim" and "Sideline/Over-the-shoulder stay pixel-identical" were mutually exclusive, and the project owner elected the physically-normal camera as canonical: `cam_space` stays the real rotation, unchanged, and Sideline/Over-the-shoulder are accepted to render **mirrored left-right** relative to today, requiring golden-fixture regeneration and a human visual-approval pass before shipping (Decision 1/Scope below reflect this).

## Scope
- **Engine** (`crates/engine/render/src/camera/`): delete `side_view.rs` and its `SideView` type entirely. Delete `free_roam.rs` and its `FreeRoamCamera` type entirely, folding its fields (`x`, `y`, `height`, `yaw_deg`, `pitch_deg`, `fov_deg`, `scale_dots`), formula (`cam_space`, `cam_forward_raw`, `forward_distance`, `half_fov_tan`, `project`, `depth_key`, `local_dots_per_world_unit`), and `nudge()` method into `PerspectiveCamera` (`perspective.rs`), replacing its current `depth_axis`/`elevation_deg`/`camera_depth`/`camera_height`/`spread_center`/`facing_sign` fields and `DepthAxis`-based formula. Delete `DepthAxis` and `axis_values` (dead once `PerspectiveCamera` no longer branches on them — confirmed nothing else in the codebase touches `.depth_axis` as a field). `AnyCamera` shrinks to `{ Orthographic(OrthographicCamera), Perspective(PerspectiveCamera) }` — the `FreeRoam` variant is deleted.
- **Engine**: `OrthographicCamera` gains a `pub fn new(scale_dots: f32) -> Self` constructor (mechanical parity with the `SideView::new` call sites being repointed to it).
- **Game** (`crates/game/src/scenes/battle_viewer/camera.rs`): new `FitMode` enum (`ExactFit`, `ViewportFit`, `Manual`) — this is the game-crate-owned "how do we frame this shot" concept spec 42 explicitly deferred (its Decision 2 called board_geometry's fit-strategy match "a second, narrower match... for a different reason... an intentionally out-of-scope, pre-existing seam"). `BattleCamera` gains `pub fit: FitMode`. All four preset constructors updated: `top_down_preset()` → `FitMode::ExactFit`; `sideline_preset()`/`over_shoulder_preset()` → `FitMode::ViewportFit`, both rebuilt as plain `PerspectiveCamera` values with explicit `x`/`y`/`yaw_deg`/`pitch_deg` instead of `depth_axis`/`camera_depth`/`facing_sign`/`spread_center`; `free_roam_preset()` → `FitMode::Manual`, now just another `PerspectiveCamera` value (no distinct type/variant), keeping its existing starting transform.
- **Game** (`crates/game/src/scenes/battle_viewer/geometry.rs`): `board_geometry` dispatches on `mode.fit` (the new `FitMode`) instead of matching `mode.camera`'s `AnyCamera` variant — replaces engine-kind-based dispatch with the game-crate concept that was actually driving the decision all along. Each arm's body is unchanged logic, just re-keyed (`FitMode::ExactFit` = today's `AnyCamera::Orthographic` arm, `FitMode::ViewportFit` = unchanged call to `fit_perspective_geometry`, `FitMode::Manual` = today's `AnyCamera::FreeRoam` arm).
- **Game** (`crates/game/src/scenes/battle_viewer/mod.rs`): `handle_input`'s free-roam nudge dispatch and the `'4'`-key idempotency guard key off `self.camera_mode.fit == FitMode::Manual` instead of `matches!(self.camera_mode.camera, AnyCamera::FreeRoam(_))` — the *behavior* ("is this shot manually driven by keypresses right now") was always what the guard meant; it now says so directly instead of inferring it from which engine variant happens to be active.
- **Game**: mechanical test fallout in `battle_viewer/camera.rs` (`battle_camera_tests`' `perspective()` helper, `near_far_depths`, `column_spread`) and `battle_viewer/geometry.rs` (`board_geometry_tests`' direct `PerspectiveCamera` literal) — rebuilt against the new fields. These are already **property-only** tests (no pinned "correct" constant, per this codebase's own existing guardrail comment) so the rewrite is mechanical, not a re-derivation of what's being tested.

Out of scope:
- Any change to `OrthographicCamera` itself, to the `Camera` trait's method set (spec 42 Decision 1), or to `board_geometry`'s actual fit *algorithms* (`fit_perspective_geometry`'s viewport-fit math, the exact-fit column/bench math) — every arm's body is preserved verbatim, only the dispatch key changes.
- Any new camera capability, preset, or shot.
- Inspector exposure, persistence, or any other free-roam capability change — still dev-tool-only, still discrete-nudge, unchanged from spec 42.

Acceptance bar (amended — see Purpose):
- **Top-down**: unaffected, byte-identical — untouched by this spec.
- **Free-roam**: its `cam_space`/`project`/`nudge()` formula is canonical and unchanged from `FreeRoamCamera` (verbatim, no sign correction) — this is the real, physically-consistent single-camera model, matching how actual game engines represent a camera, and is deliberately treated as the source of truth the other two presets are re-derived against, not the other way around.
- **Sideline / Over-the-shoulder**: their world-space camera position (`x`/`y`/`height`/`yaw_deg`/`pitch_deg`) is preserved from Decision 3's derivation (already verified correct for depth/forward/height), but their **on-screen left/right is expected to mirror** relative to today's golden fixtures — this is the accepted, intentional consequence of retiring the old axis-swap+`facing_sign` hack (which was never a real camera orientation) in favor of the real rotation. `sideline_golden_matches_baseline`/`over_shoulder_golden_matches_baseline` MUST be regenerated (`UPDATE_BATTLE_VIEWER_FIXTURES=1`) as part of this work, and the regenerated output requires a human visual-approval pass (render and inspect, per this project's standing verification discipline) before the bucket can be considered done — a pipeline gate passing against regenerated fixtures is not sufficient on its own for this specific change, since the fixtures themselves are the thing changing.

## Decisions (v1)

### 0. Delete `SideView` first, independently
Lowest-risk slice, no dependents beyond mechanical call-site updates. Delete `crates/engine/render/src/camera/side_view.rs`. Add `OrthographicCamera::new(scale_dots: f32) -> Self` (trivial). Repoint every current `SideView` caller to `OrthographicCamera::new(..)`:
- `crates/engine/render/src/composite.rs` (test fixture camera)
- `crates/engine/render/src/transform.rs` (test fixture camera)
- `crates/game/examples/render_movement.rs`
- `crates/game/examples/render_transform.rs`

Remove `SideView` from `camera/mod.rs`'s `mod`/`pub use` list and its two mentions in the `Camera` trait's doc comments (update to reference `OrthographicCamera` alone).

### 1. Generalize `PerspectiveCamera` to arbitrary yaw, absorbing `FreeRoamCamera`
```rust
pub struct PerspectiveCamera {
    pub x: f32,
    pub y: f32,
    pub height: f32,
    pub yaw_deg: f32,
    pub pitch_deg: f32,   // was `elevation_deg` — renamed to match the field this absorbs from FreeRoamCamera; the Camera trait METHOD `elevation_deg()` is unchanged (still returns pitch_deg)
    pub fov_deg: f32,
    pub scale_dots: f32,
}
```
`cam_space`, `cam_forward_raw`, `forward_distance`, `half_fov_tan`, `project`, `depth_key`, `vertical_anchor_hint` (`Bottom`), `elevation_deg()` (returns `pitch_deg`), `local_dots_per_world_unit`, and `nudge()` all move here **truly verbatim** from `FreeRoamCamera` (`crates/engine/render/src/camera/free_roam.rs`) — no sign correction, no reflection injected into `cam_space` — replacing `PerspectiveCamera`'s current `DepthAxis`-based versions. This is a deliberate, confirmed decision (see Purpose's amendment): `cam_space`'s rotation is the physically-correct camera model and is canonical; Sideline/Over-the-shoulder are accepted to render mirrored left-right versus today as the cost of retiring the old, non-physical axis-swap+`facing_sign` formula. `facing_sign` is deleted entirely — `yaw_deg` alone determines facing via `cam_space`, so the field that existed only to compensate for `DepthAxis` not encoding a real direction (and whose whole doc comment describes exactly that compensation) is no longer needed, and must NOT be reintroduced in any form (e.g. a per-instance sign multiplied into `cam_space`'s `right` term) — that would just be `facing_sign` under a new name, defeating the whole consolidation. `depth_axis`, `camera_depth`, `spread_center`, `camera_height` are deleted (the last renamed to `height`, absorbed as-is). `DepthAxis` and `axis_values` (`camera/mod.rs`) are deleted — confirmed dead: `.depth_axis` is referenced nowhere outside `perspective.rs` itself.

`free_roam.rs` is deleted; its `#[cfg(test)] pub(super) fn free_roam_representative_cam()` test fixture and its test module move into `perspective.rs`, becoming (or merging with) `PerspectiveCamera`'s own representative-camera fixture and tests.

### 2. `AnyCamera` shrinks to two variants
```rust
pub enum AnyCamera {
    Orthographic(OrthographicCamera),
    Perspective(PerspectiveCamera),
}
```
`impl Camera for AnyCamera` and `with_scale_dots` lose their `FreeRoam` match arms — otherwise unchanged. This is now a real, stable "kind" enum: it changes only when a genuinely new *projection formula* is invented, never when a new shot is.

### 3. `FitMode` — the game-crate concept `AnyCamera`'s old third variant was actually encoding
```rust
// crates/game/src/scenes/battle_viewer/camera.rs
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FitMode {
    /// Board bbox exactly fills the viewport, no perspective-fit search
    /// (today's `AnyCamera::Orthographic` arm of `board_geometry`).
    ExactFit,
    /// Viewport-fit via `fit_perspective_geometry`'s corner-projection search
    /// (today's `AnyCamera::Perspective` arm).
    ViewportFit,
    /// No auto-fit — the camera's own carried `scale_dots`/position/orientation
    /// determine framing directly; only the screen-centering offset is applied
    /// (today's `AnyCamera::FreeRoam` arm).
    Manual,
}

pub struct BattleCamera {
    pub camera: AnyCamera,
    pub fit: FitMode,
}
```
Preset constructors set both fields:
- `top_down_preset()`: `AnyCamera::Orthographic(OrthographicCamera::new(0.0))`, `fit: FitMode::ExactFit`.
- `sideline_preset()`: `AnyCamera::Perspective(PerspectiveCamera { x: SIDELINE_CAMERA_DEPTH, y: BOARD_CENTER_COL, height: 2.5, yaw_deg: 90.0, pitch_deg: 10.0, fov_deg: 55.0, scale_dots: 0.0 })`, `fit: FitMode::ViewportFit`. `yaw_deg: 90.0` for Sideline's prior `depth_axis: Col, facing_sign: 1.0` is the position/orientation spec 42's `free_roam_preset()` already used (its forward/depth/height terms confirmed correct by that spec's manual verification pass). Same divergence as Over-the-shoulder applies here: `screen_x` mirrors relative to today's golden fixture — accepted, per Scope's amended acceptance bar.
- `over_shoulder_preset()`: `AnyCamera::Perspective(PerspectiveCamera { x: BOARD_CENTER_COL, y: OVER_SHOULDER_CAMERA_DEPTH, height: OVER_SHOULDER_CAMERA_HEIGHT, yaw_deg: 180.0, pitch_deg: 30.0, fov_deg: 55.0, scale_dots: 0.0 })`, `fit: FitMode::ViewportFit`. `yaw_deg: 180.0` is derived from `over_shoulder_preset`'s old `depth_axis: Row` (depth=world-y, spread=world-x) + `facing_sign: -1.0` (looks toward decreasing row) via `FreeRoamCamera`'s convention (`yaw=0` faces `+y`, so facing `-y` is `yaw=180`), `x`/`y` from swapping `spread_center`/`camera_depth` onto world-x/world-y per the Row-axis assignment. Verified (research task, numerically worked by hand): this position/orientation reproduces the old forward/depth/height terms exactly — the ONLY divergence from today is `screen_x`'s sign (mirrored), which is the accepted, intentional consequence documented in Scope's amended acceptance bar, not a bug to chase out.
- `free_roam_preset()`: unchanged starting values (`x: SIDELINE_CAMERA_DEPTH, y: BOARD_CENTER_COL, height: 2.5, yaw_deg: 90.0, pitch_deg: 10.0, fov_deg: 55.0, scale_dots: 40.0`), now just `AnyCamera::Perspective(...)`, `fit: FitMode::Manual`.

### 4. `board_geometry` dispatches on `FitMode`, not on `AnyCamera`'s variant
```rust
pub fn board_geometry(area: Rect, mode: BattleCamera, tuning: BattleViewerTuning) -> BoardGeometry {
    match mode.fit {
        FitMode::ExactFit => { /* today's AnyCamera::Orthographic arm, verbatim */ }
        FitMode::ViewportFit => fit_perspective_geometry(area, mode, tuning), // unchanged
        FitMode::Manual => { /* today's AnyCamera::FreeRoam arm, verbatim */ }
    }
}
```
`fit_perspective_geometry` itself needs **no changes at all** — it already operates purely through `Camera::project`/`with_scale_dots` (confirmed by reading it: no field access on `PerspectiveCamera` beyond the trait), so it's agnostic to this whole restructuring already.

### 5. `handle_input`/nudge dispatch keys off `FitMode::Manual`
`crates/game/src/scenes/battle_viewer/mod.rs`: the `'4'`-key idempotency guard (`KeyCode::Char('4') if !matches!(self.camera_mode.camera, AnyCamera::FreeRoam(_))`) becomes `if !matches!(self.camera_mode.fit, FitMode::Manual)`. `nudge_free_roam`'s call site (currently `if let AnyCamera::FreeRoam(fr) = &mut self.camera_mode.camera { Self::nudge_free_roam(fr, code); }`) becomes `if matches!(self.camera_mode.fit, FitMode::Manual) { if let AnyCamera::Perspective(p) = &mut self.camera_mode.camera { Self::nudge_camera(p, code); } }` — `nudge_free_roam` is renamed `nudge_camera` (it now nudges *whatever* `PerspectiveCamera` is active, per its `FitMode::Manual` gate, not a distinct free-roam type) and its parameter type changes from `&mut FreeRoamCamera` to `&mut PerspectiveCamera`; its body (the key-to-`nudge()`-call mapping) is unchanged.

## Where the code lives
| Decision | Crate | Files |
|---|---|---|
| 0. Delete `SideView`, add `OrthographicCamera::new` | **Engine** | `crates/engine/render/src/camera/side_view.rs` (deleted), `orthographic.rs`, `mod.rs` |
| 0. Repoint `SideView` callers | **Engine** | `crates/engine/render/src/composite.rs`, `transform.rs` |
| 0. Repoint `SideView` callers | **Game** | `crates/game/examples/render_movement.rs`, `render_transform.rs` |
| 1. `PerspectiveCamera` generalized, `FreeRoamCamera`/`DepthAxis`/`axis_values` deleted | **Engine** | `crates/engine/render/src/camera/perspective.rs`, `free_roam.rs` (deleted), `mod.rs` |
| 2. `AnyCamera` shrinks to 2 variants | **Engine** | `crates/engine/render/src/camera/mod.rs` |
| 3. `FitMode`, `BattleCamera.fit`, preset rewrites | **Game** | `crates/game/src/scenes/battle_viewer/camera.rs` |
| 4. `board_geometry` re-keyed on `FitMode` | **Game** | `crates/game/src/scenes/battle_viewer/geometry.rs` |
| 5. `handle_input`/`nudge_camera` re-gated | **Game** | `crates/game/src/scenes/battle_viewer/mod.rs` |
| Test fallout | **Game** | `battle_viewer/camera.rs`, `battle_viewer/geometry.rs` |

## Open Questions / TBDs
None blocking. The handedness question (Purpose's amendment) was raised by the pipeline mid-implementation, confirmed with the project owner, and is resolved above: `FreeRoamCamera`'s rotation formula is canonical and unchanged; Sideline/Over-the-shoulder accept a left-right mirror versus today, requiring golden-fixture regeneration plus a human visual-approval pass before the affected bucket can ship.

## Dependencies
- `42-engine-camera-kind-api-and-free-roam-camera` — this spec completes the consolidation that spec 42 started but didn't finish: its `AnyCamera` enum and `Camera` trait additions are unchanged in shape (still exactly the methods spec 42 Decision 1 added), just re-scoped from 3 variants to 2, with the third variant's real distinction (fit strategy) promoted to its own explicit game-crate type (`FitMode`) instead of being smuggled in as a camera-kind variant. Spec 42's Decision 2 itself predicted this seam ("a second, narrower match... stays there deliberately... an intentionally out-of-scope... seam this spec doesn't touch") — this spec is that follow-up.
- `41-battle-viewer-perspective-camera-rework` — `PerspectiveCamera`'s original `facing_sign` field (added there to fix a real near/far-facing bug) is retired here, not reintroduced: `yaw_deg` makes facing unambiguous by construction, the same reasoning spec 42's `FreeRoamCamera` already relied on to avoid needing the field in the first place.
