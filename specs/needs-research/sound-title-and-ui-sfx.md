# Title & UI Sound Effects (needs research)

> **Status: parked pending research.** Sound for the animated title logo
> (`61-animated-title-logo`) and, more broadly, UI/scene sound effects. The
> title ships **silent** in v1; this captures the future audio pass. Unnumbered
> until scoped, per the `needs-research/` convention.

## Purpose

Give the animated title logo (`61-animated-title-logo`) sound — a metallic
sword `shing` as it falls and a heavy clang/thud on impact as it seats into the
stone, ideally with the BATTLES ignite and sparkles reinforced audibly — and,
more generally, establish the pattern for UI/scene sound effects that other
scenes reuse. The title is the natural first, self-contained case.

## Why it's parked

- **Engine audio surface.** `57-engine-audio-api` defines the audio API; this
  needs the concrete "play a one-shot SFX, timed to an animation beat" pattern
  on top of it (or a gap to fill). Whether SFX are fire-and-forget from the
  scene's `update`, or scheduled against the animation clock, is unresolved.
- **Beat-synced timing.** The title's beats are per-beat start/end constants
  (`61` Decision 8). Sounds must line up with fall / impact / ignite / sparkle
  without drifting from the visual timeline — how the audio clock relates to the
  frame clock needs to be pinned.
- **Asset pipeline & licensing.** Where SFX assets live, format, how they're
  bundled (the game deletes the title PNG in `61`; art/audio bundling policy for
  procedural scenes is open), and sourcing/licensing.
- **Mix / settings.** Master + SFX volume, a mute, and how this ties into
  `09-settings-model-config`. First-run plays the title once (`61` Decision 7) —
  does audio respect a not-yet-set volume preference?
- **Scope creep into a UI-SFX system.** Doing this well means a small reusable
  "scene plays a named SFX" facility, not a one-off in the hub. That is a design
  decision, not a guess.

## Research questions to resolve before speccing

1. One-shot SFX API shape on top of `57-engine-audio-api` — fire-and-forget vs.
   beat-scheduled, and where it's driven from (scene `update`).
2. Keeping audio beats locked to the visual timeline (`61` Decision 8) without
   drift.
3. SFX asset format, bundling, and sourcing/licensing.
4. Volume/mute/settings integration (`09-settings-model-config`) and first-run
   behavior.
5. Whether to build a reusable UI-SFX facility now vs. a title-only one-off.

## Dependencies / relationships

- `61-animated-title-logo` — the first consumer; the title's beat timings are
  the sync targets.
- `57-engine-audio-api` — the engine audio surface this builds on.
- `needs-research/audio-v2.md` — the broader audio effort; fold this in or keep
  UI-SFX as its own track (open).
- `09-settings-model-config` — volume/mute settings.
