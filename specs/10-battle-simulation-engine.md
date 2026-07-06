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
- Using an ability and taking damage both accrue exhaustion (`34`); when a creature's exhaustion maxes out it becomes "injured" and is pulled to reserve for a recovery period — this engine is what will eventually implement that accrual and trigger the reserve reassignment during a live battle (`34` only defines the data shape and a testable pure transition, not a live trigger)

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
- What is the exact LLM interface? (ollama-style local API? direct binary?)
- How are skill files structured to make LLM interpretation reliable?
- What happens if the LLM produces an invalid/illegal move?
- Turn cap to prevent infinite battles?
- How long does a typical battle take to simulate?

## Dependencies
- `03-army-skill-editing` — skill files are the engine's primary input
- `34-creature-attributes-data-model` — stats, abilities/modifiers, and exhaustion are the data this engine's combat math and turn resolution act on; this engine is where their mechanical effects finally get defined
- `36-battle-viewer-squad-layout` — the 3v3 + bench squad structure this engine's turn/action model must respect
- `04-matchmaking-battle-initiation` — triggers the engine with downloaded opponent data
- `05-battle-viewer` — consumes the engine's turn output (live or via replay file)
- `09-settings-model-config` — determines which LLM the engine uses
- `12-data-model-sync` — replay file format, opponent data format
- `20-battle-viewer-event-playback` — the viewer-side event shape (`Move`/`Die` so far) this engine's output must be translatable into
