# Local Player-Data Store

> **Status: draft (not started).** The tamper-resistant on-disk store for player-owned data: the roster today, eggs next, more player state later. Replaces the hardcoded `demo_roster()` with a file read, loads on boot, saves on change. A foundational slice carved out of `12-data-model-sync`'s "Local Data (Player's Machine)" concern so it can be built now without building all of `12`; `12` keeps the broader data model, server sync, replays, and opponent packaging.

## Purpose
Give the game one place to load and save everything the player owns, so a creature earned or an egg incubating survives a restart, and so that state can't be casually hand-edited to cheat. Every feature that owns persistent player state (the roster, `65`'s eggs) reads and writes through this store instead of hardcoding or re-implementing persistence.

## Scope
- The on-disk save file: location, format, load, and save.
- Integrity (detect tampering) and recovery (survive a bad or corrupt file).
- The serializable schema for the persisted `Creature` (roster) and `Egg`.
- Loading on boot (replacing `demo_roster()` with a file read) and saving on change.
- First-run seeding when no save exists.

Out of scope: server sync, replays, opponent-data packaging, and the network data model (`12`); the creature-attribute *semantics* (`34`); the hatchery UI and egg lifecycle behavior (`65`).

## The Store
- **Location**: a per-user application data directory (exact path a TBD below). Not alongside the binary.
- **Format**: a serialized blob (binary, e.g. bincode) carrying an **HMAC signature** over its content, keyed by a secret baked into the binary. This deters casual save-editing. It is explicitly **not** cryptographic anti-cheat: the key ships in the binary, so a determined user who extracts it can forge a valid signature. Real anti-cheat is server-side validation of battle outcomes (`11-server-backend`), not this file.
- **Integrity + recovery**:
  - **Atomic writes** — write to a temp file, flush, then rename over the real file, so a crash mid-save never leaves a half-written store.
  - **Last-known-good backup** — before overwriting, keep the prior valid save as a signed `.bak`.
  - **On load** — verify the main file's HMAC; on mismatch (hand-edit or corruption), fall back to the `.bak` and verify it; if both fail, fall back to a fresh default seed. A rejected hand-edit therefore restores the player's last legitimate state rather than losing it.

## Schema
The store serializes a single root:

```rust
PlayerData {
    roster: Vec<Creature>,   // squad position is the ordering, not a stored field (per 12)
    eggs:   Vec<Egg>,
    // player profile / credentials / model config may move here later (see Open Questions)
}
```

### Creature (persisted form)
A serializable representation of a `Creature` — a data form distinct from the runtime `crates/game` struct, which keeps its decoded sprites and is not itself made `Serialize`. Load converts the persisted form into a runtime `Creature`; save converts the other way. Two points:
- **Add `element: Element` to the runtime `Creature`** — so an egg's type maps onto the creature it hatches. Today `Element` lives only on abilities; the creature needs its own. The persisted form carries it too.
- **Art is stored as references, not decoded sprites.** The runtime `Creature` holds decoded `AnimatedSprite`s; the persisted form holds `66-asset-generation-api`'s asset handles (still image, idle clip, attack clip). Load resolves the handles back into sprites; save writes only the handles.

Persisted fields: `name`, `element`, `stats`, `level`, `xp`, `abilities` (≤4), `stamina`, and the art handles.

### Egg
A thin wrapper nesting an optional `Creature` (the hatchling), split out into `roster` on Add-to-Roster (`68`):

```rust
Egg {
    element:   Element,              // type -> tint (element_color)
    state:     EggState,             // Undefined | Incubating { started_at } | Ready
    mad_lib:   Option<String>,       // the completed prompt; None until defined
    egg_art:   Option<AssetRef>,     // the tinted egg sprite; None until generated
    hatchling: Option<Creature>,     // the nested creature; None until generated
}
```

`Incubating { started_at }` persists the timer's start, so `65`'s 24-hour countdown resumes correctly across restarts.

## Load & Save
- **On boot**: load `PlayerData` from the store. If no save exists, seed a default (the current `demo_roster()` content becomes that first-run seed) and write it. The `demo_roster()` call site is replaced by this load.
- **On change**: any mutation of the roster or eggs saves through the atomic-write + backup path above.

## Open Questions / TBDs
- Exact save-file directory and name.
- Save cadence: every mutation vs. debounced / on-scene-exit.
- Whether player profile, credentials, and model config (listed under `12`'s Local Data) move into this store now or stay separate for now.
- The exact `AssetRef` shape for persisted art handles — align with `66-asset-generation-api`.

## Dependencies
- `66-asset-generation-api` — the asset-handle types the persisted `Creature`/`Egg` store instead of decoded sprites.
- `12-data-model-sync` — the broader local/server data model this realizes a persistence slice of; server sync stays `12`'s.
- `34-creature-attributes-data-model` — the `Creature` stats/level/abilities/stamina fields this serializes (and the `element` addition extends).
- Consumed by the roster (replaces `demo_roster()`) and by `65-hatchery` (egg persistence), and the foundation future player-state persistence builds on.
