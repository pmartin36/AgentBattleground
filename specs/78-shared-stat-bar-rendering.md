# UI — Shared Stat-Bar Rendering (game)

> **Status: pending.** Hoists the roster's stat-bar rendering out of `roster_manager` into a shared **game** module so the hatchery can show a hatched creature's stat bars with the exact same look, no fork. Today the four labeled stat bars (and their asymmetric-cap border chrome, `draw_dot_cap_box`) live inside `roster_manager::stat_bar`, coupled to `RosterManager`'s own state; nothing else can draw them. This extracts them into a stats-driven component both the roster and the hatchery call, and migrates the roster onto it with no visible change. Foundational for `79`'s hatched-creature stat display.

## Purpose
One stat-bar renderer, owned in one place, that draws a creature's four stat bars into a target rect from the creature's `Stats` — so the roster and the hatchery render identical bars instead of each keeping its own copy. This is the same shared-primitive rule the tooltip (`77`) and dot-border (`76`) specs follow, applied to the stat bars the user flagged the hatchery will need. `draw_dot_cap_box` stays game-side (its asymmetric top/bottom thickness is not expressible by the engine's uniform-thickness `rounded_rect`, per `76`), but it must be shared game code, not roster-private.

## Placement
A shared game module `crates/game/src/scenes/stat_bar/` (or `stat_bar.rs`), declared `pub(crate) mod stat_bar;` in `scenes/mod.rs`, sibling to `detail_panel` / `home_button`. It composes `engine_render` dot primitives and this game's `Stats`/`StatKind`; it is game code, not engine.

## Scope

### The shared component
Move the rendering-relevant parts of `roster_manager::stat_bar` into the shared module, decoupled from `RosterManager`:
- **`draw_dot_cap_box`** (the asymmetric-thickness chamfered cap border) moves here as the shared stat-bar chrome; it is no longer `roster_manager`-private.
- **Layout + labels:** `stat_slice_parts(rect) -> Vec<(Rect, Rect, Rect)>` (the four-slice geometry) and `stat_label(StatKind)` move here (both are already pure / near-pure).
- **The bar renderer** becomes stats-driven: a free function (not a `RosterManager` method) that takes `(buf: &mut Buffer, rect: DotRect, stats: &Stats, …)` and draws the four labeled, capped bars — the `STAT_BAR_COLOR` fill scaled against `STAT_DISPLAY_CAP`, the cap-box chrome, the labels — reading the stat values from the passed `Stats`, never from a scene's `self`.
- **Caller-controlled fill/opacity:** the renderer must serve both callers' animation without either forking it:
  - the **roster's** existing per-stat fill **interpolation** during a carousel slide (today `stat_fill_dots` lerps `prev -> current`), and
  - the **hatchery's** whole-bars **fade-in** as the stats dock slides back in (`79`).
  So the drawable fill amount (or a `0.0..=1.0` fill fraction per stat) and a whole-component **opacity** (`0.0..=1.0`, applied to every lit dot) are inputs. The roster passes its interpolated fills at full opacity; the hatchery passes full fills at a ramping opacity. Interpolation/fade **policy** stays with each caller; only the drawing is shared.

### Migrate the roster
Rebuild `roster_manager`'s stat bars on the shared component with **no visible change**:
- `RosterManager::render_stat_bars` becomes a thin caller: it computes its slide-interpolated per-stat fills (its existing `stat_fill_dots` logic, which stays roster-side because it reads roster's `slide`/`current_index`/`creatures`) and hands them, at full opacity, to the shared renderer.
- The roster's stat-bar tests (`stat_bar.rs`'s tests, and any layout/`render_stat_bars` assertions) must pass **unchanged**, proving byte-identical output. Repoint tests that reached now-moved `pub(super)` items at the shared module; do not weaken assertions to accommodate the move.

## Out of scope
- Any change to what the roster stat bars look like or how their carousel interpolation behaves. Extraction + byte-identical migration only.
- The stamina capsule and abilities section (already shared via `detail_panel`) — untouched.
- The hatchery's use of this component and its fade-in choreography — that is `79`.
- Engine changes: `draw_dot_cap_box` stays game-side (see `76`).

## Dependencies
- `roster_manager::stat_bar` — the code extracted; its tests gate the migration.
- `35-roster-screen-stats-abilities-squad`, `38-roster-screen-layout-corrections` — where the stat bars and their cap-box chrome originate; their behavior is preserved.
- `crate::stats::{Stats, StatKind}` — the stat data the renderer reads.
- `engine_render` dot primitives the bars are drawn with.
- Consumed by `79-hatchery-roster-style-layout` (the hatched creature's fading-in stat bars).
