# Hatchery — Shell & Egg Lifecycle

> **Status: draft (not started).** The Hatchery container and the egg state machine: the entry point from the Roster Manager, the egg tray, the three egg states and the incubation timer, and the tap-to-focus interaction. The former single Hatchery spec is split into three: this shell/lifecycle spec, `67-hatchery-definition-generation` (the mad-lib definition and creature generation), and `68-hatchery-hatch-sequence` (the animated hatch and add-to-roster).

## Purpose
The frame every other hatchery piece plugs into: reach the Hatchery from the roster, see every owned egg, move an egg between the tray and a centered focus view, and carry each egg through its lifecycle from undefined to ready-to-hatch. What happens when an undefined egg is tapped is `67`; what happens when a ready egg is tapped is `68`.

## Scope
- A Hatchery tab/entry point reachable from the Roster Manager.
- The egg tray: undefined eggs and incubating eggs, shown together.
- The egg data model: type (mapped onto `Element`) and the three lifecycle states.
- The 24-hour incubation timer and the ready-state signal.
- The tap-to-focus / swap / hide interaction, and routing a tap by egg state.
- Egg type-tinting.
- The back-to-roster action.

Out of scope: the mad-lib definition modal and art/creature generation (`67`); the hatch sequence, reveal, stats panel, and add-to-roster (`68`); the Farm/Playpen holding area (`needs-research/hatchery-farm-playpen.md`).

## Entry Point
A new top-level **`Hatchery` scene** (its own `SceneId`, peer to `RosterManager`), reached by an entry point in the Roster Manager (`25-main-hub-navigation`, `48-roster-detail-panel-redesign`) and showing every owned egg. A back action returns to the Roster.

## Egg Type
An egg's type maps onto the existing `Element` enum (`Normal, Fire, Ice, Earth, Lightning` — `47-ability-and-instructions-data-model`, `55-combat-status-and-element-enums`), reusing the existing `element_color` mapping for the tint rather than introducing a second color system. A fire-type egg renders with a reddish tint, etc.

## Egg States & Tray
An egg is in exactly one of these states:
1. **Undefined** — no mad-lib completed yet. Renders as a purpose-drawn silhouette sprite whose form incorporates a question mark (not a text `?` overlaid on an egg), authored through the braille dot pipeline to read clean at sprite resolution, both in the tray and in focus view. Shown in the tray as its mad-lib sentence with blanks visibly unfilled.
2. **Incubating** — mad-lib completed, art generated, 24-hour timer running. Renders with its generated, type-tinted art.
3. **Ready** — timer elapsed. Same art as Incubating, but plays an idle "wiggle" animation in the tray to signal it can be hatched.

The tray is a layout convention, not a literal rendered bar — eggs simply arrange as if placed along one. Tapping any egg (in any state) toggles it between its tray position and a large, centered "focus" view; tapping again returns it to the tray. Tapping a second egg swaps focus to it.

**Tap routing by state** (this spec owns the focus mechanic; the state-specific action lives in the other two specs):
- Tapping an **undefined** egg opens the mad-lib definition modal — `67`.
- Tapping an **incubating** egg shows it in focus view (with its remaining timer); no further action.
- Tapping a **ready** egg into focus view begins the hatch sequence — `68`.

## Incubation Timer
On definition (`67`'s Done action), an egg enters **Incubating** and a 24-hour real-time timer starts. The start time persists (`69-player-data-store`'s `Incubating { started_at }`), so the countdown resumes correctly across restarts. When the timer elapses the egg becomes **Ready** and shows the tray ready-wiggle. The art shown while incubating and after is identical; only the ready-wiggle distinguishes them. A dev-only force-hatch tool that skips this timer lives in `68`.

## Dependencies
- `69-player-data-store` — persists egg state (type, mad-lib, incubation start, hatchling) across restarts; the tray reads from and writes to it. The tray is seeded from this store (how a player *acquires* eggs is out of scope for this spec).
- `25-main-hub-navigation`, `48-roster-detail-panel-redesign` — the Roster Manager entry point and shared button/interaction core.
- `47-ability-and-instructions-data-model`, `55-combat-status-and-element-enums` — the `Element` enum for egg type-tinting.
- `67-hatchery-definition-generation` — opened by tapping an undefined egg; hands its Done action back to this spec's incubation timer.
- `68-hatchery-hatch-sequence` — triggered by tapping a ready egg.
