# Data Model & Sync Protocol

## Purpose
Defines what data exists, where it lives, and how it moves between client and server. The foundation that all other specs depend on.

## Scope
- Local data model (what lives on the player's machine)
- Server data model (what the server stores)
- Replay file format
- Opponent data packaging (what gets downloaded before a battle)
- Data validation and security at boundaries

## Key Details

### Local Data (Player's Machine)
- **Player profile**: username, account credentials (token), model config
- **Army**: 6 pieces, each with name, visual definition, skill files, upgrade history, and — per `34-creature-attributes-data-model` — stats (STR/DEX/INT/VIT), level, up to 4 abilities (each with up to 4 modifier tags), an exhaustion/injury state, and its active/bench/reserve squad position (purely positional in the roster ordering, not a separate stored field)
- **Skill files**: human-readable files on disk, one per piece (or set per piece — TBD)
- **Battle history**: downloaded replays, stored locally after server pushes them
- **Shared replays**: locally cached copies of shared replays downloaded from server

### Server Data
- **Accounts**: username, hashed password, session tokens
- **Leaderboard**: player rankings and battle records
- **Pending replays**: replays queued for download by the opponent (purged after download)
- **Shared replays**: replays explicitly shared by players (persistent until removed)
- **Bot skill files**: server-authored skill files for bot opponents
- **Player public profiles**: public-facing army metadata (not skill files)

### Replay File Format
The replay is the primary artifact of a battle. It must be:
- **Compact**: small enough to store many and transfer quickly
- **Complete**: contains enough information to fully reconstruct every turn in the viewer
- **Viewable**: the Battle Viewer can render it without needing the original skill files

Contents (TBD in detail):
- Battle metadata (participants, date, outcome)
- Initial board state (both armies, piece configs)
- Turn-by-turn deltas (what happened each turn: actions, results, state changes)
- Skill annotations (which skill fired, what it resolved to)

Whatever this format ends up being (binary/text, see Open Questions), it must be able to represent `20-battle-viewer-event-playback`'s `Event`/`EventKind` shape — a real, already-built example of "turn-by-turn deltas" (`Move`/`Die` so far, targeting a piece by its stable `index`). That spec's internal event list is deliberately decoupled from this file format (same pattern as board-cell-vs-world-position in `16`), so this format doesn't need to match it byte-for-byte — only be able to produce it.

### Opponent Data Package
What gets downloaded from the server before a battle:
- Opponent's 6 pieces: names, visuals, skill files, and the same stats/level/abilities/modifiers/exhaustion/squad-position fields the Army bullet above lists (`34-creature-attributes-data-model`) — the local sim needs the opponent's full piece data to run them, not just the player's own
- This is the ONLY data the local simulation engine needs to run the opponent

**Security**: opponent skill files are untrusted input. Before they are passed to the LLM, the engine must validate and sanitize them. See `10-battle-simulation-engine` for sandboxing details.

### Data Flow Summary
```
[Player edits skill files] → stored locally
[Battle initiated] → opponent data downloaded from server
[Simulation runs] → replay produced locally
[Battle ends] → replay uploaded to server (stored for opponent download)
[Opponent connects] → replay pushed to opponent, then purged
[Player shares replay] → replay stays on server, accessible to all
```

## Open Questions / TBDs
- Replay file format: binary (compact) or text (inspectable)?
- How large is a typical replay file?
- What's the skill file format? (shared decision with `03-army-skill-editing`)
- How are piece visuals stored and transferred? (sprite/asset source format TBD — see `13-rendering`)
- Is there any versioning concern — what if skill file format changes between game versions?

## Dependencies
- All other specs reference this one. This is the lowest-level shared foundation.
