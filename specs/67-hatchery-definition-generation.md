# Hatchery — Egg Definition & Creature Generation

> **Status: draft (not started).** The mad-lib definition modal and what it produces: the egg's generated art and the hatched creature's identity (name, stats, abilities, starting attack). Second of the three hatchery specs — see `65-hatchery` (shell & egg lifecycle) and `68-hatchery-hatch-sequence` (the animated hatch and add-to-roster).

## Purpose
Turn an undefined egg into a defined, incubating one: the player fills in a mad-lib sentence, which becomes the generation prompt for the egg's art and for the creature it will hatch into.

## Scope
- The mad-lib definition modal, including meta-generated (varying) sentence templates.
- Submitting the completed sentence as the generation prompt.
- Generating the egg's art and updating the egg in place.
- Deriving the hatched creature's identity (name, stats, abilities, starting attack) from the mad-lib description.

Out of scope: the egg tray, states, and 24-hour timer (`65`, which this hands its Done action to); the hatch sequence and the reveal/display of the generated creature (`68`).

## Mad-Lib Definition Flow
1. Tapping an **undefined** egg (`65`) opens the mad-lib modal directly (no intermediate step): the full generated sentence, with its blanks, ready for input.
2. Sentence templates are themselves meta-generated (a prompt that generates mad-lib templates), producing varied sentence structures rather than one fixed template. Exact meta-prompt design is not specified here — see Open Questions.
3. Blanks accept free text. Expect long or unusual input — the sentence line-wraps like a paragraph, not fixed to a single line.
4. Modal actions: **Done**, and the standard "X" close already used by other modals (in place of a separate Cancel).
5. Typing into a blank does not trigger anything. Only **Done** — and only once every blank is filled — submits the sentence as the creature-generation prompt.
6. On Done: the egg starts generating its art (via `66-asset-generation-api`'s `generate_image`) and enters the **Incubating** state, starting `65`'s 24-hour timer. The egg stays silhouette/question-mark until generation completes, then updates in place to its real, type-tinted art.

## Generated Creature Identity
The completed mad-lib sentence is the source description for the creature the egg hatches into. From it the creature receives:
- a **name**;
- **stats** and **abilities**, derived from the description;
- exactly **one starting attack** (every hatched creature has exactly one).

This spec owns producing that identity from the description; `68` reveals and displays it during the hatch. The creature's art comes from `66` (still image via `generate_image`; the idle and starting-attack clips via `generate_animation`).

## Open Questions / TBDs
- Meta-prompt design for generating varied mad-lib templates — needs its own iteration, not specified here.
- Starting-attack sourcing: freshly generated per-hatch, or selected from an existing ability pool — not specified by the source brief.
- Whether the creature's identity (stats/abilities/attack) is derived at definition time or deferred to hatch time.

## Dependencies
- `66-asset-generation-api` — `generate_image` for egg/creature art; the still that later feeds `generate_animation` for idle and starting-attack clips.
- `65-hatchery` — the undefined egg this modal opens from, and the incubation timer its Done action starts.
- `47-ability-and-instructions-data-model`, `55-combat-status-and-element-enums` — the ability/starting-attack data shape and `Element`.
- `68-hatchery-hatch-sequence` — reveals and displays the creature identity this spec generates.
