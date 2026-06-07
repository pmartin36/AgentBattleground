# Battle Viewer

## Purpose
The heart of the game. Watching a battle play out — whether live or as a replay — is the primary experience. This is what makes the game feel alive. It is by far the most complex scene and should receive the most design and engineering attention.

## Scope
- ASCII art battlefield rendering with color
- LLM-driven turn playback
- Live battle mode (watching your initiated battle in progress)
- Replay mode (watching saved battles)
- Playback controls for replays
- First-watch tutorial overlay

## Key Details

### Visual Rendering
The game uses image-to-ASCII conversion with color, giving pieces and the battlefield a distinctive visual identity. The Battle Viewer is where this matters most. Pieces should feel like characters, not text blobs. The rendering system needs to handle:
- The battlefield grid/map
- Piece visuals (unique per piece, evolve with upgrades)
- Attack/movement animations (ASCII-style)
- Status indicators (health, active skills, etc.)

### Turn Structure
Battles are turn-based, driven by the LLM simulation engine. The viewer receives a sequence of turns (from a live battle or a replay file) and renders them. The pacing and presentation of each turn is a major UX concern — too fast and it's unreadable, too slow and it's boring.

### Live Mode vs. Replay Mode
Both modes use the same rendering path. The difference:
- **Live**: turns arrive from the simulation engine as they're computed; viewer renders in near-real-time
- **Replay**: turn sequence is loaded from file; player controls playback speed and position

### Playback Controls (Replay Mode)
- Play / pause
- Step forward / back by turn
- Speed control (1x, 2x, fast-forward)
- Jump to turn N

### Tutorial Overlay
On a player's first battle watch, a tutorial overlay explains what they're seeing — pieces, turns, skill resolution, etc. Should be skippable and not repeat on subsequent watches.

### "Tape" Mentality
Players watch other people's battles specifically to gather intelligence on what's working. The viewer should support this use case: it should be easy to read what happened and why. Clear skill/action annotations on each turn are important.

## Open Questions / TBDs
- What does the battlefield look like? Grid? Free-form? Size?
- How many turns does a typical battle last?
- How are skill decisions annotated in the viewer? (which skill fired, what it did)
- Does the viewer support any interactivity during live mode, or is it purely watch-only?
- Sound? (probably no, terminal game)
- Can you share a timestamp/turn link for discussion?

## Dependencies
- `10-battle-simulation-engine` — produces the turn sequence that the viewer consumes
- `07-replay-browser` — launches viewer in replay mode
- `04-matchmaking-battle-initiation` — launches viewer in live mode
- `12-data-model-sync` — replay file format must be viewer-readable
