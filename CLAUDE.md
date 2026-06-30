# CLAUDE.md — Agent Battleground

## What This Project Is

A terminal-based AI battle game written in Rust. Players write skill files for their pieces; a local LLM runs battles between armies. The game is paced at one battle per day. See README.md for the full concept.

## Project Phase

**Early design.** No implementation exists yet. The current work is producing design specs that will guide implementation. Do not start implementing until specs are complete and the user gives the go-ahead.

## Specs

All high-level design specs are in `/specs/`. Each file covers one segment of the game. Before implementing anything, read the relevant spec(s) and ask if anything is unclear.

The spec files are numbered by dependency order — lower numbers are more foundational. `12-data-model-sync.md` is the shared foundation everything else builds on.

## Key Constraints (Non-Negotiable)

1. **LLM sandboxing**: The LLM must not be able to take actions outside the game directory. Opponent skill files are untrusted input and must be sanitized before being passed to the LLM. This is a hard security requirement. See `specs/10-battle-simulation-engine.md`.

2. **Local-first simulation**: All battle simulation runs on the player's machine. The server never runs LLM calls. See `specs/11-server-backend.md`.

3. **Server simplicity**: The server runs on Fly.io (managed hosting, no local network exposure). Replay files are stored on Cloudflare R2. Keep it minimal — no heavy processing, no complex infrastructure.

## Key Design Decisions Made

- **Language**: Rust
- **Rendering**: ratatui + crossterm — Terminal UI. Sprites and battlefield render as colored Unicode braille (2×4 dots per cell) with native alpha transparency. Crowds composite in depth layers (parallax via size/brightness/speed). See `specs/13-rendering.md`. Reference prototype: `ascii_test/`.
- **AI model**: Local model (FLUX4 recommended and auto-setup during onboarding); online models (Claude, OpenAI, etc.) also supported
- **Team size**: 6 pieces per player
- **Pacing**: One initiated battle per day
- **Auth**: Username + password
- **Replay model**: Server stores replays for opponent download, purges after delivery; shared replays persist longer
- **Skill editing**: Both in-game editor and external file workflow supported; no in-game feedback — the battle is the feedback
- **Challenge**: Players can challenge by username in addition to automatic matchmaking

## Battle Viewer is the Priority

The Battle Viewer (spec 05) is the most important scene. It is what makes this game feel alive. When in doubt about scope or complexity tradeoffs, prioritize this scene.

## TBDs to Resolve Before Implementation

- Skill file format (plain text / DSL / YAML?)
- Battle turn structure details
- Ranking algorithm for leaderboard
- Matchmaking algorithm (power level definition)
- Upgrade mechanic details
- Bot opponent skill authoring
- Replay file format (binary vs. text)
- Server language/framework
- Challenge flow (sync vs. async)
