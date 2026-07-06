# Army Management & Skill Editing

> A minimal presentation-only slice of this spec — a one-at-a-time creature carousel with real names and art, no stats/skills yet — is split out as `24-roster-carousel` (not yet built; depends on `21-mouse-hover-input`, `22-braille-ui-chrome`, `23-piece-identity-data-model`).

## Purpose
The player's primary workspace between battles. View your pieces in detail and shape how they behave on the battlefield by editing their skills.

## Scope
- Viewing all 6 pieces (stats, visuals, current skills)
- Skill editing via in-game editor or external file workflow
- Skill file change detection

## Key Details

### Piece Detail View
Each piece has:
- Visual (braille sprite, customized over time via upgrades — see `13-rendering`)
- Name and level
- Stats (STR/DEX/INT/VIT) and up to 4 abilities, each with up to 4 modifier tags — data model in `34-creature-attributes-data-model`; stats are base data on the piece, not derived from skills
- Current skill set

### Abilities vs. Skills
`34-creature-attributes-data-model` formalizes what used to be a vague "skills" concept into two distinct layers:
- **Abilities** are the concrete things a creature can do on its turn — attacks, buffs, debuffs — up to 4 per creature, each carrying up to 4 modifier tags (`34`, shape only — no concrete modifier catalog yet).
- **Skill files** (this spec) are the separate, still free-text/DSL layer that gives the LLM *conditions for when* to use which equipped ability during a battle — "skills" in the sense this spec has always used the word. Skill editing is about authoring those conditions, not the abilities themselves (abilities/modifiers are fixed data on the piece, not something edited here).

### Skill Editing
Skill files define the conditions under which a piece uses its equipped abilities, and how it moves/positions itself during a battle. The LLM interprets and executes these skills during simulation.

Two paths are supported:
1. **In-game editor** — edit skills directly within the terminal UI
2. **External file workflow** — player opens the skill file in their own editor, saves it, and the game detects the change and reloads

Both paths produce the same artifact: a skill file on disk that the simulation engine reads.

### No In-App Feedback
There is no skill validator or preview within this scene. The feedback loop is the battle itself. Players iterate by watching how their army performs, then returning here to adjust.

### Skill File Format
TBD by design agent. Should be human-readable and LLM-interpretable. Likely plain text or a simple structured format.

## Open Questions / TBDs
- Skill file format (plain text prompt-style? structured DSL? YAML?)
- How many *skill conditions/rules* can a piece have? (Separate from the ability count, which `34` already caps at 4.)
- Are skills per-piece or shared across the army?
- What constraints exist on skill content? (ties into sandboxing)
- How does upgrading affect what skills are available?

## Dependencies
- `34-creature-attributes-data-model` — the abilities/modifiers/stats a skill file's conditions reference and act on.
- `10-battle-simulation-engine` — skills are consumed and executed here
- `06-post-battle-upgrade` — upgrades modify what's available to edit
- `12-data-model-sync` — skill file format and storage location defined here
