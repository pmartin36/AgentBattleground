# CLAUDE.md — Agent Battleground

## What This Project Is

A terminal-based AI battle game written in Rust. Players write skill files for their pieces; a local LLM runs battles between armies. The game is paced at one battle per day. See README.md for the full concept.

## Project Phase

**Early design.** No implementation exists yet. The current work is producing design specs that will guide implementation. Do not start implementing until specs are complete and the user gives the go-ahead.

## Specs

All high-level design specs are in `/specs/`. Each file covers one segment of the game. Before implementing anything, read the relevant spec(s) and ask if anything is unclear.

The spec files are numbered by dependency order — lower numbers are more foundational. `12-data-model-sync.md` is the shared foundation everything else builds on.

**Specs must be complete, buildable units — never partially done.** If only part of a spec's scope is actually built, split it: carve the completed slice into its own new spec (numbered next in sequence, marked done), and leave the remainder as a clean, fully-pending spec. Never mark a spec "done" while any of its own stated scope is still outstanding, and never describe one as "partially implemented" — that status doesn't exist here. (Precedent: `13-rendering` vs `17-creature-art-asset-pipeline`; `05-battle-viewer` vs `18-battle-viewer-baseline` / `20-battle-viewer-event-playback`; `15-debug-inspector` vs `19-debug-inspector-advanced-editing`.)

## Key Constraints (Non-Negotiable)

1. **LLM sandboxing**: The LLM must not be able to take actions outside the game directory. Opponent skill files are untrusted input and must be sanitized before being passed to the LLM. This is a hard security requirement. See `specs/10-battle-simulation-engine.md`.

2. **Local-first simulation**: All battle simulation runs on the player's machine. The server never runs LLM calls. See `specs/11-server-backend.md`.

3. **Server simplicity**: The server runs on Fly.io (managed hosting, no local network exposure). Replay files are stored on Cloudflare R2. Keep it minimal — no heavy processing, no complex infrastructure.

4. **Braille is universal except text**: every non-text visual element — sprites, board/UI chrome (grid lines, borders, panels), effects — renders through the braille dot pipeline (see `specs/completed/13-rendering.md`), never drawn directly with other Unicode/ASCII characters. Only text (scene labels, menus, HUD copy) stays plain terminal characters. No render pass may bypass the dot pipeline for non-text content, no matter how minor the element.

## Engine / Game Boundary

This is a two-product workspace (see `specs/completed/31-engine-game-crate-split.md`): everything under `crates/engine/` (`engine-core`, `engine-render`, `engine-derive`, `inspector`) is reusable by any future game; everything under `crates/game/` is this game's content only.

- **Rule of thumb**: if a change would still make sense for a hypothetical different game built on this engine, it belongs under `crates/engine/`. If it only makes sense for Agent Battleground specifically (a concrete scene, a specific creature, this game's skin assets, digit-hotkey policy), it belongs in `crates/game/`.
- **Hard invariants**: no crate under `crates/engine/` contains `include_bytes!`-bundled game art, a closed enum of concrete scenes/creatures/pieces, or a path dependency on `crates/game`.
- **When it's unclear which crate something belongs in, ask the project owner — do not guess and place it wherever compiles.**

## Key Design Decisions Made

- **Language**: Rust
- **Rendering**: ratatui + crossterm — Terminal UI. Sprites and battlefield render as colored Unicode braille (2×4 dots per cell) with native alpha transparency. Crowds composite in depth layers (parallax via size/brightness/speed). See `specs/completed/13-rendering.md`. Reference prototype: `experiments/ascii_test/` (+ `experiments/creature_lab/` for asset generation).
- **AI model**: Local model (FLUX4 recommended and auto-setup during onboarding); online models (Claude, OpenAI, etc.) also supported
- **Team size**: 6 pieces per player
- **Pacing**: One initiated battle per day
- **Auth**: Username + password
- **Replay model**: Server stores replays for opponent download, purges after delivery; shared replays persist longer
- **Skill editing**: Both in-game editor and external file workflow supported; no in-game feedback — the battle is the feedback
- **Challenge**: Players can challenge by username in addition to automatic matchmaking

## Development Gotchas

- **`game` and `inspector` are separate spawned binaries.** The game finds the inspector as a sibling next to its own executable (`current_exe().parent()/"inspector"`), not as a linked dependency. `cargo run -p game` only rebuilds `game` — if you're testing an inspector-only change, run `cargo build --workspace` (or at least `-p inspector`) too, or the game will spawn a stale inspector binary and the change won't appear.

- **Search for an existing function/type/test before adding a new one.** Duplicate helpers and duplicate tests are a recurring source of drift — grep for the behavior you're about to add (not just the exact name you have in mind) before writing it from scratch.

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
