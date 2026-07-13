# Combat Status & Element Enums

## Status

Pending. Small data-model change extending `47-ability-and-instructions-data-model`. Replaces the free-text `StatusEffect { name }` with a concrete `StatusKind` enum and renames one `Element` variant, so the ability tooltip (`49`) and the `@` mention system (`56`) reference real combat values instead of free text.

## Purpose

`47` shipped `StatusEffect { name: String }` (free text) and `Element { Normal, Fire, Water, Earth, Lightning }`. The `@` mention vocabulary (`56`) needs a **closed, referable** set of statuses (you can't autocomplete or validate free text), and the owner wants **Ice** rather than **Water** (frozen comes from ice). This makes both concrete.

**Kind vs. applied instance.** A status has two levels: the **kind** (Burn/Frozen/…) and, in battle, an **applied instance** (kind + duration/magnitude/stacking). Everything authoring-side — the ability's "applies these", the tooltip, `@self:frozen` — only ever needs the **kind**, which is genuinely just an enum. The richer applied instance is combat state, deferred to `10-battle-simulation-engine`, and will *wrap* this enum (e.g. `ActiveStatus { kind: StatusKind, turns: u8 }`). This spec defines only the kind.

## Data-Model Changes

`crates/game/src/ability.rs` (beside `Element`/`AbilityType`/`DamageClass`):

### 1. `StatusEffect { name }` → `StatusKind` enum
Replace `struct StatusEffect { name: String }` with a closed enum of status kinds:
```rust
pub enum StatusKind { Burn, Frozen, Shocked, Rooted }
```
- Derives `Debug, Clone, Copy, PartialEq, Eq` and owns a `label(&self) -> &'static str` (`"Burn"`, `"Frozen"`, `"Shocked"`, `"Rooted"`) — same discipline as the other combat enums; the tooltip reads `label()` rather than the old `.name`.
- Closed but extendable (more kinds land with combat, `10`).
- This is the status **kind only** — the applied-with-duration instance is a separate struct reserved for `10` (see Purpose); the name `StatusEffect` is left free for it.

### 2. `Element`: `Water` → `Ice`
`Element` becomes `{ Normal, Fire, Ice, Earth, Lightning }`. Update `element_color` (`crates/game/src/scenes/roster_manager/tooltip.rs`): the Ice pill color is a light cyan/ice-blue (replace the old Water blue — tunable). Update every `Element::Water` reference.

### 3. Ripple updates
- `Ability::status_effects: Vec<StatusKind>` now holds the enum (field name unchanged); `with_status_effects`/getter unchanged in shape.
- `demo_roster()`: status effects now use `StatusKind` variants; any `Element::Water` demo values become `Element::Ice`. Existing free-text demo statuses map to the nearest variant — **`Bleed`→`Burn`** (treated as the same effect; no `Bleed` variant is added), `Shock`→`Shocked` — the rest already match `{Burn, Frozen, Shocked, Rooted}`.
- Tooltip status rendering (`fill_status` in `tooltip.rs`) prints `kind.label()` instead of `effect.name`.
- Update all tests that constructed `StatusEffect { name: .. }` or referenced `Element::Water`.

## Decisions (v1)

- `StatusKind = { Burn, Frozen, Shocked, Rooted }` (closed, extendable), each owns `label()`. It is the status **kind**; the combat applied instance (duration/magnitude) is `10`'s, and `StatusEffect` is reserved as its name.
- `Element` renames `Water` → `Ice`; Ice pill color is ice-blue (tunable).
- No combat *effects* of statuses here — that's `10`; this is naming/shape only.

## Testing Guidance

- `StatusKind::Frozen.label() == "Frozen"` (one per variant).
- `Element::Ice` exists; no `Element::Water` remains (compile-enforced); Ice pill color decodes to the chosen ice-blue.
- `demo_roster()` abilities carry `StatusKind` statuses; the tooltip renders their labels (e.g. an ability with `Burn` shows `"Burn"`).

## Dependencies

- Extends `47-ability-and-instructions-data-model`.
- Updates `49-ability-hover-tooltip` (status labels + Ice color).
- Foundation for `56-at-mention-authoring` (the status/element vocabulary).
- Combat effects / the applied `StatusEffect` instance deferred to `10-battle-simulation-engine`.
