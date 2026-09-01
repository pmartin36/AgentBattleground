> # ✅ DONE! — Completed 2026-09-01

# Hatchery — Hatch Sequence & Add to Roster

> **Status: done.** The animated hatch — the payoff moment — and everything after it: the escalating crack/break/reveal sequence, the creature reveal and stats panel, and the Add-to-Roster action. Third of the three hatchery specs — see `65-hatchery` (shell & egg lifecycle) and `67-hatchery-definition-generation` (definition & generation).
>
> **Superseded in part by `72-hatch-reveal-and-roster-placement`:** the reveal render (flash/color/name/idle/attack), the stats panel, and the Add-to-Roster action are redesigned there (roster-style layout, idle-only, name fade-in, slide-in stats dock with Keep/Discard, and full-generation gating with no placeholder). The wiggle/crack/break sequence, clip generation, and dev hotkeys below remain current.

## Purpose
Play the full hatch when a ready egg is tapped into focus (`65`), reveal the creature `67` generated, and let the player add it to the active roster.

## Scope
- The escalating, non-interruptible hatch sequence and its reveal effects.
- Producing the crack/break art (approach decision + validation spike).
- The stats panel on reveal.
- The "Add to Roster" action, both when the roster has room and when it's full (full-roster case uses a discard stopgap — see that section).
- Dev-only hatchery debug hotkeys: force-hatch and force-create-egg.

Out of scope: the egg tray/states/timer (`65`); the mad-lib definition and identity generation (`67`); the Farm/Playpen holding area (`needs-research/hatchery-farm-playpen.md`) — the full-roster case discards rather than depending on it, for now.

## Hatching Sequence
Tapping a **ready** egg into focus view plays an escalating, non-interruptible sequence:
1. An aggressive "about to hatch" wiggle — stronger than the tray's ready-state wiggle (`65`).
2. **Crack**: a crack forms down the middle. Cracks happen in fast, distinct bursts with a visible pause between each one — not one continuous crackling motion. See "Crack/Break/Reveal Production" below for how this is generated; the pacing itself is a hard requirement regardless of which approach is used.
3. **Break**: the egg splits into a few pieces, slow-motion, anime-break style.
4. **Reveal**: the creature fades in as a white flash silhouette, then lerps into its true color.
5. **Name reveal**, only after the color lerp completes.
6. The creature's idle animation plays, followed by its starting attack (every hatched creature has exactly one starting attack, per `67`).
7. A stats panel appears on the right, styled consistently with the existing roster detail panel (`48-roster-detail-panel-redesign`): the same visual language, not a new panel design. It appears once the creature is revealed and named (from step 5 onward) and persists through the idle and attack; it does not wait for the attack to finish.

### Crack/Break/Reveal Production
One **shared** crack/break animation, reused for every hatch, composited over whatever egg sprite is currently showing. The cracks are black, so the animation is a black-on-transparent overlay laid on top of the current egg regardless of that egg's art or element: no per-egg generation and no tinting, the same overlay reads correctly on any egg. Because it is simple black-line art, it is a single shared bundled authored frame sequence and needs no GPU. The asset is bundled at `crates/game/src/assets/egg_crack.gif` (black jagged cracks on transparent), decoded via `AnimatedSprite::from_gif` the same way the creature idle GIFs (`crates/game/src/creatures/*.gif`) are. The break transitions into the white-flash reveal below.

The overlay supplies the crack art; the game supplies the pacing. The "fast cracks, visible pauses" requirement is met by the game controlling playback over the overlay's frames: advance through a burst of frames quickly, hold on a frame for a pause, advance the next burst, and repeat (play for X ms, hold for Y ms, repeat). The stutter-step rhythm lives in the game's frame-stepping, so it is exact.

The white-flash-to-color lerp and the name reveal are presentation-layer effects applied on top; the crack overlay does not produce them itself.

### Clip generation timing
Two **per-creature** clips are generated per egg beyond `67`'s egg still: the creature's idle and its starting-attack clip (both via `66`'s `generate_animation` over the creature's still). They are generated **during incubation**: when an egg enters `Incubating` (`67`'s Done), this feature kicks their generation off in the background, once per egg (idempotent), targeting readiness by the time the egg is `Ready` 24 hours later. The incubation window is the generation window, so the player never waits mid-hatch for a ~3-minute clip.

The crack/break overlay (above) is not part of this per-egg work: it is one shared bundled asset, egg-agnostic, needing no per-egg or GPU generation. If a per-creature clip is not ready when the hatch plays (a dev force-hatch that skipped the timer, or no GPU), the sequence uses the same placeholder fallback as the rest of `66`'s no-GPU path rather than blocking.

### Implementation placement
Hatch logic (sequence state machine, phase timing, frame-stepping, reveal effects) lives in new `hatchery/hatch*` submodules; `hatchery/mod.rs` gets only thin hooks, since it is already near the file-size budget. Frame decoding of a `ClipAsset` reuses the single existing decode site (`resolve_clip`), promoted to shared use rather than reinvented per consumer.

## Post-Hatch: Add to Roster
Below the newly hatched creature, an "Add to Roster" action.
- **Roster has an open slot:** adds the new creature directly.
- **Roster is full:** the player picks an existing roster creature to make room. **For now, that creature is discarded (removed permanently)** — a stopgap. This is expected to change to a move-to-Farm/Playpen action once `needs-research/hatchery-farm-playpen.md` is resolved; don't build the discard path in a way that's awkward to swap out later (e.g. keep "pick a creature to bump" and "what happens to it" as separate steps).

## Dev Tooling
Dev-only hatchery commands, all gated behind `cfg!(debug_assertions)` so they are compiled out of release builds and can never reach players. Each is an in-scene **debug hotkey** in the hatchery, on its own distinct key; they live in the same place, establishing the hatchery's debug-hotkey pattern.
- **Force-hatch:** with an egg focused, the key sets that egg `Ready` immediately (skipping `65`'s 24-hour timer) and plays the hatch sequence, so the sequence can be tested on demand. If the egg's hatch clips are not yet generated, the sequence uses the placeholder fallback (per Clip generation timing).
- **Force-create-egg:** the key adds a new `Undefined` egg to the tray, persisted like any other (`Egg` with a default element, no mad-lib, no art), so the define -> incubate -> hatch loop can be exercised repeatedly on demand without waiting on real egg acquisition.

## Open Questions / TBDs
- The exact playback cadence for the crack/break clip (the per-burst play duration X and per-pause hold duration Y, and how many bursts) is a tuning detail to dial in against the generated clip; the game-driven-frame-stepping approach itself is settled.

## Dependencies
- `65-hatchery` — triggered by tapping a ready egg; the force-hatch tool acts on an egg and skips its timer.
- `67-hatchery-definition-generation` — supplies the creature identity (name, stats, abilities, starting attack) this sequence reveals.
- `66-asset-generation-api` — `generate_animation` for each creature's idle and starting-attack clips (generated during incubation over the creature's still). The crack/break overlay is a shared bundled asset, not generated per egg. `generate_image` produced the egg still in `67`.
- `48-roster-detail-panel-redesign` — the stats panel visual consistency.
- `47-ability-and-instructions-data-model`, `55-combat-status-and-element-enums` — the starting-attack data shape.
- `needs-research/hatchery-farm-playpen.md` — future replacement for the roster-full discard stopgap; not a build blocker for this spec.
- `needs-research/local-generation-service-integration.md` — the hatchery is the feature that puts `66`'s remaining pre-ship gap (weight-redistribution licensing) directly in front of players (egg art, hatch animation, starting-attack animation all depend on `66`). Gated for shipping, same as `66` itself; not a blocker on writing/building now.
