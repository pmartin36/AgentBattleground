# Battle Simulation Engine

## Purpose
The brain of the game. Runs both armies against each other using the configured LLM, produces a turn-by-turn battle record, and hands it to the viewer. Security and sandboxing are first-class concerns here.

## Scope
- LLM orchestration (running both armies' skills)
- Turn structure enforcement
- Replay artifact production
- LLM sandboxing (critical security constraint)

## Key Details

### LLM Orchestration
The engine runs entirely locally. It loads:
- The player's 6 pieces and their skill files
- The opponent's 6 pieces and their skill files (downloaded from server)

For each turn, the LLM is invoked to decide what each *active* piece does, based on its skills, the current board state, and the opponent's visible state. The engine enforces rules — a piece can't act outside what its skills allow.

### Turn Structure
TBD in detail, but at a high level:
- Each turn, all pieces get an action opportunity
- Order of action (initiative, speed, etc.) is TBD
- The engine validates LLM outputs and rejects illegal moves
- A battle ends when one side has no remaining pieces, or a turn cap is reached
- Only the 3 *active* creatures per side fight (`34-creature-attributes-data-model`'s squad model, board-rendered per `36-battle-viewer-squad-layout`); the bench creature can be swapped in mid-battle, driven by the skill system rather than the player — no design exists yet for when/how that trigger fires
- Each acting creature's available actions are its up to 4 equipped abilities (each carrying up to 4 modifier tags, `34`) — this engine is what will eventually give those modifiers concrete mechanical effect, which `34` deliberately leaves undefined
- Using an ability and taking damage both drain stamina (`Stamina`, `34` / `crates/game/src/stamina.rs` — the remaining-percent model that reworked `34`'s `Exhaustion` shape); when a creature's stamina reaches zero it becomes "injured" and is pulled to reserve for a recovery period — this engine is what will eventually implement that drain and trigger the reserve reassignment during a live battle (`34` only defines the data shape and a testable pure transition, not a live trigger)

### Stat Effects
The four creature stats (`34`) each drive two mechanics — the point at which `34`'s deliberately-undefined "mechanical effect of stats on combat" becomes concrete:

| Stat | Primary       | Secondary                                                          |
|------|---------------|--------------------------------------------------------------------|
| STR  | ATK damage    | Max Stamina                                                        |
| VIT  | Health        | Stamina Recovery Rate (% of max stamina recovered per day)         |
| DEX  | Accuracy      | Dodge Chance                                                       |
| INT  | Magic damage  | Status Potency — scales with the attacker−defender INT difference  |

This mapping is the decision. The exact formulas/scaling are not settled here (see Open Questions).

STR's and VIT's secondaries also resolve the two placeholders in today's `Stamina` model (`crates/game/src/stamina.rs`), both currently flat constants:
- **STR → Max Stamina** replaces the fixed `MAX_PERCENT = 100` (identical for every creature) with a per-creature capacity.
- **VIT → Stamina Recovery Rate** replaces the fixed `RECOVERY_DURATION` (recover-to-full in one day) with a per-creature, gradual %/day recovery — the concrete recovery rule `34` deferred to this spec.

Both require evolving the `Stamina` struct when implemented: it tracks only a 0–100 `percent` with no per-creature max and a single recover-to-full duration (see Open Questions).

### Replay Artifact
After each turn, the engine records the state delta — what happened, who acted, what skills fired, resulting state. This sequence becomes the replay file, which the viewer consumes and the server stores.

The viewer already has a real, working consumer contract for a subset of this: `20-battle-viewer-event-playback` defines `Event { start_time, duration, kind }` with `EventKind::Move`/`Die`, targeting a piece by its stable `index`. Whatever this engine emits must be translatable into that shape (or its future extensions — an `Attack`/`TakeDamage` `EventKind` doesn't exist yet and should be designed alongside this engine's combat rules, not guessed at beforehand).

### Sandboxing (Critical)
The LLM must not be able to take actions outside the game directory. This is a hard constraint. Skill files are essentially prompts — a malicious skill file could attempt prompt injection to get the LLM to do something harmful.

Mitigations to design:
- LLM runs in a restricted execution context (no shell access, no filesystem access outside game dir)
- Skill file content is sanitized / wrapped in a constrained prompt frame before being passed to the LLM
- LLM outputs are parsed as structured turn data, never evaluated as code or commands
- Opponent skill files are treated as untrusted input (see `12-data-model-sync`)

This is one of the most important engineering concerns in the entire system.

### Bot Opponent
Bots use the same engine as human opponents, with server-authored skill files. Bot difficulty TBD.

## Open Questions / TBDs
- **Combat formulas per stat** — the *Stat Effects* mapping is decided, but the numbers are not: damage per STR point, how DEX accuracy resolves against DEX dodge, the magic-vs-physical damage split, and the INT attacker−defender difference → status-potency curve.
- **STR → Max Stamina modeling** — `Stamina` currently tracks only a 0–100 `percent` against a flat `MAX_PERCENT`; a per-creature max means either scaling drain-per-damage by capacity or switching to explicit current/max values.
- **VIT → Stamina Recovery Rate modeling** — replaces the current one-shot recover-to-full (`RECOVERY_DURATION`) with gradual %/day recovery; the rate curve and whether an injured creature recovers on that same %/day track are open.
- What is the exact LLM interface? (ollama-style local API? direct binary?)
- How are skill files structured to make LLM interpretation reliable?
- What happens if the LLM produces an invalid/illegal move?
- Turn cap to prevent infinite battles?
- How long does a typical battle take to simulate?

## Dependencies
- `03-army-skill-editing` — skill files are the engine's primary input
- `34-creature-attributes-data-model` — stats, abilities/modifiers, and stamina (`Stamina`, the reworked `Exhaustion`) are the data this engine's combat math and turn resolution act on; this engine is where their mechanical effects finally get defined (see *Stat Effects*)
- `36-battle-viewer-squad-layout` — the 3v3 + bench squad structure this engine's turn/action model must respect
- `04-matchmaking-battle-initiation` — triggers the engine with downloaded opponent data
- `05-battle-viewer` — consumes the engine's turn output (live or via replay file)
- `09-settings-model-config` — determines which LLM the engine uses
- `12-data-model-sync` — replay file format, opponent data format
- `20-battle-viewer-event-playback` — the viewer-side event shape (`Move`/`Die` so far) this engine's output must be translatable into
