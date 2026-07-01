# Battle Viewer

## Purpose
The heart of the game. Watching a battle play out — whether live or as a replay — is the primary experience. This is what makes the game feel alive. It is by far the most complex scene and should receive the most design and engineering attention.

## Scope
- Braille-art battlefield rendering with color (see `13-rendering`)
- LLM-driven turn playback
- Live battle mode (watching your initiated battle in progress)
- Replay mode (watching saved battles)
- Playback controls for replays
- First-watch tutorial overlay

## Key Details

### Visual Rendering
The game renders sprites as colored Unicode braille (see `13-rendering`), giving pieces and the battlefield a distinctive visual identity. The Battle Viewer is where this matters most. Pieces should feel like characters, not text blobs. The rendering system needs to handle:
- The battlefield grid/map
- Piece visuals (unique per piece, evolve with upgrades)
- Attack/movement animations
- Status indicators (health, active skills, etc.)

### Battlefield & Coordinates
The battlefield is an **8×8 grid**. Pieces occupy **discrete cells** and **moves are discrete** (a piece moves cell → cell) — this is the gameplay truth. But **movement is rendered continuously**: a piece lerps its *world position* between cell centers over the move's animation, so it glides rather than snapping (the sprite `Transform` + tween layer, `16`, exists for exactly this).

Coordinate model (per `16-world-space-and-camera`, built): the discrete grid cell is the gameplay position; a **continuous world position** is the render position (1 cell = 1 world unit, sub-cell / dot-granularity movement). The board is a layer *on top of* world space — world space is board-agnostic. A move commits instantly in gameplay (the piece *is* at the destination cell); the world position lerps to catch up (cosmetic).

Render order is per-sprite depth via `camera.depth_key(world_pos)` — painter's, no z-buffer (see `16`). With the current side-view camera, `depth = world-Y` (a piece lower on the board draws in front).

### Build Approach (presentation decoupled from battle rules)
The viewer is a *presentation* layer and can be built before the battle *rules* (the LLM sim, `10`) are designed. Staged:
1. **Static battlefield** — the 8×8 board + the 6v6 pieces placed and idle-animating, camera-framed. Needs no battle-design decisions. (The immediate next build.)
2. **Replay-driven viewer** — plays a hand-authored replay of universal events (move / attack / take-damage). Needs only a replay *format*, not the rules.
3. **Live / real sim** — the actual battle design (`10`) later, emitting the same replay format.

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
- Camera angle — side / isometric / 3-4 top-down (only a side-view camera exists so far).
- How the 6v6 pieces lay out on the 8×8 board (two facing rows? starting zones?) and board orientation.
- Are cells / terrain rendered, or only the pieces on an implied grid? Piece size on screen.
- Idle-animation behaviour for placed pieces.
- Whether **bands** (background / battlefield / foreground) are needed — spec'd in `16` but not yet implemented.
- Replay file format (the universal-events schema for the replay-driven stage).
- How many turns does a typical battle last?
- How are skill decisions annotated in the viewer? (which skill fired, what it did)
- Does the viewer support any interactivity during live mode, or is it purely watch-only?
- Sound? (probably no, terminal game)
- Can you share a timestamp/turn link for discussion?

## Dependencies
- `13-rendering` ✅ — the braille renderer the battlefield draws through.
- `16-world-space-and-camera` ✅ — world position, camera (projection + `depth_key`), and the sprite `Transform`/tween that place and move pieces.
- `10-battle-simulation-engine` — produces the turn sequence that the viewer consumes
- `07-replay-browser` — launches viewer in replay mode
- `04-matchmaking-battle-initiation` — launches viewer in live mode
- `12-data-model-sync` — replay file format must be viewer-readable
