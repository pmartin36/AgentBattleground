> # ✅ DONE! — Completed 2026-07-02

# Roster — Carousel

> **Status: implemented.** Replaces the abandoned "six slots side by side" design originally sketched for this scene (a simultaneous 6-column grid, all reusing the placeholder wizard sprite). The real design: one creature shown at a time, sliding left/right through the roster, with 6 distinct pieces of real art. `SceneId::ArmyEditor` is fully renamed to `SceneId::RosterManager` as part of this build. Depends on `21-mouse-hover-input`, `22-braille-ui-chrome`, and `23-piece-identity-data-model` — build/confirm those first.

## Purpose
Give the player's army a real, distinct-feeling home: page through your 6 pieces one at a time, each with its own name and art, navigable by mouse or keyboard, with a clear way back to the hub.

## Scope
- **Full rename: `SceneId::ArmyEditor` → `SceneId::RosterManager`.** Not a display-name-only change (that was the old plan) — the Rust variant itself renames, so `wire_name()` becomes `"RosterManager"` too. Every call site updates: `crates/scene-core/src/scene_id.rs` (`all()`, `display_name()`, `wire_name()`, `from_wire`, tests), `crates/game/src/registry.rs` (`construct`/`schema_for` match arms + tests), `crates/game/src/scenes/mod.rs` (`scene_for_digit`'s `'3' =>` arm), the scene struct itself (`army_editor.rs` → rename file/struct to `roster_manager.rs`/`RosterManager`), and `crates/game/src/cli.rs`'s `--scene` boot-flag matching. `display_name()` returns `"Roster"`.
- **One creature visible at a time**, centered in the scene: sprite (idle-animating), name label below it, 6 small position-indicator dots below the name (filled dot = current index, per `22-braille-ui-chrome`'s asset-based icon approach — small filled/unfilled circle raster images through `render::convert`, not Unicode bullets).
- **Left/right navigation, wrapping.** A left-arrow and right-arrow `Button` (per `22-braille-ui-chrome`) sit beside the creature, clickable; left/right arrow *keys* trigger the identical action. Index wraps: index 5 → right → index 0, index 0 → left → index 5.
- **Slide transition, screen-space only.** On navigating, the outgoing creature's sprite/name/dots slide fully off screen in the direction of travel while the incoming creature's slide in from the opposite edge, animated via the existing `Tween`/`ease_in_out` utility (`16-world-space-and-camera`) applied to each element's horizontal on-screen offset. This is **not** a world-space/camera transition — no `BoardGeometry`, no `SideView` camera, nothing from `BattleViewer`'s pipeline is involved; it's a self-contained 2D screen-space animation local to this scene, consistent with the open question flagged in `05-battle-viewer` about not over-building on the current world-space `Event` model.
- **Home button**, top-right, `Button` (per `22-braille-ui-chrome`) using a rounded-rect panel + house icon, hover/press tint. Clicking it returns a `Transition { target: SceneId::MainHub, params: None }` — `MainHub` already exists (as a placeholder-filled scene) and is a valid transition target today.
- **6 real, distinct creatures** — sourced per `23-piece-identity-data-model`'s Decisions (Ember Wolf, Frost Lizard, Stone Golem, Storm Hawk, Verdant Treant, Shadow Cat), each idle-animating.
- Reachable exactly as before: existing debug digit-key `3` (now mapping to `SceneId::RosterManager`) and `--scene RosterManager` CLI boot flag.

Out of scope:
- Real piece data beyond name + idle sprite (stats, abilities, skill files, upgrade history) — spec 03's fuller vision, entirely pending.
- Persistence / save-load of army composition or of "which index was last viewed."
- Any editing capability (rename, reorder, replace a piece) — this is read-only.
- Navigation *to* this scene via an in-game menu — that's the not-yet-written Main Hub Navigation spec, built after this one (and after this scene exists, since that menu will link here).
- Migrating `BattleViewer` to the new creature art/data model — untouched this round, per `23-piece-identity-data-model`.

## Decisions (v1)
- **Enum rename touches every call site listed above** — this is normal, compiler-guided Rust refactor friction for a closed-enum-per-scene architecture, not something worth introducing a codegen/macro layer to avoid for a ~6-site rename. (Flagged to the project owner as a judgment call, not a silent decision — the catalog itself is already centralized in `scene_id.rs`; what remains are mechanical, compiler-caught match arms.)
- **Wraparound is on** — the carousel has no "ends," index 0 and index 5 are adjacent.
- **Position indicator is 6 dots**, not a "3 of 6" text counter.
- **Both input modes are click and keyboard** — arrows are `Button`s (mouse) and also bound to left/right arrow keys (keyboard), firing the identical navigation action either way.
- **Only name + sprite are shown** — no stats/placeholder stat blocks reserved on screen for this build.
- **Slide direction matches navigation direction** — pressing/clicking "right" slides the current creature out to the left and the next one in from the right (i.e., the roster visually scrolls in the direction you're moving through it), matching the natural reading-order expectation of "next."

## Dependencies
- `13-rendering` ✅, `16-world-space-and-camera` ✅ (`Tween`/`ease_in_out` only — no camera/world-space usage).
- `21-mouse-hover-input` — click + hover events this scene's arrows/home button consume.
- `22-braille-ui-chrome` — the `Button` component this scene's arrows/home button/position-dot icons are built from.
- `23-piece-identity-data-model` — the 6 creatures (name + idle sprite) this scene displays.
- Feeds `03-army-skill-editing` — this remains army-*viewing*'s baseline; skill editing, stats, and abilities remain entirely pending there.
- Feeds the not-yet-written Main Hub Navigation spec — its eventual menu links here.
