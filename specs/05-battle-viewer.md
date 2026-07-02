# Battle Viewer

> **Status: draft (not started).** Its foundation — the static battlefield (board + 6v6 placeholder pieces, idle-animating) — is a separate, completed spec: `18-battle-viewer-baseline`. Everything in *this* spec (replay-driven playback, live sim, turn structure, playback controls, tutorial overlay) is unbuilt.

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
The board geometry, coordinate model (discrete cell = gameplay truth, continuous world position = render truth, cell-center convention), and depth ordering are built — see `18-battle-viewer-baseline`. What's still open here is how *movement between cells* renders once stage 2/3 exist: a move commits instantly in gameplay, and the piece's `Transform.translate` should lerp from its old cell center to its new one over the move's animation (glide, not snap) — the tween utility (`16`) exists for this but isn't wired to any move-driven trigger yet, since there are no moves to trigger it in a static layout.

### Build Approach (presentation decoupled from battle rules)
The viewer is a *presentation* layer and can be built before the battle *rules* (the LLM sim, `10`) are designed, on top of the completed static-battlefield foundation (`18-battle-viewer-baseline`). Staged:
1. **Replay-driven viewer** — plays a hand-authored replay of universal events. Move and death (piece removal) are covered by `20-battle-viewer-event-playback`. Attack and take-damage visuals are deliberately deferred — their shape depends on combat mechanics that don't exist yet (`10`), and picking a visual now risks guessing wrong. A real replay *file* format (vs. `20`'s hand-authored in-memory event list) is also still open (`12`).
2. **Live / real sim** — the actual battle design (`10`) later, emitting the same replay format.

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
- Whether **bands** (background / battlefield / foreground) are needed for terrain/obstacles that pieces must visually interleave with — spec'd in `16` but not yet implemented (the current board is a flat background layer, sufficient while there's no terrain).
- Replay file format (the universal-events schema for the replay-driven stage).
- How many turns does a typical battle last?
- How are skill decisions annotated in the viewer? (which skill fired, what it did)
- Does the viewer support any interactivity during live mode, or is it purely watch-only?
- Sound? (probably no, terminal game)
- Can you share a timestamp/turn link for discussion?

Resolved (moved out): camera angle, 6v6 board layout, terrain/cell rendering, idle-animation behavior → all decided and built in `18-battle-viewer-baseline`.

## Dependencies
- `13-rendering` ✅ — the braille renderer the battlefield draws through.
- `16-world-space-and-camera` ✅ — world position, camera (projection + `depth_key`), and the sprite `Transform`/tween that place and move pieces.
- `18-battle-viewer-baseline` ✅ — the static board + 6v6 placeholder layout stages 2/3 render on top of.
- `20-battle-viewer-event-playback` — the move/death event-playback slice of stage 2 (stage 2 also needs attack/take-damage events and a real replay file format, both still pending).
- `10-battle-simulation-engine` — produces the turn sequence that the viewer consumes
- `07-replay-browser` — launches viewer in replay mode
- `04-matchmaking-battle-initiation` — launches viewer in live mode
- `12-data-model-sync` — replay file format must be viewer-readable
