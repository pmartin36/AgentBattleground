# Debug Inspector — Advanced Editing

> **Status: draft (not started).** Cut out of `15-debug-inspector` to keep that spec a complete, buildable unit. Covers two field-editor capabilities `15` explicitly excludes: the `asset` widget and dynamic (add/remove) collection editing. Neither has a current trigger to build against — this spec is parked until one exists.

## Purpose
Extend the schema-driven field editor (`15-debug-inspector`) with the capabilities it deliberately left out because nothing in the codebase could validate them yet.

## Scope
- The `asset` widget: a path field + file picker for fields that reference an on-disk asset.
- Dynamic collection editing: add/remove elements of a `list`-tagged field (`15` supports only fixed-size lists — read/edit existing elements, no resize).

Out of scope: everything else in `15`'s field editor (already covered there).

## Why This Is Cut From `15`, Not Just Deferred Inline
- **`asset`**: no scene currently has a field that references a runtime-loaded asset path — sprites are `include_bytes!`-embedded at compile time. There is nothing to point the widget at, so it can't be validated end-to-end. Unblocked once a real asset-path field exists (most likely `17-creature-art-asset-pipeline`, when creature art is loaded from disk rather than embedded).
- **Dynamic collection sizing**: no scene has a collection that needs to grow/shrink through the inspector. Army/roster size is a fixed design constant (6 pieces per player, project-wide), not a variable the inspector would ever need to resize. Unblocked only if some future scene introduces a genuinely variable-size collection that needs live editing.

## Open Questions / TBDs
- Everything — no design work has started. In particular: does the `asset` widget validate the path (file exists / correct type) before allowing Submit? Native file dialog vs. path-as-text? For dynamic lists, does add insert a default-constructed element, and does remove need a confirmation step?

## Dependencies
- `15-debug-inspector` — extends its `Inspectable` schema, widget system, and `ApplyState` apply flow; cannot be built before `15`.
- `17-creature-art-asset-pipeline` — likely first real trigger for the `asset` widget.
