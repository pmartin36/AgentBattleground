# Hatchery — Roster-Style Layout & Hatch Hand-off

> **Status: pending.** Rearranges the hatchery browse/define surface to mirror the Roster Manager: the selected egg large on the **left**, a braille-bordered **right panel** carrying STATUS + DESCRIPTION + a single action button, and the egg **dock** along the bottom. Adds the hatch hand-off: on Hatch the egg slides to center and the right panel slides off, ceding the stage to the existing `68`/`72` reveal (which settles the creature into the left slot), with the hatched creature's stat bars fading in as `72`'s dock settles; after Keep/Discard the browse layout returns immediately. Supersedes the center-egg / full-width-mad-lib **layout** of `75` (everything else `75` built — the mad-lib paragraph model, the browse/edit state machine, hover-vs-selected rings, the Done pipeline, the deleted modal — is reused unchanged), and supersedes in part `72`'s settled view (it gains the stat bars). Right panel border via `76`, disabled-Hatch tooltip via `77`, stat bars via `78`.

## Purpose
`75`'s centered egg with a full-width mad-lib below reads poorly. The roster's left-sprite / right-panel / bottom-dock shape is this game's established visual language; the hatchery should use it. Selecting a dock egg brings it up large on the left and shows its status and description on the right, where the player fills the mad-lib and submits, or hatches.

## Scope

### Layout (mirror the roster)
- **LEFT (~2/3 width):** the selected egg, rendered large in the same slot proportion and position a roster creature occupies — the **2:1 left/right split** the hatchery's own `hatch_layout::slide_pose` already uses (and the roster's `left_w = area.width * 2 / 3`). This is the egg the dock has selected.
- **RIGHT (~1/3 width):** a panel bordered with the same braille dot chrome as the roster detail panel (grey `0x888888`), drawn via `76`'s shared `engine_render::draw_dot_border(buf, panel_rect, 1, 1, color)` — the dot-precise border migrated out of the roster, so the hatchery panel is the same border the roster uses, not a lower-fidelity copy. Panel top to bottom:
  1. **STATUS** row — one line keyed off the egg's state: `Awaiting Description` (Undefined); `Incubating — {H}h {M}m remaining` (Incubating), where `{H}` and `{M}` are the whole hours and minutes of the remaining `Duration` via a small local helper (seconds omitted — this is a multi-hour countdown, deliberately **not** the focus view's `focus::format_remaining` `HH:MM:SS` readout); `Ready to Hatch` (Ready).
  2. **DESCRIPTION** — the mad-lib, using `75`'s paragraph model (`mad_lib_paragraph`): editable (blanks + blinking cursor) while Undefined; the completed sentence rendered read-only (all-`Literal` runs) while Incubating/Ready. It wraps exactly as `75` already does, now within the panel's narrower width.
  3. **Action button**, one per state:
     - *Undefined* → **Submit**, drawn disabled (idle-grey) until every blank is non-empty, active (gold) once fillable; clicking runs `75`'s Done pipeline (`70` → `71` → `66` → Incubating).
     - *Incubating* → **Hatch**, disabled, with a hover tooltip (via `77`'s tooltip primitive) reading `Hatching is available once incubation is complete.`
     - *Ready* → **Hatch**, active; clicking starts the hatch hand-off below.
- **BOTTOM:** the egg **dock** — the existing tray of every owned egg (`tray`), a stationary strip. No carousel and no left/right arrow nav (the roster carousel does not transfer). Dock eggs show `75`'s hover vs selected rings.
- **Top chrome:** keep the existing back-to-roster affordance. Match the roster's home/badge treatment only if it drops in trivially; not required.

### Interaction
- **Browse:** arrow keys / Tab / mouse hover move a *hover* ring across the dock; **Enter or click opens** the hovered egg (ring-on-hover, click-to-open — hovering never opens by itself). Opening fills the left slot and points the right panel at that egg. The *selected* ring (gold) wins over the *hover* ring (pale) when they land on the same egg (`75`'s single `egg_highlight` decision site).
- **Edit** (an Undefined egg is selected): the panel's DESCRIPTION is editable; Tab / Shift-Tab cycle blanks; the active blank blinks; Esc leaves edit back to browse. This is `75`'s edit machine (`HatcheryMode`, `blank_editors`), relocated into the panel body region.
- A fresh player's single Undefined egg auto-selects and opens editable, as `75` does.

### Hatch hand-off (Ready egg, Hatch pressed)
1. The selected egg slides from the left slot to screen **center** while the **right panel slides off** to the right. Animate both with the engine's `DotRectTween` (`from` = the resting rect, `to` = the off-screen / centered rect), following the hatchery's own `hatch_layout::slide_pose` pattern (no roster `Slide` copy); suppress the dock during the animation. Duration matches the hatchery's existing slide. The browse layout must expose the egg's and panel's resting rects as unfloored `DotRect`s (see Implementation placement) so this tween is dot-precise, never round-tripped through a cell `Rect` (CLAUDE.md rule 5).
2. Control hands to **`68`** (crack/break) and **`72`** (white-flash reveal, name fade-in, idle then starting attack, the stats dock, Keep/Discard) **as they render today**. This spec does **not** reposition `72`'s reveal into the right panel; it clears the browse layout so `72` has the stage.
3. **Stat bars fade in** as `72`'s stats dock slides in. The hatched creature's four stat bars — the roster's, drawn via `78`'s shared `draw_stat_bars` — appear above the creature on the left (the roster-detail arrangement), and **fade in** by passing `78`'s opacity parameter a `0.0 -> 1.0` ramp driven by the settle Slide's progress (the same window the creature and dock move in). It is a smooth opacity fade of the same visual character as `72`'s name fade-in; it does **not** need to reuse `72`'s Beat-phase name-fade code. This **supersedes in part** `72`: its settled hatched view gains the stat bars; the rest of `72`'s reveal, the stamina/abilities dock, and Keep/Discard are unchanged.
4. On `72`'s completion (Keep adds the creature to the roster and removes the egg; Discard removes it), the scene **re-enters the browse layout immediately** — no animated slide, since the only animated transitions are the step-1 hatch-out and `72`'s own settle — with the nearest remaining egg selected. If no eggs remain, the left slot keeps showing the just-hatched creature (idle, read-only, its stat bars still shown at full opacity) with its name in the right panel and no action button, until the player leaves the scene — the empty-dock state. This empty-dock view **reuses the existing read-only settled layout** (`72`/`hatch_layout`'s creature and stat-zone rects plus `78`'s `draw_stat_bars`); it introduces no new geometry. (Session-scoped; egg acquisition stays TBD in `65`.)

### Carried over from `75` unchanged
The mad-lib paragraph model (`mad_lib_paragraph`), the scene state model (`HatcheryMode` browse/edit, `selected`, `blank_editors`), the hover-vs-selected `TrayHighlight`, the Done pipeline, and the removal of the old modal. This spec changes the **arrangement** (center → left/right/dock) and **adds** the STATUS + action-button panel and the hatch hand-off animation.

## Out of scope
- **Egg art quality.** The current egg sprites look bad; a better egg-definition/art approach is a separate, later item (queue a `needs-research` note), explicitly not this spec.
- **The pre-hatch egg panel showing stats.** An *unhatched egg* has no revealed stats; while browsing/defining an egg the right panel is STATUS + DESCRIPTION + button only. Stat bars appear only *after* hatch, on the settled creature (Hatch hand-off step 3). The stamina/abilities dock is `72`'s and is reused, not redesigned here.
- **`68`'s crack/break and `72`'s reveal internals** — reused, not modified.

## Decisions (settled)
- **Left/right split: 2/3 egg · 1/3 panel**, matching the roster and the split `hatch_layout::slide_pose` already uses. The egg is narrower than a creature (11:14) so it will center in its band with some margin; that is acceptable and can be nudged at render time without a spec change.
- **Panel-off slide: off to the right, duration = the hatchery's existing slide** (`slide_pose`'s curve), via `DotRectTween`.
- **Return to browse after Keep/Discard is immediate**, not an animated slide. The only animated transitions are the step-1 hatch-out (egg to center, panel off) and `72`'s own settle slide.
- **Empty dock: keep the just-hatched creature shown** (idle, read-only, stat bars at full opacity) until the player leaves, reusing the read-only settled layout — Hatch hand-off step 4.
- **STATUS Incubating string:** `Incubating — {H}h {M}m remaining`, whole hours and minutes from the remaining `Duration` (not the focus view's `HH:MM:SS`).

## Implementation placement
`hatchery/mod.rs` is already near the file-size budget (~820 / 1000 lines) and must not go over it. New work lands in submodules, not `mod.rs`:
- A new `browse_layout.rs` owns the left-slot, right-panel, and dock geometry, and **exposes the egg and panel resting rects as unfloored `DotRect`s** (so step 1's `DotRectTween` is dot-precise per CLAUDE.md rule 5).
- The panel contents (STATUS row, DESCRIPTION via `mad_lib_paragraph`, action button) render from a panel submodule; the browse/edit interaction reuses `75`'s existing `edit.rs` / `detail.rs` / `selection.rs`; the hatch-hand-off animation and the settled/empty-dock render reuse `hatch_layout.rs` / `hatch.rs`.
- `mod.rs` gets only thin hooks: input-routing dispatch and render dispatch that call into the submodules. If the necessary `mod.rs` wiring would push it past ~950 lines, split the input-routing or render dispatch into a submodule **first**, before adding more. No task may take `mod.rs` over 1000 lines.

## Dependencies
- `76-engine-subcell-dot-placement-and-border` — the dot-precise `engine_render::draw_dot_border` the right panel is drawn with. Built first.
- `77-ui-tooltip-primitive` — the shared tooltip primitive the disabled Hatch button uses for its hover hint. Built first.
- `78-shared-stat-bar-rendering` — the shared stat-bar renderer the hatched creature's fading-in stat bars use. Built first.
- `75-hatchery-inline-define-and-selection` — **superseded in part** (the center-egg / full-width layout). Its mad-lib paragraph model, state machine, hover/selected rings, and Done wiring are reused.
- `65-hatchery` — egg lifecycle, states, and 24-hour timer (unchanged); the dock is its tray.
- `68-hatchery-hatch-sequence` — the crack/break sequence the hatch hand-off yields to.
- `72-hatch-reveal-and-roster-placement` — the reveal, stats dock, Keep/Discard, and slide-back the hand-off yields to; `hatch_layout::slide_pose` is the slide pattern this spec's hand-off follows. **Superseded in part:** the settled hatched view gains the fading-in stat bars (Hatch hand-off step 3).
- `48-roster-detail-panel-redesign` — the panel border/chrome language the right panel mirrors (border only; not the stamina/abilities body).
- Reusable building blocks: `engine_render::{draw_dot_border (via 76), DotRectTween, Button, ButtonState, TextEditor, flex, DotRect, label}`, the shared stat-bar renderer (via `78`), and `hatch_layout::slide_pose`.
