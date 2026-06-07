# Matchmaking & Battle Initiation

## Purpose
The gateway to a new battle. Finds an opponent, downloads what's needed to run them locally, and hands off to the simulation engine.

## Scope
- Daily battle gate enforcement
- Opponent finding (matched, challenged, or bot)
- Opponent data download
- Battle launch

## Key Details

### Daily Battle Gate
A player can initiate one battle per day. This scene enforces that gate. If the gate is closed, the player can still watch replays and edit skills but cannot start a new battle.

### Opponent Selection Modes

**Automatic Matchmaking**
Connects to server, finds a valid opponent. Matching criteria TBD (power level, ranking, activity recency). If no valid human opponent is found — e.g., the player is first in the system, or has recently played all available opponents — a bot opponent is used as fallback.

**Challenge by Username**
A player can directly challenge another player by their username (or account ID). The server validates the challenge and facilitates the opponent data download if accepted. Details of async challenge flow TBD.

**Bot Opponent**
Used during onboarding tutorial and as automatic fallback. Bots run through the same simulation engine as human opponents. Bot skill design is TBD.

### Opponent Data Download
Before a battle can be run locally, the game downloads the opponent's piece and skill data from the server. This is the only external dependency at battle time — the simulation itself runs entirely locally.

### Security Note
Downloaded opponent skill data must be validated before being handed to the LLM. See `10-battle-simulation-engine` and `12-data-model-sync` for sandboxing and packaging details.

## Open Questions / TBDs
- Challenge flow: is it synchronous (both players online) or async (queued)?
- Power level / matchmaking algorithm
- Bot skill authoring — who writes bot skills?
- Can you re-challenge the same opponent, and if so, how often?

## Dependencies
- `11-server-backend` — matchmaking, challenge routing, opponent data retrieval
- `10-battle-simulation-engine` — receives downloaded data and launches simulation
- `12-data-model-sync` — opponent data format and validation
