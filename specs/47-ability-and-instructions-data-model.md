# Ability Combat Fields & Creature Instructions Data Model

## Status

Pending. Foundation spec for the roster detail-panel redesign. It adds the data the redesigned panel (`48-roster-detail-panel-redesign`), the ability tooltip (`49-ability-hover-tooltip`), and the prompt editor (`51-prompt-editor-popup`) render — nothing more. **No combat math, no LLM, no UI.** Just fields, enums, on-disk instruction files, and demo data.

## Purpose

Today `Ability` is `{ description, modifiers }` and `Creature` has no battle-instructions text. The redesigned roster detail panel needs to display, per ability, its stamina cost, type, element, damage class, damage, range, status effects, and flavor text — and needs each creature's battle instructions (the "skill file" from `03-army-skill-editing` / `12-data-model-sync`) as a human-editable file on disk. This spec introduces exactly those fields and the file plumbing, seeded with realistic demo data so the downstream UI specs have something real to render and verify.

This spec **resolves two open TBDs** carried by `03` and `12`: the skill-file format is **Markdown**, and there is **one file per creature**. `03`/`12` should reference this spec for those decisions.

## Scope

- New combat-taxonomy enums: `Element`, `AbilityType`, `DamageClass`.
- New `StatusEffect` value type.
- New optional combat fields on `Ability` + builders + getters.
- Per-creature instructions file: path scheme, directory resolution, autocreate, read/write helpers.
- Realistic demo data in `demo_roster()` so panel/tooltip/editor are visually verifiable.

**Out of scope:** any rendering or scene wiring (specs 48/49/51); combat effects of these fields (STR→damage etc. remain deferred to `10-battle-simulation-engine`); a concrete modifier catalog (still deferred, unchanged from `34`); the AI prompt-rewrite (deferred to the needs-research follow-up); external-file change detection (deferred, `03`); markdown *rendering* — instructions are stored and displayed as raw Markdown source.

## Data-Model Changes

All in `crates/game/` (this is game content — concrete elements/types are this game's combat vocabulary, not engine-general).

### 1. Combat-taxonomy enums

New in `crates/game/src/ability.rs` (or a sibling `combat.rs` re-exported from `lib.rs` — implementer's choice; keep them beside `Ability`). All are closed enums, **extendable later** — order is not load-bearing.

```rust
pub enum Element { Normal, Fire, Water, Earth, Lightning }
pub enum AbilityType { Attack, Buff, Debuff }
pub enum DamageClass { Physical, Magic }
```

- Each derives `Debug, Clone, Copy, PartialEq, Eq`.
- Each exposes a `label(&self) -> &'static str` returning the human string the UI prints (`"Fire"`, `"Attack"`, `"Physical"`, …). The tooltip and pills read `label()` — they never re-match the enum, keeping each enum the single source of truth for its own display text (same discipline as `StatKind`).

### 2. `StatusEffect`

```rust
pub struct StatusEffect { pub name: String }
```

`Debug, Clone, PartialEq, Eq`. **Only `name` for v1** — duration/magnitude/structured fields are deliberately deferred. It is a real named type (not a bare `String`) so the fields can grow without touching call sites.

### 3. Extend `Ability`

Add these fields to `Ability`, **all optional** so the tooltip shows only what a given ability defines (a `None` field / empty `status_effects` vec is simply omitted from the tooltip):

| Field | Type | Notes |
|---|---|---|
| `ability_type` | `Option<AbilityType>` | pill |
| `element` | `Option<Element>` | pill |
| `class` | `Option<DamageClass>` | pill |
| `cost` | `Option<u8>` | stamina cost; tooltip "Cost: N stamina" |
| `damage` | `Option<u32>` | tooltip "Damage: N" |
| `range` | `Option<u8>` | tooltip "Range: N" |
| `status_effects` | `Vec<StatusEffect>` | empty ⇒ omitted |
| `flavor` | `Option<String>` | 2-line flavor block |

Keep the existing `description` and `modifiers` fields unchanged. Preserve `Ability::new(description, modifiers)` as-is (all new fields default to `None`/empty), and add a **`with_*` builder per new field** (matching the `Creature`/`with_stats` idiom) so `demo_roster()` and tests set them fluently. Add a getter per new field. Fields stay private; the `MAX_MODIFIERS` invariant is unaffected.

### 4. Creature instructions file

The battle instructions are **not stored on `Creature`** — the on-disk file is the single source of truth (matches spec 51's "editor reflects the file, no save-on-close"). This spec provides only the path + read/write plumbing; caching for per-frame display is a scene concern (48/51).

New module `crates/game/src/instructions.rs` (declared in `lib.rs`):

- **Directory:** a subdirectory `creature_instructions/` under the game's base data directory. The base dir is resolved by a single helper so there's one definition:
  - Installed build: the directory containing the game executable (`current_exe().parent()`), mirroring how the game locates the sibling `inspector` binary.
  - Development: the workspace/repo root.
  - The resolver **must be overridable** (an explicit base-path argument, or a `AGENTBATTLEGROUND_DATA_DIR` env override) so tests target a temp dir and never touch the real repo tree.
- **Filename:** the creature's `name()` with spaces replaced by underscores, plus `.md`. `"Ember Wolf"` → `creature_instructions/Ember_Wolf.md`. No slugging/hashing — the files are meant to be found and hand-edited by players, so the name must stay legible. (Name-collision handling is out of scope; names are curated.)
- **Autocreate:** reading a missing file creates it as an **empty** file first, then returns `""`. The parent `creature_instructions/` directory is created if absent.
- **API** (free functions taking the creature name or `&Creature`):
  - `instructions_path(name) -> PathBuf`
  - `read_instructions(name) -> io::Result<String>` (autocreates empty if missing)
  - `write_instructions(name, contents) -> io::Result<()>` (creates dir/file as needed, overwrites)
- Add `creature_instructions/` to `.gitignore` — dev instruction files are local scratch, never committed.

### 5. Demo data

Update `demo_roster()` (`crates/game/src/creatures.rs`) so the panel/tooltip/editor have real content:

- Replace the placeholder `Ability::new("Placeholder ability 1", …)` entries with **abilities that exercise every new field**: give creatures multiple abilities (up to the `MAX_ABILITIES = 4` cap so the 2×2 grid fills), spanning different `AbilityType`/`Element`/`DamageClass` values, with representative `cost`/`damage`/`range`, at least one ability carrying `status_effects`, and flavor text on each. Across the roster, ensure at least one ability leaves some optional fields `None` so the tooltip's "omit when absent" behavior is demonstrable.
- These remain **non-canonical placeholders** (no balance intent), same disclaimer as the existing demo values.
- Seeding instruction-file *content* is not required here (files autocreate empty); optionally the demo may pre-populate a couple of `Ember_Wolf.md` etc. with sample Markdown so spec 48's preview isn't blank on first run — implementer's discretion, but if done it must go through `write_instructions` against the overridable base dir, never a hardcoded path.

## Decisions (v1)

Concrete and testable:

- **All new `Ability` combat fields are optional**; the UI omits any `None` field and any empty `status_effects`.
- **`Element` = {Normal, Fire, Water, Earth, Lightning}**, **`AbilityType` = {Attack, Buff, Debuff}**, **`DamageClass` = {Physical, Magic}** — closed but extendable; each owns its `label()` display string.
- **`StatusEffect` carries only `name`** this pass.
- **Instructions format is Markdown, one file per creature**, stored/displayed as raw source (no rendering).
- **Filename = name with spaces → underscores + `.md`**; no other transformation.
- **Base data dir is resolver-driven and test-overridable**; `creature_instructions/` is gitignored.
- **Missing file autocreates empty on read**; the file is the sole source of truth (no in-memory mirror on `Creature`).

## Constants

- `pub const INSTRUCTIONS_DIR: &str = "creature_instructions";`
- Reuse existing `MAX_ABILITIES` / `MAX_MODIFIERS`; no new caps.

## Testing Guidance

- `Element::Fire.label() == "Fire"` (and one case per enum) — confirms display strings.
- `Ability` builder round-trip: set each new field, read it back; a fresh `Ability::new(...)` leaves them `None`/empty.
- `instructions_path` maps `"Ember Wolf"` → a path ending in `creature_instructions/Ember_Wolf.md`.
- `read_instructions` on a missing name (under a temp base dir) **creates the file** and returns `""`.
- `write_instructions` then `read_instructions` round-trips content.
- All file tests run against an **overridden temp base dir** — no test reads or writes the real repo `creature_instructions/`.
- `demo_roster()` yields at least one ability with each new field populated **and** at least one with some optional field left `None` (so 48/49 can be verified against real absence, not just presence).

## Open Questions / TBDs

None outstanding for this spec. Deferred elsewhere: combat effects of these fields (`10`), concrete modifier catalog (`34`/future), AI prompt-rewrite (needs-research follow-up), name-collision policy for instruction filenames.

## Dependencies

- Extends `34-creature-attributes-data-model` ✅ — adds fields to the same `Ability`/`Creature`.
- Aligns with `46-post-battle-results-screen` — uses the already-landed `Stamina` type/semantics.
- Resolves format/one-per-piece TBDs from `03-army-skill-editing` and `12-data-model-sync`.
- Feeds `48-roster-detail-panel-redesign`, `49-ability-hover-tooltip`, `51-prompt-editor-popup`.
