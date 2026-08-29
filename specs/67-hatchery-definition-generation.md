# Hatchery — Egg Definition & Creature Generation

> **Status: draft (not started).** The mad-lib definition modal and the wiring that turns an undefined egg into a defined, incubating one: the player fills a sentence, the model reads it into parts (`70`), deterministic code assembles the creature (`71`), and the egg's art is generated (`66`). Second of the three hatchery specs — see `65-hatchery` (shell & egg lifecycle) and `68-hatchery-hatch-sequence` (the animated hatch and add-to-roster).

## Purpose
Turn an undefined egg into a defined, incubating one. The player fills in a mad-lib sentence; that sentence becomes the description the model interprets and the prompt the art is generated from. This spec owns the modal and the wiring between the pieces that already exist; it does not itself derive stats or attacks (that is `71`) or run the model (that is `70`).

## Scope
- The mad-lib definition modal: rendering a template with its blanks, free-text entry, line-wrapping, and the Done / close actions.
- Rendering an undefined egg's unfilled mad-lib sentence (the template with its blanks) in `65`'s hatchery tray. (`65` renders the `?` egg sprite; the sentence that accompanies it is owned here.)
- On Done: calling `70` to read the completed sentence into the creature's parts, calling `71` to construct the creature from those parts, storing it on the egg, kicking off the egg's art generation via `66`, and entering the `Incubating` state (starting `65`'s 24h timer).
- Resolving a model config to hand `70` (see Runtime configuration below).

Out of scope: the egg tray, states, and 24-hour timer (`65`); the deterministic stat/attack/name assembly (`71`, which this calls); running the model or its backends (`70`); the art backends themselves (`66`); the hatch sequence and the reveal/display of the generated creature (`68`).

## Mad-Lib Definition Flow
1. Tapping an **undefined** egg (`65`) opens the mad-lib modal directly (no intermediate step): the full sentence, with its blanks, ready for input.
2. Templates come from a **fixed starter pool** in this first pass. Meta-generated (varying) templates are an explicit later iteration — see Open Questions — not this spec's first scope.
3. Blanks accept free text. Expect long or unusual input — the sentence line-wraps like a paragraph, not fixed to a single line.
4. Modal actions: **Done**, and the standard "X" close already used by other modals (in place of a separate Cancel).
5. Typing into a blank does not trigger anything. Only **Done** — and only once every blank is filled — submits.
6. On Done, in order:
   - the completed sentence is sent to `70` (`generate_text`) with a prompt that asks for the creature's **parts**: a stat weighting, one of `71`'s four attack archetypes, a name, and a flavor description. The model returns parts only; it never returns a finished creature (see `71`).
   - `71` assembles the creature from those parts plus the egg's `Element` and a seed, and the result is stored on the egg (`Egg::hatchling`).
   - the egg's art generation starts via `66` (`generate_image` for the egg/creature still); the egg stays silhouette/`?` until it completes, then updates in place to its real, type-tinted art.
   - the egg enters **Incubating**, starting `65`'s 24-hour timer.

## The model reads parts; `71` builds the creature
This spec never turns a sentence into a stat block itself. It calls `70` to get interpretive **parts** (weighting, archetype choice, name, description) and hands them to `71`, which owns the deterministic assembly (fixed stat budget, the one starting attack's amount, the final `Creature`). Timing is definition-time: the creature is constructed here, on Done, and stored on the egg; `68` reveals it at hatch. See [[71-creature-construction]] and the parts-not-entities rule in `70`.

## Runtime configuration
`70` takes an injected `ResolvedModelConfig`; `09`'s settings/persistence layer does not exist yet. For this pass the config is resolved from the environment or a local config file (an online provider + API key is the easy real path; a local model command is the other). This is a real, minimal resolver, not a throwaway — `09` later grows the settings UI on top of it. Absent any config, Done surfaces a clear "no model configured" error rather than silently failing.

## Open Questions / TBDs
- Meta-prompt design for generating varied mad-lib templates — its own later iteration; the first pass uses a fixed starter pool.
- The exact prompt that reads a completed sentence into `71`'s parts (weighting, archetype, name, description) and how strictly the archetype choice is constrained (ties to `70`'s constrained-decoding open question).
- Where the resolved model config ultimately lives once `09` is built (this pass reads env/file).

## Dependencies
- `70-text-generation-api` — `generate_text` reads the completed sentence into the creature's parts.
- `71-creature-construction` — assembles the `Creature` from those parts; this spec calls it and stores the result on the egg.
- `66-asset-generation-api` — `generate_image` for the egg/creature still; the still that later feeds `generate_animation` (`68`) for idle and starting-attack clips.
- `65-hatchery` — the undefined egg this modal opens from, and the incubation timer its Done action starts.
- `47-ability-and-instructions-data-model`, `55-combat-status-and-element-enums` — the ability/attack data shape and `Element` that flow through `71`.
- `68-hatchery-hatch-sequence` — reveals and displays the creature this flow constructs and stores.
