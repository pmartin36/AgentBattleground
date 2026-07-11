# Prompt Editor Popup

## Status

Pending. The large modal overlay opened by the roster panel's **[Edit]** button (`48`) for editing a creature's battle instructions. Consumes the engine `TextEditor` (`50`). The AI-assisted rewrite behind the top input is **stubbed** here and specified for real in the needs-research follow-up (`needs-research/ai-prompt-rewrite-agent`).

## Purpose

Let the player read and edit a creature's battle-instructions Markdown file in-game, with a place to (eventually) ask an agent to rewrite it, and a visible path to the file on disk so the external-file workflow (`03`) stays discoverable. Editing writes through to the file live — the file is the single source of truth (spec 47), so there is no save step.

## Reference Mock

Player sketch (a large centered popup): a closable **X** top-right; a **"Prompt agent to update"** input across the top; a large **text editor** with a **scrollbar** on its right; and a **`<path to file>`** line beneath the editor.

## Layout

A **large centered popup** built on the `BattleMenu` overlay pattern: a chamfered frame via `ui_primitives::rounded_rect(..., Dot::Occlude)` centered on the screen (`POPUP_W_FRAC` × `POPUP_H_FRAC` of the screen, tunable), the `Dot::Occlude` fill covering the roster beneath. Everything inside is **centered horizontally**. Top-to-bottom:

1. **Close (X)** — a small hit-target in the **top-right** corner (an `engine_render::ButtonCore` or `Button`), clickable to close. Also closes on **Esc**.
2. **Agent input** — a `TextEditor` in **`Grow` mode**, `submit_on_enter: true`, placeholder `"Prompt agent to update"`. It starts one row tall and **grows by wrapping** as the player types; when it grows, the **popup grows and re-centers** to fit it. `Enter` **submits** (fires the stubbed agent, below); `Shift+Enter` inserts a newline.
3. **1-cell margin.**
4. **Instructions editor** — a `TextEditor` in **`Fixed` mode**, `submit_on_enter: false`, filling the main body, with its built-in **scrollbar** on the right. Seeded via `set_text(read_instructions(creature.name()))` (spec 47) when the popup opens. Fully editable; scroll via **wheel + keyboard** (spec 50).
5. **File path** — directly beneath the editor, the `instructions_path(creature.name())` shown as plain text.

## Behavior

- **Modal.** While open, roster left/right navigation, select-and-swap, and ability hover are **disabled**; all keyboard/mouse input routes to the popup. The popup renders on top of (and occludes) the roster.
- **Open:** the Edit button sets the roster's `Option<PromptEditor>` to `Some`, constructs the two `TextEditor`s, and loads the file into the instructions editor.
- **Close:** the **X** or **Esc** clears the overlay back to `None`. **No save-on-close** is needed because edits already wrote through (below). No confirm dialog.
- **Live write-through (no Save button):** whenever the instructions editor returns `EditorEvent::Changed`, write the buffer to disk via `write_instructions(creature.name(), editor.text())` (spec 47), **debounced** (`WRITE_DEBOUNCE`, tunable) so a burst of keystrokes coalesces into one write. The file is the source of truth; the roster panel's cached preview (spec 48) is refreshed from disk when the popup closes.
- **Agent input submit (stubbed):** on `Enter`/`Submit` from the agent input, the real behavior — an LLM rewriting the instructions file and reloading the editor — is **out of scope**. For v1 the submit is a **no-op** (optionally clears the input); the full flow is specified in `needs-research/ai-prompt-rewrite-agent`. No functional model call ships in this spec.
- **No external-file watching** while open (deferred, `03`).

## Scene State & Input

In `crates/game/src/scenes/roster_manager/`:
- Add `prompt_editor: Option<PromptEditor>`, where `PromptEditor` owns the two `TextEditor`s, the close (X) `ButtonCore`, the target creature index, and the debounce timer.
- `handle_input`: when `prompt_editor.is_some()`, consume input into the popup (route keys to whichever field is focused; route mouse to X + the editors' `handle_mouse` for wheel scroll; Esc / X close) and **do not** run the normal roster bindings.
- `render`: when open, draw the popup after the roster (topmost), sized from the agent input's `desired_rows` so the popup grows with it.
- `update`: advance the write debounce; flush a pending write when it elapses.

## Decisions (v1)

- Large centered modal via the `Dot::Occlude` overlay pattern; closes on **X and Esc**.
- Agent input = `Grow` `TextEditor`, Enter=submit / Shift+Enter=newline; growing it grows/re-centers the popup.
- Instructions editor = `Fixed` `TextEditor` with scrollbar; seeded from the file.
- **Live debounced write-through; no Save action; no save-on-close** (file is source of truth).
- File path shown beneath the editor.
- Agent submit is a **no-op stub**; real rewrite deferred to the needs-research spec.
- Modal input; no external-change watching.

## Constants (placeholders — tunable)

- `POPUP_W_FRAC`, `POPUP_H_FRAC` (large), popup min height.
- `WRITE_DEBOUNCE` (e.g. ~300 ms).
- Agent input `max_rows` (grow cap before it scrolls).

## Testing Guidance

- Opening the popup loads the creature's file text into the instructions editor (`set_text` == `read_instructions`).
- Typing in the instructions editor, after the debounce, writes the new text to disk (`read_instructions` returns the edited content) — assert against a **temp base dir**, never the real repo.
- Closing via Esc and via the X both clear the overlay; the roster panel's cached preview reflects the edited file afterward.
- Agent input: `Enter` submits (no-op, input handling doesn't crash / optionally clears); `Shift+Enter` adds a line and the popup's `desired_rows` grows.
- While open, a roster left/right key or ability-hover event does **not** navigate or open a tooltip (modal input is exclusive).
- Popup is centered and occludes the roster beneath (decode a covered cell).

## Open Questions / TBDs

- The real agent rewrite (model, file/UI tool, sandboxing, apply/discard, async UX) — `needs-research/ai-prompt-rewrite-agent`.

## Dependencies

- Needs `47-ability-and-instructions-data-model` (`read_/write_instructions`, `instructions_path`) and `50-engine-text-editing-primitives` (`TextEditor`).
- Opened by `48-roster-detail-panel-redesign`.
- Reuses `ui_primitives::rounded_rect` + `Dot::Occlude`, `engine_render::Button`/`ButtonCore`, the `BattleMenu` modal pattern.
- Followed by `needs-research/ai-prompt-rewrite-agent`.
