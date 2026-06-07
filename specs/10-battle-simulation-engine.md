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

For each turn, the LLM is invoked to decide what each piece does, based on its skills, the current board state, and the opponent's visible state. The engine enforces rules — a piece can't act outside what its skills allow.

### Turn Structure
TBD in detail, but at a high level:
- Each turn, all pieces get an action opportunity
- Order of action (initiative, speed, etc.) is TBD
- The engine validates LLM outputs and rejects illegal moves
- A battle ends when one side has no remaining pieces, or a turn cap is reached

### Replay Artifact
After each turn, the engine records the state delta — what happened, who acted, what skills fired, resulting state. This sequence becomes the replay file, which the viewer consumes and the server stores.

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
- `04-matchmaking-battle-initiation` — triggers the engine with downloaded opponent data
- `05-battle-viewer` — consumes the engine's turn output (live or via replay file)
- `09-settings-model-config` — determines which LLM the engine uses
- `12-data-model-sync` — replay file format, opponent data format
