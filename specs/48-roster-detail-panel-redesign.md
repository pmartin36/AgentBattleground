# Roster Detail Panel Redesign

## Status

Pending. Replaces the contents of the roster manager's right-hand details panel. Same footprint as today (right third of the screen, existing procedural dot border) — only the interior changes.

## Purpose

Today the details panel shows a single stamina line plus a flat, top-down ability list (`crates/game/src/scenes/roster_manager/details_panel.rs`). This spec fills it out into the real creature dashboard: a centered stamina bar, a 2×2 grid of hoverable abilities, and a scrollable-in-a-popup battle-instructions preview with an Edit button. The ability tooltip (`49`) and the prompt editor popup (`51`) are the two overlays this panel launches; they are specified separately.

## Reference Mock

Player sketch (top-to-bottom, inside the existing bordered right-third panel):

1. Centered **"Stamina"** label + a single-line bar.
2. blank space.
3. **"Abilities"** header with a braille underline, then a 2×2 grid of ability names (each terminal-underlined).
4. blank space.
5. **"Instructions"** header (braille underline) on the left, an **[Edit]** button on the right, then a multi-line preview of the creature's battle instructions filling the rest of the panel.

## Layout

Reuse the existing panel geometry (`layout.rs::right_col_dots` / `details_panel_rects`) for the outer border and interior rect; re-carve the **interior** (inside the 1-cell inset already applied) into the regions below with a Column `flex` over `DotRect`. All chrome (bar, underlines, button panel, tooltip) goes through the dot pipeline per CLAUDE.md rule 4; text (labels, ability names, headers, instructions) stays plain terminal characters.

Top-to-bottom, with **1 cell of margin at the very top** of the interior:

### 1. Stamina row
- A centered **"Stamina"** label + single-line bar on the same row. Reuse the post-battle bar helper (`crates/game/src/scenes/post_battle/bars.rs::draw_labeled_bar`) — reference it directly if visibility is widened, otherwise lift the two `draw_bar`/`draw_labeled_bar` fns into a small shared `crates/game/src/scenes/bars.rs` game module both scenes call. **No engine promotion** (per owner: the bar stays game-side).
- Fill fraction = `creature.stamina().percent() / 100`. Color **banding matches post-battle**: `>50` green, `15..=50` yellow, `<15` red (`stamina_fill_color`).
- The label+bar row is **centered horizontally** within the panel interior (a Row `flex`, `Justify::Center`: `"Stamina"` label slot + bar).
- **Injured creature** (`stamina().is_injured()`, i.e. percent 0): show the empty (red) bar and, in place of/alongside the label, the recovery text — days until return, aligned with spec 46's "days remain" wording. Derive days from `stamina().injured_until()`.

### 2. Gap — 2 cells.

### 3. Abilities section
- **Header:** the word **"Abilities"** as left-aligned text (`label` with `TextAlign::Left`, spec 52) at the interior's left edge, with a **braille underline** in the cell-row directly beneath it. The underline is a horizontal lit-dot run, `HEADER_UNDERLINE_THICKNESS_DOTS` (= 2) dot-rows tall, spanning the header text width (+ a small pad), drawn through the dot pipeline (`DotBuffer` → `draw_grid`). This is **bespoke game rendering**, not a reusable primitive.
- **2×2 grid** below the header (a Row `flex` of 2 equal columns × 2 rows, or a 2×2 index map):
  ```
  Ability 0        Ability 1
  Ability 2        Ability 3
  ```
  - Each cell renders `creature.abilities()[i].description()` as **left-aligned, terminal-underlined** text via the spec-52 API: `label(.., TextAlign::Left, Style::default().fg(ABILITY_COLOR).add_modifier(Modifier::UNDERLINED))`. Truncate to the column width.
  - **Fewer than 4 abilities → the empty slots render blank** (no placeholder, no border). Max is `MAX_ABILITIES = 4`.
  - Each non-empty ability's rendered text extent is a **hover hit-target**. On mouse hover it drives the ability tooltip (spec 49). Hover only — no keyboard focus this pass.

### 4. Gap — 2 cells.

### 5. Instructions section
- A single header row: **"Instructions"** as left-aligned text with a braille underline (same style as the Abilities header) on the **left**, and the **[Edit] button** on the **right** of that row.
  - Edit button = `engine_render::Button` (`assets::BUTTON_PANEL` background, label `"Edit"`, default/roster color scheme), matching how RosterManager builds its home/arrow buttons. Its rect is refreshed each frame from layout before hit-testing (same pattern as the existing buttons). Clicking it **opens the prompt editor popup** (spec 51) for the current creature.
- **Instructions preview:** fills **all remaining vertical space** in the panel below the header row. Renders the creature's instructions as **raw Markdown source** (no rendering), left-aligned, **word-wrapped** to the preview width, clipped to the available rows. If the text overflows the available space, **truncate and append an ellipsis (`…`) at the end of the whole block** (not per line). Rendered via the engine `wrapped_text` helper (`TextAlign::Left`, `ellipsis: true`) from spec 52.
  - Content source: `instructions::read_instructions(creature.name())` (spec 47), which autocreates an empty file if none exists. **Read on navigate/settle and cache** on the scene — do **not** read the file every frame.

## Scene State & Input

In `crates/game/src/scenes/roster_manager/`:
- Add per-ability hover hit-target rects (up to 4) and a `hovered_ability: Option<usize>`, set from `InputEvent::Mouse(Moved)` hit-testing (mirrors how the existing buttons hit-test against the prior frame's rects). `hovered_ability` feeds spec 49's tooltip.
- Add an `edit_button` (`engine_render::Button`) with rect refreshed in `render` and click handled in `handle_input` — on click, open the spec-51 overlay (a new scene-owned `Option<PromptEditor>`).
- Add a cached `current_instructions: String` reloaded when `current_index` settles after navigation (and once in `new()`).
- The whole panel remains **suppressed during a slide transition** and shows the incoming creature after settle (unchanged existing behavior). When the spec-51 popup is open, the panel is covered by the modal (see 51).

## Decisions (v1)

- Stamina bar reuses the post-battle bar + its green/yellow/red banding; injured shows empty bar + days-until-return.
- Abilities render in a fixed 2×2 grid, left-aligned, terminal-underlined; empty slots blank.
- Ability hover is **mouse-only**; hovering an ability opens the tooltip (49).
- Section headers use a **braille** underline (dot pipeline); ability names use the **terminal** underline (text style) — two deliberately different underlines.
- Instructions preview is raw Markdown, word-wrapped, tail-ellipsis on overflow, cached (not read per frame).
- Edit button is the engine `Button`; it opens the prompt editor popup.

## Constants (placeholders — tunable)

- `PANEL_TOP_MARGIN_CELLS = 1`
- `STAMINA_ABILITIES_GAP_CELLS = 2`
- `ABILITIES_INSTRUCTIONS_GAP_CELLS = 2`
- `HEADER_UNDERLINE_THICKNESS_DOTS = 2`
- Ability grid inter-column gap, Edit button width — pin during layout.

## Testing Guidance

- Stamina bar fill fraction and band color track `percent()` (e.g. 60→green, 30→yellow, 10→red); injured creature renders the recovery text.
- The 2×2 grid places abilities at the four expected cells; a 3-ability creature leaves cell 3 blank (assert nothing drawn there); a 2-ability creature leaves the bottom row blank.
- Ability text is rendered with the underline style (decode the buffer cell `Style` at an ability position).
- Header underline: decode the dot cell-row beneath a header and assert lit dots span the header width (per CLAUDE.md, verify by decoding rendered dots — reuse `test_util`'s dot helpers — not by comparing rects).
- Hover hit-test: a `Moved` event inside ability 1's extent sets `hovered_ability == Some(1)`; outside all abilities sets `None`.
- Edit button click opens the popup (scene's `Option<PromptEditor>` becomes `Some`).
- Instructions preview: a long instructions string is truncated with a trailing `…`; a short one is not; the file is read on settle, not every frame.

## Open Questions / TBDs

None outstanding. The tooltip content/position is spec 49; the popup is spec 51.

## Dependencies

- Needs `47-ability-and-instructions-data-model` (ability fields, `read_instructions`).
- Needs `52-engine-text-rendering` (aligned + styled `label`, `wrapped_text`) for the headers, underlined ability names, and instructions preview.
- Reuses the post-battle bar (`46-post-battle-results-screen`), `engine_render::Button` (`45-button-widget-unification` ✅), `flex` (`40-flex-layout-primitive` ✅), the dot pipeline (`13-rendering` ✅).
- Launches `49-ability-hover-tooltip` and `51-prompt-editor-popup`.
- Modifies the roster scene from `35`/`38` ✅.
