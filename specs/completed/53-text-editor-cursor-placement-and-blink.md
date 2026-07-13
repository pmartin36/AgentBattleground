> # ✅ DONE! — Completed 2026-07-13
> Status: implemented via the tdd-pipeline, shipped to `main`.

# TextEditor Cursor Placement, Focus & Blink

## Status

Done (shipped to `main`). Follow-up to `50-engine-text-editing-primitives` and `51-prompt-editor-popup`, covering three text-editor interaction features.

## Purpose

The v1 `TextEditor` has a static, always-on block cursor, switches focus only via Tab, and cannot be clicked into. This spec adds three interaction improvements: (1) click to place the caret **within** an editor, (2) click to move focus **between** the popup's two editors, and (3) a **slow cursor blink**. The caret-placement and blink are engine `TextEditor` changes (every future editor inherits them); the field hit-test + focus routing is game-side in the prompt-editor popup.

## Scope

- **Engine** `crates/engine/render/src/text_editor.rs`: click-to-place caret within the editor; a blink phase advanced by a per-frame tick.
- **Game** `crates/game/src/scenes/roster_manager/prompt_editor.rs`: route a left-click to the field it lands in, set focus there, and forward `dt` to both editors' tick.

**Out of scope** (remain in `needs-research/text-editor-v2`): text selection, copy/paste/cut, drag, horizontal scroll, undo/redo, `@` mentions.

## Feature 1 — Click-to-place caret (within an editor)

- Add a `MouseEventKind::Down(MouseButton::Left)` arm to `TextEditor::handle_mouse` (today it handles only wheel scroll, `text_editor.rs:340-346`).
- Map the click cell to a caret position:
  - `display_row = scroll_offset + (ev.row - rect.y)`, clamped to the last wrapped row;
  - `col = ev.column - rect.x`;
  - reuse the existing `wrap_rows(viewport_width)` + `set_from_display(&rows, display_row, col)` (`text_editor.rs:76-108`, `:264`), which already applies the wrap-boundary max-column rule.
- The editor must know its render **origin**: today `render` caches only `viewport_width`/`viewport_height` (`text_editor.rs:397-398`) — also cache the render `rect`'s `x`/`y` (or pass the rect into `handle_mouse`).
- A click **outside** the editor's cached rect is ignored (no caret move, no buffer mutation).
- Placing the caret resets the blink to visible (Feature 3).

## Feature 2 — Click-to-focus (between editors)

- In `PromptEditor::handle_input`'s mouse branch (`prompt_editor.rs:142-148`), on a left `Down`: hit-test the `agent_input` and `instructions` field rects, set `focus` to the hit field, and forward the click **only** to that editor — so a click in one field does not also move the other's caret (today both editors receive every mouse event unconditionally).
- A left `Down` inside the popup but outside both fields leaves `focus` unchanged.
- Existing keyboard **Tab** focus-cycling (`prompt_editor.rs:128,155-160`) is unaffected.

## Feature 3 — Slow cursor blink

- Add a blink accumulator (`Duration`) to `TextEditor`, advanced by a new `pub fn tick(&mut self, dt: Duration)`.
- In `render`, gate the reverse-video caret block (`text_editor.rs:423-432`) on the blink phase: caret **visible** for the first half of each `BLINK_PERIOD`, **hidden** for the second.
- Any edit or caret move (typing, arrows, Home/End, click-to-place) **resets the phase to visible**, so the caret never blinks off mid-interaction.
- Only the **focused** editor shows a caret; the unfocused editor renders no caret at all. (`TextEditor` needs a focused flag, set by the popup — or the popup only calls the caret-bearing render path on the focused field. Prefer a `set_focused(bool)` on the widget so the behavior is self-contained.)
- The popup forwards `dt` to both editors' `tick` every frame: **un-gate** `PromptEditor::update`'s current `if !dirty { return }` early-return (`prompt_editor.rs:182-190`) so ticks always reach the editors. The 30 fps app loop already calls `update` + `render` every frame (`app.rs:162,174`), so no new redraw plumbing is required.

## Decisions (v1)

- Click-to-place caret and blink live in the **engine** `TextEditor`; field hit-test + focus routing is **game-side** in the popup.
- Blink period ~**600 ms visible / 600 ms hidden** (a `BLINK_PERIOD` const, tunable), reset-to-visible on any edit or caret move.
- Only the **focused** editor renders a (blinking) caret.
- Mouse never mutates the buffer; clicks outside an editor's rect are ignored.

## Constants (placeholders — tunable)

- `BLINK_PERIOD` (~600 ms half-cycle).

## Testing Guidance (headless)

- `handle_mouse` `Down(Left)` at a known cell places the caret at the expected `(line, col)` given a set `scroll_offset` and wrapped multi-line content; a click outside the cached rect leaves the caret unchanged and mutates nothing.
- `tick` drives the blink: caret visible at phase 0, hidden after half a period; an edit during the hidden phase flips it back to visible (decode the caret cell's `REVERSED` style, or expose a `caret_visible()` helper for the assertion).
- Only the focused editor renders a caret (render both fields; assert exactly one shows a caret).
- Popup routing: a `Down` in the instructions field sets focus to it and does **not** move the agent-input caret; a `Down` in the agent-input field focuses it.

## Dependencies

- Extends `50-engine-text-editing-primitives` (the `TextEditor` widget) and `51-prompt-editor-popup` (the two-field popup + Tab focus + input routing).
- `needs-research/text-editor-v2` covers the remaining editor features (selection, copy/paste, drag, horizontal scroll, undo/redo, `@` mentions).
