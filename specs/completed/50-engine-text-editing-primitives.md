> # ✅ DONE! — Completed 2026-07-13
> Status: implemented via the tdd-pipeline, shipped to `main`.

# Engine Text-Editing Primitives

## Status

Done (shipped to `main`). Adds the first **terminal** text-editing widget to the engine. Engine-level and reusable by any game built on this workspace (per owner: only the text-input and multi-line scrollable editor — with its scrollbar as a built-in fixture — become engine primitives; the underline/pill/tooltip do not). Consumed by the prompt editor popup (`51`).

## Purpose

The codebase has no terminal text input or editor — the only editing anywhere lives in the separate egui inspector app. The prompt editor popup needs two closely-related things: a small **auto-growing input** (the "Prompt agent to update" box) and a **fixed, vertically-scrolling multi-line editor** (the creature-instructions editor) with an integrated scrollbar. Both are the same core widget under one config, so this spec defines a single `TextEditor` in `crates/engine/render`.

## Widget: `TextEditor`

New `crates/engine/render/src/text_editor.rs`, re-exported from `lib.rs`. Renders text as plain terminal characters (the documented text exception to rule 4); all chrome it owns — the **scrollbar** and the **block cursor** — and nothing else. The surrounding border/frame is the **caller's** responsibility (composition), so the widget is frame-agnostic.

### Configuration

```rust
pub enum Sizing {
    Fixed,                 // occupies the caller's rect; scrolls when content overflows
    Grow { max_rows: u16 },// height grows with content up to max_rows, then scrolls
}

pub struct TextEditorConfig {
    pub sizing: Sizing,
    pub submit_on_enter: bool, // true: Enter => Submit, Shift+Enter => newline
                               // false: Enter => newline (no Submit)
    pub placeholder: String,   // dim text shown when empty & unfocused
}

pub enum EditorEvent { None, Changed, Submit }
```

### API

```rust
impl TextEditor {
    pub fn new(config: TextEditorConfig) -> Self;
    pub fn set_text(&mut self, text: &str);
    pub fn text(&self) -> String;
    pub fn handle_key(&mut self, key: KeyEvent) -> EditorEvent;   // edit + cursor + scroll keys
    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> bool;      // wheel scroll; true if consumed
    pub fn desired_rows(&self, width_cells: u16) -> u16;          // Grow sizing: rows the caller should give it
    pub fn render(&mut self, buf: &mut Buffer, rect: Rect);       // text + cursor + scrollbar
}
```

### Behavior (v1)

- **Text model:** a growable buffer of logical lines. Long lines **soft-wrap** to the render width (word-wrap, breaking an over-long token by character). **Vertical scrolling only** — there is no horizontal scroll.
- **Cursor:** a **block cursor** = the caret cell drawn in **reverse video** (a `Style` swapping fg/bg); at end-of-line it's a reverse space.
- **Editing keys:** printable chars insert at the cursor; `Backspace`/`Delete` remove; `Enter` inserts a newline **unless** `submit_on_enter` — then `Enter` returns `EditorEvent::Submit` and `Shift+Enter` inserts a newline.
- **Cursor movement:** `Left/Right/Up/Down`, `Home/End`. The viewport **follows the cursor** (scrolls to keep it visible).
- **Scrolling:** mouse **wheel** and keyboard `PgUp`/`PgDn` (and arrow-driven cursor movement) scroll the viewport. Scrolling is **wheel + keyboard only — the scrollbar is a non-draggable indicator.**
- **Scrollbar (fixture):** when content rows exceed the viewport, the widget draws a vertical scrollbar in its **right-most dot column(s)** through the dot pipeline — a full-height track with a thumb sized/positioned to the visible fraction. When content fits, no scrollbar (and `Grow` mode simply reports a smaller `desired_rows`).
- **`Grow` sizing:** `desired_rows(width)` returns the wrapped line count clamped to `[1, max_rows]`; the caller resizes its container to fit (spec 51's growing popup). At `max_rows` it behaves like `Fixed` (scrolls).
- **Placeholder:** shown dimmed when the buffer is empty.
- **Explicitly out of v1 (deferred to `needs-research/text-editor-v2`):** text **selection**, **copy/paste**, mouse click-to-place-cursor, mouse drag on the scrollbar, horizontal scroll, undo/redo, and **`@` mention commands** (inline autocomplete popup for referencing creatures/entities).

### Events → caller

`handle_key` returns `Changed` whenever the buffer mutated (the caller uses this to persist — spec 51 writes through to disk), `Submit` on an Enter in `submit_on_enter` mode, else `None`.

## Decisions (v1)

- One `TextEditor` widget, two configs (`Grow` input, `Fixed` scrolling editor).
- Soft word-wrap, vertical scroll only, block (reverse-video) cursor.
- Scrollbar is a built-in **fixture** of the editor, indicator-only (wheel + keyboard scroll).
- `submit_on_enter` gives the agent box its Enter=submit / Shift+Enter=newline behavior.
- No selection/copy-paste/click-to-place/drag in v1; those are the v2 set.
- The widget draws text + cursor + scrollbar only; the frame is the caller's.

## Testing Guidance (headless, no terminal)

- `set_text("a\nb\nc")` then `text()` round-trips; typing chars and `Backspace` mutate as expected and return `Changed`.
- `Enter` with `submit_on_enter: true` returns `Submit` and does **not** insert a newline; `Shift+Enter` inserts one and returns `Changed`. With `submit_on_enter: false`, `Enter` inserts a newline.
- Cursor movement keys reposition the caret; the viewport scrolls to keep the caret visible (assert visible line range).
- Wheel / `PgDn` scroll the viewport without moving the buffer contents; the scrollbar thumb position tracks the offset (decode the scrollbar dots).
- `Grow` sizing: `desired_rows` increases as text wraps to more lines and clamps at `max_rows`.
- Render: the caret cell is reverse-video (decode the cell `Style`); soft-wrap places a long line across the expected number of rows.

## Open Questions / TBDs

- v2 feature set (selection, copy/paste, click-to-place cursor, draggable scrollbar, horizontal scroll, undo/redo, and `@` mention commands) — deferred to `needs-research/text-editor-v2`.

## Dependencies

- Builds on the dot pipeline for the scrollbar (`13-rendering` ✅) and ratatui `Buffer`/`Style` for text + cursor.
- Consumed by `51-prompt-editor-popup`.
