# Hatch Reveal & Roster Placement

> **Status: draft (not started).** Redesigns the hatch reveal and post-hatch placement into the roster detail screen's layout, with a choreographed reveal and full-generation gating. Supersedes the reveal effects, stats panel, and add-to-roster of `68-hatchery-hatch-sequence`; `68` keeps the wiggle/crack/break sequence, the clip-generation timing, and the dev hotkeys.

## Purpose
The hatch's payoff resolves into the same layout the player already knows from the roster: creature on the left, a stats/attack dock on the right, mirroring `48-roster-detail-panel-redesign`. And no hatch, on any path, may show a half-generated creature: every path, including the dev force-hatch, waits until the creature is fully generated.

## Relationship to 68
`68` owns and keeps: the ready-egg trigger, the Wiggle -> Crack -> Break sequence, the shared black crack overlay, the per-creature idle/attack clip generation during incubation, and the dev force-hatch / force-create-egg hotkeys. This spec REPLACES `68`'s reveal render (its `RevealFlash`/`RevealColor`/`Name`/`Idle`/`Attack` handling), its stats panel, and its Add-to-Roster action, with the gating, sequence, layout, and dock below. The `HatchPhase` timeline is revised accordingly: the `Attack` phase is removed (see Sequence).

## Scope
- Full-generation gating on every hatch path, with a "generating" wait state and no placeholder reveal.
- The revised reveal sequence: idle-only creature, name fade-in, and the slide-into-layout choreography.
- The settled roster-style layout (name / creature-left / stats-dock-right / stationary egg dock below).
- The stats dock, reusing the roster detail panel's rendering, with Keep / Discard in place of the agent-prompt section.
- Keep / Discard behavior, including the full-roster bump step.

Out of scope: the wiggle/crack/break sequence and clip generation (`68`); the deterministic creature identity (`71`); the model/art backends (`66`/`70`).

## Full-generation gating (no placeholder, no shortcut)
Every hatch path requires the creature FULLY generated before the sequence plays: the still (`67`'s `generate_image`) AND both clips (idle + attack, generated during incubation per `68`).
- Tapping a Ready egg: by the time an egg is Ready (24h), incubation generation has completed, so the hatch plays immediately.
- Dev force-hatch: skips only the 24h timer. If the creature's still/clips are not yet resolved, it ensures generation is running and shows a **"generating…"** wait state; the sequence begins only once the still, idle, and attack all resolve. There is NO placeholder-art reveal on any path (this replaces `68`'s `still_dots`/`unwrap_or_else` placeholder fallbacks).
- The attack clip is required for readiness (and for battles) even though it never plays during the hatch (see Sequence).

## Sequence
The creature's **idle** animation plays continuously from the moment it is revealed, through the reveal and the settled state. The starting-attack clip **never** plays during the hatch; it is shown as data in the stats dock and exists for battles.
1. Wiggle -> Crack -> Break (`68`, unchanged).
2. **Reveal**: the white-flash silhouette lerps to true color (`68`'s reveal effects, retained), centered, with the idle already playing.
3. **Beat**: a brief hold during which the creature's **name fades in** above it.
4. **Slide**: the creature slides LEFT while the **stats dock slides in from the RIGHT**, settling into the layout below. The egg dock does NOT move. Reuse the roster carousel's slide-offset pattern (`roster_manager`'s `slide_offsets`) so the motion matches the roster's.
5. **Settled**: creature on the left (idling), stats dock on the right, name above the creature, egg dock stationary below, Keep / Discard in the dock.

## Layout (mirrors the roster detail screen)
The settled layout is the roster detail screen's shape (see `48-roster-detail-panel-redesign` and `roster_manager`'s `layout` / `panel_interior_regions`): a 2:1 LEFT/RIGHT split with the creature and its name on the left, the stats dock on the right, and the egg dock as a stationary row along the bottom (the hatchery's equivalent of the roster carousel, which stays put).
- **Name**: its own zone directly above the creature. It never overlaps the creature or the dock. If the name wraps to two lines, the creature zone shrinks to make room (the name zone grows, the creature flexes down) rather than overlapping. This fixes `68`'s bug where the name and the button both anchored to the focus rect's bottom and collided.

## Stats dock (reuse the roster detail panel)
The stats dock reuses the roster detail panel's rendering (`roster_manager`'s `details_panel`: the stamina/stat rows and the abilities section that lists the creature's stats and its one starting attack as data, exactly as the roster lists abilities). Reuse the shared rendering rather than reimplementing it: extract the shared panel-rendering into a component both the roster and this dock call, so the two never drift. The one difference: where the roster panel has the Instructions / agent-prompt section and its Edit button, the stats dock has the **Keep / Discard** actions in that slot. Same visual language, not a new panel design.

## Keep / Discard
The dock's two actions replace `68`'s single "Add to Roster" button:
- **Keep**: adds the hatchling to the roster. Open slot -> added directly. Full roster -> the pick-a-creature-to-bump step (`68`'s picker, retained), whose bumped creature is discarded via the existing `dispose_bumped` stopgap (still swappable for a future Farm/Playpen).
- **Discard**: permanently discards the hatchling; it is not added, and the egg is retired the same way a placed hatch retires it.
Either action ends the hatch and returns to the base tray.

## Dependencies
- `68-hatchery-hatch-sequence` — the wiggle/crack/break sequence, clip generation, and dev hotkeys this builds on; this spec supersedes `68`'s reveal / stats panel / add-to-roster.
- `48-roster-detail-panel-redesign` — the panel visual language and the `details_panel` / `layout` / `panel_interior_regions` / `slide_offsets` code this reuses for the stats dock, the left/right split, and the slide motion.
- `67-hatchery-definition-generation` — the still generated at define; `71`'s constructed creature (stats + one attack) shown in the dock.
- `66-asset-generation-api` — the idle/attack clips whose readiness gates the hatch.
- `65-hatchery` — the egg dock (tray strip) shown stationary below.

## Open Questions / TBDs
- Slide duration/easing for the creature-left + dock-in choreography (a tuning detail against the real render).
- Whether the name fade-in and the slide strictly sequence or slightly overlap (baseline: name fades in during the beat, then the slide).
