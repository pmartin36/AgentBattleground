> # ✅ DONE! — Completed 2026-07-13
> Status: implemented via the tdd-pipeline, shipped to `main` (undo-coalescing seam fixed post-validation).

# TextEditor v2: Selection, Clipboard, Draggable Scrollbar, Undo/Redo

## Status

Done (shipped to `main`). Extends the engine `TextEditor` from `50-engine-text-editing-primitives` with four of the deferred v2 mechanics. Engine-level (every `TextEditor` inherits them); the prompt-editor popup (`51`) gets them for free. Text selection is included here because copy/cut require it.

## Purpose

The v1 editor has no selection, clipboard, draggable scrollbar, or history. This adds them: select text, cut/copy/paste via the system clipboard, drag the scrollbar thumb, and undo/redo edits.

## Scope

- **Text selection** — keyboard (shift+movement) and mouse-drag; a highlighted range.
- **Copy / cut / paste** — system clipboard.
- **Draggable scrollbar** — the v1 indicator thumb becomes draggable.
- **Undo / redo** — an edit-history stack with sensible coalescing.

**Out of scope:** horizontal scroll (stays in `needs-research/text-editor-v2`); `@` mentions (`56`).

## 1. Text selection

- Model: a `selection: Option<(anchor, caret)>` where both are logical `(line, col)` positions; the caret is the active end. `None` = no selection.
- **Keyboard:** `Shift`+`Left/Right/Up/Down/Home/End` extends the selection from the current anchor (setting the anchor on the first shift-move). A non-shift movement (or a click, or typing) **collapses** the selection to the caret.
- **Mouse:** `Down` sets the anchor + caret at the clicked cell (reuse the click-to-place mapping from `53`); `Drag` extends the caret; `Up` finalizes. (This composes with `53`'s click-to-place — a click with no drag just places the caret.)
- **Editing over a selection:** typing a char, `Enter`, `Backspace`/`Delete`, or paste **replaces** the selection.
- **Render:** selected cells draw with a distinct background (reverse-video or a tint style), applied per wrapped row over the selected span. The caret still renders (blinking per `53`) at the active end.

## 2. Copy / cut / paste (system clipboard)

- Uses the OS clipboard via a small cross-platform crate (**recommend `arboard`**; note the new dependency + that it needs a display/clipboard backend). Wrap it behind a tiny `Clipboard` trait so tests inject an in-memory fake and don't touch the real system clipboard.
- Bindings: `Ctrl+C` copy selection; `Ctrl+X` cut selection (copy + delete); `Ctrl+V` paste at caret (replacing any selection). Copy/cut with no selection are no-ops (or copy the current line — pick no-op for v1 unless the owner wants line-copy).
- Multi-line paste inserts newlines correctly and re-wraps.

## 3. Draggable scrollbar

- The v1 scrollbar (`text_editor.rs::draw_scrollbar`, now a thumb) becomes interactive: `handle_mouse` handles `Down` on the thumb/track column → begin drag; `Drag` maps the cursor's vertical position to a `scroll_offset` proportional to the content; `Up` ends the drag. Wheel + keyboard scroll (v1) still work.
- Clicking the track above/below the thumb pages up/down (optional; note if deferred).

## 4. Undo / redo

- An **undo stack** of editor states (or reversible edits) + a redo stack. `Ctrl+Z` undo, `Ctrl+Y` (and `Ctrl+Shift+Z`) redo.
- **Coalescing:** consecutive same-kind edits (a run of typed characters, or a run of deletes) collapse into a single undo step; a caret move, newline, paste, or cut starts a new step. Undo/redo restore the buffer **and** caret/selection.
- Bound stack depth (a constant) to cap memory.

## Decisions (v1 of this spec)

- Selection is `Option<(anchor, caret)>`, collapses on non-shift movement / typing / click.
- Clipboard via `arboard` behind a `Clipboard` trait (fake in tests); `Ctrl+C/X/V`; no-op copy/cut without a selection.
- Scrollbar thumb is drag-scrollable; wheel/keyboard unchanged.
- Undo/redo with typing-run coalescing; restores caret+selection; depth-capped.

## Constants (placeholders — tunable)

- `UNDO_STACK_DEPTH`, selection highlight style/color.

## Testing Guidance (headless)

- Shift+movement builds the expected `(anchor, caret)`; a plain movement collapses it; typing over a selection replaces it.
- Copy then paste (via the fake `Clipboard`) round-trips the selected text at the caret; cut removes it; multi-line paste re-wraps.
- Scrollbar: a `Down`+`Drag` on the thumb maps to the expected `scroll_offset`; wheel/keyboard still scroll.
- Undo restores the prior buffer+caret; a run of typed chars is one undo step; redo re-applies; stack depth is bounded.
- Selection renders a distinct background over the selected span (decode cell styles).

## Dependencies

- Extends `50-engine-text-editing-primitives`; composes with `53-text-editor-cursor-placement-and-blink` (click-to-place, blink).
- Consumed by `51-prompt-editor-popup` (both fields inherit these).
- Carved out of `needs-research/text-editor-v2` (horizontal scroll remains there).
