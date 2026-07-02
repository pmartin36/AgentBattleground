# Piece Identity Data Model

> **Status: draft (not started).** The first real slice of a piece's eventual data model: a name and a catalog of named animations ("sprite data" — what animations exist, how to play one). A narrow foundational cut of `03-army-skill-editing` and `12-data-model-sync`'s fuller vision, in the same spirit as `21-mouse-hover-input` — build the piece this round's actual consumer (`24-roster-carousel`) needs, shaped so it survives being reused later rather than thrown away.

## Purpose
Every piece the player owns needs an identity beyond "the wizard placeholder, tinted": a name, and a way to look up "play this creature's idle animation" (and, later, its attack/hurt/death animations, without redesigning the type when those are added). This spec defines that shape and populates it with 6 real, distinct creatures — the first non-placeholder art in the game.

## Scope
- A new type pairing a creature's `name: String` with a small catalog of named animations, each resolvable to a playable `render::AnimatedSprite`.
- The animation-name concept (`AnimationKind` or equivalent) is designed to grow — this build only populates `Idle`, but adding `Attack`/`Hurt`/`Death` later must not require restructuring the type, only adding entries.
- 6 concrete creature instances, each with a real name and a real idle animation, bundled into the binary (`include_bytes!`, same pattern as the existing `wizard.gif`) — produced via `experiments/creature_lab`'s `generate.sh` (high-detail + simplified/background-removed source images) and `animate.sh` (idle loop), **not** the in-app generation UI `17-creature-art-asset-pipeline` describes (that spec's own scope — in-app generation, drag-n-drop import — remains entirely pending; this is offline content production using its reference prototype, exactly as `17`'s own "Reference Prototype" section describes `creature_lab`).
- Lives in the `render` crate as a new module (e.g. `render::creature`), **not** `scene-core`: `scene-core` has no dependency on `render` today (confirmed — `render` depends on `scene-core`, not the reverse) and must not gain one, since `inspector` depends on `scene-core` alone and has no reason to pull in image/animation decoding just for field-schema data. `render` already depends on `scene-core` and already hosts `AnimatedSprite` — the natural home for a type that bundles named `AnimatedSprite`s.

Out of scope:
- Migrating `BattleViewer` to consume this type — `BattleViewer`'s own `Piece`/`AnimatedSprite` usage is untouched this round. A future migration is expected but not built here.
- Stats, abilities, skill files, upgrade history — spec 03's fuller vision, entirely pending.
- More than the `Idle` animation — no attack/hurt/death animations are produced or wired up this round, only the extensible shape for them.
- Persistence — the 6 creatures are compiled-in bundled data, not loaded/saved from disk.

## Decisions (v1)
- **The 6 creatures** (names + generation concept — see `24-roster-carousel` for the scene that displays them; actual bundled art may deviate from the prompt below if generation quality requires iteration, per `creature_lab`'s own judged-by-eye workflow):
  1. **Ember Wolf** — a wolf wreathed in low flame, quadruped.
  2. **Frost Lizard** — a crystalline ice lizard, quadruped, low profile. (`creature_lab` already has a prior `frostlizard` prototype run in `out/` — reuse if it holds up, regenerate if not.)
  3. **Stone Golem** — a hulking rock-construct humanoid, broad silhouette.
  4. **Storm Hawk** — a lightning-crackling hawk, winged silhouette.
  5. **Verdant Treant** — a small animate tree/plant guardian, spindly-limbed silhouette.
  6. **Shadow Cat** — a sleek dark-wisped panther, low sleek silhouette.
  Deliberately silhouette-diverse (quadruped/hulking/winged/spindly/sleek) — per `creature_lab`'s README, simple, distinct shapes are what survive being crushed down to small braille sizes; six similar-silhouette creatures would read as visual mush next to each other.
- **`AnimationKind` is a plain enum with only `Idle` as a variant today**, documented as "add variants here, don't restructure" rather than a stringly-typed map — keeps compile-time exhaustiveness for the one kind that exists now while making the extension path explicit.
- **Bundled, not loaded.** All 6 creatures' image bytes are `include_bytes!`-embedded at compile time, matching the existing `wizard.gif` precedent — no runtime file loading, no asset directory scanning.

## Dependencies
- `13-rendering` ✅ — `render::AnimatedSprite`, which this type wraps/catalogs.
- `17-creature-art-asset-pipeline` — this spec's asset *production* uses that spec's own reference prototype (`experiments/creature_lab`) directly; `17`'s actual scope (in-app generation, import UI) remains unbuilt and untouched.
- Feeds `24-roster-carousel` — the scene this data model exists to serve.
- Feeds a future `BattleViewer` migration (not built here) and the fuller `03-army-skill-editing` data model.
