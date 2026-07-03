> # ✅ DONE! — Completed 2026-07-02

# Mouse & Hover Input

> **Status: implemented.** Extends `14-scene-architecture`'s shared `InputEvent` enum with mouse support — clicks *and* continuous hover tracking — so any scene can build clickable, hover-reactive UI. Foundational engine capability, not a scene. Required by `22-braille-ui-chrome` (button hover/press states) and `24-roster-carousel` (clickable arrows/home button); also what the not-yet-written Main Hub Navigation spec will need for a clickable menu.

## Purpose
Give scenes a way to know "the mouse is at (col, row)" every frame and "a button was pressed/released at (col, row)" as discrete events — the two primitives a hover-reactive, clickable UI needs. Today the engine has neither: `InputEvent` has exactly one variant (`Key`), and `crossterm`'s mouse capture is never enabled.

## Scope
- `InputEvent` gains a `Mouse` variant carrying `crossterm::event::MouseEvent` directly — same precedent as the existing `Key(crossterm::event::KeyEvent)` variant: wrap the crate's own event type, don't re-encode it.
- Mouse capture enabled at startup (`crossterm::execute!(..., EnableMouseCapture)`) alongside the existing raw-mode/alt-screen setup in `crates/game/src/app.rs`, and disabled on teardown (`DisableMouseCapture`) alongside the existing cleanup.
- The main loop's input-polling step (`crates/game/src/app.rs`, the `event::poll`/`event::read` block) additionally matches `Event::Mouse(me)` and dispatches `InputEvent::Mouse(me)` to the active scene's `handle_input` — the exact same dispatch path keyboard events already use, not a parallel channel.
- `crossterm::event::MouseEvent` already carries everything needed: `kind` (`Down`/`Up`/`Drag`/`Moved`/`ScrollUp`/`ScrollDown`/...), `column`/`row` (terminal cell coordinates — the same coordinate space `ratatui::layout::Rect` already uses, so no translation layer is needed), `modifiers`.

Out of scope:
- Any concrete widget (button, hover tint, click handling) — that's `22-braille-ui-chrome`. This spec only makes mouse events *reach* a scene's `handle_input`; what a scene does with them is out of scope here.
- Drag-and-drop, scroll-driven scrolling/zooming, right-click context menus — no current scene needs them; add variants/handling when a real one does.
- Windows support — mirrors `14-scene-architecture`'s existing "Linux only for now" stance; `crossterm` mouse capture is cross-platform in principle but untested here.

## Decisions (v1)
- **Wrap, don't re-encode.** `InputEvent::Mouse(crossterm::event::MouseEvent)`, mirroring `InputEvent::Key(crossterm::event::KeyEvent)` exactly. No bespoke mouse-event type.
- **Terminal cell coordinates are the coordinate space.** A mouse event's `(column, row)` is directly comparable to any `ratatui::layout::Rect` a scene already computed for its own layout (e.g. a button's on-screen area) — no scaling/offset conversion required.
- **Hover requires `Moved` events, not just clicks.** `EnableMouseCapture` reports motion events by default in `crossterm`; this is what makes continuous hover tracking possible at all (a click-only capture mode would not support it). A widget checks "does the latest `Moved` event's (col,row) fall inside my rect" to know if it's currently hovered.
- **No hit-testing here.** This spec delivers events to `handle_input`; deciding "is (col,row) inside my button" is `22-braille-ui-chrome`'s job (rect containment against a widget's own stored area).

## Dependencies
- `14-scene-architecture` ✅ — extends the shared `InputEvent` enum and the main loop's input-dispatch step this spec adds to; scenes and structures already defined there are unchanged, just handed a new event kind.
- Feeds `22-braille-ui-chrome` — hover/press state for the button component is built on the `Mouse` events this spec delivers.
- Feeds `24-roster-carousel` — clickable arrows and home button need this.
- Feeds the not-yet-written Main Hub Navigation spec — its clickable menu needs this too.
