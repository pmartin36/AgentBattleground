# TextEditor v2 (needs research)

> **Status: parked pending research.** The v2 feature set for the engine `TextEditor` shipped in `50-engine-text-editing-primitives`, which deliberately scopes v1 to basic editing + scrolling. Unnumbered until scoped, per the `needs-research/` convention.

## Purpose

Grow the engine `TextEditor` from a basic editing surface into a full-featured one, and add **`@` mention commands** — an inline autocomplete for referencing game entities (creatures, etc.) from within battle instructions.

## v2 feature set

Carried over from `50`'s deferred list:

- **Text selection** (shift+arrows, and mouse drag).
- **Copy / paste / cut** (system clipboard).
- **Draggable scrollbar** thumb (v1's scrollbar is a wheel/keyboard-driven indicator only).
- **Horizontal scroll** (v1 soft-wraps and scrolls vertically only).
- **Undo / redo.**

> **Carved out:** click-to-place caret, focus-switching between editors, and the slow cursor blink are now specced as a buildable unit in `53-text-editor-cursor-placement-and-blink` — no longer parked here.

## `@` mention commands

Typing `@` opens a small inline **autocomplete popup** anchored at the cursor, listing selectable options; picking one inserts a mention token (e.g. `@EmberWolf`).

Sketch of behavior (details to be fleshed out during research):
- `@` triggers the popup; continued typing **filters** the option list; `Esc` dismisses; `Enter`/`Tab` (or click) accepts the highlighted option; arrow keys move the highlight.
- Accepted mentions render as a distinct **token** (styling TBD) and carry a stable reference, not just display text, so downstream consumers (the battle sim reading the instructions file) can resolve them.

### Open questions for the mention system

1. **What can be mentioned?** Own creatures by name, enemy creatures, abilities, board/zone concepts — and is the candidate list context-dependent?
2. **On-disk representation.** How a mention is stored in the Markdown instructions file (raw `@EmberWolf` text vs. a structured token/link) so it survives external hand-editing (`03-army-skill-editing`) and round-trips.
3. **Resolution & validation.** How the battle sim resolves a mention, and what happens to a mention whose target was renamed/removed.
4. **Rendering.** Token styling in the editor and in the roster panel's raw-Markdown preview (`48`).
5. **Reuse vs. game-specific.** The autocomplete *mechanism* is engine-general; the *candidate source* (this game's creatures) is game content — where the boundary sits, and how the engine widget gets its candidate list injected.
6. **Scope of the trigger.** Is `@` the only trigger, or are there others (e.g. `/` for actions)?

## Dependencies / relationships

- Extends `50-engine-text-editing-primitives` (the v1 widget).
- Mention candidates draw on `47-ability-and-instructions-data-model` / the creature roster; resolution ties to `10-battle-simulation-engine`.
- Mentions must survive the external-file editing workflow in `03-army-skill-editing`.
