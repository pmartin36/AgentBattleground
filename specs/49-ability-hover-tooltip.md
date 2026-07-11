# Ability Hover Tooltip

## Status

Pending. The overlay shown when the player hovers an ability in the roster detail panel (`48`). Bespoke game-side rendering (an overlay + pills), **not** reusable engine primitives — per owner, underline/pill/tooltip are one-off for this game.

## Purpose

Give each ability an at-a-glance stat card on hover: its stamina cost, a row of category pills (type / element / class), the numeric combat fields, and flavor text. Reads the `Ability` fields added in spec 47; shows only what a given ability defines.

## Reference Mock

Player sketch (a small bordered card): centered **"Cost: x stamina"**, a row of **three rounded pills**, then **"Damage: N"** / **"Status: …"** lines, a gap, and a **flavor-text** block at the bottom.

## Positioning

- The tooltip appears **up and to the left** of the hovered ability: its **bottom-right corner sits just above-left of the ability's top-left cell**.
- **No repositioning / clamping logic** this pass — the abilities live in the center-right of the screen, so the card never reaches the top or left edge (owner-confirmed). (If a future layout moves abilities near an edge, revisit; noted, not built.)
- Only one tooltip at a time — the ability currently in `hovered_ability` (spec 48). It is **transient**: shown while hovered, **vanishes on mouse-out**, no click needed.

## Rendering

- The card is a **centered-content overlay** built on the `BattleMenu` pattern: a chamfered frame via `engine_render::ui_primitives::rounded_rect(w, h, thickness, corner_radius, BORDER_COLOR, Dot::Occlude)` — the `Dot::Occlude` fill **erases the panel content beneath** so text underneath doesn't bleed through. Drawn after the panel, before nothing else (topmost).
- Card **width is fixed** (`TOOLTIP_WIDTH_CELLS`, tunable); **height = the sum of the present rows** (absent fields take no space).
- Interior has a **1-cell padding**. Content rows top-to-bottom (each omitted entirely when its data is absent):

  1. **Cost** — `"Cost: {n} stamina"`, **centered**. Omitted if `cost` is `None`.
  2. **Pill row** — the present pills among `ability_type`, `element`, `class`, laid **horizontally in a row**, the row centered. Each pill omitted if its field is `None`; if all three are `None` the row is skipped.
  3. **Damage / Range line** — **two columns on one line**: `"Damage: {n}"` in the left column, `"Range: {n}"` in the right. Each half omitted if its field is `None` (if both `None`, the line is skipped; if one present, only that column shows).
  4. **Status line(s)** — `"Status:"` followed by the effect names laid out in **two columns** (effect 0 | effect 1 / effect 2 | effect 3 …). Skipped if `status_effects` is empty.
  5. **1-cell gap.**
  6. **Flavor** — `flavor` text via the engine `wrapped_text` helper (spec 52), word-wrapped to the interior width and clipped to **2 rows** with a tail-ellipsis if longer. Skipped if `None`.

- Text is plain terminal characters (centered/aligned rows via the spec-52 `label`, flavor via `wrapped_text`); the frame and pills go through the dot pipeline (rule 4).

## Pills

- A pill is a **rounded capsule** — visibly **rounder than the standard chamfer**: 1 text-line tall, with a **large corner radius** so the two ends read as rounded caps (still braille chamfer geometry, just a deeper cut). Pin the exact radius/height as constants; the owner will eyeball and iterate.
- Label = the field's `label()` (spec 47), centered in the capsule.
- **Color-coded by value** (tint of the capsule border/fill), starter palette (tunable):
  - Element — Fire = orange, Water = blue, Earth = green, Lightning = yellow, Normal = grey.
  - Type — Attack = red, Buff = green, Debuff = purple.
  - Class — Physical = tan, Magic = violet.
- Bespoke game rendering (a small `pill(buf, rect, text, color)` helper in the roster scene), not an engine primitive.

## Scene State & Input

- The tooltip is a pure function of `hovered_ability` + the current creature's ability data — no extra persistent state beyond what spec 48 already tracks. It is rendered in the roster `render` pass **after** the details panel when `hovered_ability.is_some()` and no modal (spec 51) is open.
- Dismissal is automatic: when `hovered_ability` becomes `None` (mouse-out, handled in spec 48), nothing is drawn.

## Decisions (v1)

- Positioned up-left of the hovered ability, no clamping (abilities are center-right).
- Fixed width, content-driven height; absent fields consume no space.
- Row order: Cost → pills → Damage/Range (2-col) → Status (2-col) → gap → flavor (≤2 lines).
- Pills are deep-radius capsules, color-coded per the palette above.
- Overlay erases content beneath via `Dot::Occlude`; transient, hover-only.

## Constants (placeholders — tunable)

- `TOOLTIP_WIDTH_CELLS`, interior padding = 1 cell.
- `PILL_CORNER_RADIUS_DOTS` (deep), `PILL_HEIGHT` (1 text line), inter-pill gap.
- Palette RGBs per the pill list above.

## Testing Guidance

- With an ability defining every field, the card renders every row in order; assert row count/content.
- With an ability whose `cost`/`range`/`flavor` are `None` and `status_effects` empty, those rows are **absent** and the card is correspondingly shorter (assert height shrinks and the skipped strings don't appear).
- Damage/Range render as two columns on one line; a Range-only ability shows only the right column.
- Pill color matches the value (decode the pill's lit-dot color for, e.g., Fire = orange).
- The card's bottom-right sits above-left of the hovered ability's top-left (assert the anchor offset).
- Occlusion: a cell under the card shows the card, not the panel text beneath (decode the rendered cell).

## Open Questions / TBDs

None outstanding. Palette + pill radius are explicitly iterate-later.

## Dependencies

- Needs `47-ability-and-instructions-data-model` (fields + `label()`) and `48-roster-detail-panel-redesign` (`hovered_ability`, ability rects).
- Needs `52-engine-text-rendering` for the aligned/centered text rows and the wrapped flavor block.
- Reuses `ui_primitives::rounded_rect` + `Dot::Occlude` (`13-rendering` ✅, `22-braille-ui-chrome` ✅) and the `BattleMenu` overlay pattern.
