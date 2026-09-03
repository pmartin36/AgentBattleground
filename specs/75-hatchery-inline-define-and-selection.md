# Hatchery — Inline Define Surface & Egg Selection

> **Status: pending.** Replaces the pop-up mad-lib **modal** (`67`) with an inline master-detail hatchery surface: the selected egg shown large in the upper third, its mad-lib laid out below as one flowing paragraph, and the egg tray along the bottom carrying two distinct highlight states (hovered vs selected). Also replaces `65`'s tap-to-focus interaction with a browse/edit two-tier model that navigates the tray. Supersedes the modal-and-tap slices of `67` and `65`; the downstream generation wiring (`70` → `71` → `66` → `Incubating`) and the hatch sequence (`68`/`72`) are reused unchanged.

## Purpose
Make defining and browsing eggs feel like one live scene instead of a pop-up form. The player moves a hover highlight across the tray, selects an egg to bring it up large, and (for an undefined egg) fills its mad-lib in place. The mad-lib reads as normal underlined prose that wraps like a paragraph, not a grid of little boxes.

## Motivation (what's wrong today)
- The mad-lib modal is a fixed box, so a blank that grows as the player types has nowhere to go and wraps **inside its own little field**. A blank must instead be plain underlined text that flows in the surrounding paragraph and wraps at word boundaries like any other text.
- The modal hides the tray, so there is no room to show which egg is hovered vs which is selected.

## Scope
- **Inline master-detail layout**, replacing the modal:
  - The **selected** egg is rendered large, anchored roughly one third from the top of the scene.
  - The selected egg's **mad-lib** is laid out below it, using the full scene width (wrapping to as many rows as it needs).
  - The **tray** of all owned eggs runs along the bottom (as today), and is the navigation strip.
- **Two-tier interaction:**
  - **Browse mode:** a *hover* highlight moves across the tray via arrow keys / Tab / mouse hover. Enter or click on a tray egg *selects* it (brings it up large). One egg auto-selects when the scene opens; with a single egg it is both the hover and the selection target.
  - **Edit mode** (an *undefined* egg is selected): the mad-lib's blanks are editable. Tab / Shift-Tab cycle between blanks; the active blank shows a blinking cursor. Esc leaves edit mode back to browse. While editing, Tab means "next blank," not "next egg" (egg switching is arrow keys after Esc, or a tray click), so the two never collide.
- **Two distinct tray highlight states**, both rendered through the braille dot pipeline (never drawn with raw glyphs): a *hovered* treatment and a *selected* treatment, visually distinguishable from each other and from an idle egg, so the player can always tell what the cursor is on vs what is currently open above.
- **Mad-lib paragraph model** (see next section) — the core of this spec.
- **Submit affordance** for a fully-filled undefined egg, replacing the modal's Done: available only once every blank is non-empty, and running the exact same Done sequence as `67` (send the completed sentence to `70`, assemble via `71`, store on `Egg::hatchling`, kick off art via `66`, enter `Incubating` and start `65`'s timer). The standard "close/back" affordance leaves the egg undefined, as the modal's X did.
- **Defined eggs on the same surface:** selecting an `Incubating` or `Ready` egg brings it up large with its **completed** mad-lib shown read-only, and preserves the existing per-state affordances that lived in the old focus view (`Incubating` shows the hatch countdown; `Ready` taps into the hatch sequence per `68`). The one surface serves browsing, defining, and the pre-hatch view.
- **Deletion of the modal:** `define_modal.rs` (the pop-up box and its fixed-field, per-blank rendering) is removed. The single scene-state model below replaces the old `focused: Option<usize>` + `define_modal: Option<DefineModal>` pair.

Out of scope: the egg lifecycle, states, and 24-hour timer (`65`); the model call and creature assembly (`70`/`71`); art backends (`66`); the hatch/reveal animation and Add-to-Roster (`68`/`72`) — all reused as-is. Meta-generated (varying) mad-lib templates remain a later iteration (`67` open question), unchanged. The mad-lib **template pool content** is unchanged by this spec.

## Mad-Lib Paragraph Model
The mad-lib is **one continuous paragraph**, not a set of independent fields. It is a sequence of runs:
- **Literal runs** — fixed template text, plain.
- **Blank runs** — the editable regions, styled with an **underline**.

Layout and wrapping:
- The whole paragraph is laid out with normal word-wrap across the available width: tokens flow left to right and break to the next row at word boundaries when a row fills. **Blank text participates in this exactly like literal text.**
- A blank whose text is long enough **wraps across a line break at a word boundary**, and its underline decoration simply continues under whichever cells its words occupy on each row. A blank is **not** its own bounded box and **never** wraps within a fixed sub-field. (This is the specific behavior the current modal gets wrong.)
- An **empty or partially-filled** blank shows a **fixed minimum underline width** so the reader can see where to type. As the player types past that width, the underline grows with the text. The minimum width is a floor, not a container the text is trapped inside.
- The **active** blank shows a **blinking cursor** at the insertion point, reusing the engine's cursor-blink behavior/timing (`engine_render`'s `TextEditor` tick, driven from the scene's `update(dt)`), so it blinks identically to the roster's prompt field.

Rendering must honor the braille invariants: the underline is a non-text visual element and so is drawn through the dot pipeline (not a raw underscore glyph run); any `DotRect` carrying sub-cell placement is threaded unfloored to the draw call; and any alignment claim (e.g. underline sitting directly under its text) is verified by decoding the rendered dots, never by comparing `Rect`/`DotRect` fields. Text glyphs (the literal words and the typed blank characters) stay plain terminal characters per the text exception.

## Scene State Model
Replace the old dual state with a single explicit model, e.g. a selection index plus a mode (`Browsing { hover: usize }` / `Editing { egg: usize, active_blank: usize }`), so "what is hovered," "what is selected," and "am I editing" are one source of truth rather than split across `focused`/`define_modal`. The per-egg mad-lib fill state (the blank values + which blank is active + cursor) moves out of the deleted modal onto this scene state. Hatch logic (`68`) still keys off the selected `Ready` egg exactly as it keyed off the focused one.

## Open Questions / TBDs
- The exact **visual treatment** of the hovered vs selected tray states (ring, glow, brightness, scale, lift) is a design detail to settle against real references during build, consistent in language with the rest of the game's braille chrome. The hard requirement is that the two states, and idle, are mutually distinguishable; the precise styling is tuning.
- Cursor and underline **color** for the active/inactive blanks (pick from the existing palette; match the roster field where it makes sense).
- Tray behavior when the egg count exceeds the row width (scroll vs shrink) — not a concern at current counts; leave to a later pass.

## Dependencies
- `67-hatchery-definition-generation` — **superseded in part**: this replaces its mad-lib **modal** and its unfilled-sentence tray rendering. Its Done sequence and all generation wiring are reused unchanged.
- `65-hatchery` — **superseded in part**: this replaces its tap-to-focus interaction and centered focus view with the browse/edit surface. The egg lifecycle, states, and timer are unchanged.
- `68-hatchery-hatch-sequence`, `72-hatch-reveal-and-roster-placement` — the hatch/reveal that a selected `Ready` egg triggers; this spec only changes how that egg is reached (selection vs the old focus tap).
- `engine_render` `TextEditor` — reused for the blinking-cursor timing/behavior on the active blank.
- `13-rendering` (completed) — the braille dot pipeline the tray highlights and blank underlines render through.
