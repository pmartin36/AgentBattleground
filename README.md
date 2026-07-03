# Agent Battleground

A terminal-based AI battle game written in Rust. Assemble a team of 6 pieces, write the skills that govern how they fight, and send them into battle — watched unfold in colored braille art, driven by a local LLM.

## Concept

You are an AI commander. Your army is 6 pieces, each defined by skill files you write. Once per day, your army is matched against another player's army. A local LLM runs both sides and plays out the battle. You watch. You learn. You iterate.

Pieces evolve over time — upgraded after victories, growing more unique with each win. The gameplay loop lives outside the battle: edit skills, watch tape, improve.

## Key Design Principles

- **The battle viewer is the experience.** Everything else supports it.
- **The player edits skills, the LLM runs them.** No direct battle control.
- **Local-first.** Simulation runs on your machine. The server is minimal — matchmaking, leaderboard, replay exchange.
- **The LLM is sandboxed.** It cannot act outside the game directory.
- **One battle per day.** Pacing is intentional.

## Tech Stack

- **Language**: Rust
- **Interface**: ratatui + crossterm — Terminal UI with colored Unicode braille sprite rendering
- **AI**: Local LLM (FLUX4 recommended; online models supported)
- **Server**: Lightweight, designed for Raspberry Pi hosting

## Architecture Overview

```
[Player Machine]                    [Server (RPI)]
  Local binary                        Auth
  Terminal UI          <──────────>   Matchmaking
  Skill files                         Replay storage
  Local LLM                           Leaderboard
  Replay storage
```

## Specs

High-level design specs for each segment of the game live in `/specs`:

| # | Segment |
|---|---------|
| 01 | [Onboarding & First Run](specs/01-onboarding-first-run.md) |
| 02 | [Main Hub / Dashboard](specs/02-main-hub-dashboard.md) |
| 03 | [Army Management & Skill Editing](specs/03-army-skill-editing.md) |
| 04 | [Matchmaking & Battle Initiation](specs/04-matchmaking-battle-initiation.md) |
| 05 | [Battle Viewer](specs/05-battle-viewer.md) ⭐ |
| 06 | [Post-Battle & Upgrade Flow](specs/06-post-battle-upgrade.md) |
| 07 | [Replay Browser](specs/07-replay-browser.md) |
| 08 | [Leaderboard & Social](specs/08-leaderboard-social.md) |
| 09 | [Settings & Model Configuration](specs/09-settings-model-config.md) |
| 10 | [Battle Simulation Engine](specs/10-battle-simulation-engine.md) |
| 11 | [Server / Backend](specs/11-server-backend.md) |
| 12 | [Data Model & Sync Protocol](specs/12-data-model-sync.md) |
| 13 | [Rendering](specs/completed/13-rendering.md) ✅ |
| 14 | [Scene Architecture & Debug Scene Switcher](specs/completed/14-scene-architecture.md) ✅ |
| 15 | [Debug Inspector — Field Editing](specs/completed/15-debug-inspector.md) ✅ |
| 16 | [World Space & Camera](specs/completed/16-world-space-and-camera.md) ✅ |
| 17 | [Creature Art & Asset Pipeline](specs/17-creature-art-asset-pipeline.md) |
| 18 | [Battle Viewer — Baseline](specs/completed/18-battle-viewer-baseline.md) ✅ |
| 19 | [Debug Inspector — Advanced Editing](specs/19-debug-inspector-advanced-editing.md) |
| 20 | [Battle Viewer — Event Playback](specs/completed/20-battle-viewer-event-playback.md) ✅ |
| 21 | [Mouse & Hover Input](specs/completed/21-mouse-hover-input.md) ✅ |
| 22 | [Braille UI Chrome — Buttons & Panels](specs/completed/22-braille-ui-chrome.md) ✅ |
| 23 | [Piece Identity Data Model](specs/completed/23-piece-identity-data-model.md) ✅ |
| 24 | [Roster — Carousel](specs/completed/24-roster-carousel.md) ✅ |
| 25 | [Main Hub Navigation](specs/completed/25-main-hub-navigation.md) ✅ |
| 26 | [Screen-Space Positioning](specs/completed/26-screen-space-positioning.md) ✅ |
| 28 | [Anchor Margin Support](specs/completed/28-anchor-margin-support.md) ✅ |
| 29 | [Tint Shape Invariance](specs/29-tint-shape-invariance.md) |
| 30 | [Asset Decode Caching](specs/30-asset-decode-caching.md) |

## Status

Early design phase. Specs are being written. No implementation yet.
