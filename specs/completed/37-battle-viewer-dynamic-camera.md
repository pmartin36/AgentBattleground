> # ✅ DONE! — Completed 2026-07-08
> Status: implemented (mechanism). The three-camera-mode `BattleCamera` enum, key 1/2/3 direct-selection switching, camera-dependent grid-line prominence, and full removal of the global digit-key scene-switcher are all in place and tested. The camera *formulas* this spec originally shipped (`TopDownView`/`OverShoulderView`) were later replaced by `39-battle-viewer-camera-perspective-rework`'s unified `ObliqueCamera`; that formula rework — and its own flagged issues — are `39`'s concern, not this spec's. `cargo test --workspace` green.

# Battle Viewer — Dynamic Camera

## Purpose
The battlefield has used one fixed side-on camera since `18-battle-viewer-baseline`. This spec gives the player three switchable views — over-the-shoulder, sideline, and top-down — reusing `16-world-space-and-camera`'s existing design for exactly this ("supporting several angles = supplying several `(project, depth_key)` pairs over the same world positions"). Builds on `36-battle-viewer-squad-layout`'s new 7×7/3v3+bench geometry; changes no gameplay data, only how it's projected to screen.

## Scope
- Three `Camera` implementations: **over-the-shoulder**, **sideline**, **top-down**. Sideline is `18`'s existing `SideView`, kept as one of the three options rather than replaced.
- **Stylized, not physically-accurate perspective.** Over-the-shoulder does NOT implement true foreshortening/vanishing-point perspective this round — a cheaper approximation (e.g. a repositioned/angled variant built from the same orthographic-style primitives `SideView` already uses) is acceptable. True perspective projection is an explicit future upgrade if the stylized version doesn't read well, not attempted here.
- **Switching via keys 1/2/3**, captured locally by `BattleViewer::handle_input` — no mouse/UI control this round (keyboard only, per the project owner). The active camera does not persist across sessions or re-entering the scene; it resets to a default on `enter()`.
- **Grid-line prominence varies by camera**: faint in over-the-shoulder and sideline, clearly visible in top-down. `draw_board_lines` (or its successor) takes the active camera/view into account for line opacity/color rather than always using `GRID_LINE_COLOR` at full strength.
- **Removes the global digit-key scene-switcher entirely**: `crate::scenes::scene_for_digit` and its interception in `app.rs::handle_key` are deleted. Keys 1/2/3 are freed for Battle-Viewer-local camera switching; there is no replacement global digit shortcut for any scene. Real navigation (Main Hub's menu from `25-main-hub-navigation`, Roster's arrows/home from `24-roster-carousel`) already covers getting to every scene without it.

Out of scope:
- Mouse/click camera controls, or any UI affordance (e.g. a camera-icon button) for switching views.
- Camera persistence (per-player saved preference) — out of scope until there's a settings/save system to persist it in.
- True perspective projection (see *Stylized, not physically-accurate* above) — flagged as a possible future spec, not attempted now.
- Any change to piece data, event playback, or squad layout — this spec is purely the projection/view layer on top of `36`'s geometry.

## Decisions (v1)
- **New `BattleCamera` enum** (name TBD at implementation) wrapping the three view variants, each implementing `engine_render::camera::Camera` (`project`/`depth_key`), consumed by `board_geometry`/`composite_scene` exactly as `SideView` is today — no change to the `Camera` trait itself, since `16` already designed it to support exactly this kind of swap.
  - **Sideline** = today's `SideView`, unchanged.
  - **Top-down**: a projection where `depth_key` derives primarily from world `y` as before (so far-vs-near ordering along the board's length still makes sense), but framed/scaled to read as looking straight down — e.g. board width and "depth" (today's y-axis) both represented as flat plan-view distances, no vertical foreshortening.
  - **Over-the-shoulder**: positioned/angled from behind one team's back row, still a `project`/`depth_key` pair over the same world positions — no true perspective (see Scope).
- **`BattleViewer` gains a `camera_mode` field**, defaulting on `enter()` to one fixed starting view (implementation call — e.g. sideline, since it matches today's default experience most closely). `handle_input` matches `KeyCode::Char('1'|'2'|'3')` to set it directly (no wrapping/cycling ambiguity — each digit is a direct selection, not a next/prev step).
- **Grid-line opacity/color is a per-camera-mode parameter** passed into the board-line-drawing function, e.g. a dimmed variant of `GRID_LINE_COLOR` for over-the-shoulder/sideline vs. full `GRID_LINE_COLOR` for top-down — reuses the existing dot/line-drawing mechanism, just parameterizes its color rather than hardcoding `GRID_LINE_COLOR` directly.
- **Global digit-switcher removal is in-scope here** (not a separate spec) because this spec is what creates the key collision that makes removal necessary — `app.rs::handle_key`'s digit interception and `scenes::scene_for_digit` are deleted; the corresponding tests in `app.rs`'s `#[cfg(test)] mod tests` (the digit-hotkey dispatch block) are removed, not left disabled.

## Open Questions / TBDs
- Exact "default camera on enter" choice (sideline vs. top-down vs. over-the-shoulder) — implementation call, low stakes.
- Whether a future settings/save system should persist the last-used camera per player (deferred, no such system exists yet).
- Whether true perspective projection is ever warranted for over-the-shoulder (explicitly punted — "cross that bridge when we get there").

## Dependencies
- `16-world-space-and-camera` ✅ — the `Camera` trait this spec's three implementations satisfy; explicitly designed to support multiple angles over the same world positions.
- `36-battle-viewer-squad-layout` — the 7×7/3v3+bench geometry these cameras render.
- `33-scene-composite-primitive` ✅ — `composite_scene` is generic over any `Camera` impl already; no change needed there.
- Removes/supersedes the global digit-switcher built in `14-scene-architecture`/wired in `app.rs` — `25-main-hub-navigation` and `24-roster-carousel` already provide the real navigation paths that make it redundant.
