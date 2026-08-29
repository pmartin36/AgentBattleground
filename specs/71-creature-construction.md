# Creature Construction

> **Status: draft (not started).** Deterministic assembly of a full `Creature` from the model's interpretive parts plus fixed game rules. The model supplies flavor and choices (name, description, a stat weighting, which of four attack archetypes); this spec's deterministic code allocates a fixed stat budget, sets the one starting attack's numbers, and assembles the creature. Game-specific (`crates/game/`). The concrete home of the "model builds parts, game assembles" principle for creatures. Consumed by the hatchery (`67`), and available to onboarding (`01`) and bot opponents.

## Purpose
Turn a described concept into a real, balanced `Creature` by fixed rules, so the result is reproducible and balanced no matter what the model returned. The model never authors a stat block or an attack; it advises. This spec is where the advice becomes a creature.

## Division of labor: model vs deterministic
The model (via `70`, prompted by `67`) produces:
- the **name**;
- the **description** (flavor text, also the art/animation prompt for `66`);
- a **stat weighting** over the four stats (STR/DEX/INT/VIT);
- the **starting-attack archetype**: one of `ranged`, `melee`, `debuff`, `buff`.

Deterministic construction (this spec) produces:
- the **stat allocation**: a fixed budget distributed by the weighting;
- the **starting attack's amount**: a value inside the chosen archetype's small range, derived from stats;
- the **assembled `Creature`**: level 1, one ability, element, stamina, and the art handles.

The creature's art and animation are AI, produced through `66` (the still via `generate_image`, the idle and starting-attack clips via `generate_animation`) and are out of scope here.

## Inputs
A construction request carries the model-derived parts above, the egg's `Element`, and a **seed**. `67` assembles this request from the mad-lib flow and its `70` calls; this spec consumes it. Given the same request, construction is fully reproducible.

## Stat allocation
- A fixed base **budget** of points is distributed across the four stats. Existing creatures total roughly 74 to 85 points, so ~80 is the starting budget. It is a tunable constant, not a magic literal scattered through the code.
- The weighting is normalized, the budget distributed in proportion, rounding reconciled so the parts sum to exactly the budget, with a per-stat **floor** (no stat lands at zero) and a **cap** (the weighting cannot dump the whole budget into one stat).
- Deterministic given the weighting, budget, and seed.

## Starting attack
Every hatchling has exactly **one** ability: its starting attack. Creatures grow toward the four ability slots (`34`) through post-battle upgrades (`06`); construction fills only the first.

The effect comes from a small **pool of four archetypes**; the model picks which one, the game sets the number:
- **ranged** damage at range greater than 1;
- **melee** damage at adjacent range;
- **debuff** reduces an enemy stat / applies a negative status;
- **buff** raises a stat / applies a positive status.

Each archetype defines a **small amount range**. The amount is **deterministic**: derived from the creature's keyed stat (for example melee from STR, ranged from DEX/INT, buff/debuff magnitude from INT), mapped into the archetype's range so a stronger creature lands higher in the band. The attack's `element` is the egg's `Element`; class and cost follow from the archetype and stats. The concrete ability fields are `47`'s.

The model choosing the archetype and the game fixing the amount is the parts-not-entities rule in miniature: variety comes from the model's choice, balance from the deterministic number.

## Assembly
Compose the parts into a `Creature`: name, description, level 1, the allocated stats, the single starting attack, default stamina, the egg's element, and the art/animation handles from `66`. The whole assembly is deterministic and reproducible from the request.

## Timing
Construction happens at **definition time** (the mad-lib Done in `67`), and the result is stored on the egg (`Egg::hatchling`). So the 24-hour incubation and the art generation run against an already-decided creature, and the hatch (`68`) reveals it rather than rolling it. A creature is never re-rolled at hatch.

## Placement
`crates/game/`. Construction is this game's mechanics (its stat budget, its attack pool, its assembly rules), not engine-level. Reusable across the hatchery (`67`), onboarding's first-run creatures (`01`), and bot opponents.

## Consumers
- `67-hatchery-definition-generation` builds the construction request from the mad-lib flow and `70`, calls this, and stores the resulting hatchling on the egg.
- `68-hatchery-hatch-sequence` reveals and displays the constructed creature.
- `01` onboarding and bot-opponent authoring may reuse the same constructor later.

## Open Questions / TBDs
- The exact budget value, per-stat floor/cap, and whether the budget scales with anything (level) later.
- The per-archetype amount range and which stat keys each archetype's amount.
- Whether the model returns the archetype as a direct choice from the four, or a softer leaning the game maps onto one.
- Buff/debuff target, stat, and duration specifics, which depend on `47`/`10`'s status model.

## Dependencies
- `70-text-generation-api` produces the model parts (name, description, weighting, archetype choice).
- `47-ability-and-instructions-data-model` the ability fields the starting attack is built from; `34-creature-attributes-data-model` the stats, ability slots, and stamina; `55-combat-status-and-element-enums` the `Element` and status kinds.
- `66-asset-generation-api` the still and animation handles the assembled creature carries.
- `67-hatchery-definition-generation` assembles the request and stores the hatchling; `68-hatchery-hatch-sequence` reveals it; `06-post-battle-upgrade` grows abilities past the first.
