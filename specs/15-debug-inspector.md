# Debug Inspector

> **Status: draft (high-level).** Builds directly on `14-scene-architecture`. That spec defines the scene model, registry, per-scene field schema, and the IPC channel; this spec defines the **inspector GUI** that consumes them. Read `14` first.

## Purpose
A Unity-inspector-style developer tool for driving and tweaking the running game. It lets a developer switch scenes and edit a scene's exposed state live, without touching code or rebuilding. It is a debug-only companion process to the game.

## Scope
- The inspector as a **native desktop GUI** (egui/eframe), separate process, debug builds only
- Launch + connection lifecycle (driven by the game's `--inspect` flag)
- The **Scene Switcher** UI (top bar: catalog dropdown + "Go")
- The **field editor** UI (per-scene inspectable fields with typed widgets)
- The **Submit / live-apply** flow
- Connection status and message log

Out of scope: everything in `14` (scene model, registry, schema derive, IPC transport/protocol), the game's own rendering.

## Decisions (this draft)
- **Native egui window**, not a TUI or web app. Closest to Unity's docked-panel feel; avoids HTTP.
- **Separate process**, spawned by the game under `--inspect`, connecting over the Unix socket from `14`.
- **Bidirectional / live**: the inspector reflects the game's current field values and pushes edits back.
- **Schema-driven**: every widget is generated from the field schema in `14`; the inspector hard-codes no scene-specific UI.
- **Schema generation is automatic.** The field schema (`Inspectable`/`schema()`, per `14`'s M2 hook) is derived directly from each scene struct's fields via a `#[derive(Inspectable)]`-style macro, the same way `serde`'s `#[derive(Serialize, Deserialize)]` already does for wire types in this codebase — struct and schema always stay in sync. Exact derive grammar/attributes are TBD (see Open Questions).

---

## Key Details

### Form Factor & Tech
- egui via `eframe`, a single resizable window titled for the connected game (pid / socket).
- Lives in the `inspector` crate; depends on `scene-core` (from `14`) for the schema and IPC envelope types, so its widgets and the game's fields can't drift.
- Debug builds only. Not shipped in release.

### Launch & Connection Lifecycle
- The game, run with `--inspect`, binds the socket and spawns the inspector child with the socket path (see `14` → *Launch Model*).
- On start the inspector connects, receives `Hello`, and renders the catalog + the active scene's fields.
- Connection is shown in a status strip. On disconnect, the inspector greys out controls and attempts reconnect; the game keeps running regardless.

### Layout (Unity-like)
```
┌───────────────────────────────────────────────┐
│ Scene: [ Battle Viewer  ▼ ]   [ Go ]    ● live │  ← top bar (switcher + status)
├───────────────────────────────────────────────┤
│  ▾ Battle Viewer                                │
│      Background   [■ #c81e1e]                   │  ← field editors
│      Brightness   [────●──]  0.80               │     (schema-driven)
│      Caption      [ Round 3            ]        │
│                                                 │
├───────────────────────────────────────────────┤
│ [ Submit ]  [ Revert ]      ☑ apply on change   │  ← apply bar
├───────────────────────────────────────────────┤
│ › SwitchScene BattleViewer  ✓ ack               │  ← message log
└───────────────────────────────────────────────┘
```

### Scene Switcher
- Dropdown populated from the `Hello` catalog (`scenes[].name`), keyed by `SceneId`. Generated, never hand-listed.
- **"Go"** sends `SwitchScene { target }`. On the resulting `SceneChanged`, the field panel rebuilds for the new scene from its schema and is populated with the returned live `snapshot`.
- The dropdown also reflects gameplay-driven switches: an unsolicited `SceneChanged` (the game navigated on its own) updates the selection and panel.

### Field Editor (Schema-Driven)
The panel is built from the active scene's `schema()`, generated automatically from the struct (see *Decisions*). Each field's type tag selects a default widget; attributes refine it. Default mapping (mirrors `14`):

| Type tag | Widget |
|---|---|
| `bool` | checkbox |
| `int` | drag value; slider when `range` present |
| `float` | drag value; slider when `range` present |
| `string` | single-line text |
| `enum` | dropdown of variants |
| `color` | color picker (24-bit RGB) |
| `struct` | collapsible foldout (nested fields) |
| `list` | foldout list (read/edit values; add/remove TBD) |
| `asset` | path field + file picker |

Attributes from `14` are honored: `label` (display name), `range` (slider bounds), `readonly` (display only), `hidden` (omitted). Unknown/unsupported type tags fall back to a read-only JSON view so no field is ever undisplayable.

### Apply Flow
- Edits are **buffered** locally; changed fields are marked dirty (highlighted).
- **Submit** sends `ApplyState { id, patch }` containing only dirty fields; on `Ack` + `StateSnapshot` the buffer clears and values refresh from the game.
- **Revert** discards the buffer and re-reads the last snapshot.
- **Apply on change** toggle (`Subscribe { live: true }` + per-edit `ApplyState`) gives the continuous Unity-style behavior; off by default to avoid flooding.
- If the game pushes a `StateSnapshot` for a field the user is actively editing, the local edit wins until Submit/Revert (no clobbering mid-edit).

### Message Log
A scrolling log of sent commands and received events (type + seq + ack/error). Primary debugging aid for the IPC channel itself.

---

## Test Plan
Rides on `14`'s end-to-end test: connect → catalog shows the four example scenes → **Go** to each (screen recolors) → edit `background`/`caption` → **Submit** → change is visible in the game. Plus inspector-specific checks: widget selection per type, dirty highlighting, Revert, disconnect/reconnect.

## Open Questions / TBDs
- Collection editing (add/remove `Vec` elements) — deferred or v1?
- Presets: save/load a set of field values per scene?
- Undo/redo of applied changes?
- Multiple inspectors against one game, or one-to-one only?
- Triggering scene-specific debug *actions* (buttons/commands), not just field edits — future extension?
- Theming / window persistence (size, last scene) — nice-to-have.

## Dependencies
- `14-scene-architecture` — provides the scene catalog, per-scene field schema, IPC transport, and message protocol this GUI consumes. Hard dependency.
- `13-rendering` — only indirectly (the game it inspects renders through it).
