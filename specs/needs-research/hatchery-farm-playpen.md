# Hatchery Farm / Playpen (needs research)

> **Status: parked pending research/decision.** `65-hatchery`'s roster-full "Add to Roster" case discards the bumped creature as a stopgap for now; this is its planned future replacement, not a current build blocker. The owner has opinions on this but wants it decided as its own pass, not folded into the Hatchery spec. Don't run this through the TDD pipeline as-is.

## Purpose
A holding area for hatched creatures that aren't added to the active roster, so hatching doesn't have to discard a creature outright — explicitly not framed as a "kill" action anywhere in the UI, once built.

## Open Questions
- Naming/framing: "Farm" vs. "Playpen" (or something else) — affects tone throughout the UI copy.
- Capacity: unlimited, or its own cap? If capped, what happens when it also fills?
- Can creatures be viewed/managed while parked here (see stats, promote back to roster, release permanently)?
- Does a parked creature do anything passively, or is it purely inert storage?
- Where does this live in the UI — its own tab, a sub-view of the Roster Manager, part of the Hatchery submenu?
- Does moving a roster creature here affect its state (in-progress cooldowns, etc.)?

## Dependencies
- Referenced by `65-hatchery`'s roster-full "Add to Roster" flow as its future replacement for the current discard stopgap.
