# Hatchery

> **Status: draft (not started).** Lets players grow new creatures from eggs: define an egg via a mad-lib prompt, wait out an incubation timer, then hatch it into a new creature with generated art, stats, and a starting attack. The roster-full "Add to Roster" path uses a **discard** stopgap for now — see that section — pending the Farm/Playpen holding area (`needs-research/hatchery-farm-playpen.md`).

## Purpose
A creature-acquisition loop distinct from whatever the primary onboarding/roster-building flow is (`01`, `02`): players collect eggs, define what hatches from them via a free-text mad-lib prompt, wait out a real-time incubation timer, then watch it hatch and decide whether to add it to their active roster.

## Scope
- A Hatchery tab/entry point reachable from the Roster Manager (`25`, `48`).
- The egg tray: undefined eggs and incubating eggs, both shown together.
- The mad-lib definition flow, including meta-generated (varying) sentence templates.
- The hatch interaction and its full animated sequence.
- The post-hatch reveal (art, name, idle + starting attack, stats panel).
- The "Add to Roster" action, both when the roster has room and when it's full (full-roster case uses a discard stopgap — see that section).
- A dev-only force-hatch tool.

Out of scope: the Farm/Playpen holding area itself (`needs-research/hatchery-farm-playpen.md`) — the full-roster case discards rather than depending on it, for now.

## Entry Point
A "Hatchery" tab reachable from the Roster Manager scene (`25-main-hub-navigation`, `48-roster-detail-panel-redesign`), opening a dedicated submenu showing every owned egg. Exact tab mechanism (a tab bar alongside existing Roster Manager views vs. a separate menu entry) follows whatever convention `48`'s screen already uses — not re-specified here.

## Egg Type
An egg's type maps onto the existing `Element` enum (`Normal, Fire, Ice, Earth, Lightning` — `47-ability-and-instructions-data-model`, `55-combat-status-and-element-enums`), reusing the existing `element_color` mapping for the tint rather than introducing a second color system. A fire-type egg renders with a reddish tint, etc.

## Egg States & Tray
An egg is in exactly one of these states:
1. **Undefined** — no mad-lib completed yet. Renders as a silhouette egg with a question mark, both in the tray and in focus view. Shown in the tray as its mad-lib sentence with blanks visibly unfilled.
2. **Incubating** — mad-lib completed, art generated, 24-hour timer running. Renders with its generated, type-tinted art.
3. **Ready** — timer elapsed. Same art as Incubating, but plays an idle "wiggle" animation in the tray to signal it can be hatched.

The tray is a layout convention, not a literal rendered bar — eggs simply arrange as if placed along one. Tapping any egg (in any state) toggles it between its tray position and a large, centered "focus" view; tapping again returns it to the tray.

## Mad-Lib Definition Flow
1. Tapping an **undefined** egg opens the mad-lib modal directly (no intermediate step): the full generated sentence, with its blanks, ready for input.
2. Sentence templates are themselves meta-generated (a prompt that generates mad-lib templates), producing varied sentence structures rather than one fixed template. Exact meta-prompt design is not specified here — see Open Questions.
3. Blanks accept free text. Expect long or unusual input — the sentence line-wraps like a paragraph, not fixed to a single line.
4. Modal actions: **Done**, and the standard "X" close already used by other modals (in place of a separate Cancel).
5. Typing into a blank does not trigger anything. Only **Done** — and only once every blank is filled — submits the sentence as the creature-generation prompt.
6. On Done: the egg starts generating its art (via `64-creature-animation-pipeline`'s sibling still-image pipeline, `17-creature-art-asset-pipeline`) and starts its 24-hour incubation timer. The egg stays silhouette/question-mark until generation completes, then updates in place to its real, type-tinted art.

## Hatching Sequence
Tapping a **ready** egg into focus view plays an escalating, non-interruptible sequence:
1. An aggressive "about to hatch" wiggle — stronger than the tray's ready-state wiggle.
2. **Crack**: a crack forms down the middle. Cracks happen in fast, distinct bursts with a visible pause between each one — not one continuous crackling motion. See "Crack/Break/Reveal Production" below for how this is generated; the pacing itself is a hard requirement regardless of which approach is used.
3. **Break**: the egg splits into a few pieces, slow-motion, anime-break style.
4. **Reveal**: the creature fades in as a white flash silhouette, then lerps into its true color.
5. **Name reveal**, only after the color lerp completes.
6. The creature's idle animation plays, followed by its starting attack (every hatched creature has exactly one starting attack).
7. A stats panel appears on the right, styled consistently with the existing roster detail panel (`48-roster-detail-panel-redesign`) — same visual language, not a new panel design.

### Crack/Break/Reveal Production
Two candidate approaches to actually producing steps 2-3's art; not validated against the "fast cracks, visible pauses" pacing requirement yet — resolve with a quick spike before committing, don't guess:
- **(a) A single generated clip via `64-creature-animation-pipeline`.** Prompt the whole crack-to-break sequence as explicit timed beats, following this project's proven prompt convention (concrete physical beats, not a vague description) — e.g. describing each crack event and the pause after it directly in the prompt. Risk: unconfirmed whether the model reliably hits an exact stutter-step rhythm from prompt text alone.
- **(b) Sequenced still images with game-driven timing.** Generate a handful of discrete crack-stage stills (intact → first crack → more cracks → burst) via `17-creature-art-asset-pipeline`, with the game's own code controlling hold/pause durations between them. Likely more reliable for exact pacing than hoping a video generation lands the timing, at the cost of a more comic-panel, less fluid feel.

The white-flash-to-color lerp and the name reveal are presentation-layer effects applied on top of whichever art comes out of this step — neither generation approach needs to produce them itself.

## Post-Hatch: Add to Roster
Below the newly hatched creature, an "Add to Roster" action.
- **Roster has an open slot:** adds the new creature directly.
- **Roster is full:** the player picks an existing roster creature to make room. **For now, that creature is discarded (removed permanently)** — a stopgap. This is expected to change to a move-to-Farm/Playpen action once `needs-research/hatchery-farm-playpen.md` is resolved; don't build the discard path in a way that's awkward to swap out later (e.g. keep "pick a creature to bump" and "what happens to it" as separate steps).

## Dev Tooling
A dev-only tool to force-hatch a chosen egg immediately, skipping the 24-hour timer, so the hatch sequence can be tested on demand.

## Open Questions / TBDs
- Meta-prompt design for generating varied mad-lib templates — needs its own iteration, not specified here.
- Whether egg/hatch art uses `17`'s full two-fidelity model (viewer + battlefield images) or a single fidelity — not specified by the source brief.
- Crack/break/reveal production approach, (a) vs (b) above — needs a validation spike.
- Starting-attack sourcing: freshly generated per-hatch, or selected from an existing ability pool — not specified by the source brief.
- Exact Hatchery tab mechanism within the Roster Manager scene.

## Dependencies
- `64-creature-animation-pipeline` — hatch-sequence animation (if approach (a) is chosen), starting-attack animation.
- `17-creature-art-asset-pipeline` — egg art generation; crack-stage stills (if approach (b) is chosen).
- `47-ability-and-instructions-data-model`, `55-combat-status-and-element-enums` — `Element` enum for egg type-tinting; starting-attack data shape.
- `48-roster-detail-panel-redesign` — stats panel visual consistency.
- `25-main-hub-navigation` — shared tab/button interaction core.
- `needs-research/hatchery-farm-playpen.md` — future replacement for the roster-full discard stopgap; not a build blocker for this spec.
- `needs-research/local-generation-service-integration.md` — this feature is the one that puts `64`'s remaining pre-ship gap (weight-redistribution licensing) directly in front of players (egg art gen, hatch animation, starting-attack animation all depend on `64`). Gated for shipping, same as `64` itself; not a blocker on writing/building the rest of this spec now.
