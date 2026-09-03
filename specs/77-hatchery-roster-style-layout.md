# Hatchery — Roster-Style Layout & Hatch Hand-off

> **Status: pending.** Rearranges the hatchery browse/define surface to mirror the Roster Manager: the selected egg large on the **left**, a braille-bordered **right panel** carrying STATUS + DESCRIPTION + a single action button, and the egg **dock** along the bottom. Adds the hatch hand-off — on Hatch the egg slides to center and the right panel slides off, ceding the stage to the existing `68`/`72` reveal, then sliding back. Supersedes the center-egg / full-width-mad-lib **layout** of `75`; everything else `75` built (the mad-lib paragraph model, the browse/edit state machine, hover-vs-selected rings, the Done pipeline, the deleted modal) is reused unchanged. The disabled-Hatch tooltip uses the shared tooltip primitive from `76`.

## Purpose
`75`'s centered egg with a full-width mad-lib below reads poorly. The roster's left-sprite / right-panel / bottom-dock shape is this game's established visual language; the hatchery should use it. Selecting a dock egg brings it up large on the left and shows its status and description on the right, where the player fills the mad-lib and submits, or hatches.

## Scope

### Layout (mirror the roster)
- **LEFT (~2/3 width):** the selected egg, rendered large in the same slot proportion and position a roster creature occupies (roster's `left_w = area.width * 2 / 3`). This is the egg the dock has selected.
- **RIGHT (~1/3 width):** a panel bordered with the same braille dot chrome as the roster detail panel (grey `0x888888` via `draw_dot_border`), containing, top to bottom:
  1. **STATUS** row — one line keyed off the egg's state: `Awaiting Description` (Undefined), `Incubating — {Xh Ym remain}` (Incubating, formatted via `focus::format_remaining`), `Ready to Hatch` (Ready).
  2. **DESCRIPTION** — the mad-lib, using `75`'s paragraph model (`mad_lib_paragraph`): editable (blanks + blinking cursor) while Undefined; the completed sentence rendered read-only (all-`Literal` runs) while Incubating/Ready. It wraps exactly as `75` already does, now within the panel's narrower width.
  3. **Action button**, one per state:
     - *Undefined* → **Submit**, drawn disabled (idle-grey) until every blank is non-empty, active (gold) once fillable; clicking runs `75`'s Done pipeline (`70` → `71` → `66` → Incubating).
     - *Incubating* → **Hatch**, disabled, with a hover tooltip (via `76`'s tooltip primitive) reading `Hatching is available once incubation is complete.`
     - *Ready* → **Hatch**, active; clicking starts the hatch hand-off below.
- **BOTTOM:** the egg **dock** — the existing tray of every owned egg (`tray`), a stationary strip. No carousel and no left/right arrow nav (the roster carousel does not transfer). Dock eggs show `75`'s hover vs selected rings.
- **Top chrome:** keep the existing back-to-roster affordance. Match the roster's home/badge treatment only if it drops in trivially; not required.

### Interaction
- **Browse:** arrow keys / Tab / mouse hover move a *hover* ring across the dock; **Enter or click opens** the hovered egg (ring-on-hover, click-to-open — hovering never opens by itself). Opening fills the left slot and points the right panel at that egg. The *selected* ring (gold) wins over the *hover* ring (pale) when they land on the same egg (`75`'s single `egg_highlight` decision site).
- **Edit** (an Undefined egg is selected): the panel's DESCRIPTION is editable; Tab / Shift-Tab cycle blanks; the active blank blinks; Esc leaves edit back to browse. This is `75`'s edit machine (`HatcheryMode`, `blank_editors`), relocated into the panel body region.
- A fresh player's single Undefined egg auto-selects and opens editable, as `75` does.

### Hatch hand-off (Ready egg, Hatch pressed)
1. The selected egg slides from the left slot to screen **center**; at the same time the **right panel slides off** the screen. Reuse the roster `Slide` struct + `slide_offsets` + `Tween` pattern and suppress the dock during the animation, exactly as the roster suppresses its panel mid-slide.
2. Control hands to **`68`** (crack/break) and **`72`** (white-flash reveal, name fade-in, idle then starting attack, the stats dock, Keep/Discard) **as they render today**. This spec does **not** reposition `72`'s reveal into the right panel; it clears the browse layout so `72` has the stage.
3. On `72`'s completion (Keep adds the creature to the roster and removes the egg; Discard removes it), **slide back** to the browse layout with the next egg selected, or the empty state if none remain.

### Carried over from `75` unchanged
The mad-lib paragraph model (`mad_lib_paragraph`), the scene state model (`HatcheryMode` browse/edit, `selected`, `blank_editors`), the hover-vs-selected `TrayHighlight`, the Done pipeline, and the removal of the old modal. This spec changes the **arrangement** (center → left/right/dock) and **adds** the STATUS + action-button panel and the hatch hand-off animation.

## Out of scope
- **Egg art quality.** The current egg sprites look bad; a better egg-definition/art approach is a separate, later item (queue a `needs-research` note), explicitly not this spec.
- **The roster panel's stats/abilities body** (stamina bar + 2×2 ability grid). An unhatched egg has no revealed stats; the right panel is STATUS + DESCRIPTION + button only. `72` owns the post-hatch stats dock.
- **`68`'s crack/break and `72`'s reveal internals** — reused, not modified.

## Open Questions / TBDs
- **Shared chrome/slide promotion.** `draw_dot_border` and the `Slide` / `slide_offsets` pattern currently live inside `roster_manager` (`pub(super)`). Reusing them here should not fork a second copy. `draw_dot_border` is pure braille chrome with no game types and reads as `engine_render`-worthy; the `Slide`/`slide_offsets` helper is generic but layout-driven. **Owner decision:** promote `draw_dot_border` to `engine_render` and share the slide helper from a common location, vs. another placement. (Recommendation: promote `draw_dot_border` to `engine_render`; hoist the slide helper to a shared game-scene module — one owner, every caller inherits it.)
- **Left/right proportions.** Default to roster's 2/3 · 1/3. The egg's 11:14 aspect is narrower than a creature, so the left band may read sparse; tune against a render (possibly a slightly narrower left band). Tuning, not a blocker.
- **Panel-off slide timing/direction.** Default to the roster's `SLIDE_DUR` (300 ms) and slide the panel off to the right; adjust if it reads too fast or slow. Tuning.

## Dependencies
- `76-ui-tooltip-primitive` — the shared tooltip primitive the disabled Hatch button uses for its hover hint. Built first.
- `75-hatchery-inline-define-and-selection` — **superseded in part** (the center-egg / full-width layout). Its mad-lib paragraph model, state machine, hover/selected rings, and Done wiring are reused.
- `65-hatchery` — egg lifecycle, states, and 24-hour timer (unchanged); the dock is its tray.
- `68-hatchery-hatch-sequence` — the crack/break sequence the hatch hand-off yields to.
- `72-hatch-reveal-and-roster-placement` — the reveal, stats dock, Keep/Discard, and slide-back the hand-off yields to.
- `48-roster-detail-panel-redesign` — the panel border/chrome language the right panel mirrors (border only; not the stamina/abilities body).
- Reusable building blocks: `roster_manager::borders::draw_dot_border`, the roster `Slide` / `slide_offsets` pattern, `engine_render::{Button, ButtonState, TextEditor, flex, DotRect, Tween, label}`.
