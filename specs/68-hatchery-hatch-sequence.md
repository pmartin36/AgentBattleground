# Hatchery — Hatch Sequence & Add to Roster

> **Status: draft (not started).** The animated hatch — the payoff moment — and everything after it: the escalating crack/break/reveal sequence, the creature reveal and stats panel, and the Add-to-Roster action. Third of the three hatchery specs — see `65-hatchery` (shell & egg lifecycle) and `67-hatchery-definition-generation` (definition & generation).

## Purpose
Play the full hatch when a ready egg is tapped into focus (`65`), reveal the creature `67` generated, and let the player add it to the active roster.

## Scope
- The escalating, non-interruptible hatch sequence and its reveal effects.
- Producing the crack/break art (approach decision + validation spike).
- The stats panel on reveal.
- The "Add to Roster" action, both when the roster has room and when it's full (full-roster case uses a discard stopgap — see that section).
- A dev-only force-hatch tool.

Out of scope: the egg tray/states/timer (`65`); the mad-lib definition and identity generation (`67`); the Farm/Playpen holding area (`needs-research/hatchery-farm-playpen.md`) — the full-roster case discards rather than depending on it, for now.

## Hatching Sequence
Tapping a **ready** egg into focus view plays an escalating, non-interruptible sequence:
1. An aggressive "about to hatch" wiggle — stronger than the tray's ready-state wiggle (`65`).
2. **Crack**: a crack forms down the middle. Cracks happen in fast, distinct bursts with a visible pause between each one — not one continuous crackling motion. See "Crack/Break/Reveal Production" below for how this is generated; the pacing itself is a hard requirement regardless of which approach is used.
3. **Break**: the egg splits into a few pieces, slow-motion, anime-break style.
4. **Reveal**: the creature fades in as a white flash silhouette, then lerps into its true color.
5. **Name reveal**, only after the color lerp completes.
6. The creature's idle animation plays, followed by its starting attack (every hatched creature has exactly one starting attack, per `67`).
7. A stats panel appears on the right, styled consistently with the existing roster detail panel (`48-roster-detail-panel-redesign`) — same visual language, not a new panel design.

### Crack/Break/Reveal Production
Two candidate approaches to actually producing steps 2-3's art; not validated against the "fast cracks, visible pauses" pacing requirement yet — resolve with a quick spike before committing, don't guess:
- **(a) A single generated clip via `66-asset-generation-api`'s `generate_animation`.** Prompt the whole crack-to-break sequence as explicit timed beats, following this project's proven prompt convention (concrete physical beats, not a vague description) — e.g. describing each crack event and the pause after it directly in the prompt. Risk: unconfirmed whether the model reliably hits an exact stutter-step rhythm from prompt text alone.
- **(b) Sequenced still images with game-driven timing.** Generate a handful of discrete crack-stage stills (intact → first crack → more cracks → burst) via `66`'s `generate_image`, with the game's own code controlling hold/pause durations between them. Likely more reliable for exact pacing than hoping a video generation lands the timing, at the cost of a more comic-panel, less fluid feel.

The white-flash-to-color lerp and the name reveal are presentation-layer effects applied on top of whichever art comes out of this step — neither generation approach needs to produce them itself.

## Post-Hatch: Add to Roster
Below the newly hatched creature, an "Add to Roster" action.
- **Roster has an open slot:** adds the new creature directly.
- **Roster is full:** the player picks an existing roster creature to make room. **For now, that creature is discarded (removed permanently)** — a stopgap. This is expected to change to a move-to-Farm/Playpen action once `needs-research/hatchery-farm-playpen.md` is resolved; don't build the discard path in a way that's awkward to swap out later (e.g. keep "pick a creature to bump" and "what happens to it" as separate steps).

## Dev Tooling
A dev-only tool to force-hatch a chosen egg immediately, skipping the 24-hour timer (`65`), so the hatch sequence can be tested on demand.

## Open Questions / TBDs
- Crack/break/reveal production approach, (a) vs (b) above — needs a validation spike.

## Dependencies
- `65-hatchery` — triggered by tapping a ready egg; the force-hatch tool acts on an egg and skips its timer.
- `67-hatchery-definition-generation` — supplies the creature identity (name, stats, abilities, starting attack) this sequence reveals.
- `66-asset-generation-api` — `generate_animation` for the hatch clip (if approach (a)), the idle, and the starting attack; `generate_image` for crack-stage stills (if approach (b)).
- `48-roster-detail-panel-redesign` — the stats panel visual consistency.
- `47-ability-and-instructions-data-model`, `55-combat-status-and-element-enums` — the starting-attack data shape.
- `needs-research/hatchery-farm-playpen.md` — future replacement for the roster-full discard stopgap; not a build blocker for this spec.
- `needs-research/local-generation-service-integration.md` — the hatchery is the feature that puts `66`'s remaining pre-ship gap (weight-redistribution licensing) directly in front of players (egg art, hatch animation, starting-attack animation all depend on `66`). Gated for shipping, same as `66` itself; not a blocker on writing/building now.
