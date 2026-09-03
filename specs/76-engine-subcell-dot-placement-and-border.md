# Engine — Sub-Cell Dot Placement & Shared Dot Border

> **Status: pending.** Adds the one rendering capability the engine is missing — placing a `DotBuffer` into a `Buffer` at **sub-cell `DotRect` precision** — and a shared `draw_dot_border` built on it, then migrates the roster panel's border onto the engine primitive. Today that sub-cell placement lives only inside `roster_manager::borders::draw_dot_border` (`pub(super)`); the engine's own `draw_dots` floors to whole cells. This lifts the higher-fidelity capability into `engine_render` so the roster keeps exactly what works today, the roster's local border rasterizer duplicate is deleted, and new callers (the tooltip primitive `77`, the hatchery panel `79`) get the same dot-precise border. Foundational for both.

## Purpose
Draw a braille border at the exact dot position it was computed at, not floored to the nearest cell. The roster panel already does this (its border must line up dot-for-dot with the stat-bar border — flooring caused a shipped "border 2 dots off" bug). That capability belongs in the engine, where every scene inherits it, instead of trapped in one scene as a helper the next scene would have to copy. This is the standing rule: improve the shared primitive rather than recreate it per caller, and never downgrade a working element to fit what the engine happens to expose today.

## Background: what exists, what's missing
- `engine_render::ui_primitives::rounded_rect(w, h, thickness, corner_radius, border, fill: Dot)` already rasterizes a chamfered border ring with a caller-chosen interior (`Dot::Transparent` hollow, `Dot::Occlude` masking, `Dot::Lit` filled) into a `DotBuffer`. At `thickness = 1, corner_radius = 1, fill = Transparent` it drops exactly the four outermost corner dots and lights the rest of the 1-dot perimeter — **byte-identical** to the roster's local `draw_dot_box`. The engine already owns the rasterizer.
- `engine_render::draw_dots(buf, area: Rect, &DotBuffer)` places a `DotBuffer` into a `Buffer`, but takes a **cell** `Rect` — a caller with a dot-precise `DotRect` must `.to_cell_rect()` first, which floors and loses sub-cell position.
- `roster_manager::borders::draw_dot_border(buf, rect: DotRect, color)` is the only code that places at dot precision: it offsets the raw dots by `rect.cell_remainder()` into a buffer sized to include the remainder and converts once (`dots_to_grid` → `draw_grid`). It also reimplements the rasterization (`draw_dot_box`) instead of using `rounded_rect` — a duplicate of what `rounded_rect` already does.

So the only missing engine capability is the **sub-cell placement**; the rasterizer is already there.

## Scope
- **`engine_render::draw_dots_at(buf: &mut Buffer, dot_rect: DotRect, dots: &DotBuffer)`** — place `dots` into `buf` honoring `dot_rect`'s position at dot precision, using the `cell_remainder()` offset technique currently inside the roster's `draw_dot_border` (offset the raw dots into a buffer sized `(w + dx, h + dy)`, convert once, draw at the floored cell origin). A pure addition; the existing cell-`Rect` `draw_dots` is unchanged and stays the right tool for cell-aligned callers. No-ops on a zero-size rect; never panics.
- **`engine_render::draw_dot_border(buf, dot_rect: DotRect, thickness, corner_radius, color)`** — the shared chamfered-border helper: `rounded_rect(dot_rect.w, dot_rect.h, thickness, corner_radius, color, Dot::Transparent)` placed via `draw_dots_at`. This is the roster's `draw_dot_border` behavior, now engine-owned and composed from the existing rasterizer + the new placement. Interior stays transparent (content behind the ring is preserved).
- **Migrate the roster** onto the engine primitive: `roster_manager::borders::draw_dot_border` becomes a thin call to `engine_render::draw_dot_border` (or is removed in favor of direct calls), and the local `draw_dot_box` uniform-border rasterizer is deleted. The roster's existing border tests (`borders.rs`'s `draw_dot_border_tests` and `details_panel_border_tests`) decode the actual rendered dots — edge cells painted, the corner's bit-0 chamfer clipped, a mid-edge dot kept, the panel perimeter and margins — and must pass **unchanged**, proving the migration is byte-identical. If any assertion moves, the migration is wrong, not the test.

## Out of scope
- **`draw_dot_cap_box`** (the stat bars' asymmetric top/bottom thickness) stays game-side; `rounded_rect` takes a single uniform thickness and cannot express it. It is a separate concern handled by `78-shared-stat-bar-rendering`: the stat-bar rendering it backs is hoisted into a shared **game** module there (so the roster and the hatchery's hatched-creature stats share one copy), not here.
- Any visual change to the roster border. This is extraction + a byte-identical migration, not a redesign.
- `rounded_rect` itself is unchanged (no new thickness/radius behavior); this spec only adds placement and a border helper that composes it.

## Dependencies
- `38-roster-screen-layout-corrections`, `48-roster-detail-panel-redesign` — the origin of `draw_dot_border` and the border tests that gate this migration; those tests are the correctness proof.
- `engine_render::ui_primitives::rounded_rect`, `dots::{DotBuffer, draw_dots, dots_to_grid}`, `grid::draw_grid`, `DotRect::{to_cell_rect, cell_remainder}` — the existing pieces composed here.
- Consumed by `77-ui-tooltip-primitive` (its card frame) and `79-hatchery-roster-style-layout` (its right panel border).
