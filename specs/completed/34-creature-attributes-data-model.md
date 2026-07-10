> # ✅ DONE! — Completed 2026-07-09
> Status: implemented. Confirmed against the actual codebase: `stats.rs` (`StatKind`/`Stats`), `ability.rs` (`Modifier`/`StatRequirement`), `exhaustion.rs` (percent + injured-state transition), and `squad_role.rs` (slot-count constants + positional role lookup) each exist, match this spec's scope, and carry their own unit tests (22 `#[test]` fns across the four files). Filed here retroactively — the implementation predates this filing.

# Creature Attributes & Progression Data Model

## Purpose
Every creature is about to become more than a name and an idle animation: it has stats, a level, up to 4 abilities (each carrying up to 4 modifier tags), an exhaustion meter, and a position in the 6-creature roster that determines whether it's active/bench/reserve. This spec defines that data shape. It is data-model only — no new rendering, no roster-screen changes (that's `35-roster-screen-stats-abilities-squad`), no combat math, no concrete modifier catalog.

## Scope
- `Stats`: `strength`, `dexterity`, `intelligence`, `vitality` (displayed as STR/DEX/INT/VIT) as plain numeric fields.
- `level`: a plain numeric field, no XP/leveling mechanic (that's `06-post-battle-upgrade`, explicitly undesigned — see Out of scope).
- `Ability`: up to 4 slots per creature, each with a short description and up to 4 `Modifier` tags.
- `Modifier`: a named/id "shape only" type — a pool sized 36, but this spec does NOT enumerate concrete modifiers or their effects (see Out of scope). It does define the structural mechanism by which a modifier can require a stat threshold to unlock (e.g. "requires `strength >= 30`"), since that's a data-shape decision, not a content decision.
- `Exhaustion`: a 0-100 meter, plus an "injured" state entered when it maxes out — a recovery duration during which the creature cannot be set active, and (per the project owner's "injured list" framing) is moved out of the active/bench slots into a reserve slot for the duration.
- Squad role: purely positional. The roster's ordering IS the role assignment — no independent role field. This spec defines the slot-count constants and a pure lookup from index to role; the actual player-facing swap interaction lives in `35`.

Out of scope:
- Any mechanical effect of stats on combat (rough intent — STR→damage, DEX→hit chance, INT→buff/debuff strength, VIT→health — is documented below as *intent*, not implemented; real combat math is `10-battle-simulation-engine`'s job, once it exists).
- The concrete 36-modifier catalog and their effects. Only the shape (a modifier is nameable, and can gate on a stat threshold) is built.
- XP/leveling mechanic — `level` is a field that exists and displays; how it increases is `06-post-battle-upgrade`'s undesigned progression loop.
- Mid-battle exhaustion-triggered bench↔active swaps — per the project owner, this will be skill-driven (the LLM decides), not player-driven, and has no design yet. This spec only defines the data transition (a creature crossing the exhaustion threshold becomes injured and drops to reserve); nothing in this round *triggers* that transition during a real battle, since no combat exists yet to cause damage/exhaustion. It's exercised by unit tests (and optionally the inspector), not gameplay.
- Server sync / persistence format for any of this (`11-server-backend`, `12-data-model-sync` — both still draft).

## Decisions (v1)
- **Lives directly on `Creature` in `crates/game`, no separate wrapper type.** `Creature` (name + `AnimationKind` → `AnimatedSprite` catalog) is `crates/game/src/creatures.rs`'s own type — `engine_render` retains only the generic `AnimatedSprite` primitive. Stats/abilities/modifiers/exhaustion are Agent-Battleground-specific RPG mechanics with no meaning to a different game, so per the engine/game boundary rule they belong in `crates/game` — but since `Creature` itself already lives there (not in `engine_render`), there is no remaining reason to hold this data behind a second wrapper type (an earlier pass introduced one, `RosterEntry`, before catching that its own justification had been overtaken by the `Creature` move — corrected: the project owner flagged the name as oddly roster-screen-specific for what is really "a creature's full battle data," and on inspection the wrapper no longer served a purpose). `Creature` gains `stats: Stats`, `level: u32`, `abilities: Vec<Ability>`, `exhaustion: Exhaustion` as direct fields, set via `with_stats`/`with_level`/`with_abilities`/`with_exhaustion` builders (same builder style as the existing `with_animation`). Squad role stays purely positional (see below) — not a field on `Creature` either.
- **`Stats` fields are plain `u32`s**, no computed current/max split — nothing modifies them at runtime this round (no combat exists to do so).
- **Placeholder demo values.** The 6 bundled creatures each get illustrative, distinguishing stat values (so the roster screen's 4 bars actually look different per creature) — e.g. Ember Wolf skews STR/DEX, Stone Golem skews VIT. These are non-canonical placeholders, the same spirit as the bundled wizard sprite / pastel team tints elsewhere in this codebase — not balanced game design.
- **`Ability` shape**: `{ description: String, modifiers: Vec<Modifier> }`, with `modifiers.len() <= 4` as a documented invariant (debug-asserted at construction, not a fixed-size array — keeps the type ergonomic for a "shape only" build with no real content yet). A creature has `Vec<Ability>` with `len() <= 4` under the same convention.
- **`Modifier` shape**: `{ name: String, requires: Option<StatRequirement> }`, `StatRequirement { stat: StatKind, threshold: u32 }`, `StatKind` a 4-variant enum (`Strength`/`Dexterity`/`Intelligence`/`Vitality`) — the single source of truth both `Stats` field access and `StatRequirement` reference, so a future modifier catalog and any bar-drawing code share one enum rather than two independently-hardcoded stat lists. No concrete modifiers are populated for the 6 bundled creatures beyond placeholder names (e.g. `"Modifier A"`), documented as non-canonical.
- **`Exhaustion` shape**: `{ percent: u8, injured_until: Option<Duration> }` (or equivalent) — `percent` is 0-100; `injured_until` is `None` normally, `Some(remaining)` once `percent` reaches 100. A pure function (e.g. `apply_damage_exhaustion` / `apply_ability_use_exhaustion`, names TBD at implementation) models "using an ability costs exhaustion, taking damage costs exhaustion, reaching 100 enters the injured state and forces a reserve reassignment" as documented intent + a testable pure transition — not wired to any live trigger.
- **Squad role constants**: `ACTIVE_SLOTS = 3`, `BENCH_SLOTS = 1`, `RESERVE_SLOTS = 2` (total 6 — unchanged from the existing "6 pieces per player" decision). `fn squad_role(index: usize) -> SquadRole { Active, Bench, Reserve }` is a pure function of position in the roster `Vec`, matching the "purely positional" decision from the project owner — no independent role field to keep in sync.

## Open Questions / TBDs
- Concrete mechanical effect of each stat on combat (deferred to `10`).
- The actual 36 modifiers and their effects (deferred to a future spec, once combat mechanics exist to design them against).
- XP/leveling source (deferred to `06`).
- What triggers the exhaustion-driven mid-battle swap, and how the LLM/skill system decides it (deferred — no design yet, per the project owner).

## Dependencies
- `23-piece-identity-data-model` ✅ — the `Creature` (name + animation) this pairs with.
- Feeds `35-roster-screen-stats-abilities-squad` — the data this spec's types back.
- Feeds `36-battle-viewer-squad-layout` — squad-role constants inform which creatures are on-board vs. bench vs. not-rendered.
- Referenced by (not blocking) `03-army-skill-editing`, `10-battle-simulation-engine`, `12-data-model-sync`, `06-post-battle-upgrade` — see their updated Dependencies sections.
