> # ✅ DONE! — Completed 2026-07-09
> Status: implemented via the `tdd-pipeline`, all tasks GREEN, full workspace gate clean. Manually verified beyond the automated gate: exercised Sideline/Over-the-shoulder/Top-down (pixel-identical to pre-refactor) and free-roam (movement/rotation/pitch/zoom, idempotent re-selection, fresh reset on re-entry), which caught one design gap (free-roam's starting view rendered in a screen corner due to a zero `screen_offset`), fixed and re-verified. Later generalized further by `44-engine-camera-shots-vs-kinds-consolidation`.

# Engine Camera-Kind API & Free-Roam Camera

## Purpose
Two related engine asks, both prompted directly by how spec 41 actually went: finding good camera numbers took many rounds of hand-guessing a constant, rendering, and looking — and every one of `BattleCamera`'s per-kind behaviors (vertical anchor, grid-line prominence, elevation-driven shadow squash, per-position sprite sizing) is dispatched today via `match self { Sideline(_) => .., TopDown(_) => .., OverShoulder(_) => .. }`, keyed on **3 named presets**, repeated across roughly half a dozen methods. Adding a new preset, or changing which projection type backs an existing one, means editing match arms in multiple places, not writing one constructor.

This spec fixes both: (1) an engine-level API where a camera's own kind determines its rendering-relevant behavior directly (so game code that just wants "a camera" never matches on preset identity), and (2) a genuine free-roaming camera (arbitrary position + yaw + pitch + FOV) so good framings can be found by flying around and reading off the resulting numbers, instead of guessing constants blind between renders.

**Scope confirmed with the project owner:** the free-roam camera is a real, unrestricted **engine** capability (any future game can use it as a normal camera), but in *this* game it is wired up as **dev/debug tooling only** — not a player-facing mode. Movement is discrete nudge-per-keypress (relies on the terminal's own key-repeat for a "held key" feel, like every other input in this codebase already does — no continuous velocity integration, no keyboard-enhancement-protocol dependency). This spec is about the camera abstraction and switching mechanism only — it does not touch `board_geometry`'s existing fit-to-viewport algorithm for the two camera kinds spec 41 shipped; free-roam gets the smallest new branch necessary to render at all (fixed zoom, no auto-fit), not a redesign of that logic.

## Scope
- **Engine** (`crates/engine/render/src/camera.rs`): a handful of new `Camera` trait methods, each with a default, so a camera's rendering-relevant behavior (billboard anchor, elevation, per-position dot scale) is queryable from the camera value itself.
- **Engine**: `FreeRoamCamera` — a real pinhole camera with position (`x`, `y`, `height`), `yaw_deg`, `pitch_deg`, `fov_deg`, `scale_dots` — generalizing `PerspectiveCamera`'s row/col-only `DepthAxis` restriction into an actual 2D rotation. Same formula shape as `PerspectiveCamera`, just not locked to one of two world axes.
- **Engine**: `AnyCamera` — an enum wrapping `{Orthographic, Perspective, FreeRoam}`, implementing `Camera` and the new trait methods by delegating. This is the one place a match over "which camera kind" lives — a small, engine-owned, rarely-changing set. Picking a different kind for a preset becomes "construct a different `AnyCamera` variant," not "add a match arm in five places."
- **Game** (`crates/game/src/scenes/battle_viewer.rs`): `BattleCamera` simplifies to a thin wrapper holding an `AnyCamera` (dropping its own 3-named-variant enum and every match keyed on preset identity). Preset constructors (`sideline_preset()`, `over_shoulder_preset()`, `top_down_preset()`, `free_roam_preset()`) become the only place that picks parameter values; nothing else needs to know which preset is active.
- **Game**: free-roam wired up as a 4th mode, key `4`, discrete-nudge keyboard controls, dev-tool only (see Decision 5).

Out of scope:
- Any redesign of `board_geometry`'s existing fit-to-viewport algorithm for Orthographic/Perspective — unchanged. Free-roam's own geometry handling (Decision 5) is new plumbing, not a refactor of the existing two paths.
- Player-facing exposure of free-roam, input polish beyond "functional for dev use," bounds/collision (flying through the board or off into the void is fine — it's a debug tool), persistence across sessions, inspector/live-tunable exposure of free-roam's transform (a natural future extension, not built here).
- Any change to `WorldPos` staying 2D, or to the `Camera` trait's existing `project`/`depth_key` signatures — this spec adds new *default* methods, it doesn't change the two that exist.

## Decisions (v1)

### 0. `ObliqueCamera` → `OrthographicCamera`, dead generality stripped
`ObliqueCamera` (spec 39) was built as a general tilted/oblique-family projection (`elevation_deg`/`depth_axis`/`taper_per_world_unit`/`taper_min` — depth compresses below `elevation_deg = 90`, spread converges via the taper term), but spec 41 restricted it to backing Top-Down ONLY, always at `elevation_deg = 90.0`. At `k = sin(90°) = 1`, every non-flat term cancels and the formula is exactly `screen_x = x·scale_dots, screen_y = y·scale_dots` — true orthographic projection, no tilt. Nothing in this codebase has ever constructed it any other way, so the general (non-90°) machinery is untested-in-anger dead code today, and the name overstates what the type actually does.

Renamed to `OrthographicCamera`, fields reduced to just `scale_dots`:
```rust
pub struct OrthographicCamera {
    pub scale_dots: f32,
}
impl Camera for OrthographicCamera {
    fn project(&self, pos: WorldPos) -> (i32, i32) {
        ((pos.x * self.scale_dots).round() as i32, (pos.y * self.scale_dots).round() as i32)
    }
    fn depth_key(&self, pos: WorldPos) -> i32 {
        (pos.y * self.scale_dots).round() as i32
    }
}
```
`depth_axis`, `elevation_deg`, `camera_depth`, `taper_per_world_unit`, `taper_min`, and `taper_factor` are deleted along with it — confirmed with the project owner: the tradeoff (losing a currently-unused "tilted-but-not-perspective" capability, in exchange for an honest name and less code to carry) is the right one; a future preset that wants that look can reintroduce it deliberately, informed by an actual use case, rather than carrying it speculatively.

### 1. `Camera` trait grows a few generically-meaningful methods, each with a default
Today, `BattleCamera` (game crate) re-derives these per-preset via its own matches; they move onto the camera value itself, implemented once per **kind** (engine crate), not per **preset** (game crate):

```rust
pub trait Camera {
    fn project(&self, pos: WorldPos) -> (i32, i32);
    fn depth_key(&self, pos: WorldPos) -> i32;

    /// Billboard anchor a camera with this kind of view wants: `Center` for a
    /// camera with no meaningful verticality (looking straight down), `Bottom`
    /// for one with real elevation (a standing sprite's feet plant on the
    /// ground point, body extends upward). Default `Center` — a camera type
    /// that never overrides this is asserting it has no vertical tilt.
    fn vertical_anchor_hint(&self) -> VerticalAnchor { VerticalAnchor::Center }

    /// Degrees above horizontal the camera looks from: `90` = straight down,
    /// `0` = level with the ground. Drives shadow squash (`k = sin(elevation)`)
    /// and grid-line dimming (dim below some threshold) generically, instead
    /// of each caller matching on preset identity to guess it. Default `90.0`
    /// (matches today's implicit "flat/no elevation" assumption).
    fn elevation_deg(&self) -> f32 { 90.0 }

    /// Dots per world unit AT `pos` specifically — how wide is exactly one
    /// world unit, right where a sprite/shadow actually stands. Constant
    /// everywhere for a flat/orthographic camera; shrinks with distance for
    /// a real perspective camera. This is the ONE sizing method battle-viewer
    /// content uses for both sprite width-fill and shadow sizing — replaces
    /// today's `BattleCamera::sprite_scale_dots` (a separate, TopDown-only,
    /// near-reference-only method) entirely, since a flat camera's version of
    /// this is already just its constant `scale_dots` regardless of `pos`.
    fn local_dots_per_world_unit(&self, pos: WorldPos) -> f32;
}
```
`local_dots_per_world_unit` has no sensible universal default (an orthographic camera's answer and a perspective camera's answer are genuinely different formulas), so it stays a required method; the other two get defaults specifically so `SideView` (unmodified per spec 41's own call) and any future minimal camera don't need to opt in to properties they don't have.

`OrthographicCamera` and `PerspectiveCamera` each override `vertical_anchor_hint`/`elevation_deg` to return their real values (already-existing fields — no new state, just exposing what's already there through the trait instead of through a game-crate match). `grid_line_color` (game crate, `battle_viewer.rs`) changes from matching on preset name to checking `camera.elevation_deg() < 89.5` (dim) vs. not (opaque) — a real elevation threshold, not a preset lookup.

### 2. `AnyCamera` — one value type, one place the kind-match lives
```rust
pub enum AnyCamera {
    Orthographic(OrthographicCamera),
    Perspective(PerspectiveCamera),
    FreeRoam(FreeRoamCamera),
}
impl Camera for AnyCamera { /* delegates project/depth_key/vertical_anchor_hint/
                                elevation_deg/local_dots_per_world_unit to whichever
                                variant is active */ }
impl AnyCamera {
    pub fn with_scale_dots(&self, scale_dots: f32) -> Self { /* delegates */ }
}
```
This is the only exhaustive match over "which camera kind" for RENDERING behavior (anchor, elevation, sizing) anywhere — it lives in the engine crate because the 3 kinds it names (`OrthographicCamera`, `PerspectiveCamera`, `FreeRoamCamera`) are all engine types, content-free, and this match changes only when a genuinely new *projection kind* is invented (a rare, architecture-level event, correctly gated by an exhaustive match) — never when a *preset* is added.

One deliberate exception: `board_geometry`'s fit-STRATEGY selection (Decision 5) also matches on `AnyCamera`'s variant, in the game crate. That's a second, narrower match — for a different reason (which sizing *algorithm* to run: flat exact-fit, viewport-fit, or free-roam's fixed-zoom), not a per-kind rendering property — and it stays there deliberately (per the project owner's explicit call: this spec is about camera props, not board framing/sizing). It's not a gap this spec fails to close; it's an intentionally out-of-scope, pre-existing seam this spec doesn't touch beyond adding the one new arm free-roam needs to render at all.

### 3. `BattleCamera` (game crate) simplifies to data, not dispatch
```rust
pub struct BattleCamera {
    pub camera: AnyCamera,
}
```
replacing today's `enum BattleCamera { Sideline(PerspectiveCamera), TopDown(OrthographicCamera), OverShoulder(PerspectiveCamera) }` and its half-dozen exhaustive matches. Preset constructors are unchanged in spirit (`sideline_preset()`, `over_shoulder_preset()`, `top_down_preset()`, new `free_roam_preset()`), each just building the right `AnyCamera` variant with the right parameter values — adding a 5th preset, or moving an existing preset to a different camera kind, is changing what one constructor returns, not editing match arms scattered across the file. `handle_input`'s digit-key dispatch (`1`/`2`/`3`/new `4`) is unchanged in shape — it already just assigns `self.camera_mode = BattleCamera::x_preset()`.

Existing tests that pattern-match a named `BattleCamera` variant to reach the wrapped camera (e.g. `let BattleCamera::Sideline(inner) = ... `) update to match on `.camera` (an `AnyCamera`) instead — mechanical, expected fallout of this restructuring, not enumerated task-by-task here.

### 4. `FreeRoamCamera` — the same formula shape, minus the axis restriction
```rust
pub struct FreeRoamCamera {
    pub x: f32,
    pub y: f32,
    pub height: f32,
    pub yaw_deg: f32,    // rotation in the world xy-plane
    pub pitch_deg: f32,  // 0 = level with the ground, 90 = straight down (same convention as elevation_deg)
    pub fov_deg: f32,
    pub scale_dots: f32, // a fixed zoom the camera itself carries — NOT solved by a viewport fit (Decision 5)
}
```
`PerspectiveCamera` splits a world position into (depth, spread) via `DepthAxis` (only two hard-coded axis assignments, deliberately, because spec 41 had no free-roaming camera to justify more). `FreeRoamCamera` replaces that restriction with a real 2D rotation by `yaw_deg`:
```rust
fn cam_space(&self, pos: WorldPos) -> (f32 /* right */, f32 /* forward */) {
    let (dx, dy) = (pos.x - self.x, pos.y - self.y);
    let yaw = self.yaw_deg.to_radians();
    (dx * yaw.cos() - dy * yaw.sin(), dx * yaw.sin() + dy * yaw.cos())
}
```
`forward` here is already signed correctly by construction (yaw fully determines which way the camera looks), so unlike `PerspectiveCamera` there's no separate `facing_sign` to get right — that field only existed to compensate for `DepthAxis` not encoding a real direction. The rest of the formula is the same shape `PerspectiveCamera` already validated (Decision 1 of spec 41): `cam_forward_raw = forward * pitch.cos() + height * pitch.sin()`, clamped to `NEAR_EPS` for the perspective divide, `cam_vertical = height * pitch.cos() - forward * pitch.sin()`, `screen_x`/`screen_y` divide by `forward_distance * half_fov_tan` exactly as today. `vertical_anchor_hint` returns `Bottom`, `elevation_deg` returns `pitch_deg`, `local_dots_per_world_unit(pos)` is `scale_dots / (forward_distance(pos) * half_fov_tan())` — the same expression `PerspectiveCamera` already has, just called through the trait now.

A small, generic, game-agnostic movement helper ships alongside it (still engine-level — no board/battle knowledge):
```rust
impl FreeRoamCamera {
    /// Nudges position (in camera-relative forward/right terms, not world
    /// x/y) and orientation by fixed deltas — one call per input event, not
    /// integrated over time (spec confirms discrete-nudge control, not
    /// continuous velocity).
    pub fn nudge(&mut self, forward: f32, right: f32, yaw_delta: f32, pitch_delta: f32, height_delta: f32) {
        let yaw = self.yaw_deg.to_radians();
        self.x += forward * yaw.sin() + right * yaw.cos();
        self.y += forward * yaw.cos() - right * yaw.sin();
        self.yaw_deg += yaw_delta;
        self.pitch_deg = (self.pitch_deg + pitch_delta).clamp(-89.0, 89.0); // avoid the exact-vertical singularity
        self.height += height_delta;
    }
}
```

### 5. Game-crate wiring: dev-only 4th mode, minimal new geometry branch
- `handle_input` gains `KeyCode::Char('4')`: if `camera_mode` is NOT already `AnyCamera::FreeRoam`, assign `self.camera_mode = BattleCamera::free_roam_preset()` (direct selection from a fixed preset, same convention as `1`/`2`/`3`); if it's already `FreeRoam`, `'4'` is a no-op. This is the one place free-roam deliberately breaks the "digit always picks the same fixed view" convention: `1`/`2`/`3` are stateless presets where re-selecting is harmless, but free-roam carries mutable position/orientation state the user has been flying around — re-running `free_roam_preset()` on an accidental second `'4'` press would silently discard it. Pressing `1`, `2`, or `3` while in free-roam exits it back to that fixed preset — no separate dedicated exit key needed. (The existing test `non_digit_key_leaves_camera_mode_unchanged`, which currently asserts `'4'` is a no-op from every start, needs updating to reflect that `'4'` now does something — expected fallout, not a gap.)
- **Starting transform: `free_roam_preset()` starts from Sideline's own position**, converted from `PerspectiveCamera`'s parameterization into `FreeRoamCamera`'s: `x = SIDELINE_CAMERA_DEPTH`, `y = BOARD_CENTER_COL`, `height = 2.5`, `pitch_deg = 10.0` (Sideline's `elevation_deg`, same convention), `fov_deg = 55.0`, `scale_dots = 40.0` (a reasonable starting zoom — see the zoom control below), and `yaw_deg = 90.0` (Sideline's `depth_axis: Col` + `facing_sign: 1.0` means "looks toward increasing world-x"; in `FreeRoamCamera`'s convention — `yaw = 0` faces `+y` — facing `+x` is `yaw = 90.0`).
- Movement keys, active only while `camera_mode` wraps `AnyCamera::FreeRoam`, each calling `nudge` once per keypress with a fixed step:
  - `W`/`S` forward/back, `A`/`D` strafe — step `0.5` world units (a board cell is `1.0` world unit, so this is roughly "half a cell per press," fine-grained enough to line up a shot without needing dozens of presses to cross the board).
  - `Q`/`E` yaw, `R`/`F` pitch — step `5.0` degrees (72 presses for a full rotation; coarse enough to get somewhere fast, fine enough to land on a specific angle).
  - `Z`/`X` height — step `0.5` world units (matches the move step, and existing preset heights span roughly `2.5`–`16`, so this covers that range in a reasonable number of presses).
  - `[`/`]` zoom out/in — multiplies `scale_dots` by `0.9`/`1.1`. Needed because free-roam's `scale_dots` is fixed/manual (Decision 5's `board_geometry` arm, below) — without a zoom control there'd be no way to adjust framing tightness after entering, only position/angle.
- `board_geometry` gains a third match arm for `AnyCamera::FreeRoam`: `board_rect` = the full render area (consistent with the perspective path's own margin handling), `scale_dots` = the camera's own carried value (not solved by a fit — the whole point of free-roam is the user controls framing directly), `screen_offset` = `(0, 0)` (no fit-centering needed; real camera position already determines what's on screen). This is the smallest addition that lets `FreeRoamCamera` render through the existing pipeline at all — it is not a reworking of the Orthographic/Perspective fit paths, which are untouched.
- No inspector exposure, no persistence, no bounds — confirmed out of scope (Purpose).

## Where the code lives
| Decision | Crate | Files |
|---|---|---|
| 0. `OrthographicCamera` rename + field strip | **Engine** | `crates/engine/render/src/camera.rs` |
| 1. `Camera` trait's new default methods | **Engine** | `crates/engine/render/src/camera.rs` |
| 1. `grid_line_color`'s elevation-threshold rewrite | **Game** | `crates/game/src/scenes/battle_viewer.rs` |
| 2. `AnyCamera` | **Engine** | `crates/engine/render/src/camera.rs` |
| 3. `BattleCamera` restructuring, preset constructors | **Game** | `crates/game/src/scenes/battle_viewer.rs` |
| 4. `FreeRoamCamera` + `nudge` | **Engine** | `crates/engine/render/src/camera.rs` |
| 5. Key `4` / movement keys / `board_geometry`'s FreeRoam arm | **Game** | `crates/game/src/scenes/battle_viewer.rs` |

No change touches `crates/engine`-forbidden content (per CLAUDE.md's hard invariants): `FreeRoamCamera`/`AnyCamera` carry zero board/piece/team knowledge, and the dev-tool wiring (keybindings, when free-roam is reachable) lives entirely in the game crate.

## Open Questions / TBDs
None — starting transform and movement step sizes are pinned in Decision 5. Confirmed with the project owner: no live toggle to flip a single preset between Orthographic/Perspective at the same position — preset-level switching (Decision 3, the existing `1`/`2`/`3`/`4` keys) already covers "compare projection kinds live," since Top-Down (Orthographic) and Sideline/Over-the-shoulder (Perspective) are already different presets a player can switch between with zero new code.

## Dependencies
- `41-battle-viewer-perspective-camera-rework` — `PerspectiveCamera`, `facing_sign`, and the whole per-position sizing apparatus this spec generalizes and exposes through the trait; `ObliqueCamera` (spec 39/41) is renamed and reduced per Decision 0. `SideView` is untouched beyond gaining default trait methods.
- `16-world-space-and-camera` ✅ — unrevised: world space stays 2D, `Camera` is still exactly two required pure functions of `WorldPos` (`project`/`depth_key`); this spec only adds *optional* (defaulted) methods alongside them, and `FreeRoamCamera`'s `height` is a camera-internal parameter (like `PerspectiveCamera::camera_height` already is), not a third `WorldPos` axis.
