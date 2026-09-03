# UI — Shared Tooltip Primitive (game)

> **Status: pending.** Extracts the hover-tooltip card into a **reusable game primitive** and migrates the existing roster ability tooltip onto it. Today the only tooltip in the codebase lives inside `roster_manager::tooltip` (`pub(super)`, `Ability`-coupled): its generic half (anchor placement, screen-edge clamping, the chamfered occluding frame, stacked row layout) is trapped in that scene and cannot serve any other caller. This spec promotes that generic half to a shared game module every scene can call, and rebuilds the ability tooltip on top of it with no visible change. Foundational for `79` (the hatchery's disabled-Hatch hover hint) and any future tooltip.

## Purpose
One tooltip mechanism, owned in one place, that any scene can point at an on-screen anchor to pop a bordered card. The primitive owns the parts that are the same for every tooltip; each caller supplies only its own content. The rule this serves is the standing one: a mechanism every scene would otherwise reinvent belongs where every caller inherits it, not copied per scene (`CLAUDE.md` engine/game boundary + the "put the fix where every caller inherits it" rule).

## Placement
A shared game module `crates/game/src/scenes/tooltip/`, declared `pub(crate) mod tooltip;` in `scenes/mod.rs`, sibling to the existing shared UI modules (`detail_panel`, `home_button`, `close_button`, `bars`). It is a **game** primitive, not an engine one: it composes `engine_render` dot/flex/label primitives but carries this game's tooltip styling defaults. (No `crates/engine/` change; the engine/game split is unaffected.)

## Scope

### The primitive
Lift the generic shell out of `roster_manager::tooltip::shell` (its `ShellRow`, `layout_shell`, `draw_shell_frame`) into the shared module, and give it a small public surface:
- **Anchor placement + clamping:** given an anchor `DotRect` (the hovered element's cell) and a set of row specs, compute the card's outer `DotRect` (fixed width, content-driven height) and each row's absolute `DotRect`, anchored off the element and clamped to the screen so the card never runs off an edge. This is `layout_shell`'s existing behavior, generalized and made `pub(crate)`.
- **Frame:** draw the chamfered (chamfer-1), border-ringed, `Occlude`-filled card frame into a `Buffer`, returning whether anything was drawn (a zero-area card draws nothing and does not panic). This is `draw_shell_frame`, generalized.
- **Row model:** a caller supplies `&[RowSpec { height_cells, gap_above_cells }]` (the current `ShellRow`) and, after layout, fills each returned row rect itself. The primitive does not know what a row contains.
- **Config:** shared default constants carry this game's tooltip look (amber border `0xffbf00`, chamfer 1, the interior padding the roster card uses today, corner radius 1); the roster caller keeps passing those same defaults so its card is unchanged by construction. Card **width** is a per-call parameter (the roster passes its current `TOOLTIP_WIDTH_CELLS`; the plain-text helper below sizes its own). No caller forks the frame code.
- **Plain-text convenience:** a helper for the common case of a tooltip that is just a wrapped message string: given an anchor and a `&str` (and a max width), it measures the wrapped text, builds the single text row, lays out and draws the card, and renders the text. This is what `79`'s disabled-Hatch tooltip needs, and what most future tooltips will use. Word-wrap and clip via `engine_render::wrapped_text`.

**Entry-point shape (decided):** free functions mirroring the current shell — `layout(anchor, &[RowSpec], width) -> {card, rows}`, `draw_frame(buf, card) -> bool`, and the `render_text(buf, anchor, msg, max_width)` convenience — not a builder. This migrates the roster tooltip with the least churn (its `layout_tooltip`/`render_tooltip` already call the shell as free functions).

### Migrate the existing tooltip
Rebuild `roster_manager::tooltip` on the primitive with **no behavioral or visual change**:
- The ability-specific content stays roster-side: `TooltipRow`, `present_rows(ability)`, `row_height_cells`, and the per-row fillers (`fill_cost`/`fill_pills`/`fill_damage_range`/`fill_status`/`fill_flavor`), plus `pills` and `palette`.
- The shell, anchor math, clamping, and frame now come from the primitive: delete the local `shell.rs`; `layout_tooltip`/`render_tooltip` build their `RowSpec`s (from `present_rows` + `row_height_cells` + the pre-flavor gap) and call the primitive to place, frame, and hand back row rects, then fill each rect exactly as now.
- The roster tooltip's existing tests (`tooltip/mod.rs` unit tests, `tooltip_integration_tests.rs`, `ability_hover_tests.rs`) must still pass unchanged — the migration is a refactor behind a stable observable result (same card geometry, same anchoring, same occlusion, same row content). Where a test reaches a now-moved `pub(super)` shell item directly, repoint it at the primitive; do not weaken an assertion to accommodate the move.

## Out of scope
- Any change to what the ability tooltip shows or how it looks. This is extraction + migration, not a redesign.
- New tooltip callers other than proving the plain-text path (the hatchery caller is `79`). Do not wire the primitive into scenes beyond the roster migration here.
- The diagnostics/lint warning card and `[!!]` badge in the prompt editor — a different overlay, not a hover tooltip; untouched.

## Dependencies
- `48-roster-detail-panel-redesign`, `49` (ability tooltip) — the existing tooltip whose generic half is extracted and whose ability card is migrated; its tests are the migration's correctness gate.
- `engine_render` — the `flex`, `label`, `wrapped_text`, `DotRect`, and dot-frame primitives the tooltip composes (unchanged).
- Consumed by `79-hatchery-roster-style-layout` (the disabled-Hatch hover hint) and available to any future scene.
