> # ✅ DONE! — Completed 2026-07-02

# Debug Inspector — Field Editing

> **Status: implemented.** Live field editing — schema-driven widgets, buffered edits, Submit/Revert/live-apply — is built and validated end-to-end against a real scene (`BattleViewer`). The `Inspectable` derive macro, wire protocol extensions, and widget matrix are all real, tested code (see *Dependencies* for the built pieces this extends). The `asset` widget and dynamic (add/remove) collection editing are cut from this spec's scope — both live in `19-debug-inspector-advanced-editing`.

## Purpose
Extend the (already-built) debug inspector from scene *switching* to scene-state *editing*: let a developer read and change a scene's exposed fields live, without touching code or rebuilding.

## Scope
- Per-scene inspectable field **schema**, derived from scene structs
- The **field editor** UI (typed widgets per field)
- The **Submit / live-apply** flow (`ApplyState`)

Out of scope (all built with `14`): the inspector process + egui shell, the launch/connection lifecycle, the scene-switch dropdown + "Go", and the message log.

## Decisions
- **Schema-driven**: every widget is generated from the field schema; the inspector hard-codes no scene-specific UI.
- **Schema generation is automatic.** The field schema (`Inspectable`/`schema()`, per `14`'s M2 hook) is derived directly from each scene struct's fields via `#[derive(Inspectable)]` (a real proc-macro, `scene-core-derive`) — the same way `serde`'s `#[derive(Serialize, Deserialize)]` already does for wire types in this codebase — so struct and schema can't drift. Attribute grammar is serde-style: `#[inspect(label = "...", range = a..b, readonly, hidden)]` per field.
- **Bidirectional / live**: the inspector reflects the game's current field values and pushes edits back.

---

## Key Details

### Field Editor (Schema-Driven)
The panel docks below the (already-built) scene-switch bar and is built from the active scene's `schema()`, generated automatically from the struct (see *Decisions*). Each field's type tag selects a default widget; attributes refine it:

| Type tag | Widget |
|---|---|
| `bool` | checkbox |
| `int` | drag value; slider when `range` present |
| `float` | drag value; slider when `range` present |
| `string` | single-line text |
| `enum` | dropdown of variants |
| `color` | color picker (24-bit RGB) |
| `struct` | collapsible foldout (nested fields) |
| `list` | foldout list, **fixed size** — read/edit each element's fields; add/remove is out of scope (`19-debug-inspector-advanced-editing`) |

Attributes honored: `label` (display name), `range` (slider bounds), `readonly` (display only), `hidden` (omitted). Unknown/unsupported type tags — including `asset` (deferred to `19`) — fall back to a read-only JSON view so no field is ever undisplayable.

Switching scenes (via the built switcher) rebuilds the panel for the new scene from its schema and populates it with the returned live `snapshot`.

### Apply Flow
- Edits are **buffered** locally; changed fields are marked dirty (highlighted).
- **Submit** sends `ApplyState { id, patch }` containing only dirty fields; on `Ack` + `StateSnapshot` the buffer clears and values refresh from the game.
- **Revert** discards the buffer and re-reads the last snapshot.
- **Apply on change** toggle (`Subscribe { live: true }` + per-edit `ApplyState`) gives the continuous Unity-style behavior; off by default to avoid flooding.
- If the game pushes a `StateSnapshot` for a field the user is actively editing, the local edit wins until Submit/Revert (no clobbering mid-edit).

---

## Test Plan
Connect (via the built switcher) → switch to a scene → its schema-driven fields render → edit an exposed field → **Submit** → change is visible in the game. Plus: widget selection per type, dirty highlighting, Revert, live-apply toggle.

## Open Questions / TBDs
- Presets: save/load a set of field values per scene?
- Undo/redo of applied changes?
- Triggering scene-specific debug *actions* (buttons/commands), not just field edits — future extension?

Resolved (moved out): `asset` widget and dynamic (add/remove) collection editing → `19-debug-inspector-advanced-editing`.

## Dependencies
- `14-scene-architecture` — the built inspector base (process, egui shell, connection, scene switcher, message log) this extends, plus the IPC envelope and the `Inspectable`/`schema()` M2 hook.
- `16-world-space-and-camera` / `13-rendering` — only indirectly (the game it inspects renders through them).
- Consumed by `19-debug-inspector-advanced-editing` — extends this spec's schema/widget system with the `asset` widget and dynamic collection sizing.
